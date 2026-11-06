// Browser devices cannot synchronously map guest write-back buffers.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use super::coherence::Readback;
use wgpu::util::DeviceExt;

pub(super) struct Store {
    pub extent: (u32, u32),
    pub buffer: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
}

impl Store {
    pub fn new(device: &wgpu::Device, extent: (u32, u32)) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-fill"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../native_fill.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("native-fill"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-rgba16"),
            size: u64::from(extent.0) * u64::from(extent.1) * 12,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        Self {
            extent,
            buffer,
            pipeline,
        }
    }

    pub fn update(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        bounds: (u32, u32, u32, u32),
        parity: u32,
        literal: Option<u32>,
    ) -> Option<FillReadback> {
        let (x, y, w, h) = bounds;
        if w == 0 || h == 0 {
            return None;
        }
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native-fill"),
            contents: bytemuck::cast_slice(&[
                x,
                y,
                w,
                h,
                self.extent.0,
                parity,
                literal.unwrap_or(0),
                u32::from(literal.is_some()),
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native-fill"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("native-fill"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
        literal.map(|_| FillReadback {
            bounds,
            readback: Readback::native_fill(device, encoder, &self.buffer, self.extent.0, bounds),
        })
    }
}

pub(super) struct FillReadback {
    bounds: (u32, u32, u32, u32),
    readback: Readback,
}

#[cfg(not(target_arch = "wasm32"))]
impl FillReadback {
    pub fn read(self, device: &wgpu::Device) -> Pixels {
        Pixels {
            bounds: self.bounds,
            bytes: self.readback.read(device),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct Pixels {
    bounds: (u32, u32, u32, u32),
    bytes: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Pixels {
    pub fn get(&self, pixel: usize, width: usize) -> (u16, u8) {
        let (x, y, w, _) = self.bounds;
        let index = (pixel / width - y as usize) * w as usize + pixel % width - x as usize;
        let data = &self.bytes[index * 12..index * 12 + 12];
        let [word, coverage, valid] =
            std::array::from_fn(|i| u32::from_ne_bytes(data[i * 4..i * 4 + 4].try_into().unwrap()));
        assert_eq!(valid, 1);
        (word as u16, coverage as u8)
    }
}
