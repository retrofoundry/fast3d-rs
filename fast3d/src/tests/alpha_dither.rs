use super::common::{render_to_pixels, scene_from_fixture};
use crate::hle::{AlphaCompare, BlendClass, RenderMode, Scene, ZMode};
use crate::render::{headless_device_forced_fallback, CombinerUniform, SceneRenderer};

const FRAME_0: [u64; 32] = [
    0x57b9a53c07ec2b4e,
    0x3f50f0b8da0da95d,
    0x2ed9e4276bf7ca3b,
    0x96ad945254fa45d2,
    0xd06e6f12e1f0b5af,
    0xd5343d072772b2bc,
    0xc015534a6267efde,
    0xcf904c4e556d7cc3,
    0x536ffa2a008c8c74,
    0xa60270b9ffead714,
    0xfcce93f90a7e43ed,
    0xa54179249377d844,
    0x9870d73ea4c98911,
    0xcd03e46bbeb5a623,
    0xab05c20601f99b37,
    0x0eb4ca68a4d51312,
    0xf00cabcb5acf72b3,
    0xafddd221684898b9,
    0xdb577b42afb69a63,
    0xf854491528a53ac8,
    0xa546ffa71f2539f5,
    0x15f5de346cc9204a,
    0xe50ecf4fd752d8e0,
    0xc47d082ae316cf84,
    0x91e4ecc7e7e48079,
    0x1152218ae4460965,
    0x1f2b550f625ccda2,
    0xdce186f5f56b0849,
    0x8361710806bd1961,
    0xff19ac9ed9d2f8e6,
    0xa9ff6539c769b817,
    0xd06d72bfe767df63,
];

const FRAME_1: [u64; 32] = [
    0x5a57a9f280f9d785,
    0x63975ae4a2d93d1e,
    0x64b61986ea86a0cc,
    0xf7f03e1ddeb8e2fb,
    0x1ec05c675ef6d739,
    0x17de7c69166731c2,
    0x1d0de7500b34dd30,
    0x9ab5acc6b6dc627a,
    0x406ed325ee54324e,
    0x192ba42188300b65,
    0x44a7f1aafbffe242,
    0xe65b2fa41b9e2048,
    0x0666cedac22784be,
    0xe3f081e7b9d3bbe5,
    0xaa6e75e397271cbe,
    0x7776ebb4f1d504f9,
    0xc1844514829ee1f3,
    0x197f9571e341c1e6,
    0xd1f8daed0af8fb08,
    0xf7ecedb3a93c0e4f,
    0x9ec4add5b06dcc6a,
    0x49f1c9633d4aaa89,
    0x07c1edeeede7902a,
    0x430c0c5f027a3419,
    0x7657fe12afeb4e73,
    0xf5a025116d1e6877,
    0xec0850638fa369f4,
    0xef01c82920f97b11,
    0x60a00eddf9f72141,
    0xf4294f8e2da2a33d,
    0xaa02e2959e5f7153,
    0x1bbf4d3781ed5b20,
];

fn scene(alpha: f32) -> Scene {
    let mut scene = scene_from_fixture("framebuffer-extent--white1");
    scene.raw_pos = vec![
        [-64.0, 64.0, 0.0],
        [-64.0, -64.0, 0.0],
        [64.0, -64.0, 0.0],
        [64.0, 64.0, 0.0],
    ];
    let mat = &mut scene.materials[0];
    mat.selectors = crate::hle::combiner::decode_combine(0x00ff_ffff, 0xffd8_feff);
    mat.prim = [255, 0, 0, 255];
    mat.prim_lod_frac = alpha;
    scene.render_modes = vec![RenderMode {
        alpha_compare: AlphaCompare::Dither,
        ..Default::default()
    }];
    scene
}

fn mask(pixels: &[u8]) -> [u64; 32] {
    std::array::from_fn(|y| {
        (0..64).fold(0, |row, x| {
            row | (u64::from(pixels[(y * 64 + x) * 4] > 13) << x)
        })
    })
}

