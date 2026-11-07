pub(super) fn shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    let source = format!(
        "{}\n{}",
        include_str!("combiner_prelude.wgsl"),
        r#"
struct DeltaOutput {
    @location(0) code: u32,
    @builtin(frag_depth) depth: f32,
};
@fragment
fn fs_delta(in: VsOut) -> DeltaOutput {
    let code = delta_code(incoming_delta(in));
    if combiner.alpha_flags != 0u {
        alpha_discard(eval_combiner(in), in.clip_position.xy);
    }
    return DeltaOutput(code, fragment_depth(in.clip_position.z));
}
@fragment
fn fs_delta_decal(in: VsOut) -> DeltaOutput {
    let code = delta_code(incoming_delta(in));
    decal_test(in);
    if combiner.alpha_flags != 0u {
        alpha_discard(eval_combiner(in), in.clip_position.xy);
    }
    return DeltaOutput(code, fragment_depth(in.clip_position.z));
}
"#
    );
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("depth-delta"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn triangle_delta_normalization_literals() {
        let (device, queue, _) = super::super::headless_device();
        let source = format!(
            "{}\n{}",
            include_str!("combiner_prelude.wgsl"),
            r#"
@vertex fn vs_test(@builtin(vertex_index) i:u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32((i<<1u)&2u)*2.0-1.0, 1.0-f32(i&2u)*2.0, 0.0,1.0);
}
@fragment fn fs_test(@builtin(position) p:vec4<f32>) -> @location(0) vec4<f32> {
    let slopes=array<vec2<f32>,10>(vec2<f32>(0,0),vec2<f32>(1,0),vec2<f32>(4,0),vec2<f32>(-4,0),vec2<f32>(16384,0),vec2<f32>(32768,0),vec2<f32>(65536,0),vec2<f32>(-65536,0),vec2<f32>(65540,0),vec2<f32>(-65540,0));
    let delta=triangle_delta(slopes[min(u32(p.x),9u)]);
    return vec4<f32>(f32(delta&255u)/255.0,f32(delta>>8u)/255.0,0.0,1.0);
}
"#
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_test"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_test"),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let pixels = crate::tests::common::pixels_from_render(
            &device,
            &queue,
            64,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            |view| {
                let mut encoder = device.create_command_encoder(&Default::default());
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
                drop(pass);
                queue.submit([encoder.finish()]);
            },
        );
        assert_eq!(
            &pixels.as_chunks::<4>().0[..10],
            &[
                [1u8, 0, 0, 255],
                [3, 0, 0, 255],
                [8, 0, 0, 255],
                [4, 0, 0, 255],
                [0, 128, 0, 255],
                [0, 128, 0, 255],
                [1, 0, 0, 255],
                [1, 0, 0, 255],
                [8, 0, 0, 255],
                [4, 0, 0, 255]
            ]
        );
    }
}
