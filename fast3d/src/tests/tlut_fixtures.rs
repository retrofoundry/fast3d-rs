use crate::capture::Provenance;
use crate::hle::{gbi::GbiUcode, SceneOp};
use crate::{DataFormat, RdramImage};
use n64_gbi::encode::*;

use super::dl_builder::{Built, DlBuilder};

fn scene(ia16: bool) -> Built {
    let mut b = DlBuilder::new();
    let indices = b.bytes(
        8,
        &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef].repeat(16),
    );
    let mut commands = vec![
        gdp_set_color_image(0, 2, 320, 0x0010_0000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gsp_texture_f3d(65535, 65535, 0, 0, true),
        gsp_setothermode_h_f3d(14, 2, if ia16 { 0xc000 } else { 0x8000 }),
        gsp_setothermode_h_f3d(12, 2, 0),
        gdp_set_cycle_type_f3d(2),
        gsp_setothermode_l_f3d(0, 2, 0),
        gdp_set_render_mode_f3d(0, 0),
    ];
    for bank in [0, 15] {
        let entries: Vec<_> = (0..16u16)
            .flat_map(|i| {
                if ia16 {
                    [
                        if bank == 0 {
                            i as u8 * 17
                        } else {
                            255 - i as u8 * 17
                        },
                        255,
                    ]
                } else {
                    (if bank == 0 {
                        ((2 * i + 1) << 11) | 1
                    } else {
                        ((31 - 2 * i) << 6) | 63
                    })
                    .to_be_bytes()
                }
            })
            .collect();
        let palette = b.bytes(8, &entries);
        commands.extend([
            gdp_set_texture_image(0, 2, 1, palette),
            gdp_set_tile(0, 2, 0, 0x100 + 16 * bank, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_sync(),
            gdp_load_tlut(7, 15),
            gdp_pipe_sync(),
        ]);
    }
    commands.extend([
        gdp_set_texture_image(2, 2, 1, indices),
        gdp_set_tile(2, 2, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0),
        gdp_load_sync(),
        gdp_load_block(7, 0, 0, 63, 2048),
        gdp_pipe_sync(),
    ]);
    for (tile, bank, x) in [(0, 0, 40), (3, 15, 72)] {
        commands.extend([
            gdp_set_tile(2, 0, 1, 0, tile, bank, 2, 4, 0, 2, 4, 0),
            gdp_set_tile_size(tile, 0, 0, 60, 60),
            (
                0xe400_0000 | ((x + 15) * 4) << 12 | (47 * 4),
                tile << 24 | (x * 4) << 12 | (32 * 4),
            ),
            (0xb400_0000, 0),
            (0xb300_0000, 0x1000_0400),
            gdp_pipe_sync(),
        ]);
    }
    commands.push(gsp_enddl_f3d());
    b.list("main", &commands);
    b.finish("main")
}

fn expected(ia16: bool, bank: usize, index: usize) -> [u8; 4] {
    if ia16 {
        let intensity = if bank == 0 {
            index as u8 * 17
        } else {
            255 - index as u8 * 17
        };
        [intensity, intensity, intensity, 255]
    } else {
        let red = [
            8, 24, 41, 57, 74, 90, 107, 123, 140, 156, 173, 189, 206, 222, 239, 255,
        ];
        if bank == 0 {
            [red[index], 0, 0, 255]
        } else {
            [0, red[15 - index], 255, 255]
        }
    }
}

fn provenance(ia16: bool) -> Provenance {
    Provenance {
        decomp_revision: "libultra gbi.h TLUT macros; authored PR 2 scene".into(),
        source_symbols: "gsDPLoadTLUT_pal16, gsDPLoadTextureBlock_4b, gsSPTextureRectangle".into(),
        command_vector: format!("CI4 palettes 0 and 15 at TMEM words 0x100 and 0x1f0; F0000000/0703C000 loads 16 packed {} entries each before either draw; copy rectangles [40,56)x[32,48) and [72,88)x[32,48)", if ia16 { "IA16" } else { "RGBA16" }),
        synthetic_data: "shared 16x16 CI4 indices 0..15 across each row; independent ascending/descending palettes; IMAGE addresses and framebuffer wrapper".into(),
    }
}

#[test]
fn tlut_two_palettes_in_one_list() {
    for ia16 in [false, true] {
        let built = scene(ia16);
        let result = crate::hle::interpret(
            RdramImage::new(&built.rdram),
            built.entry.into(),
            GbiUcode::F3d,
            DataFormat::Fixed,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let materials: Vec<_> = result
            .scene
            .framebuffer_pairs
            .iter()
            .flat_map(|p| &p.ops)
            .filter_map(|op| {
                if let SceneOp::TexRect { material_index, .. } = op {
                    Some(&result.scene.materials[*material_index as usize])
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(materials.len(), 2);
        for (bank, material) in materials.into_iter().enumerate() {
            assert_eq!(material.texture.len(), 16 * 16 * 4);
            for (i, pixel) in material.texture.chunks_exact(4).enumerate() {
                assert_eq!(
                    pixel,
                    expected(ia16, bank, i % 16),
                    "ia16={ia16}, bank={bank}, texel={i}"
                );
            }
        }
    }
}

fn fixture(ia16: bool) -> crate::capture::Fixture {
    let built = scene(ia16);
    super::capture_fixture::make(built.rdram, built.entry, 320, 240, provenance(ia16))
}

#[test]
fn fixture_tlut_two_palettes_pixels() {
    for ia16 in [false, true] {
        let (device, queue) = crate::render::headless_device_forced_fallback();
        let output = pollster::block_on(fixture(ia16).replay(device, queue)).unwrap();
        assert!(
            output.diagnostics.iter().all(Vec::is_empty),
            "{:?}",
            output.diagnostics
        );
        for (i, pixel) in output.rgba8.chunks_exact(4).enumerate() {
            let (x, y) = (i % 320, i / 320);
            let want = if (32..48).contains(&y) && (40..56).contains(&x) {
                expected(ia16, 0, x - 40)
            } else if (32..48).contains(&y) && (72..88).contains(&x) {
                expected(ia16, 1, x - 72)
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(pixel, want, "ia16={ia16}, ({x},{y})");
        }
    }
}

fn write(ia16: bool, filename: &str) {
    let built = scene(ia16);
    super::capture_fixture::write(
        built.rdram,
        built.entry,
        320,
        240,
        filename,
        provenance(ia16),
    );
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_tlut_ci4_rgba16_banks_fixture() {
    write(false, "tlut-ci4-rgba16-banks.f3dcap");
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_tlut_ci4_ia16_banks_fixture() {
    write(true, "tlut-ci4-ia16-banks.f3dcap");
}
