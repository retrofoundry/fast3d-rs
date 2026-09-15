use std::borrow::Cow;

use crate::{
    hle::texture_request::{EncodedInput, EncodedTextureRequest},
    profiling::Recorder,
};

const SHADER: &str = include_str!("texture_decode.wgsl");
const PARAM_BYTES: u64 = 64;

pub(crate) struct DecodePipeline {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

pub(crate) struct DecodeStaging {
    input: wgpu::Buffer,
    params: wgpu::Buffer,
}

impl DecodeStaging {
    pub(crate) fn capacity(&self) -> u64 {
        self.input.size()
    }
}

pub(crate) struct DecodeJob {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub staging: DecodeStaging,
    bind_group: wgpu::BindGroup,
    extent: [u32; 2],
}

pub(crate) fn required_input_bytes(request: &EncodedTextureRequest) -> u64 {
    match request.input() {
        EncodedInput::Tmem(_) => 4096,
        EncodedInput::LinearCompat { bytes, .. } => bytes.len().div_ceil(4) as u64 * 4 + 4096,
    }
}

struct PackedInput<'a> {
    bytes: Cow<'a, [u8]>,
    palette_offset: u32,
    linear_len: u32,
}

impl<'a> PackedInput<'a> {
    fn new(request: &'a EncodedTextureRequest) -> Self {
        match request.input() {
            EncodedInput::Tmem(bank) => Self {
                bytes: Cow::Borrowed(&bank[..]),
                palette_offset: 0,
                linear_len: 0,
            },
            EncodedInput::LinearCompat {
                bytes,
                palette_bank,
            } => {
                let palette_offset = bytes.len().div_ceil(4) * 4;
                let mut packed = Vec::with_capacity(palette_offset + palette_bank.len());
                packed.extend_from_slice(bytes);
                packed.resize(palette_offset, 0);
                packed.extend_from_slice(&palette_bank[..]);
                Self {
                    bytes: Cow::Owned(packed),
                    palette_offset: palette_offset as u32,
                    linear_len: bytes.len() as u32,
                }
            }
        }
    }
}

struct DecodeParams {
    words: [[u32; 4]; 4],
}

impl DecodeParams {
    fn new(request: &EncodedTextureRequest, input: &PackedInput<'_>) -> Self {
        let recipe = request.recipe();
        Self {
            words: [
                [
                    recipe.output[0],
                    recipe.output[1],
                    recipe.logical[0],
                    recipe.logical[1],
                ],
                [
                    recipe.representation as u32,
                    recipe.fmt.into(),
                    recipe.siz.into(),
                    u32::from(recipe.base) * 8,
                ],
                [
                    u32::from(recipe.line) * 8,
                    recipe.palette.into(),
                    recipe.tlut.into(),
                    input.linear_len,
                ],
                [input.palette_offset, 0, 0, 0],
            ],
        }
    }

    fn bytes(&self) -> [u8; PARAM_BYTES as usize] {
        let mut result = [0; PARAM_BYTES as usize];
        for (word, bytes) in self
            .words
            .iter()
            .flatten()
            .zip(result.as_chunks_mut::<4>().0)
        {
            *bytes = word.to_le_bytes();
        }
        result
    }
}

