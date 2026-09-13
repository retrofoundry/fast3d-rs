use super::{flat_color_hw, headless_renderer, store_pixels, ImgHw};
use crate::render::workload::TargetId;
use crate::{ClearPolicy, Microcode, NopSink};
use n64_gbi::encode::*;
use std::ops::ControlFlow;

fn hw(commands: impl IntoIterator<Item = (u32, u32)>) -> ImgHw {
    ImgHw {
        rdram: commands
            .into_iter()
            .flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
            .collect(),
    }
}

#[test]
fn triangles_inherit_color_image_including_zero_across_tasks_and_frames() {
    for address in [0, 0x100000] {
        let mut renderer = headless_renderer();
        let setup = hw([
            gdp_set_color_image(0, 2, 64, address),
            gdp_set_scissor(0, 0, 0, 256, 256),
            gsp_enddl(),
        ]);
        let summary = renderer.process_dl(&setup, 0, Microcode::F3dex2, &mut NopSink);
        assert!(!summary.renderable);
        let (draw, entry) = flat_color_hw();
        for task in 0..3 {
            if task == 2 {
                renderer.begin_frame();
            }
            let mut diags = Vec::new();
            let summary = renderer.process_dl(&draw, entry, Microcode::F3dex2, &mut diags);
            assert!(diags.is_empty(), "{diags:?}");
            assert!(summary.renderable);
            let scene = renderer.frame_scenes.last().unwrap();
            assert!(scene.draw_runs.is_empty());
            assert_eq!(scene.framebuffer_pairs.len(), 1);
            assert_eq!(
                scene.framebuffer_pairs[0].color_image.addr,
                u64::from(address)
            );
            assert_eq!(
                renderer.last_scanout_addr,
                Some(TargetId::Guest(address.into()))
            );
            assert!(!renderer.inner.has_fb(TargetId::Legacy));
            assert!(store_pixels(&mut renderer, address.into())
                .as_chunks::<4>()
                .0
                .contains(&[64, 200, 255, 255]));
        }
    }
}

#[test]
fn inherited_fill_and_scissor_respect_clear_policy() {
    for address in [0, 0x100000] {
        for policy in [ClearPolicy::PerFrame, ClearPolicy::Persist] {
            let mut renderer = headless_renderer();
            renderer.config.clear_policy = policy;
            renderer.process_dl(
                &hw([
                    gdp_set_color_image(0, 2, 64, address),
                    gdp_set_scissor(0, 0, 0, 256, 256),
                    gdp_set_fill_color(0xf801f801),
                    gdp_fill_rectangle(0, 0, 252, 252),
                    gdp_set_fill_color(0x07c107c1),
                    gdp_set_scissor(0, 0, 0, 128, 256),
                    gsp_enddl(),
                ]),
                0,
                Microcode::F3dex2,
                &mut NopSink,
            );
            for task in 0..3 {
                if task == 2 {
                    renderer.begin_frame();
                }
                let mut diags = Vec::new();
                let summary = renderer.process_dl(
                    &hw([gdp_fill_rectangle(0, 0, 252, 252), gsp_enddl()]),
                    0,
                    Microcode::F3dex2,
                    &mut diags,
                );
                assert!(diags.is_empty(), "{diags:?}");
                assert!(summary.renderable);
                let pixels = store_pixels(&mut renderer, address.into());
                assert_eq!(&pixels[(32 * 64 + 16) * 4..][..4], [0, 255, 0, 255]);
                let right = if task == 2 && policy == ClearPolicy::PerFrame {
                    [13, 13, 20, 255]
                } else {
                    [255, 0, 0, 255]
                };
                assert_eq!(&pixels[(32 * 64 + 48) * 4..][..4], right);
            }
        }
    }
}

