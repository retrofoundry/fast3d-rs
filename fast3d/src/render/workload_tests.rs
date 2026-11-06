use super::*;
use crate::render::{extents::TargetExtents, headless_device, inputs::RenderInputs, SceneRenderer};
use crate::scene::{ColorImage, FramebufferPair, Rect};
use crate::ClearPolicy;

fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let stride = (texture.width() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: if texture.format() == crate::render::DEPTH_FORMAT {
                wgpu::TextureAspect::DepthOnly
            } else {
                wgpu::TextureAspect::All
            },
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let data = buffer.slice(..).get_mapped_range();
    data.chunks(stride as usize)
        .flat_map(|row| &row[..texture.width() as usize * 4])
        .copied()
        .collect()
}

#[test]
fn scaled_pass_keeps_presentation_pixels_and_native_bounds_separate() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 16, 12, dual);
    renderer.set_test_scale(2, 256 * 1024 * 1024);
    renderer.coherence = Some(crate::render::framebuffers::coherence::Coherence::new(
        crate::FramebufferEncoding::BigEndian,
    ));
    let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Counters);
    renderer.profiling = recorder.clone();
    let id = TargetId::Guest(0x100000);
    let depth_id = TargetId::Guest(0x200000);
    let mut scene = Scene {
        framebuffer_pairs: vec![FramebufferPair {
            color_image: ColorImage {
                addr: 0x100000,
                width: 16,
                fmt: 0,
                siz: 2,
            },
            depth_image: Some(0x200000),
            size_extent: (16, 12),
            active_scissor: Scissor {
                ulx: 3,
                uly: 2,
                lrx: 14,
                lry: 10,
                ..Default::default()
            },
            ops: vec![SceneOp::FillRect {
                rect: Rect {
                    ulx: 0,
                    uly: 0,
                    lrx: 15,
                    lry: 11,
                },
                color_raw: 0xf801f801,
                convert: [0; 6],
                key: Default::default(),
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let render = |renderer: &mut SceneRenderer, scene: &Scene| {
        let mut inputs = RenderInputs::new(scene, (16, 12), [0; 2]).unwrap();
        assert_eq!(inputs.targets.len(), 1);
        let target = &mut inputs.targets[0];
        target.any_depth = true;
        if !target.operations.is_empty() {
            assert_eq!(
                target.write_bounds(0),
                if target.depth_clear {
                    (5, 3, 7, 6)
                } else if target.extents.native.1 == 6 {
                    (3, 2, 11, 4)
                } else {
                    (3, 2, 11, 8)
                }
            );
        }
        target.extents =
            TargetExtents::new(target.id, target.extents.logical, (16, 12), 2).unwrap();
        renderer.render_inputs(&device, &queue, &inputs, ClearPolicy::Persist);
    };
    render(&mut renderer, &scene);
    let fb = &renderer.framebuffers[&id];
    assert_eq!(
        (fb.color.width(), fb.color.height(), fb.height),
        (16, 12, 12)
    );
    let counters = recorder.drain().counters;
    assert_eq!(counters["readback.coverage_bytes"], 1536);
    assert_eq!(counters["readback.color_bytes"], 768);
    let writes = renderer.coherence.as_mut().unwrap().finish(&device);
    assert_eq!(writes.len(), 8);
    for (row, write) in writes.iter().enumerate() {
        assert_eq!(write.address, 0x100046 + row as u64 * 32);
        assert_eq!(write.bytes, [0xf8, 0x01].repeat(11));
    }
    let presentation = &fb.presentation.as_ref().unwrap().texture;
    assert_eq!((presentation.width(), presentation.height()), (32, 24));
    let pixels = read_texture(&device, &queue, presentation);
    for y in 0..24 {
        for x in 0..32 {
            let pixel = crate::tests::common::pixel(&pixels, 32, x, y);
            assert_eq!(
                pixel[0] == 255,
                (6..28).contains(&x) && (4..20).contains(&y),
                "({x}, {y})"
            );
        }
    }
    let scanout = crate::tests::common::pixels_from_render(
        &device,
        &queue,
        32,
        24,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.scanout(&device, &mut encoder, view, id, None);
            queue.submit([encoder.finish()]);
        },
    );
    assert!(
        scanout == pixels,
        "scanout must include all presentation rows"
    );
    let cropped = crate::tests::common::pixels_from_render(
        &device,
        &queue,
        32,
        16,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.scanout(&device, &mut encoder, view, id, Some(2..10));
            queue.submit([encoder.finish()]);
        },
    );
    assert!(
        cropped == pixels[32 * 4 * 4..32 * 20 * 4],
        "VI rows stay native"
    );
    let descriptor = renderer.target_descriptors.get(0x100000, false).unwrap();
    assert_eq!(descriptor.range().unwrap(), 0x100000..0x100180);

    scene.framebuffer_pairs[0].color_image.addr = 0x200000;
    scene.framebuffer_pairs[0].is_depth_clear = true;
    scene.framebuffer_pairs[0].ops[0] = SceneOp::FillRect {
        rect: Rect {
            ulx: 5,
            uly: 3,
            lrx: 11,
            lry: 8,
        },
        color_raw: 0,
        convert: [0; 6],
        key: Default::default(),
    };
    render(&mut renderer, &scene);
    let depth = &renderer.depthbuffers[&depth_id];
    assert_eq!((depth.texture.width(), depth.texture.height()), (16, 12));
    let presentation_depth = &depth.presentation.as_ref().unwrap().texture;
    assert_eq!(
        (presentation_depth.width(), presentation_depth.height()),
        (32, 24)
    );
    let pixels = read_texture(
        &device,
        &queue,
        &depth.presentation.as_ref().unwrap().texture,
    );
    for y in 0..24 {
        for x in 0..32 {
            let start = ((y * 32 + x) * 4) as usize;
            let value = f32::from_le_bytes(pixels[start..start + 4].try_into().unwrap());
            assert_eq!(
                value == 0.0,
                (10..24).contains(&x) && (6..18).contains(&y),
                "depth ({x}, {y}): {value}"
            );
        }
    }
    scene.framebuffer_pairs[0].color_image.addr = 0x100000;
    scene.framebuffer_pairs[0].is_depth_clear = false;
    scene.framebuffer_pairs[0].size_extent.1 = 6;
    scene.framebuffer_pairs[0].active_scissor.lry = 6;
    scene.framebuffer_pairs[0].ops[0] = SceneOp::FillRect {
        rect: Rect {
            ulx: 0,
            uly: 0,
            lrx: 15,
            lry: 11,
        },
        color_raw: 0xf801f801,
        convert: [0; 6],
        key: Default::default(),
    };
    render(&mut renderer, &scene);
    let counters = recorder.drain().counters;
    assert_eq!(counters["readback.coverage_bytes"], 1536);
    assert_eq!(counters["readback.color_bytes"], 768);
    let fb = &renderer.framebuffers[&id];
    let depth = &renderer.depthbuffers[&depth_id];
    assert_eq!(
        (fb.color.width(), fb.color.height(), fb.height),
        (16, 12, 12)
    );
    assert_eq!((depth.texture.width(), depth.texture.height()), (16, 12));
    let presentation_depth = &depth.presentation.as_ref().unwrap().texture;
    assert_eq!(
        (presentation_depth.width(), presentation_depth.height()),
        (32, 24)
    );
}
