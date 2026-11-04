use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use super::super::texture_decode::{DecodeJob, DecodePipeline};
use crate::hle::{
    framebuffer_load::{FramebufferCopy, FramebufferTexture},
    texture_request::EncodedInput,
};
use crate::profiling::Recorder;

#[derive(Default)]
pub(crate) struct Copies {
    snapshots: HashMap<usize, (Weak<FramebufferCopy>, wgpu::Buffer)>,
    requests: HashMap<usize, wgpu::TextureView>,
    pending: HashMap<usize, Vec<PendingDecode>>,
    order: HashMap<usize, usize>,
    pipeline: Option<DecodePipeline>,
    failed: HashMap<usize, crate::DiagKind>,
}

struct PendingDecode {
    job: DecodeJob,
    sources: Vec<(wgpu::Buffer, u64)>,
}

pub(crate) fn key(copy: &Arc<FramebufferCopy>) -> usize {
    Arc::as_ptr(copy) as usize
}

impl Copies {
    pub fn begin(
        &mut self,
        device: &wgpu::Device,
        copies: impl Iterator<Item = Arc<FramebufferCopy>>,
    ) {
        self.snapshots
            .retain(|_, (copy, _)| copy.strong_count() != 0);
        self.failed
            .retain(|key, _| self.snapshots.contains_key(key));
        self.requests.clear();
        self.pending.clear();
        self.order.clear();
        for (index, copy) in copies.enumerate() {
            let key = key(&copy);
            self.failed.remove(&key);
            self.order.insert(key, index + 1);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("framebuffer-copy"),
                size: u64::from(copy.size),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            self.snapshots.insert(key, (Arc::downgrade(&copy), buffer));
        }
    }