fn render(
    renderer: &mut SceneRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
) -> Vec<u8> {
    render_to_pixels(device, queue, renderer, scene, 64, 32)
}

#[test]
fn alpha_compare_flags_are_independent() {
    let scene = scene(0.5);
    for (compare, coverage, flags) in [
        (AlphaCompare::Dither, false, 3),
        (AlphaCompare::Dither, true, 7),
        (AlphaCompare::Threshold, true, 5),
        (AlphaCompare::None, true, 4),
    ] {
        let rm = RenderMode {
            alpha_compare: compare,
            cvg_x_alpha: coverage,
            ..Default::default()
        };
        let uniform = CombinerUniform::from_run(&scene.materials[0], &rm, [0; 4]);
        let words: &[u32] = bytemuck::cast_slice(bytemuck::bytes_of(&uniform));
        assert_eq!(words[6], flags, "{compare:?}, coverage={coverage}");
    }
}

#[test]
fn alpha_copy_preserves_dither_mode() {
    let rm = RenderMode {
        alpha_compare: AlphaCompare::Dither,
        ..Default::default()
    };
    let uniform = CombinerUniform::tex_copy(Some(&rm), 0);
    let words: &[u32] = bytemuck::cast_slice(bytemuck::bytes_of(&uniform));
    assert_eq!(words[6], 3);
}

#[test]
fn alpha_copy_coverage_keeps_format_gate() {
    let rm = RenderMode {
        cvg_x_alpha: true,
        ..Default::default()
    };
    let uniform = CombinerUniform::tex_copy(Some(&rm), 4);
    assert_eq!(uniform.alpha_flags & 4, 0);
}

#[test]
fn alpha_dither_copy_uses_texel_alpha_and_framebuffer_extent() {
    use crate::hle::{ColorImage, FramebufferPair, SceneOp, Scissor, TexRectBounds};
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 128, 64, false);
    for (copy, coverage, alpha) in [(true, false, 128), (true, true, 64), (false, false, 128)] {
        let mut scene = scene(0.5);
        scene.draw_runs.clear();
        scene.render_modes[0].cvg_x_alpha = coverage;
        let mat = &mut scene.materials[0];
        mat.texture = vec![255, 0, 0, alpha];
        mat.tex_w = 1;
        mat.tex_h = 1;
        mat.tex_enable = true;
        mat.fmt = 0;
        mat.sampling = Default::default();
        scene.framebuffer_pairs = vec![FramebufferPair {
            color_image: ColorImage {
                fmt: 0,
                siz: 3,
                width: 64,
                addr: 0x100000,
            },
            active_scissor: Scissor {
                lrx: 64,
                lry: 32,
                ..Default::default()
            },
            size_extent: (64, 32),
            ops: vec![SceneOp::TexRect {
                rect: TexRectBounds {
                    ulx: 0,
                    uly: 0,
                    lrx: if copy { 252 } else { 256 },
                    lry: if copy { 124 } else { 128 },
                },
                tile: 0,
                uls: 0,
                ult: 0,
                dsdx: 0,
                dtdy: 0,
                flip: false,
                copy_mode: copy,
                material_index: 0,
                render_mode_index: 0,
                fog_color: [0; 4],
                prim_depth: Default::default(),
                fb_source: None,
            }],
            ..Default::default()
        }];
        let mut expected = if coverage { [0; 32] } else { FRAME_0 };
        if copy && !coverage {
            for (x, y) in [
                (38, 5),
                (29, 7),
                (32, 11),
                (41, 11),
                (42, 15),
                (34, 18),
                (37, 19),
                (31, 21),
                (48, 24),
                (20, 27),
            ] {
                expected[y] |= 1 << x;
            }
        }
        assert_eq!(
            mask(&render(&mut renderer, &device, &queue, &scene)),
            expected,
            "copy={copy}, coverage={coverage}"
        );
    }
}

#[test]
fn alpha_frame_uniform_fits_slot() {
    assert_eq!(std::mem::size_of::<CombinerUniform>(), 176);
    assert!(std::mem::size_of::<CombinerUniform>() <= 256);
}

