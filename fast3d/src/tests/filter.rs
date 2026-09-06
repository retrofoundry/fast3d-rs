use super::common::{pixel, render_to_pixels, scene_from_fixture};
use crate::hle::combiner::build_material;
use crate::hle::rdp::{Rdp, TileDescriptor};
use crate::hle::rsp::Rsp;
use crate::hle::Scene;
use crate::render::{headless_device_forced_fallback, SceneRenderer};

const TEXELS: [[u8; 4]; 4] = [
    [32, 64, 96, 128],
    [224, 32, 64, 192],
    [64, 224, 32, 64],
    [128, 96, 224, 240],
];

fn state(filter: u32) -> Rdp {
    let mut rdp = Rdp {
        combine_l: 0x00ff_ffff,
        combine_h: 0xfffc_f279,
        other_mode_h: filter << 12,
        load_via_tile: true,
        tmem: TEXELS.as_flattened().to_vec(),
        ..Default::default()
    };
    rdp.tiles[0] = TileDescriptor {
        fmt: 0,
        siz: 3,
        width: 2,
        height: 2,
        lrs: 4,
        lrt: 4,
        line: 1,
        cms: 2,
        cmt: 2,
        ..Default::default()
    };
    rdp.tmem_bank
        .write_tile(TEXELS.as_flattened(), 0, 1, 2, 1, 8, 3);
    rdp
}

fn scene(rdp: &Rdp, uv: [f32; 2], base: u8, levels: u8) -> Scene {
    let mut scene = scene_from_fixture("framebuffer-extent--white1");
    let mut rsp = Rsp::default();
    rsp.set_texture(base, levels, true, 65535, 65535);
    let mut diags = Vec::new();
    scene.materials = vec![build_material(rdp, &rsp, &mut diags, 0).unwrap()];
    assert!(diags.is_empty(), "{diags:?}");
    scene.raw_st.fill(uv);
    scene.texcoord_table.fill([1.0; 2]);
    scene
}

fn assert_probes(cases: impl IntoIterator<Item = (Scene, [u8; 4])>) {
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer =
        SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, false);
    for (scene, expected) in cases {
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
        assert_eq!(pixel(&pixels, 320, 100, 80), expected);
    }
}

fn rect_scene(filters: &[(u32, bool)], uv: [i16; 2]) -> Scene {
    use crate::hle::{ColorImage, FramebufferPair, SceneOp, Scissor};
    let mut result = scene(&state(0), [0.0; 2], 0, 0);
    result.materials.clear();
    result.draw_runs.clear();
    let mut ops = Vec::new();
    for (i, &(filter, copy)) in filters.iter().enumerate() {
        let mut rdp = state(filter);
        if copy {
            rdp.other_mode_h |= 2 << 20;
        }
        result
            .materials
            .push(scene(&rdp, [0.0; 2], 0, 0).materials.remove(0));
        let x = 80 + i as i32 * 40;
        ops.push(SceneOp::TexRect {
            rect: crate::hle::TexRectBounds {
                ulx: x * 4,
                uly: 64 * 4,
                lrx: (x + 31) * 4,
                lry: 95 * 4,
            },
            tile: 0,
            uls: uv[0],
            ult: uv[1],
            dsdx: 0,
            dtdy: 0,
            flip: false,
            copy_mode: copy,
            material_index: i as u32,
            render_mode_index: 0,
            fog_color: [0; 4],
            fb_source: None,
        });
    }
    result.framebuffer_pairs = vec![FramebufferPair {
        color_image: ColorImage {
            fmt: 0,
            siz: 3,
            width: 320,
            addr: 0x100000,
        },
        ops,
        active_scissor: Scissor {
            lrx: 320,
            lry: 240,
            ..Default::default()
        },
        size_extent: (320, 240),
        ..Default::default()
    }];
    result
}

#[test]
fn filter_point_has_no_neighbor_contribution() {
    assert_probes([
        (scene(&state(0), [0.75, 0.75], 0, 0), TEXELS[0]),
        (scene(&state(0), [1.0, 0.0], 0, 0), TEXELS[1]),
        (scene(&state(0), [0.999, 0.0], 0, 0), TEXELS[1]),
    ]);
}

#[test]
fn filter_normalized_images_use_selected_mode() {
    assert_probes(
        [
            (0, [0.375, 0.375], TEXELS[0]),
            (2, [0.125, 0.125], [88, 96, 72, 128]),
            (3, [0.25, 0.25], [112, 104, 104, 156]),
        ]
        .map(|(filter, uv, expected)| {
            let mut scene = scene(&state(filter), uv, 0, 0);
            scene.materials[0].sampling.image[2] = 2;
            (scene, expected)
        }),
    );
}