    pub fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &Arc<FramebufferTexture>,
        profiling: &Recorder,
    ) -> wgpu::TextureView {
        let id = Arc::as_ptr(request) as usize;
        if let Some(view) = self.requests.get(&id) {
            return view.clone();
        }
        let EncodedInput::Tmem(bank) = request.encoded.input() else {
            unreachable!()
        };
        let mut bytes = bank.to_vec();
        bytes.resize(4096 + 4096 * 4, 0);
        let mut offsets = HashMap::new();
        let mut sources = Vec::new();
        let mut after = 0;
        for tag in request.tags.bytes.iter().filter(|tag| tag.copy != 0) {
            offsets.entry(tag.copy).or_insert_with(|| {
                let copy = &request.tags.copies[usize::from(tag.copy) - 1];
                let key = key(copy);
                after = after.max(self.order.get(&key).copied().unwrap_or(0));
                let offset = bytes.len() as u64;
                bytes.resize(bytes.len() + copy.size as usize, 0);
                sources.push((self.snapshots[&key].1.clone(), offset));
                (offset / 4) as u32
            });
        }
        for (index, tag) in request.tags.bytes.iter().enumerate() {
            if tag.copy != 0 {
                let pointer = offsets[&tag.copy] * 2 + tag.pointer + 1;
                bytes[4096 + index * 4..4100 + index * 4].copy_from_slice(&pointer.to_le_bytes());
            }
        }
        let pipeline = self
            .pipeline
            .get_or_insert_with(|| DecodePipeline::new(device));
        let job = pipeline.create_job_input(
            device,
            queue,
            &request.encoded,
            profiling,
            None,
            Some(bytes),
        );
        let view = job.view.clone();
        self.pending
            .entry(after)
            .or_default()
            .push(PendingDecode { job, sources });
        self.requests.insert(id, view.clone());
        view
    }

    pub(in crate::render) fn failure(
        &self,
        texture: &super::super::inputs::TextureInputs,
    ) -> Option<crate::DiagKind> {
        if self.failed.is_empty() {
            return None;
        }
        std::iter::once(&texture.texture)
            .chain(texture.tex1.iter())
            .chain(texture.mip_levels.iter())
            .chain(texture.detail_tex.iter())
            .find_map(|binding| {
                let crate::hle::texture_request::TextureSource::Framebuffer(request) =
                    &binding.source
                else {
                    return None;
                };
                request
                    .tags
                    .bytes
                    .iter()
                    .filter(|tag| tag.copy != 0)
                    .find_map(|tag| {
                        self.failed
                            .get(&key(&request.tags.copies[usize::from(tag.copy) - 1]))
                            .copied()
                    })
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::render) fn copy(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        copy: &Arc<FramebufferCopy>,
        targets: &HashMap<super::super::workload::TargetId, super::Framebuffer>,
        versions: &HashMap<super::coherence::Writer, wgpu::Texture>,
        owners: Option<&super::coherence::Ownership>,
        profiling: &Recorder,
    ) {
        use super::super::workload::TargetId;
        use super::coherence::Writer;
        let key = key(copy);
        let buffer = &self.snapshots[&key].1;
        for span in &copy.spans {
            let start = copy.source.address
                + u64::from(span.origin[1] * copy.source.layout.width + span.origin[0]) * 2;
            let mut x = 0;
            while x < span.width {
                let address = start + u64::from(x) * 2;
                let owned = owners
                    .and_then(|owners| owners.0.range(..=address).next_back())
                    .filter(|(start, range)| address < **start + range.bytes.len() as u64);
                let end = owners
                    .and_then(|owners| owners.0.range(address..).next())
                    .map_or(start + u64::from(span.width) * 2, |(&next, _)| next);
                let (writer, end) = owned.map_or(
                    (
                        Writer {
                            address: copy.source.address,
                            layout: copy.source.layout,
                            depth: false,
                        },
                        end,
                    ),
                    |(&start, range)| (range.writer, start + range.bytes.len() as u64),
                );
                let fail =
                    |reason| crate::DiagKind::UnsupportedFramebufferAccess { address, reason };
                if writer.depth
                    || (writer.layout.fmt, writer.layout.siz) != (0, 2)
                    || end < address + 2
                {
                    self.failed.insert(
                        key,
                        fail(if writer.depth {
                            crate::FramebufferAccess::DepthTexture
                        } else {
                            crate::FramebufferAccess::Reinterpretation
                        }),
                    );
                    return;
                }
                let pixel = (address - writer.address) / 2;
                let origin = wgpu::Origin3d {
                    x: (pixel % u64::from(writer.layout.width)) as u32,
                    y: (pixel / u64::from(writer.layout.width)) as u32,
                    z: 0,
                };
                let width = (span.width - x)
                    .min(((end - address) / 2) as u32)
                    .min(writer.layout.width - origin.x);
                let Some(texture) = targets
                    .get(&TargetId::Guest(writer.address))
                    .filter(|target| {
                        target.layout == writer.layout && origin.y < target.color.height()
                    })
                    .map(|target| &target.color)
                    .or_else(|| {
                        versions
                            .get(&writer)
                            .filter(|texture| origin.y < texture.height())
                    })
                else {
                    self.failed
                        .insert(key, fail(crate::FramebufferAccess::MissingSource));
                    return;
                };
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: u64::from(span.buffer_offset + x * 4),
                            bytes_per_row: None,
                            rows_per_image: None,
                        },
                    },
                    wgpu::Extent3d {
                        width,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                profiling.count("framebuffer.copy_bytes", u64::from(width) * 4);
                x += width;
            }
        }
        self.encode(encoder, self.order[&key], profiling);
    }

    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        after: usize,
        profiling: &Recorder,
    ) {
        if let Some(jobs) = self.pending.remove(&after) {
            for PendingDecode { job, sources } in jobs {
                for (source, offset) in sources {
                    encoder.copy_buffer_to_buffer(
                        &source,
                        0,
                        &job.staging.input,
                        offset,
                        source.size(),
                    );
                }
                self.pipeline
                    .as_ref()
                    .unwrap()
                    .encode(encoder, &[job], profiling);
            }
        }
    }
}
