use crate::hle::{gbi::GbiUcode, SceneOp};
use crate::{DataFormat, RdramImage};
use n64_gbi::encode::*;

fn hud_memory(eu: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x3000];
    for y in 0..16 {
        for x in 0..16 {
            let i = y * 16 + x;
            bytes[0x1000 + i * 2..0x1002 + i * 2].copy_from_slice(&0xf801u16.to_be_bytes());
            let alpha = u16::from((x + y) % 5 != 0);
            let word = (((2 * x + 1) as u16) << 11) | (((2 * y + 1) as u16) << 6) | 62 | alpha;
            bytes[0x1200 + i * 2..0x1202 + i * 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let mut words = vec![
        gdp_set_color_image(0, 2, 320, 0x0010_0000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gsp_texture_f3d(65535, 65535, 0, 0, true),
        gsp_setothermode_h_f3d(12, 2, 0x2000),
        (0xe700_0000, 0),
        (0xba00_1402, 0x0020_0000),
        (0xba00_1301, 0),
        (0xb900_0002, 1),
        (0xf900_0000, 0xffff_ffff),
        (0xb900_031d, if eu { 0 } else { 0x0050_41c8 }),
    ];
    if eu {
        words.push((0xba00_0c02, 0));
    }
    for (addr, tmem, tile, origin) in [(0x1000, 0, 0, [0, 0]), (0x1200, 64, 3, [16, 24])] {
        words.extend([
            gdp_set_texture_image(0, 2, 1, addr),
            gdp_set_tile(0, 2, 0, tmem, 7, 0, 0, 4, 0, 0, 4, 0),
            gdp_load_sync(),
            gdp_load_block(7, 0, 0, 255, 512),
            (0xe700_0000, 0),
            gdp_set_tile(0, 2, 4, tmem, tile, 0, 0, 4, 0, 0, 4, 0),
            gdp_set_tile_size(tile, origin[0], origin[1], origin[0] + 60, origin[1] + 60),
        ]);
    }
    words.extend([
        (0xe40d_c0bc, 0x030a_0080),
        (0xb400_0000, 0x0080_00c0),
        (0xb300_0000, 0x1000_0400),
        (0xe411_c0bc, 0x030e_0080),
        (0xb400_0000, 0x0080_00c0),
        (0xb300_0000, 0x1000_0400),
        gsp_enddl_f3d(),
    ]);
    for (i, (a, b)) in words.into_iter().enumerate() {
        bytes[0x2000 + i * 8..0x2004 + i * 8].copy_from_slice(&a.to_be_bytes());
        bytes[0x2004 + i * 8..0x2008 + i * 8].copy_from_slice(&b.to_be_bytes());
    }
    bytes
}

fn interpret(bytes: &[u8]) -> crate::hle::interp::InterpResult {
    let result = super::inspect::equivalent(
        || RdramImage::new(bytes),
        0x2000,
        GbiUcode::F3d,
        DataFormat::Fixed,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result
}

#[test]
fn texrect_uses_command_tile_three() {
    for eu in [false, true] {
        let mut bytes = hud_memory(eu);
        let end = bytes[0x2000..]
            .as_chunks::<8>()
            .0
            .iter()
            .position(|word| word[..4] == 0xb800_0000u32.to_be_bytes())
            .unwrap()
            * 8
            + 0x2000;
        for (i, (a, b)) in [
            (0xba00_1402u32, 0),
            (0xfcff_ffff, 0xfffc_f279),
            (0x0420_0000, 0x1800),
            (0xbf00_0000, 0x0000_0a14),
            gsp_enddl_f3d(),
        ]
        .into_iter()
        .enumerate()
        {
            bytes[end + i * 8..end + i * 8 + 4].copy_from_slice(&a.to_be_bytes());
            bytes[end + i * 8 + 4..end + i * 8 + 8].copy_from_slice(&b.to_be_bytes());
        }
        let result = interpret(&bytes);
        let mat = &result.scene.materials[0];
        assert_eq!(mat.sampling.bounds, [16, 24, 76, 84]);
        assert_eq!(mat.sampling.tmem, [512, 32, 0, 2]);
        assert_eq!(&mat.texture[..8], &[8, 8, 255, 0, 24, 8, 255, 255]);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.set_texture(0, 0, true, 65535, 65535);
        let wrong =
            crate::hle::combiner::build_material(&result.rdp, &rsp, &mut Vec::new(), 0).unwrap();
        assert_eq!(&wrong.texture[..8], &[255, 0, 0, 255, 255, 0, 0, 255]);
        assert_ne!(mat.texture, wrong.texture);
        let ops = &result.scene.framebuffer_pairs[0].ops;
        assert!(matches!(ops[0], SceneOp::TexRect { tile: 3, .. }));
        let SceneOp::Tris(run) = ops.last().unwrap() else {
            panic!("expected trailing triangle")
        };
        assert_eq!(
            result.scene.materials[run.material_index as usize].texture,
            wrong.texture
        );
    }
}

#[test]
fn texrect_nonzero_origin_matches_triangle_sampling_material() {
    let result = interpret(&hud_memory(false));
    let mut rsp = crate::hle::rsp::Rsp::default();
    rsp.set_texture(3, 0, true, 65535, 65535);
    let triangle =
        crate::hle::combiner::build_material(&result.rdp, &rsp, &mut Vec::new(), 0).unwrap();
    let rect = &result.scene.materials[0];
    assert_eq!(rect.sampling, triangle.sampling);
    assert_eq!(rect.texture, triangle.texture);
}

#[cfg(feature = "capture")]
fn provenance(eu: bool) -> crate::capture::Provenance {
    crate::capture::Provenance {
        decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
        source_symbols: format!("bin/segment2.c: dl_hud_img_begin ({}), dl_hud_img_load_tex_block", if eu { "EU" } else { "US" }),
        command_vector: "F3D HUD copy state and LoadBlock sequence; tile 3 at origin (4,6), adjacent rectangles (40,32)-(55,47) and (56,32)-(71,47), dsdx=4096".into(),
        synthetic_data: "16x16 RGBA16 coordinate-colored glyph with transparent holes; deliberately red tile 0; synthetic addresses and framebuffer wrapper".into(),
    }
}

#[cfg(feature = "capture")]
fn assert_hud(eu: bool) {
    let fixture = hud_fixture(eu);
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % 320, i / 320);
        let mut expected = [0, 0, 0, 255];
        if (40..72).contains(&x) && (32..48).contains(&y) {
            let (s, t) = ((x - 40) % 16, y - 32);
            if (s + t) % 5 != 0 {
                let expand = |v| ((v << 3) | (v >> 2)) as u8;
                expected = [expand(2 * s + 1), expand(2 * t + 1), 255, 255];
            }
        }
        assert_eq!(*pixel, expected, "({x},{y}), eu={eu}");
    }
}

#[cfg(feature = "capture")]
#[test]
fn fixture_sm64_hud_us_copy() {
    assert_hud(false);
}

#[cfg(feature = "capture")]
#[test]
fn fixture_sm64_hud_eu_point() {
    assert_hud(true);
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_hud_us_copy_fixture() {
    super::capture_fixture::write(
        hud_memory(false),
        0x2000,
        320,
        240,
        "sm64-hud-us-copy.f3dcap",
        provenance(false),
    );
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_hud_eu_point_fixture() {
    super::capture_fixture::write(
        hud_memory(true),
        0x2000,
        320,
        240,
        "sm64-hud-eu-point.f3dcap",
        provenance(true),
    );
}

#[test]
fn texrect_nonzero_origin_matches_triangle_sampling() {
    use super::common::{pixel, render_to_pixels, scene_from_fixture};
    use crate::render::{headless_device_forced_fallback, SceneRenderer};
    let mut bytes = hud_memory(true);
    for command in bytes[0x2000..].as_chunks_mut::<8>().0 {
        let mut a = u32::from_be_bytes(command[..4].try_into().unwrap());
        let mut b = u32::from_be_bytes(command[4..].try_into().unwrap());
        match a {
            0xba00_1402 | 0xb900_0002 => b = 0,
            0xe700_0000 => {
                a = 0xfcff_ffff;
                b = 0xfffc_f279;
            }
            0xb400_0000 => b = 0x0160_0088,
            0xb300_0000 => b = 0,
            _ if a >> 24 == 0xf5 && b >> 24 == 3 => b |= (15 << 10) | 1,
            _ => {}
        }
        command[..4].copy_from_slice(&a.to_be_bytes());
        command[4..].copy_from_slice(&b.to_be_bytes());
    }
    let result = interpret(&bytes);
    let mut rsp = crate::hle::rsp::Rsp::default();
    rsp.set_texture(3, 0, true, 65535, 65535);
    let mut triangle = scene_from_fixture("framebuffer-extent--white1");
    triangle.materials =
        vec![crate::hle::combiner::build_material(&result.rdp, &rsp, &mut Vec::new(), 0).unwrap()];
    triangle.raw_st.fill([11.0, 4.25]);
    triangle.texcoord_table.fill([1.0; 2]);
    let rect = result.scene;
    let (device, queue) = headless_device_forced_fallback();
    let mut renderer =
        SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, false);
    let a = render_to_pixels(&device, &queue, &mut renderer, &triangle, 320, 240);
    let b = render_to_pixels(&device, &queue, &mut renderer, &rect, 320, 240);
    assert_eq!(pixel(&a, 320, 100, 80), [24, 41, 255, 255]);
    assert_eq!(pixel(&b, 320, 41, 33), [24, 41, 255, 255]);
}

#[cfg(feature = "capture")]
pub(super) fn hud_fixture(eu: bool) -> crate::capture::Fixture {
    super::capture_fixture::make(hud_memory(eu), 0x2000, 320, 240, provenance(eu))
}
