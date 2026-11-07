use wgpu::util::DeviceExt;

pub(super) struct BoxFilter {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

impl BoxFilter {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("box-reduction"),
            source: wgpu::ShaderSource::Wgsl(include_str!("downsample.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("box-reduction"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
            label: Some("box-reduction"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("box-reduction"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("box_filter"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, layout }
    }

    pub fn reduce(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        rows: std::ops::Range<u32>,
        factor: u32,
    ) -> wgpu::Texture {
        let mut descriptor = super::framebuffers::color_descriptor(
            "box-reduced",
            (
                (source.width() / factor).max(1),
                ((rows.end - rows.start) / factor).max(1),
            ),
        );
        descriptor.usage |= wgpu::TextureUsages::STORAGE_BINDING;
        let output = device.create_texture(&descriptor);
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("box-reduction"),
            contents: bytemuck::cast_slice(&[rows.start, rows.end, factor, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("box-reduction"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &source.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &output.create_view(&Default::default()),
                    ),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("box-reduction"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        let block = if factor <= 8 { 8 } else { 1 };
        pass.dispatch_workgroups(
            output.width().div_ceil(block),
            output.height().div_ceil(block),
            1,
        );
        drop(pass);
        output
    }
}