#[test]
fn filter_bilerp_uses_three_taps() {
    assert_probes([
        (scene(&state(2), [0.25, 0.25], 0, 0), [88, 96, 72, 128]),
        (scene(&state(2), [0.75, 0.75], 0, 0), [136, 112, 136, 184]),
        (scene(&state(2), [0.5, 0.5], 0, 0), [144, 128, 48, 128]),
        (scene(&state(2), [1.0, 0.0], 0, 0), TEXELS[1]),
    ]);
}

#[test]
fn filter_average_midpoint_uses_four_taps() {
    assert_probes(
        [
            [0.5, 0.5],
            [63.0 / 128.0, 65.0 / 128.0],
            [65.0 / 128.0, 63.0 / 128.0],
        ]
        .map(|uv| (scene(&state(3), uv, 0, 0), [112, 104, 104, 156])),
    );
}

#[test]
fn filter_average_off_midpoint_uses_three_taps() {
    assert_probes([
        (scene(&state(3), [0.25, 0.25], 0, 0), [88, 96, 72, 128]),
        (scene(&state(3), [0.75, 0.75], 0, 0), [136, 112, 136, 184]),
        (scene(&state(3), [0.5, 0.25], 0, 0), [136, 88, 64, 144]),
        (
            scene(&state(3), [0.5, 60.0 / 128.0], 0, 0),
            [143, 123, 50, 130],
        ),
        (
            scene(&state(3), [60.0 / 128.0, 0.5], 0, 0),
            [138, 129, 49, 126],
        ),
    ]);
}

#[test]
fn filter_mode_changes_between_draws() {
    assert_probes([
        (scene(&state(0), [0.5, 0.5], 0, 0), TEXELS[0]),
        (scene(&state(2), [0.5, 0.5], 0, 0), [144, 128, 48, 128]),
        (scene(&state(3), [0.5, 0.5], 0, 0), [112, 104, 104, 156]),
        (scene(&state(0), [0.5, 0.5], 0, 0), TEXELS[0]),
    ]);
    let scene = rect_scene(&[(0, false), (2, false), (3, false), (0, false)], [16, 16]);
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer =
        SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, false);
    let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 320, 240);
    for (x, expected) in [
        (100, TEXELS[0]),
        (140, [144, 128, 48, 128]),
        (180, [112, 104, 104, 156]),
        (220, TEXELS[0]),
    ] {
        assert_eq!(pixel(&pixels, 320, x, 80), expected);
    }
}

#[test]
fn filter_copy_forces_point() {
    // COPY is a rectangle-only cycle type on the RDP; triangles in COPY mode are undefined.
    assert_probes(
        [2, 3]
            .into_iter()
            .map(|filter| (rect_scene(&[(filter, true)], [24, 24]), TEXELS[0])),
    );
}

#[test]
fn filter_texel1_and_detail_use_selected_mode() {
    let mut cases = Vec::new();
    for (filter, expected) in [
        (0, TEXELS[0]),
        (2, [144, 128, 48, 128]),
        (3, [112, 104, 104, 156]),
    ] {
        let mut rdp = state(filter);
        rdp.tiles[1] = rdp.tiles[0].clone();
        rdp.tiles[1].tmem_addr = 4;
        rdp.tmem_bank
            .write_tile(TEXELS.as_flattened(), 4, 1, 2, 1, 8, 3);
        rdp.tmem_bank.write_tile(&[0; 16], 0, 1, 2, 1, 8, 3);
        rdp.other_mode_h |= 1 << 20;
        rdp.combine_h = 0xfffd_0438;
        let texel1_scene = scene(&rdp, [0.5, 0.5], 0, 0);
        assert!(texel1_scene.materials[0].tex1.is_some());
        cases.push((texel1_scene, expected));
        rdp.other_mode_h = (filter << 12) | (1 << 16) | (2 << 17);
        rdp.combine_h = 0xfffc_f279;
        rdp.tmem_bank
            .write_tile(TEXELS.as_flattened(), 0, 1, 2, 1, 8, 3);
        rdp.tmem_bank.write_tile(&[0; 16], 4, 1, 2, 1, 8, 3);
        rdp.tiles[2] = rdp.tiles[1].clone();
        let detail_scene = scene(&rdp, [0.5, 0.5], 1, 1);
        assert!(detail_scene.materials[0].lod);
        assert!(detail_scene.materials[0].detail_tex.is_some());
        cases.push((detail_scene, expected));
    }
    assert_probes(cases);
}

#[test]
fn filter_asymmetric_texture_survives_tmem_decode() {
    let scene = scene(&state(0), [0.0; 2], 0, 0);
    assert_eq!(scene.materials[0].texture, TEXELS.as_flattened());
}
