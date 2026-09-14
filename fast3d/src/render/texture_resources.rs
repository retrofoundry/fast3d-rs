use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};

#[cfg(all(feature = "profiling", feature = "capture"))]
pub(crate) mod probe;

use super::{
    texture_decode::{DecodeJob, DecodePipeline, DecodeStaging},
    texture_residency::{Limits, Residency, Resident},
};
use crate::{
    hle::texture_request::{EncodedTextureRequest, TextureSource},
    profiling::Recorder,
};

pub(super) struct GpuImage {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

pub(super) type Image = Arc<Resident<GpuImage>>;

struct Submission {
    complete: Arc<AtomicBool>,
    staging: Vec<DecodeStaging>,
    images: Vec<Image>,
}

pub(super) struct TextureResources {
    pub cache: Residency<GpuImage>,
    pipeline: DecodePipeline,
    free: Vec<DecodeStaging>,
    submissions: Vec<Submission>,
    jobs: Vec<DecodeJob>,
    images: Vec<Image>,
    transients: Vec<Weak<Resident<GpuImage>>>,
}

impl TextureResources {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            cache: Residency::new(Limits::default()),
            pipeline: DecodePipeline::new(device),
            free: Vec::new(),
            submissions: Vec::new(),
            jobs: Vec::new(),
            images: Vec::new(),
            transients: Vec::new(),
        }
    }

    pub fn collect(&mut self, profiling: &Recorder) {
        if self.submissions.is_empty() {
            return;
        }
        let mut index = 0;
        let mut reclaimed = false;
        while index < self.submissions.len() {
            if self.submissions[index].complete.load(Ordering::Acquire) {
                self.free
                    .extend(self.submissions.swap_remove(index).staging);
                reclaimed = true;
            } else {
                index += 1;
            }
        }
        if !reclaimed {
            return;
        }
        let active: std::collections::HashSet<_> = self
            .images
            .iter()
            .chain(
                self.submissions
                    .iter()
                    .flat_map(|submission| &submission.images),
            )
            .map(Arc::as_ptr)
            .collect();
        self.cache.retain_pending(|image| active.contains(&image));
        self.transients.retain(|image| image.strong_count() != 0);
        self.free.sort_unstable_by_key(DecodeStaging::capacity);
        let mut bytes = 0;
        self.free.retain(|staging| {
            bytes += staging.capacity() + 64;
            bytes <= 8 * 1024 * 1024
        });
        self.free.truncate(4096);
        self.profile(profiling);
    }

    pub fn resolve(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &EncodedTextureRequest,
        role: &'static str,
        profiling: &Recorder,
    ) -> Image {
        let key = request.key_profiled(profiling);
        let witness = request.witness_profiled(profiling);
        let [w, h] = request.recipe().output;
        let bytes = u64::from(w) * u64::from(h) * 4;
        let mut created = false;
        let image = self.cache.resolve(key, witness, bytes, profiling, || {
            created = true;
            let required = super::texture_decode::required_input_bytes(request);
            let staging = self
                .free
                .iter()
                .position(|staging| staging.capacity() >= required)
                .map(|index| self.free.swap_remove(index));
            if staging.is_some() {
                profiling.count("tmem.staging_reuses", 1);
            }
            let job = self
                .pipeline
                .create_job(device, queue, request, profiling, staging);
            let image = GpuImage {
                texture: job.texture.clone(),
                view: job.view.clone(),
            };
            debug_assert_eq!(
                u64::from(image.texture.width()) * u64::from(image.texture.height()) * 4,
                bytes
            );
            self.jobs.push(job);
            profiling.texture(role, bytes, 0);
            image
        });
        if created && !image.admitted {
            self.transients.push(Arc::downgrade(&image));
        }
        self.images.push(image.clone());
        image
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, profiling: &Recorder) {
        self.pipeline.encode(encoder, &self.jobs, profiling);
    }

    pub fn submitted(&mut self, queue: &wgpu::Queue, profiling: &Recorder) {
        self.cache.submitted();
        if !self.images.is_empty() {
            let complete = Arc::new(AtomicBool::new(false));
            let callback = complete.clone();
            queue.on_submitted_work_done(move || callback.store(true, Ordering::Release));
            self.submissions.push(Submission {
                complete,
                images: std::mem::take(&mut self.images),
                staging: self.jobs.drain(..).map(|job| job.staging).collect(),
            });
        }
        self.profile(profiling);
    }

    pub fn profile(&self, profiling: &Recorder) {
        if !profiling.active() {
            return;
        }
        self.cache.profile(profiling);
        let mut in_flight = std::collections::HashSet::new();
        let (gpu, cpu) = self
            .submissions
            .iter()
            .flat_map(|submission| &submission.images)
            .filter(|image| in_flight.insert(Arc::as_ptr(image)))
            .fold((0, 0), |(gpu, cpu), image| {
                (gpu + image.gpu_bytes, cpu + image.cpu_bytes())
            });
        profiling.gauge("tmem.in_flight_entries", in_flight.len() as u64);
        profiling.gauge("tmem.in_flight_gpu_bytes", gpu);
        profiling.gauge("tmem.in_flight_cpu_bytes", cpu as u64);
        profiling.gauge("tmem.in_flight_submissions", self.submissions.len() as u64);
        let mut transient = std::collections::HashSet::new();
        let (transient_bytes, transient_cpu) = self
            .transients
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|image| transient.insert(Arc::as_ptr(image)))
            .fold((0, 0), |(gpu, cpu), image| {
                (gpu + image.gpu_bytes, cpu + image.cpu_bytes())
            });
        profiling.gauge("tmem.transient_gpu_bytes", transient_bytes);
        profiling.gauge("tmem.transient_cpu_bytes", transient_cpu as u64);
        profiling.gauge("tmem.transient_entries", transient.len() as u64);
        let pending = self
            .jobs
            .iter()
            .map(|job| job.staging.capacity() + 64)
            .sum::<u64>()
            + self
                .submissions
                .iter()
                .flat_map(|submission| &submission.staging)
                .map(|staging| staging.capacity() + 64)
                .sum::<u64>();
        let free = self
            .free
            .iter()
            .map(|staging| staging.capacity() + 64)
            .sum::<u64>();
        profiling.gauge("tmem.staging_pending_bytes", pending);
        profiling.gauge("tmem.staging_free_bytes", free);
        profiling.gauge("tmem.staging_bytes", pending + free);
        let overhead = self.free.capacity() * std::mem::size_of::<DecodeStaging>()
            + self.jobs.capacity() * std::mem::size_of::<DecodeJob>()
            + self.images.capacity() * std::mem::size_of::<Image>()
            + self.transients.capacity() * std::mem::size_of::<Weak<Resident<GpuImage>>>()
            + self.submissions.capacity() * std::mem::size_of::<Submission>()
            + self
                .submissions
                .iter()
                .map(|submission| {
                    submission.images.capacity() * std::mem::size_of::<Image>()
                        + submission.staging.capacity() * std::mem::size_of::<DecodeStaging>()
                        + 2 * std::mem::size_of::<usize>()
                        + std::mem::size_of::<AtomicBool>()
                })
                .sum::<usize>();
        profiling.gauge("tmem.submission_allocation_overhead_bytes", overhead as u64);
    }
}

pub(super) struct RgbaImage {
    pub source: Arc<[u8]>,
    pub extent: [u32; 2],
    pub view: wgpu::TextureView,
}

pub(super) fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &TextureSource,
    extent: [u32; 2],
    role: &'static str,
    profiling: &Recorder,
) -> Arc<RgbaImage> {
    let TextureSource::Rgba(bytes) = source else {
        unreachable!()
    };
    let [width, height] = extent;
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("n64-rgba"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    profiling.texture(
        role,
        u64::from(width) * u64::from(height) * 4,
        bytes.len() as u64,
    );
    Arc::new(RgbaImage {
        source: bytes.clone(),
        extent,
        view: texture.create_view(&Default::default()),
    })
}
