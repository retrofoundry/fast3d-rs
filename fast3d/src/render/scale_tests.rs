use super::*;
use crate::ScalePolicy;

#[test]
fn live_transitions_rebuild_all_retained_color_and_depth_from_native() {
    for fallback in [false, true] {
        let mut renderer = facade(1, fallback);
        let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
        submit(
            &mut renderer,
            &mut memory,
            &fill_task(A, 32, 37, 0x07c107c1),
        );
        for address in [A, B] {
            submit(
                &mut renderer,
                &mut memory,
                &triangle(address, Z, 12000, 0xf08040ff),
            );
        }
        let color_id = workload::TargetId::Guest(A.into());
        let depth_id = workload::TargetId::Guest(Z.into());
        let native = read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.framebuffers[&color_id].color,
        );
        let depth = read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.depthbuffers[&depth_id].texture,
        );
        let descriptors = renderer.inner.target_descriptors.clone();
        let serial = renderer.inner.frame_serial;
        renderer.inner.dither_seed = 519;
        for (index, scale) in [3, 2, 1].into_iter().enumerate() {
            renderer
                .set_scale_policy(ScalePolicy::Fixed(scale))
                .unwrap();
            assert_eq!(renderer.effective_resolution_multiplier(), [1, 3, 2][index]);
            renderer.begin_frame();
            assert_eq!(renderer.effective_resolution_multiplier(), scale);
            assert_eq!(renderer.inner.target_descriptors, descriptors);
            assert_eq!(renderer.inner.frame_serial, serial + index as u64 + 1);
            assert_eq!(renderer.inner.dither_seed, 519);
            let fb = &renderer.inner.framebuffers[&color_id];
            assert_eq!(
                read_texture(&renderer.device, &renderer.queue, &fb.color),
                native
            );
            let z = &renderer.inner.depthbuffers[&depth_id];
            assert_eq!(
                read_texture(&renderer.device, &renderer.queue, &z.texture),
                depth
            );
            if scale > 1 {
                for (texture, expected, width) in [
                    (
                        &fb.presentation.as_ref().unwrap().texture,
                        &native,
                        fb.color.width(),
                    ),
                    (
                        &z.presentation.as_ref().unwrap().texture,
                        &depth,
                        z.texture.width(),
                    ),
                ] {
                    let bytes = read_texture(&renderer.device, &renderer.queue, texture);
                    for y in 0..texture.height() {
                        for x in 0..texture.width() {
                            let source = (((y / scale) * width + x / scale) * 4) as usize;
                            let dest = ((y * texture.width() + x) * 4) as usize;
                            assert_eq!(bytes[dest..dest + 4], expected[source..source + 4]);
                        }
                    }
                }
            } else {
                assert!(fb.presentation.is_none());
                assert!(z.presentation.is_none());
            }
        }
    }
}

#[test]
fn live_policy_latches_and_auto_uses_selected_crop_and_previous_reference() {
    let mut renderer = facade(1, true);
    let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
    submit(
        &mut renderer,
        &mut memory,
        &fill_task(A, 64, 131, 0x07c107c1),
    );
    renderer.set_scale_policy(ScalePolicy::Auto).unwrap();
    let vi = crate::ViRegisters {
        origin: A + 64 * 2 * 11,
        width: 64,
        status: 2,
        v_start: 0x0000_00c8,
        y_scale: 1024,
        ..Default::default()
    };
    for (height, scale) in [
        (0, 1),
        (100, 1),
        (101, 2),
        (200, 2),
        (201, 3),
        (301, 4),
        (5000, 4),
    ] {
        renderer.set_output_content_height(height);
        renderer.begin_frame_with_vi(Some(vi)).unwrap();
        assert_eq!(renderer.auto_reference_height(), Some(100));
        assert_eq!(renderer.effective_resolution_multiplier(), scale);
        renderer.set_output_content_height(1);
        assert_eq!(renderer.effective_resolution_multiplier(), scale);
    }
    renderer
        .begin_frame_with_vi(Some(crate::ViRegisters { width: 32, ..vi }))
        .unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(131));
    renderer.begin_frame_with_vi(Some(vi)).unwrap();
    renderer.set_output_content_height(250);
    renderer.begin_frame_with_vi(None).unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(100));
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
    renderer.set_scale_policy(ScalePolicy::Fixed(2)).unwrap();
    assert_eq!(renderer.requested_scale_policy(), ScalePolicy::Fixed(2));
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
    renderer.begin_frame();
    assert_eq!(renderer.effective_resolution_multiplier(), 2);
    assert_eq!(renderer.auto_reference_height(), None);
}