#[cfg(feature = "capture")]
#[test]
fn alpha_dither_seed_replays_exactly() {
    use super::alpha_dither_fixture::{fixture, HALF_MASK, REGION};
    let (device, queue) = headless_device_forced_fallback();
    let mut original = fixture();
    original.frame.serial = 0x1_0000_1234;
    original.frame.dither_seed = 0x1234;
    let encoded = original.to_bytes().unwrap();
    let first = pollster::block_on(original.replay(device.clone(), queue.clone())).unwrap();
    drop(original);
    let restored = crate::capture::Fixture::from_bytes(&encoded).unwrap();
    let second = pollster::block_on(restored.replay(device.clone(), queue.clone())).unwrap();
    assert_eq!(first.rgba8, second.rgba8);
    let [x, y, _, _] = REGION;
    for (row, expected) in HALF_MASK.iter().enumerate() {
        let got = (0..32).fold(0u32, |mask, col| {
            mask | (u32::from(first.rgba8[((y + row) * 320 + x + col) * 4] != 0) << col)
        });
        assert_eq!(got, *expected, "row {row}");
    }
    let mut changed = restored;
    changed.frame.dither_seed ^= 1;
    let third = pollster::block_on(changed.replay(device, queue)).unwrap();
    assert_ne!(first.rgba8, third.rgba8);
}

#[cfg(feature = "capture")]
#[test]
fn alpha_dither_capture_records_renderer_frame_count() {
    use crate::capture::{CaptureFrame, Provenance, ReplayHardware};
    let fixture = super::alpha_dither_fixture::fixture();
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = crate::Renderer::with_device(
        device.clone(),
        queue.clone(),
        crate::PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 320,
            height: 240,
        },
        fixture.frame.config,
    );
    renderer.begin_frame();
    renderer.begin_frame();
    renderer.reconfigure(fixture.frame.config);
    let mut capture = CaptureFrame::begin(&mut renderer, 999, 3, Provenance::default());
    let task = &fixture.tasks[0];
    let hardware = ReplayHardware::new(task, None).unwrap();
    let mut diagnostics = Vec::new();
    capture
        .process_dl(
            &mut renderer,
            &hardware,
            task.entry,
            task.microcode,
            task.data_format,
            &mut diagnostics,
        )
        .unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let mut recorded = None;
    let live = super::common::pixels_from_render(
        &device,
        &queue,
        320,
        240,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| {
            recorded = Some(capture.present_to(&mut renderer, &hardware, view).unwrap());
        },
    );
    let recorded = recorded.unwrap();
    assert_eq!(recorded.frame.serial, 3);
    assert_eq!(recorded.frame.dither_seed, 3);
    let replayed = pollster::block_on(recorded.replay(device, queue)).unwrap();
    assert_eq!(live, replayed.rgba8);
    assert_eq!(
        &live[(104 * 320 + 145) * 4..(104 * 320 + 146) * 4],
        &[0, 0, 0, 255]
    );
}

#[test]
fn alpha_dither_zero_half_one() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, false);
    let zero = render(&mut renderer, &device, &queue, &scene(0.0));
    assert_eq!(mask(&zero), [0; 32]);
    assert!(zero
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [13, 13, 20, 255]));
    let half = render(&mut renderer, &device, &queue, &scene(0.5));
    assert_eq!(mask(&half), FRAME_0);
    assert_eq!(FRAME_0.iter().map(|r| r.count_ones()).sum::<u32>(), 1045);
    for (i, pixel) in half.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(
            *pixel,
            if FRAME_0[i / 64] & (1 << (i % 64)) != 0 {
                [255, 0, 0, 128]
            } else {
                [13, 13, 20, 255]
            }
        );
    }
    let one = render(&mut renderer, &device, &queue, &scene(1.0));
    assert_eq!(mask(&one), [u64::MAX; 32]);
    assert!(one
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
    let mut previous = [0; 32];
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        let next = mask(&render(&mut renderer, &device, &queue, &scene(alpha)));
        for (a, b) in previous.iter().zip(next) {
            assert_eq!(a & !b, 0);
        }
        previous = next;
    }
}

