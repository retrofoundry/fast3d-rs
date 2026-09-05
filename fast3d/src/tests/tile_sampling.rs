use super::common::{pixel, render_to_pixels, scene_from_source};
use crate::hle::combiner::build_material;
use crate::hle::rdp::{Rdp, TileDescriptor};
use crate::hle::rsp::Rsp;
use crate::hle::Scene;
use crate::render::{headless_device, SceneRenderer};

fn tile() -> TileDescriptor {
    TileDescriptor {
        fmt: 4,
        siz: 1,
        width: 8,
        height: 4,
        lrs: 28,
        lrt: 12,
        line: 1,
        masks: 3,
        maskt: 2,
        ..Default::default()
    }
}

fn rdp(tile: TileDescriptor) -> Rdp {
    let mut rdp = Rdp {
        combine_l: 0x00ff_ffff,
        combine_h: 0xfffc_f27c,
        load_via_tile: true,
        tmem: vec![1; 4096],
        ..Default::default()
    };
    rdp.tiles[0] = tile;
    let bytes: Vec<_> = (0..4)
        .flat_map(|y| (0..8).map(move |x| 16 * x + 32 * y))
        .collect();
    rdp.tmem_bank.write_tile(&bytes, 0, 1, 4, 1, 8, 1);
    rdp
}

fn scene(rdp: &Rdp, uv: [f32; 2], levels: u8) -> Scene {
    scene_for_base(rdp, uv, levels, 0)
}

fn scene_for_base(rdp: &Rdp, uv: [f32; 2], levels: u8, base: u8) -> Scene {
    let mut scene = scene_from_source("framebuffer-extent.n64", &[255; 4], 1, 1);
    let mut rsp = Rsp::default();
    rsp.set_texture(base, levels, true, 65535, 65535);
    let mut diags = Vec::new();
    scene.materials = vec![build_material(rdp, &rsp, &mut diags, 0).unwrap()];
    assert!(diags.is_empty(), "{diags:?}");
    scene.raw_st.fill(uv);
    scene.texcoord_table.fill([1.0; 2]);
    scene
}

fn assert_probe(scene: &Scene, expected: [u8; 4]) {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
    let pixels = render_to_pixels(&device, &queue, &mut renderer, scene, 320, 240);
    assert_eq!(pixel(&pixels, 320, 100, 80), expected);
}

#[test]
fn tile_origin_is_applied_after_shift_pixels() {
    assert_probe(
        &scene(
            &rdp(TileDescriptor {
                uls: 4,
                ult: 8,
                lrs: 32,
                lrt: 20,
                shifts: 1,
                shiftt: 15,
                ..tile()
            }),
            [9.0, 1.25],
            0,
        ),
        [48, 48, 48, 255],
    );
}

#[test]
fn tile_shift_all_16_values_pixels() {
    let scales = [
        1.0,
        0.5,
        0.25,
        0.125,
        0.0625,
        0.03125,
        0.015625,
        0.0078125,
        0.00390625,
        0.001953125,
        0.0009765625,
        32.0,
        16.0,
        8.0,
        4.0,
        2.0,
    ];
    for (shift, scale) in scales.into_iter().enumerate() {
        assert_probe(
            &scene(
                &rdp(TileDescriptor {
                    shifts: shift as u8,
                    shiftt: shift as u8,
                    ..tile()
                }),
                [2.5 / scale, 1.5 / scale],
                0,
            ),
            [64, 64, 64, 255],
        );
    }
}

#[test]
fn tile_repeat_extent_preserves_bilinear_pixels() {
    for (mode, uv) in [(0, [3.25, 1.75]), (0, [11.25, 5.75]), (1, [12.75, 6.25])] {
        assert_probe(
            &scene(
                &rdp(TileDescriptor {
                    cms: mode,
                    cmt: mode,
                    ..tile()
                }),
                uv,
                0,
            ),
            [84, 84, 84, 255],
        );
    }
}

#[test]
fn tile_mask_zero_clamps_pixels() {
    assert_probe(
        &scene(
            &rdp(TileDescriptor {
                masks: 0,
                maskt: 0,
                ..tile()
            }),
            [9.5, 5.5],
            0,
        ),
        [208, 208, 208, 255],
    );
}

#[test]
fn tile_mask_period_differs_from_image_extent_pixels() {
    let state = rdp(TileDescriptor {
        masks: 2,
        maskt: 1,
        ..tile()
    });
    assert_probe(&scene(&state, [4.5, 2.5], 0), [0, 0, 0, 255]);
    assert_probe(&scene(&state, [3.75, 1.75], 0), [60, 60, 60, 255]);
}

#[test]
fn tile_negative_wrap_and_mirror_pixels() {
    for (mode, uv, expected) in [(0, [-1.5, -0.5], 64), (1, [-5.5, -2.5], 64)] {
        assert_probe(
            &scene(
                &rdp(TileDescriptor {
                    cms: mode,
                    cmt: mode,
                    masks: 2,
                    maskt: 1,
                    ..tile()
                }),
                uv,
                0,
            ),
            [expected, expected, expected, 255],
        );
    }
}

#[test]
fn tile_clamp_precedes_mask_for_each_tap_pixels() {
    assert_probe(
        &scene(
            &rdp(TileDescriptor {
                cms: 3,
                cmt: 2,
                masks: 2,
                maskt: 1,
                ..tile()
            }),
            [8.0, 4.0],
            0,
        ),
        [32, 32, 32, 255],
    );
}