impl DecodePipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("texture-decode"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture-decode"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(4096),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARAM_BYTES),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("texture-decode"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("texture-decode"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, layout }
    }

    pub(crate) fn create_job(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: &EncodedTextureRequest,
        profiling: &Recorder,
        staging: Option<DecodeStaging>,
    ) -> DecodeJob {
        let input = PackedInput::new(request);
        let params = DecodeParams::new(request, &input);
        let staging = staging
            .filter(|v| v.capacity() >= input.bytes.len() as u64)
            .unwrap_or_else(|| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("texture-decode-input"),
                    size: input.bytes.len() as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let params = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("texture-decode-params"),
                    size: PARAM_BYTES,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                profiling.buffer("texture_decode_input", buffer.size(), 0);
                profiling.buffer("texture_decode_uniform", params.size(), 0);
                DecodeStaging {
                    input: buffer,
                    params,
                }
            });
        queue.write_buffer(&staging.input, 0, &input.bytes);
        queue.write_buffer(&staging.params, 0, &params.bytes());
        profiling.count(
            "buffer.texture_decode_input.upload_bytes",
            input.bytes.len() as u64,
        );
        profiling.count("buffer.texture_decode_uniform.upload_bytes", PARAM_BYTES);
        profiling.count("tmem.decode_input_upload_calls", 1);
        profiling.count("tmem.decode_uniform_upload_calls", 1);
        profiling.count("tmem.decode_input_upload_bytes", input.bytes.len() as u64);
        profiling.count("tmem.decode_uniform_upload_bytes", PARAM_BYTES);
        profiling.count(
            "tmem.decode_upload_padding_bytes",
            match request.input() {
                EncodedInput::Tmem(_) => 0,
                EncodedInput::LinearCompat { bytes, .. } => {
                    u64::from(input.palette_offset) - bytes.len() as u64
                }
            },
        );
        let extent = request.recipe().output;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("texture-decode-output"),
            size: wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        profiling.count("tmem.decode_output_allocations", 1);
        profiling.count(
            "tmem.decode_output_bytes",
            u64::from(extent[0]) * u64::from(extent[1]) * 4,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture-decode"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: staging.input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: staging.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        DecodeJob {
            texture,
            view,
            staging,
            bind_group,
            extent,
        }
    }

    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        jobs: &[DecodeJob],
        profiling: &Recorder,
    ) {
        if jobs.is_empty() {
            return;
        }
        profiling.count("tmem.decode_compute_passes", 1);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("texture-decode"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        for job in jobs {
            pass.set_bind_group(0, &job.bind_group, &[]);
            pass.dispatch_workgroups(job.extent[0].div_ceil(8), job.extent[1].div_ceil(8), 1);
            profiling.count("tmem.decode_dispatches", 1);
        }
    }
}

#[cfg(test)]
#[path = "texture_decode_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "texture_decode_vectors.rs"]
mod vectors;

#[cfg(test)]
use crate::profiling;
#[cfg(all(
    test,
    not(target_arch = "wasm32"),
    not(all(feature = "capture", feature = "profiling"))
))]
use crate::tests::readback_export as export;

#[cfg(any(test, all(feature = "profiling", feature = "capture")))]
pub(crate) async fn readback_requests(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    requests: &[crate::profiling::Request],
) -> Result<Vec<Vec<u8>>, String> {
    let requests = requests
        .iter()
        .map(|v| v.encoded_request())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{e:?}"))?;
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let profiling = Recorder::default();
    let pipeline = DecodePipeline::new(device);
    let jobs: Vec<_> = requests
        .iter()
        .map(|request| pipeline.create_job(device, queue, request, &profiling, None))
        .collect();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("texture-decode-readback"),
    });
    pipeline.encode(&mut encoder, &jobs, &profiling);
    let buffers: Vec<_> = jobs
        .iter()
        .map(|job| {
            let row_bytes = (job.extent[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("texture-decode-readback"),
                size: u64::from(row_bytes) * u64::from(job.extent[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                job.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: None,
                    },
                },
                job.texture.size(),
            );
            (buffer, row_bytes)
        })
        .collect();
    queue.submit([encoder.finish()]);
    let ready: Vec<_> = buffers
        .iter()
        .map(|(buffer, _)| {
            #[cfg(not(target_arch = "wasm32"))]
            let (sender, receiver) = std::sync::mpsc::channel();
            #[cfg(target_arch = "wasm32")]
            let (sender, receiver) = futures_channel::oneshot::channel();
            buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sender.send(result);
            });
            receiver
        })
        .collect();
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    let mut result = Vec::with_capacity(jobs.len());
    for ((job, (buffer, row_bytes)), ready) in jobs.iter().zip(&buffers).zip(ready) {
        #[cfg(not(target_arch = "wasm32"))]
        let ready = ready.recv();
        #[cfg(target_arch = "wasm32")]
        let ready = ready.await;
        ready
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = buffer.get_mapped_range(..);
        let mut pixels = Vec::with_capacity((job.extent[0] * job.extent[1] * 4) as usize);
        for row in 0..job.extent[1] as usize {
            let start = row * *row_bytes as usize;
            pixels.extend_from_slice(&mapped[start..start + job.extent[0] as usize * 4]);
        }
        drop(mapped);
        buffer.unmap();
        result.push(pixels);
    }
    Ok(result)
}

#[cfg(all(
    test,
    not(target_arch = "wasm32"),
    not(all(feature = "capture", feature = "profiling"))
))]
#[path = "texture_decode_gpu_tests.rs"]
mod gpu_tests;

#[cfg(all(
    test,
    not(target_arch = "wasm32"),
    not(all(feature = "capture", feature = "profiling"))
))]
#[path = "texture_decode_test_support.rs"]
mod test_support;