#[test]
fn alpha_dither_changes_with_frame() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, false);
    assert_eq!(
        mask(&render(&mut renderer, &device, &queue, &scene(0.5))),
        FRAME_0
    );
    renderer.begin_frame();
    assert_eq!(
        mask(&render(&mut renderer, &device, &queue, &scene(0.5))),
        FRAME_1
    );
    assert_ne!(FRAME_0, FRAME_1);
}

#[test]
fn alpha_dither_is_invariant_to_run_splitting() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, false);
    let mut scene = scene(0.5);
    let whole = render(&mut renderer, &device, &queue, &scene);
    assert_eq!(mask(&whole), FRAME_0);
    let mut second = scene.draw_runs[0];
    second.index_start += 3;
    second.index_count = 3;
    scene.draw_runs[0].index_count = 3;
    scene.draw_runs.push(second);
    assert_eq!(render(&mut renderer, &device, &queue, &scene), whole);
}

#[test]
fn alpha_compare_uses_first_cycle_alpha() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, false);
    for (first, last, survives) in [(0, 255, false), (255, 0, true)] {
        let mut scene = scene(0.5);
        let mat = &mut scene.materials[0];
        mat.cycle_type = 1;
        mat.selectors = crate::hle::combiner::decode_combine(0x00ff_ffff, 0xfffd_f6fd);
        mat.prim[3] = first;
        mat.env[3] = last;
        mat.blend_color[3] = 128;
        scene.render_modes[0].alpha_compare = AlphaCompare::Threshold;
        let pixels = render(&mut renderer, &device, &queue, &scene);
        assert_eq!(
            mask(&pixels),
            if survives { [u64::MAX; 32] } else { [0; 32] }
        );
        assert_eq!(
            &pixels[..4],
            if survives {
                &[255, 0, 0, 0]
            } else {
                &[13, 13, 20, 255]
            }
        );
    }
}

#[test]
fn alpha_compare_and_cvg_x_alpha_both_apply() {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, false);
    for (alpha, threshold, survives) in [(0.5, 192, false), (0.0625, 0, false), (0.5, 64, true)] {
        let mut scene = scene(alpha);
        scene.materials[0].blend_color[3] = threshold;
        scene.render_modes[0].alpha_compare = AlphaCompare::Threshold;
        scene.render_modes[0].cvg_x_alpha = true;
        assert_eq!(
            mask(&render(&mut renderer, &device, &queue, &scene)),
            if survives { [u64::MAX; 32] } else { [0; 32] }
        );
    }
}

#[test]
#[ignore = "requires a DUAL_SOURCE_BLENDING adapter"]
fn alpha_dither_dualsrc_matches_fallback_mask() {
    let (device, queue, dual) = crate::render::headless_device();
    assert!(dual, "requires dual-source blending");
    let (fallback_device, fallback_queue) = headless_device_forced_fallback();
    let mut dual_renderer =
        SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 32, true);
    let mut fallback_renderer = SceneRenderer::new(
        &fallback_device,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        32,
        false,
    );
    for decal in [false, true] {
        let mut scene = scene(0.5);
        scene.render_modes[0].blender_mux = 0x0040;
        scene.render_modes[0].force_blend = true;
        scene.render_modes[0].blend_class = BlendClass::DualSrc;
        scene.render_modes[0].fallback_class = BlendClass::AlphaOver;
        if decal {
            scene.render_modes[0].z_mode = ZMode::Decal;
            scene.render_modes[0].z_test = true;
            for (scale, translate) in &mut scene.viewport_table {
                scale[2] = 0.0;
                translate[2] = 1.0;
            }
        }
        let dual = render(&mut dual_renderer, &device, &queue, &scene);
        let fallback = render(
            &mut fallback_renderer,
            &fallback_device,
            &fallback_queue,
            &scene,
        );
        assert_eq!(mask(&dual), FRAME_0, "decal={decal}");
        assert_eq!(mask(&fallback), FRAME_0, "decal={decal}");
    }
}