#[test]
fn box_reduction_anchors_at_crop_and_uses_floor_sizes_and_clamped_edges() {
    let (device, queue, dual) = headless_device();
    for scale in [1, 3] {
        let mut renderer = SceneRenderer::new(&device, FORMAT, 7, 11, dual);
        renderer.set_test_scale(scale, BUDGET);
        renderer.render_into_store(
            &device,
            &queue,
            &fill_scene(A, 7, 11, 0x07c107c1),
            ClearPolicy::Persist,
        );
        let id = workload::TargetId::Guest(A.into());
        let fb = &renderer.framebuffers[&id];
        let source = fb.presentation.as_ref().map_or(&fb.color, |p| &p.texture);
        let (width, height) = (source.width(), source.height());
        let bytes: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    [
                        ((x * 19 + y * 7) % 256) as u8,
                        (y * 9) as u8,
                        (x * 13) as u8,
                        255,
                    ]
                })
            })
            .collect();
        queue.write_texture(
            source.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            source.size(),
        );
        for d in [1, 2, 4, 64] {
            renderer.reduction_factor = d;
            let crop_height = 5 * scale;
            let out = ((width / d).max(1), (crop_height / d).max(1));
            let pixels =
                common::pixels_from_render(&device, &queue, out.0, out.1, FORMAT, |view| {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    renderer.scanout(&device, &mut encoder, view, id, Some(3..8));
                    queue.submit([encoder.finish()]);
                });
            for y in 0..out.1 {
                for x in 0..out.0 {
                    for channel in 0..4 {
                        let mut total = 0u64;
                        for dy in 0..d {
                            for dx in 0..d {
                                let sx = (x * d + dx).min(width - 1);
                                let sy = 3 * scale + (y * d + dy).min(crop_height - 1);
                                total +=
                                    u64::from(bytes[((sy * width + sx) * 4) as usize + channel]);
                            }
                        }
                        let expected = (total as f64 / f64::from(d).powi(2)).round() as i32;
                        assert!(
                            (i32::from(pixels[((y * out.0 + x) * 4) as usize + channel])
                                - expected)
                                .abs()
                                <= i32::from(d > 1),
                            "scale {scale} d {d} ({x},{y}) channel {channel}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn live_writeback_transitions_keep_bytes_patches_coverage_and_guards() {
    for fallback in [false, true] {
        for encoding in [
            crate::FramebufferEncoding::Native,
            crate::FramebufferEncoding::BigEndian,
        ] {
            let mut baseline = None;
            for changing in [false, true] {
                let mut renderer = facade(1, fallback);
                renderer.set_guest_writeback_with_encoding(true, encoding);
                let mut memory = Memory(vec![0xa5; B as usize + 4096]);
                let mut results = Vec::new();
                renderer.set_output_content_height(481);
                for policy in [
                    ScalePolicy::Fixed(1),
                    ScalePolicy::Fixed(3),
                    ScalePolicy::Fixed(2),
                    ScalePolicy::Fixed(1),
                    ScalePolicy::Auto,
                    ScalePolicy::Fixed(2),
                ] {
                    renderer
                        .set_scale_policy(if changing {
                            policy
                        } else {
                            ScalePolicy::Fixed(1)
                        })
                        .unwrap();
                    let pending_before = renderer.guest_writes.clone();
                    renderer.begin_frame();
                    assert_eq!(renderer.guest_writes, pending_before);
                    apply(&mut renderer, &mut memory);
                    let task = triangle(A, Z, 0x4000, 0x4080c0ff);
                    let (summary, diagnostics) = submit(&mut renderer, &mut memory, &task);
                    assert!(summary.renderable, "{diagnostics:?}");
                    assert!(diagnostics.is_empty());
                    let patches = renderer.guest_writes.clone();
                    let coverage: Vec<_> = (0..32 * 24)
                        .map(|pixel| {
                            renderer
                                .inner
                                .coherence
                                .as_ref()
                                .unwrap()
                                .stored_coverage_code(u64::from(A) + pixel * 2)
                        })
                        .collect();
                    let color = read_texture(
                        &renderer.device,
                        &renderer.queue,
                        &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())].color,
                    );
                    let depth = read_texture(
                        &renderer.device,
                        &renderer.queue,
                        &renderer.inner.depthbuffers[&workload::TargetId::Guest(Z.into())].texture,
                    );
                    results.push((patches, coverage, color, depth));
                    assert_eq!(&memory.0[A as usize - 8..A as usize], &[0xa5; 8]);
                    assert_eq!(&memory.0[A as usize + 32 * 24 * 2..][..8], &[0xa5; 8]);
                }
                apply(&mut renderer, &mut memory);
                let result = (results, memory.0);
                assert_eq!(baseline.get_or_insert_with(|| result.clone()), &result);
            }
        }
    }
}

#[test]
fn auto_minimum_fallback_legacy_and_clipped_crop_references() {
    let mut renderer = facade(1, true);
    renderer.set_scale_policy(ScalePolicy::Auto).unwrap();
    renderer.set_output_content_height(720);
    renderer.begin_frame_with_vi(None).unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(240));
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
    let vi = crate::ViRegisters {
        origin: A,
        width: 64,
        status: 2,
        v_start: 20,
        y_scale: 1024,
        ..Default::default()
    };
    renderer.begin_frame_with_vi(Some(vi)).unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(60));
    assert_eq!(renderer.effective_resolution_multiplier(), 4);
    renderer
        .begin_frame_with_vi(Some(crate::ViRegisters::default()))
        .unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(60));
    let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
    submit(
        &mut renderer,
        &mut memory,
        &fill_task(A, 64, 131, 0x07c107c1),
    );
    renderer
        .begin_frame_with_vi(Some(crate::ViRegisters {
            origin: A + 128 * 61,
            v_start: 400,
            ..vi
        }))
        .unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(70));
    let legacy = common::scene_from_fixture("flat-color--white1");
    let device = renderer.device.clone();
    let queue = renderer.queue.clone();
    renderer
        .inner
        .render_into_store(&device, &queue, &legacy, ClearPolicy::Persist);
    renderer.last_scanout_addr = Some(workload::TargetId::Legacy);
    renderer.set_output_content_height(49);
    renderer.begin_frame_with_vi(Some(vi)).unwrap();
    assert_eq!(renderer.auto_reference_height(), Some(24));
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
}

