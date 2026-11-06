use super::coherence::{CoverageDraw, CoverageGeometry};
use wgpu::util::DeviceExt;

pub(super) fn dispatch(
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
    let [triangle] = triangles.as_slice() else {
        unreachable!()
    };
    let mode = draw.mode.unwrap();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("native-triangle"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../native_triangle.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("native-triangle"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let (x, y, w, h) = draw.scissor;
    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("native-triangle"),
        contents: bytemuck::cast_slice(&[
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
        ]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native-triangle"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: store.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: vertices.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("native-triangle"),
        timestamp_writes: None,
    });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.dispatch_workgroups(store.extent.0.div_ceil(8), store.extent.1.div_ceil(8), 1);
}
