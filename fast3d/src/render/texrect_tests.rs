use super::*;
use crate::hle::{gbi::GbiUcode, mem::RdramImage, SceneOp};
use n64_gbi::encode::*;

fn decode(bounds: [u32; 4], base: [i16; 2], step: [i16; 2], cycle: u32, flip: bool) -> SceneOp {
    let [ulx, uly, lrx, lry] = bounds;
    let mut words = vec![
        gdp_set_color_image(0, 2, 32, 0x10000),
        gdp_set_other_mode_h(20, 2, cycle << 20),
    ];
    words.extend(gsp_texture_rectangle(
        ulx,
        uly,
        lrx,
        lry,
        3,
        base[0] as u32,
        base[1] as u32,
        step[0] as u32,
        step[1] as u32,
        flip,
    ));
    words.push(gsp_enddl());
    let bytes: Vec<_> = words
        .into_iter()
        .flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
        .collect();
    let result = crate::hle::interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        crate::DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene.framebuffer_pairs[0].ops[0].clone()
}

fn quad(op: &SceneOp) -> [OutVertex; 6] {
    let SceneOp::TexRect {
        rect,
        uls,
        ult,
        dsdx,
        dtdy,
        flip,
        copy_mode,
        ..
    } = op
    else {
        panic!("expected rectangle")
    };
    texrect_quad(
        rect,
        (*uls, *ult),
        (*dsdx, *dtdy),
        *flip,
        *copy_mode,
        (32, 32),
    )
}

fn bounds(vertices: &[OutVertex; 6]) -> [i32; 4] {
    let tl = vertices[0].position;
    let br = vertices[2].position;
    [
        ((tl[0] + 1.0) * 16.0) as i32,
        ((1.0 - tl[1]) * 16.0) as i32,
        ((br[0] + 1.0) * 16.0) as i32,
        ((1.0 - br[1]) * 16.0) as i32,
    ]
}

fn assert_mask(vertices: &[OutVertex; 6], expected: [i32; 4]) {
    let [l, t, r, b] = bounds(vertices);
    let [el, et, er, eb] = expected;
    for y in 0..32 {
        for x in 0..32 {
            assert_eq!(
                (l..r).contains(&x) && (t..b).contains(&y),
                (el..er).contains(&x) && (et..eb).contains(&y),
                "coverage at ({x},{y}); bounds {:?}",
                bounds(vertices)
            );
        }
    }
}

#[test]
fn texrect_lr_is_exclusive_in_one_and_two_cycle() {
    for cycle in [0, 1] {
        let q = quad(&decode([0, 0, 36, 36], [0, 0], [1024, 1024], cycle, false));
        assert_mask(&q, [0, 0, 9, 9]);
        assert_eq!(q[2].uv, [9.0, 9.0]);
    }
}

#[test]
fn texrect_copy_lr_is_inclusive() {
    let q = quad(&decode([3, 3, 36, 36], [0, 0], [4096, 1024], 2, false));
    assert_mask(&q, [0, 0, 10, 10]);
    assert_eq!(q[0].uv, [0.0, 0.0]);
    assert_eq!(q[2].uv, [10.0, 10.0]);
}

#[test]
fn texrect_adjacent_edges_have_no_extra_pixel() {
    for cycle in [0, 1, 2] {
        let end = if cycle == 2 { 32 } else { 36 };
        let a = quad(&decode(
            [0, 0, end, end],
            [0, 0],
            [1024, 1024],
            cycle,
            false,
        ));
        let b = quad(&decode(
            [36, 0, end + 36, end],
            [0, 0],
            [1024, 1024],
            cycle,
            false,
        ));
        assert_mask(&a, [0, 0, 9, 9]);
        assert_mask(&b, [9, 0, 18, 9]);
        assert_eq!(bounds(&a)[2], bounds(&b)[0]);
    }
}

#[test]
fn texrect_fractional_bounds_follow_reference() {
    for cycle in [0, 1] {
        for x in 5..=7 {
            for y in 9..=11 {
                let q = quad(&decode(
                    [x, y, 38, 45],
                    [11, -13],
                    [333, -777],
                    cycle,
                    false,
                ));
                assert_mask(&q, [2, 3, 10, 12]);
                assert_eq!(q[0].uv, [0.34375, -1.1875]);
                assert_eq!(q[2].uv, [2.9375, -8.03125]);
                let q = quad(&decode([x, y, 38, 45], [11, -13], [333, -777], cycle, true));
                assert_mask(&q, [2, 3, 10, 12]);
                assert_eq!(q[0].uv, [0.34375, -1.1875]);
                assert_eq!(q[5].uv, [0.34375, -7.28125]);
                assert_eq!(q[2].uv, [3.25, -7.28125]);
                assert_eq!(q[1].uv, [3.25, -1.1875]);
            }
        }
    }
}