#[test]
fn auto_upper_bound_rejects_transition_and_task_without_losing_state() {
    let mut renderer = facade(1, true);
    let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
    submit(
        &mut renderer,
        &mut memory,
        &fill_task(A, 64, 64, 0x07c107c1),
    );
    let id = workload::TargetId::Guest(A.into());
    let before = read_texture(
        &renderer.device,
        &renderer.queue,
        &renderer.inner.framebuffers[&id].color,
    );
    let descriptors = renderer.inner.target_descriptors.clone();
    renderer.inner.attachment_budget = 65536;
    renderer.set_scale_policy(ScalePolicy::Auto).unwrap();
    renderer.set_output_content_height(2160);
    let error = renderer.begin_frame_with_vi(None).unwrap_err();
    assert!(matches!(
        error.kind,
        crate::DiagKind::ResourceAdmission {
            reason: crate::diag::ResourceAdmission::AttachmentBudget,
            ..
        }
    ));
    assert_eq!(renderer.effective_resolution_multiplier(), 1);
    assert_eq!(renderer.requested_scale_policy(), ScalePolicy::Auto);
    let (summary, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &fill_task(B, 32, 32, 0xf801f801),
    );
    assert!(!summary.renderable);
    assert_eq!(diagnostics[0].kind, error.kind);
    assert_eq!(renderer.inner.target_descriptors, descriptors);
    assert_eq!(renderer.last_scanout_addr, Some(id));
    assert_eq!(
        read_texture(
            &renderer.device,
            &renderer.queue,
            &renderer.inner.framebuffers[&id].color
        ),
        before
    );
    renderer.inner.attachment_budget = BUDGET;
    renderer.begin_frame_with_vi(None).unwrap();
    assert_eq!(renderer.effective_resolution_multiplier(), 4);
    let (summary, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &fill_task(B, 4096, 2048, 0xf801f801),
    );
    assert!(!summary.renderable);
    assert!(diagnostics
        .iter()
        .any(|d| matches!(d.kind, crate::DiagKind::ResourceAdmission { .. })));
    assert_eq!(renderer.effective_resolution_multiplier(), 4);
    assert_eq!(renderer.inner.target_descriptors, descriptors);
}

#[test]
fn snapshot_taken_at_three_survives_live_switch_to_two() {
    let mut renderer = facade(3, false);
    let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
    let (summary, diagnostics) = submit(&mut renderer, &mut memory, &feedback_task(0));
    assert!(summary.renderable, "{diagnostics:?}");
    let rdp = renderer.rdp.clone();
    let copy = renderer
        .rdp
        .tmem_bank
        .framebuffer_tags
        .as_ref()
        .unwrap()
        .copies[0]
        .clone();
    let id = workload::TargetId::Guest(B.into());
    let before = read_texture(
        &renderer.device,
        &renderer.queue,
        &renderer.inner.framebuffers[&id]
            .presentation
            .as_ref()
            .unwrap()
            .texture,
    );
    renderer.set_scale_policy(ScalePolicy::Fixed(2)).unwrap();
    renderer.begin_frame();
    assert_eq!(renderer.rdp, rdp);
    assert_eq!(renderer.inner.framebuffer_copies.source_scale(&copy, 2), 3);
    let mut commands = vec![
        gdp_set_color_image(0, 2, 32, B),
        gdp_set_scissor(0, 0, 0, 128, 96),
    ];
    commands.extend(gsp_texture_rectangle(
        0, 0, 32, 32, 0, 0, 0, 1024, 1024, false,
    ));
    let (summary, diagnostics) = submit(
        &mut renderer,
        &mut memory,
        &finish(DlBuilder::new(), commands),
    );
    assert!(summary.renderable, "{diagnostics:?}");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let output = read_texture(
        &renderer.device,
        &renderer.queue,
        &renderer.inner.framebuffers[&id]
            .presentation
            .as_ref()
            .unwrap()
            .texture,
    );
    for y in 0..16 {
        for x in 0..16 {
            let sx = ((x as f32 + 0.5) * 1.5).floor() as u32;
            let sy = ((y as f32 + 0.5) * 1.5).floor() as u32;
            assert_eq!(
                &output[((y * 64 + x) * 4) as usize..][..4],
                &before[((sy * 96 + sx) * 4) as usize..][..4]
            );
        }
    }
}