#[test]
fn tmem_tiles_and_registers_survive_without_reloading_guest_memory() {
    for via_tile in [false, true] {
        let mut builder = crate::tests::dl_builder::DlBuilder::new();
        let texture = builder.bytes(8, &[0xf8, 1].repeat(4));
        let palette = builder.bytes(8, &[0x07, 0xc1].repeat(16));
        builder.list(
            "setup",
            &[
                gdp_set_color_image(0, 2, 64, 0),
                gdp_set_depth_image(0x200000),
                gdp_set_scissor(0, 0, 0, 256, 256),
                gdp_set_cycle_type(2),
                gdp_set_render_mode(
                    n64_gbi::consts::G_RM_OPA_SURF,
                    n64_gbi::consts::G_RM_OPA_SURF2,
                ),
                (0xfcffffff, 0xfffcf279),
                gdp_set_texture_image(0, 2, 1, palette),
                gdp_set_tile(0, 2, 0, 0x100, 7, 0, 0, 0, 0, 0, 0, 0),
                gdp_load_tlut(7, 15),
                gdp_set_texture_image(0, 2, 4, texture),
                gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
                if via_tile {
                    (0xf4000000, 12 << 12)
                } else {
                    gdp_load_block(0, 0, 0, 3, 0)
                },
                gdp_set_tile_size(0, 0, 0, 12, 0),
                gdp_set_prim_color(7, 128, 0x12345678),
                gdp_set_env_color(0x23456789),
                gdp_set_fog_color(0x3456789a),
                gdp_set_blend_color(0x456789ab),
                gdp_set_fill_color(0x56789abc),
                gdp_set_prim_depth(123, 45),
                gdp_set_convert(1, -2, 3, -4, 5, -6),
                gdp_set_key_r(7, 8, 9),
                gdp_set_key_gb(10, 11, 12, 13, 14, 15),
                gsp_enddl(),
            ],
        );
        let setup = builder.finish("setup");
        let mut renderer = headless_renderer();
        let mut diags = Vec::new();
        renderer.process_dl(
            &ImgHw { rdram: setup.rdram },
            setup.entry.into(),
            Microcode::F3dex2,
            &mut diags,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let mut expected = renderer.rdp.clone();
        expected.color_changed = false;
        expected.depth_changed = false;
        assert_eq!(expected.load_via_tile, via_tile);
        assert!(expected.texture_loaded);
        assert_eq!(
            &expected.tmem_bank.palette()[..128],
            [0x07, 0xc1].repeat(64)
        );
        assert_eq!(expected.prim, [0x12, 0x34, 0x56, 0x78]);
        assert_eq!(expected.convert, [1, -2, 3, -4, 5, -6]);
        let draw = hw(
            gsp_texture_rectangle(0, 0, 16, 4, 0, 0, 0, 4096, 1024, false)
                .into_iter()
                .chain([gsp_enddl()]),
        );
        for task in 0..3 {
            if task == 2 {
                renderer.begin_frame();
            }
            let summary = renderer.process_dl(&draw, 0, Microcode::F3dex2, &mut diags);
            assert!(diags.is_empty(), "{diags:?}");
            assert!(summary.renderable);
            assert_eq!(renderer.rdp, expected);
            assert_eq!(&store_pixels(&mut renderer, 0)[4..8], [255, 0, 0, 255]);
        }
    }
}

#[test]
fn rsp_fog_factors_reset_between_tasks_while_rdp_fog_color_persists() {
    let mut renderer = headless_renderer();
    renderer.process_dl(
        &hw([
            gsp_fog_position_f3d(800, 1000),
            gdp_set_fog_color(0x12345678),
            gsp_enddl_f3d(),
        ]),
        0,
        Microcode::F3d,
        &mut NopSink,
    );
    let mut builder = crate::tests::dl_builder::DlBuilder::new();
    let vertex = builder.bytes(16, &[0; 16]);
    builder.list(
        "load",
        &[
            gsp_set_geometrymode(n64_gbi::consts::G_FOG),
            gsp_vertex(0, 1, vertex),
            gsp_enddl(),
        ],
    );
    let load = builder.finish("load");
    renderer.process_dl(
        &ImgHw { rdram: load.rdram },
        load.entry.into(),
        Microcode::F3dex2,
        &mut NopSink,
    );
    assert_eq!(renderer.frame_scenes.last().unwrap().fog_table, [[0, 0]]);
    assert_eq!(renderer.rdp.fog_color, [0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn explicit_scissor_remains_established_for_inherited_legacy_triangles() {
    let mut renderer = headless_renderer();
    renderer.process_dl(
        &hw([gdp_set_scissor(0, 32, 48, 128, 160), gsp_enddl()]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    let (draw, entry) = flat_color_hw();
    for _ in 0..2 {
        renderer.begin_frame();
        renderer.process_dl(&draw, entry, Microcode::F3dex2, &mut NopSink);
        let scene = renderer.frame_scenes.last().unwrap();
        assert!(scene.framebuffer_pairs.is_empty());
        assert_eq!(
            scene.draw_origins[0].scissor,
            crate::scene::Scissor {
                ulx: 8,
                uly: 12,
                lrx: 32,
                lry: 40,
                mode: 0,
            }
        );
    }
}

#[test]
fn legacy_routing_lasts_until_color_image_is_established_or_state_is_reset() {
    let mut renderer = headless_renderer();
    let (draw, entry) = flat_color_hw();
    for task in 0..3 {
        if task == 2 {
            renderer.begin_frame();
        }
        renderer.process_dl(&draw, entry, Microcode::F3dex2, &mut NopSink);
        assert_eq!(renderer.last_scanout_addr, Some(TargetId::Legacy));
        assert!(renderer
            .frame_scenes
            .last()
            .unwrap()
            .framebuffer_pairs
            .is_empty());
    }
    renderer.process_dl(
        &hw([gdp_set_color_image(0, 2, 64, 0), gsp_enddl()]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    renderer.reset_rdp_state();
    renderer.process_dl(&draw, entry, Microcode::F3dex2, &mut NopSink);
    assert_eq!(renderer.last_scanout_addr, Some(TargetId::Legacy));
    assert!(!renderer.inner.has_fb(0));
}

#[test]
fn cancelled_and_faulted_tasks_discard_register_changes() {
    let mut renderer = headless_renderer();
    renderer.process_dl(
        &super::fill_hw(0, 0xf801f801),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    let before = renderer.rdp.clone();
    let scenes = renderer.frame_scenes.clone();
    let pixels = store_pixels(&mut renderer, 0);
    let changed = hw([
        gdp_set_color_image(0, 2, 32, 0x200000),
        gdp_set_fill_color(0x07c107c1),
        gsp_enddl(),
    ]);
    let summary = renderer.process_dl_observed(
        &changed,
        0,
        Microcode::F3dex2,
        &mut NopSink,
        &mut |step: crate::inspect::WalkStep<'_>| {
            if step.seq == 1 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );
    assert_eq!(
        summary.termination,
        crate::inspect::WalkTermination::ObserverStopped
    );
    assert!(!summary.renderable);
    assert_eq!(renderer.rdp, before);
    assert_eq!(renderer.frame_scenes, scenes);
    assert_eq!(store_pixels(&mut renderer, 0), pixels);
    let summary = renderer.process_dl(
        &hw([gdp_set_fill_color(0x07c107c1)]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    assert_eq!(summary.termination, crate::inspect::WalkTermination::Bounds);
    assert_eq!(renderer.rdp, before);
    renderer.process_dl(
        &hw([gdp_fill_rectangle(0, 0, 252, 252), gsp_enddl()]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    assert_eq!(store_pixels(&mut renderer, 0), pixels);
}

#[test]
fn prefixes_start_from_defaults_without_changing_live_registers() {
    let mut renderer = headless_renderer();
    let mut fresh = headless_renderer();
    renderer.process_dl(
        &super::fill_hw(0, 0xf801f801),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    renderer.process_dl(
        &hw([gdp_set_convert(1, 2, 3, 4, 5, 6), gsp_enddl()]),
        0,
        Microcode::F3dex2,
        &mut NopSink,
    );
    let before = renderer.rdp.clone();
    let (draw, entry) = flat_color_hw();
    for count in [1, u32::MAX, 2, u32::MAX] {
        renderer.begin_frame();
        fresh.begin_frame();
        let (mut actual_diags, mut expected_diags) = (Vec::new(), Vec::new());
        let actual =
            renderer.process_dl_prefix(&draw, entry, Microcode::F3dex2, &mut actual_diags, count);
        let expected =
            fresh.process_dl_prefix(&draw, entry, Microcode::F3dex2, &mut expected_diags, count);
        assert_eq!(actual, expected);
        assert_eq!(actual_diags, expected_diags);
        assert_eq!(renderer.frame_scenes, fresh.frame_scenes);
        assert_eq!(renderer.rdp, before);
        if expected.renderable {
            assert_eq!(
                super::target_pixels(&mut renderer, TargetId::Legacy),
                super::target_pixels(&mut fresh, TargetId::Legacy)
            );
        }
    }
}

#[cfg(feature = "capture")]
#[test]
fn reset_makes_backward_inspector_prefixes_and_dither_repeatable() {
    use crate::capture::ReplayHardware;
    use crate::{PresentTarget, Renderer};
    let fixture = crate::tests::alpha_dither_fixture::fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let make_renderer = || {
        Renderer::with_device(
            device.clone(),
            queue.clone(),
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 320,
                height: 240,
            },
            fixture.frame.config,
        )
    };
    let task = &fixture.tasks[0];
    let hardware = ReplayHardware::new(task, None).unwrap();
    let pixels = |renderer: &mut Renderer| {
        crate::tests::common::pixels_from_render(
            &device,
            &queue,
            320,
            240,
            wgpu::TextureFormat::Rgba8Unorm,
            |view| renderer.present_last_to(view),
        )
    };
    let mut renderer = make_renderer();
    for count in [u32::MAX, 4, u32::MAX, 1, u32::MAX] {
        renderer.process_dl(
            &super::fill_hw(0, 0xf801f801),
            0,
            Microcode::F3dex2,
            &mut NopSink,
        );
        renderer.begin_frame();
        renderer.inner.dither_seed = 123;
        renderer.reset();
        assert!(!renderer.inner.has_fb(0));
        assert!(renderer.frame_scenes.is_empty());
        assert_eq!(renderer.last_scanout_addr, None);
        let mut fresh = make_renderer();
        let (mut actual_diags, mut expected_diags) = (Vec::new(), Vec::new());
        let actual = renderer.process_dl_prefix(
            &hardware,
            task.entry,
            task.microcode,
            &mut actual_diags,
            count,
        );
        let expected = fresh.process_dl_prefix(
            &hardware,
            task.entry,
            task.microcode,
            &mut expected_diags,
            count,
        );
        assert_eq!(actual, expected);
        assert_eq!(actual_diags, expected_diags);
        assert_eq!(renderer.frame_scenes, fresh.frame_scenes);
        assert_eq!(pixels(&mut renderer), pixels(&mut fresh));
        assert_eq!(renderer.rdp, Default::default());
    }
}