#[test]
fn texrect_flip_and_signed_copy_step() {
    let q = quad(&decode([5, 11, 38, 45], [640, 960], [-4097, -513], 2, true));
    assert_mask(&q, [1, 2, 10, 12]);
    assert_eq!(q[0].uv, [20.0, 30.0]);
    assert_eq!(q[5].uv, [20.0, 25.46875]);
    assert_eq!(q[2].uv, [9.96875, 25.46875]);
    assert_eq!(q[1].uv, [9.96875, 30.0]);
}

#[test]
fn texrect_preserves_raw_fixed_and_float_bounds() {
    let op = decode([5, 11, 38, 45], [0, 0], [0, 0], 0, false);
    let SceneOp::TexRect { rect, .. } = op else {
        unreachable!()
    };
    assert_eq!([rect.ulx, rect.uly, rect.lrx, rect.lry], [5, 11, 38, 45]);
    let words: [(u32, u32); 5] = [
        gdp_set_color_image(0, 2, 32, 0x10000),
        (0xe4000026, 0x0300002d),
        (0xe1fffffb, 0),
        (0xf1fffff5, 0),
        gsp_enddl(),
    ];
    let bytes: Vec<_> = words
        .into_iter()
        .flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
        .collect();
    let result = crate::hle::interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        crate::DataFormat::Float,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let SceneOp::TexRect { rect, .. } = &result.scene.framebuffer_pairs[0].ops[0] else {
        unreachable!()
    };
    assert_eq!([rect.ulx, rect.uly, rect.lrx, rect.lry], [-5, -11, 38, 45]);
}

#[test]
fn texrect_uniform_uses_texel_units() {
    let rdp = crate::hle::rdp::Rdp {
        other_mode_h: 2 << 20,
        ..Default::default()
    };
    let mut mat = crate::hle::combiner::build_rect_material(
        &rdp,
        &crate::hle::rsp::Rsp::default(),
        0,
        &mut Vec::new(),
        0,
    )
    .unwrap();
    mat.tex_enable = true;
    mat.tex_w = 16;
    mat.tex_h = 8;
    let rm = crate::hle::blender::decode_render_mode(0, 0, 0);
    let uniform = CombinerUniform::from_rect(&mat, &rm, [0; 4]);
    assert_eq!(uniform.inv_tex_size, [0.0625, 0.125, 1.0, 0.0]);
}

#[test]
fn texrect_coverage_pixels() {
    // The readback helper needs 256-byte rows, so the target is 64 wide.
    use crate::hle::{ColorImage, FramebufferPair, Scissor};
    use crate::tests::common::{pixel, render_to_pixels, scene_from_fixture};
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, false);
    for (cycle, raw, expected) in [
        (0, [0, 0, 36, 36], [0, 0, 9, 9]),
        (1, [0, 0, 36, 36], [0, 0, 9, 9]),
        (2, [3, 3, 36, 36], [0, 0, 10, 10]),
        (0, [5, 11, 38, 45], [2, 3, 10, 12]),
    ] {
        let mut scene = scene_from_fixture("framebuffer-extent--white1");
        scene.materials[0].tex_enable = true;
        scene.materials[0].texture = vec![255; 4];
        scene.materials[0].tex_w = 1;
        scene.materials[0].tex_h = 1;
        scene.framebuffer_pairs = vec![FramebufferPair {
            color_image: ColorImage {
                fmt: 0,
                siz: 2,
                width: 64,
                addr: 0x10000,
            },
            ops: vec![decode(raw, [0, 0], [0, 0], cycle, false)],
            active_scissor: Scissor {
                lrx: 64,
                lry: 64,
                ..Default::default()
            },
            size_extent: (64, 64),
            ..Default::default()
        }];
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 64, 64);
        let [l, t, r, b] = expected;
        for y in 0..64 {
            for x in 0..64 {
                let expected = if (l..r).contains(&x) && (t..b).contains(&y) {
                    [255; 4]
                } else {
                    [13, 13, 20, 255]
                };
                assert_eq!(
                    pixel(&pixels, 64, x, y),
                    expected,
                    "cycle {cycle}, ({x},{y})"
                );
            }
        }
    }
}
