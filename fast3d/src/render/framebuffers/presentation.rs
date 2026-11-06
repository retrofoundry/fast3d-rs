use super::super::buffer_init;

pub(in crate::render) struct Color {
    pub texture: wgpu::Texture,
    pub attach: wgpu::TextureView,
}

impl Color {
    pub fn new(device: &wgpu::Device, allocation: (u32, u32)) -> Self {
        let texture =
            device.create_texture(&super::color_descriptor("presentation-color", allocation));
        let attach = texture.create_view(&Default::default());
        Self { texture, attach }
    }
}

pub(in crate::render) struct Transfers {
    color: wgpu::RenderPipeline,
    color_layout: wgpu::BindGroupLayout,
}

impl Transfers {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("presentation-import"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../presentation.wgsl").into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("presentation-import"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("presentation-import"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("presentation-import"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("import_color"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            color: pipeline,
            color_layout: layout,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn import(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        scale: u32,
        spans: &[(u32, u32, u32)],
        profiling: &crate::profiling::Recorder,
    ) {
        let buffer = buffer_init(
            device,
            profiling,
            "presentation",
            &wgpu::util::BufferInitDescriptor {
                label: Some("presentation-scale"),
                contents: bytemuck::cast_slice(&[scale, 0, 0, 0]),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("presentation-import"),
            layout: &self.color_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        let color = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("presentation-import"),
            color_attachments: &color,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.color);
        pass.set_bind_group(0, &group, &[]);
        for &(x, y, width) in spans {
            pass.set_scissor_rect(x * scale, y * scale, width * scale, scale);
            pass.draw(0..3, 0..1);
        }
    }
}
