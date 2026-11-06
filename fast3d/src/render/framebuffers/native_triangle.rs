use super::coherence::{CoverageDraw, CoverageGeometry};
use wgpu::util::DeviceExt;

const PRIMITIVE_CAPACITY: usize = 256;
const TILE_CAPACITY: u32 = 4096;
const MASK_WORDS: u64 = (PRIMITIVE_CAPACITY / 32) as u64;

pub(super) struct Batch {
    setup: wgpu::ComputePipeline,
    spans: wgpu::ComputePipeline,
    binner: wgpu::ComputePipeline,
    ordered: wgpu::ComputePipeline,
    bins: wgpu::Buffer,
    primitives: Vec<[u32; 32]>,
    descriptors: wgpu::Buffer,
    params: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    group: Option<(wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup)>,
    upload: Vec<u32>,
    primitive_capacity: usize,
    tile_capacity: u32,
}

impl Batch {
    pub fn new(device: &wgpu::Device, limits: Option<(usize, u32)>) -> Self {
        let (primitive_capacity, tile_capacity) =
            limits.unwrap_or((PRIMITIVE_CAPACITY, TILE_CAPACITY));
        assert!((1..=PRIMITIVE_CAPACITY).contains(&primitive_capacity));
        assert!((1..=TILE_CAPACITY).contains(&tile_capacity));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-triangle"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../native_triangle.wgsl").into()),
        });
        let entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 4 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding == 1,
                        }
                    },
                    has_dynamic_offset: binding == 4,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native-triangle"),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native-triangle"),
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            setup: pipeline("setup"),
            spans: pipeline("spans"),
            binner: pipeline("bin"),
            ordered: pipeline("main"),
            bins: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-tile-masks"),
                size: u64::from(tile_capacity) * MASK_WORDS * 8,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            descriptors: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-primitives"),
                size: PRIMITIVE_CAPACITY as u64 * 128,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-batch"),
                size: u64::from(device.limits().min_uniform_buffer_offset_alignment),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            layout: group_layout,
            group: None,
            upload: Vec::new(),
            primitives: Vec::with_capacity(primitive_capacity),
            primitive_capacity,
            tile_capacity,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        store: &super::native_fill::Store,
        vertices: &wgpu::Buffer,
        draw: &CoverageDraw,
        render_height: u32,
        slot: u32,
    ) {
        let CoverageGeometry::Triangles(triangles) = &draw.geometry else {
            unreachable!()
        };
        let mode = draw.mode.unwrap();
        let (x, y, w, h) = draw.scissor;
        for triangle in triangles {
            if self.primitives.len() == self.primitive_capacity {
                self.flush(device, encoder, store, vertices);
            }
            let mut descriptor = [0; 32];
            descriptor[..16].copy_from_slice(&[
                triangle[0],
                triangle[1],
                triangle[2],
                slot,
                x,
                y,
                w,
                h,
                store.extent.0,
                store.extent.1,
                (render_height as f32).to_bits(),
                u32::from(draw.combiner.as_ref().unwrap().strict_color().unwrap()),
                u32::from(mode.antialias),
                u32::from(draw.cull),
                u32::from(mode.cvg_dst),
                u32::from(mode.image_read),
            ]);
            self.primitives.push(descriptor);
        }
    }

    pub fn flush(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        store: &super::native_fill::Store,
        vertices: &wgpu::Buffer,
    ) {
        if self.primitives.is_empty() {
            return;
        }
        let tiles_x = store.extent.0.div_ceil(8);
        let tiles = tiles_x * store.extent.1.div_ceil(8);
        let windows = tiles.div_ceil(self.tile_capacity);
        let rows = store.extent.1.min(2048);
        let descriptor_size = PRIMITIVE_CAPACITY as u64 * (128 + u64::from(rows) * 32);
        if self.descriptors.size() < descriptor_size {
            self.descriptors = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-primitives"),
                size: descriptor_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.group = None;
        }
        let alignment = device.limits().min_uniform_buffer_offset_alignment.max(32);
        let uniform_size = u64::from(windows) * u64::from(alignment);
        if self.params.size() < uniform_size {
            self.params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-batch"),
                size: uniform_size.next_power_of_two(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.group = None;
        }
        if self
            .group
            .as_ref()
            .is_none_or(|(buffer, source, _)| buffer != &store.buffer || source != vertices)
        {
            let buffers = [&store.buffer, vertices, &self.descriptors, &self.bins];
            let mut entries: Vec<_> = buffers
                .into_iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.params,
                    offset: 0,
                    size: std::num::NonZeroU64::new(24),
                }),
            });
            self.group = Some((
                store.buffer.clone(),
                vertices.clone(),
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native-batch"),
                    layout: &self.layout,
                    entries: &entries,
                }),
            ));
        }
        self.upload.clear();
        self.upload
            .extend_from_slice(bytemuck::cast_slice(&self.primitives));
        let descriptor_bytes = (self.upload.len() * 4) as u64;
        for start in (0..tiles).step_by(self.tile_capacity as usize) {
            self.upload.extend_from_slice(&[
                store.extent.0,
                store.extent.1,
                self.primitives.len() as u32,
                tiles_x,
                start,
                rows,
            ]);
            self.upload
                .resize(self.upload.len() + alignment as usize / 4 - 6, 0);
        }
        let upload = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native-batch-upload"),
            contents: bytemuck::cast_slice(&self.upload),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&upload, 0, &self.descriptors, 0, descriptor_bytes);
        encoder.copy_buffer_to_buffer(&upload, descriptor_bytes, &self.params, 0, uniform_size);
        let group = &self.group.as_ref().unwrap().2;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("native-batch"),
            timestamp_writes: None,
        });
        for start in (0..tiles).step_by(self.tile_capacity as usize) {
            let count = self.tile_capacity.min(tiles - start);
            let offset = (start / self.tile_capacity) * alignment;
            if start == 0 {
                pass.set_pipeline(&self.setup);
                pass.set_bind_group(0, group, &[offset]);
                pass.dispatch_workgroups((self.primitives.len() as u32).div_ceil(64), 1, 1);
                pass.set_pipeline(&self.spans);
                pass.dispatch_workgroups(
                    (self.primitives.len() as u32).div_ceil(8),
                    rows.div_ceil(8),
                    1,
                );
            }
            for pipeline in [&self.binner, &self.ordered] {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[offset]);
                pass.dispatch_workgroups(count, 1, 1);
            }
        }
        self.primitives.clear();
    }
}