#[test]
fn tile_large_mask_uses_bounded_tmem_lookup_pixels() {
    for (fmt, siz, expected) in [
        (4, 0, [187, 187, 187, 255]),
        (4, 1, [0xab, 0xab, 0xab, 255]),
        (0, 3, [0x12, 0x34, 0x56, 0x78]),
    ] {
        let mut state = rdp(TileDescriptor {
            fmt,
            siz,
            masks: 15,
            maskt: 15,
            line: 511,
            tmem_addr: 511,
            ..tile()
        });
        if siz == 3 {
            state.combine_h = 0xfffc_f279;
        }
        let mut bytes = vec![0; 4096];
        // (32767, 1): row-relative offset wraps; odd-row XOR precedes base addition.
        let rel = match siz {
            0 => 4088 + 16383,
            1 => 4088 + 32767,
            _ => 4088 + 65534,
        };
        let mask = if siz == 3 { 2047 } else { 4095 };
        let addr = (4088 + (rel ^ 4)) & mask;
        bytes[addr] = if siz == 3 { 0x12 } else { 0xab };
        if siz == 3 {
            bytes[(4088 + ((rel + 1) ^ 4)) & mask] = 0x34;
            bytes[addr | 2048] = 0x56;
            bytes[((4088 + ((rel + 1) ^ 4)) & mask) | 2048] = 0x78;
        }
        state.tmem_bank.write_block(&bytes, 0, 0, 0, 512, 1);
        let scene = scene(&state, [32767.5, 1.5], 0);
        assert_eq!(scene.materials[0].texture.len(), 65536);
        assert_probe(&scene, expected);
    }
}

fn lod_scene() -> Scene {
    let mut state = rdp(TileDescriptor {
        width: 32,
        height: 32,
        lrs: 124,
        lrt: 124,
        line: 4,
        masks: 5,
        maskt: 5,
        ..tile()
    });
    state.other_mode_h = (1 << 20) | (1 << 16);
    // Cycle 0 reads physical 1; cycle 1 passes COMBINED through.
    state.combine_l = 0x00ff_ffff;
    state.combine_h = 0xfffd_0838;
    state.tiles[1] = TileDescriptor {
        uls: 8,
        ult: 12,
        lrs: 132,
        lrt: 136,
        shifts: 1,
        shiftt: 15,
        tmem_addr: 128,
        ..state.tiles[0].clone()
    };
    let bytes: Vec<_> = (0..32)
        .flat_map(|y| (0..32).map(move |x| x + 4 * y))
        .collect();
    state.tmem_bank.write_tile(&bytes, 0, 4, 32, 4, 32, 1);
    state.tmem_bank.write_tile(&bytes, 128, 4, 32, 4, 32, 1);
    let mut scene = scene(&state, [13.0, 8.75], 1);
    for (st, pos) in scene.raw_st.iter_mut().zip(&scene.raw_pos) {
        let screen_x = pos[0] * 2.5 + 160.0;
        st[0] = 13.0 + (screen_x - 100.5) * 1.5;
    }
    scene
}

#[test]
fn lod_texel1_only_preserves_texel_coordinates() {
    let scene = lod_scene();
    let mat = &scene.materials[0];
    assert!(!mat.tex_enable);
    assert!(mat.lod);
    assert_eq!(
        crate::render::triangle_inv_tex_size(mat),
        [1.0 / 32.0, 1.0 / 32.0, 0.0, 0.0]
    );
    let tiles = crate::render::material_sampling(mat);
    assert_eq!(tiles[0].image, [32, 32, 0, 0]);
    assert_eq!(tiles[3].bounds, [8, 12, 132, 136]);
    assert_eq!(tiles[3].shift_mask, [1, 15, 5, 5]);
    assert_eq!(
        pixel(&mat.mip_levels[1].texture, 32, 4, 14),
        [60, 60, 60, 60]
    );
}

#[test]
fn lod_tiles_apply_independent_origin_and_shift_pixels() {
    assert_probe(&lod_scene(), [60, 60, 60, 255]);
}

#[test]
fn tile_sampling_changes_invalidate_texture_cache() {
    let (device, queue, dual) = headless_device();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
    for (origin, expected) in [(0, 32), (4, 16), (0, 32)] {
        let scene = scene(
            &rdp(TileDescriptor {
                uls: origin,
                lrs: origin + 28,
                ..tile()
            }),
            [2.5, 0.5],
            0,
        );
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
        assert_eq!(
            pixel(&pixels, 320, 100, 80),
            [expected, expected, expected, 255]
        );
    }
}

#[test]
fn tile_texel1_applies_independent_sampling_pixels() {
    let mut state = rdp(tile());
    state.other_mode_h = 1 << 20;
    state.combine_h = 0xfffd_0838;
    state.tiles[1] = TileDescriptor {
        uls: 4,
        ult: 8,
        lrs: 32,
        lrt: 20,
        shifts: 1,
        shiftt: 15,
        ..tile()
    };
    assert_probe(&scene(&state, [9.0, 1.25], 0), [48, 48, 48, 255]);
}

#[test]
fn tile_detail_applies_independent_sampling_pixels() {
    let mut state = rdp(TileDescriptor {
        uls: 4,
        ult: 8,
        lrs: 32,
        lrt: 20,
        shifts: 1,
        shiftt: 15,
        ..tile()
    });
    state.other_mode_h = (1 << 16) | (2 << 17);
    state.tiles[1] = tile();
    state.tiles[2] = tile();
    assert_probe(
        &scene_for_base(&state, [9.0, 1.25], 1, 1),
        [48, 48, 48, 255],
    );
}