#[test]
fn transitions_keep_queued_resources_alive() {
    let mut renderer = facade(3, true);
    let mut memory = Memory(vec![0; 8 * 1024 * 1024]);
    for scale in [2, 4, 1, 3] {
        let (summary, diagnostics) = submit(
            &mut renderer,
            &mut memory,
            &fill_task(A, 32, 37, 0x07c107c1),
        );
        assert!(summary.renderable, "{diagnostics:?}");
        renderer
            .set_scale_policy(ScalePolicy::Fixed(scale))
            .unwrap();
        renderer.begin_frame();
    }
    let texture = &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())]
        .presentation
        .as_ref()
        .unwrap()
        .texture;
    assert!(read_texture(&renderer.device, &renderer.queue, texture)
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [0, 255, 0, 255]));
}

#[test]
fn large_box_preserves_constant_unorm_bytes() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, FORMAT, 512, 512, dual);
    renderer.render_into_store(
        &device,
        &queue,
        &fill_scene(A, 512, 512, 0x07c107c1),
        ClearPolicy::Persist,
    );
    let texture = &renderer.framebuffers[&workload::TargetId::Guest(A.into())].color;
    let bytes = [224, 240, 160, 255].repeat(512 * 512);
    queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(512 * 4),
            rows_per_image: None,
        },
        texture.size(),
    );
    renderer.reduction_factor = 512;
    let pixels = common::pixels_from_render(&device, &queue, 1, 1, FORMAT, |view| {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.scanout(&device, &mut encoder, view, u64::from(A), None);
        queue.submit([encoder.finish()]);
    });
    assert_eq!(pixels, [224, 240, 160, 255]);
}

#[test]
fn decal_history_survives_fixed_and_auto_boundaries() {
    for fallback in [false, true] {
        let mut renderer = facade(1, fallback);
        let mut draw = decal_scene(true, 0);
        let SceneOp::Tris(base) = &mut draw.framebuffer_pairs[0].ops[0] else {
            unreachable!()
        };
        base.prim_depth.dz = 32;
        let query = draw.framebuffer_pairs[0].ops.pop().unwrap();
        renderer.inner.render_into_store(
            &renderer.device,
            &renderer.queue,
            &draw,
            ClearPolicy::Persist,
        );
        draw.framebuffer_pairs[0].ops = vec![query];
        for (policy, hint, scale, color) in [
            (ScalePolicy::Fixed(2), 0, 2, [0, 255, 0, 255]),
            (ScalePolicy::Auto, 240, 4, [0, 0, 255, 255]),
            (ScalePolicy::Fixed(1), 0, 1, [255, 255, 0, 255]),
        ] {
            renderer.set_scale_policy(policy).unwrap();
            renderer.set_output_content_height(hint);
            renderer
                .begin_frame_with_vi(Some(crate::ViRegisters {
                    origin: A,
                    width: 32,
                    status: 2,
                    v_start: 48,
                    y_scale: 1024,
                    ..Default::default()
                }))
                .unwrap();
            assert_eq!(renderer.effective_resolution_multiplier(), scale);
            draw.materials[1].prim = color;
            renderer.inner.render_into_store(
                &renderer.device,
                &renderer.queue,
                &draw,
                ClearPolicy::Persist,
            );
            let fb = &renderer.inner.framebuffers[&workload::TargetId::Guest(A.into())];
            for (texture, factor) in std::iter::once((&fb.color, 1))
                .chain(fb.presentation.as_ref().map(|p| (&p.texture, scale)))
            {
                let pixels = read_texture(&renderer.device, &renderer.queue, texture);
                let offset = ((3 * factor * texture.width() + 3 * factor) * 4) as usize;
                assert_eq!(&pixels[offset..offset + 4], color, "{policy:?}, {fallback}");
            }
        }
    }
}
