#[allow(unused_imports)]
use super::common::TexturePixels;

use super::dl_builder::{Built, DlBuilder};
use crate::{capture::Provenance, DataFormat, RdramImage};
use n64_gbi::encode::*;

#[allow(dead_code)]
#[path = "../../tests/common/tmem_semantics.rs"]
mod semantics;

fn prologue(b: &mut DlBuilder) -> Vec<(u32, u32)> {
    let matrix = b.bytes(8, &mtx_identity_bytes());
    let viewport = b.viewport(Vp {
        vscale: [640, 480, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    vec![
        gdp_set_color_image(0, 3, 320, 0x0010_0000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_set_cycle_type_f3d(3),
        gdp_set_fill_color(0x0000_00ff),
        gdp_fill_rectangle(0, 0, 1276, 956),
        gdp_pipe_sync(),
        gsp_clear_geometrymode_f3d(u32::MAX),
        gsp_set_geometrymode_f3d(n64_gbi::consts::rsp_f3d::G_CLIPPING),
        gsp_matrix_f3d(matrix, false, true, false),
        gsp_matrix_f3d(matrix, true, true, false),
        gsp_viewport_f3d(viewport),
        gsp_texture_f3d(65535, 65535, 0, 0, true),
        gdp_set_cycle_type_f3d(2),
        gdp_set_render_mode_f3d(0, 0),
        gsp_setothermode_h_f3d(12, 2, 0),
        gsp_setothermode_l_f3d(0, 2, 0),
    ]
}
fn rect(commands: &mut Vec<(u32, u32)>, x: u32, y: u32, w: u32, h: u32) {
    commands.extend([
        (
            0xe400_0000 | ((x + w - 1) * 4) << 12 | ((y + h - 1) * 4),
            (x * 4) << 12 | (y * 4),
        ),
        (0xb400_0000, 0),
        (0xb300_0000, 0x1000_0400),
        gdp_pipe_sync(),
    ]);
}
fn palette(b: &mut DlBuilder, commands: &mut Vec<(u32, u32)>, destination: u32, entries: &[u8]) {
    let address = b.bytes(8, entries);
    commands.extend([
        gdp_set_texture_image(0, 2, 1, address),
        gdp_set_tile(0, 2, 0, destination, 7, 0, 0, 0, 0, 0, 0, 0),
        gdp_load_tlut(7, (entries.len() / 2 - 1) as u32),
        gdp_pipe_sync(),
    ]);
}
fn layouts() -> Built {
    let mut b = DlBuilder::new();
    let mut commands = prologue(&mut b);
    for (row, f) in semantics::formats().into_iter().enumerate() {
        if f.fmt == 2 {
            palette(
                &mut b,
                &mut commands,
                256,
                &[0xf8, 1, 7, 0xc1, 0, 0x3f, 0xff, 0xff],
            );
        }
        let source = b.bytes(8, &f.encoded.repeat(12));
        let line = if f.siz == 3 {
            4
        } else {
            ((16u32 << f.siz) / 2).div_ceil(8)
        };
        commands.extend([
            gsp_setothermode_h_f3d(14, 2, if f.fmt == 2 { 2 << 14 } else { 0 }),
            gdp_set_texture_image(f.fmt, f.siz, 16, source),
            gdp_set_tile(f.fmt, f.siz, line, 16, 7, 0, 2, 0, 0, 2, 0, 0),
            (0xf400_0000, (7 << 24) | (60 << 12) | 8),
            gdp_set_tile(f.fmt, f.siz, line, 16, 0, 0, 2, 0, 0, 2, 0, 0),
            gdp_set_tile_size(0, 0, 0, 60, 8),
        ]);
        for alpha_panel in [false, true] {
            let color = if alpha_panel {
                CcPass {
                    a: 6,
                    b: ZERO_C,
                    c: 8,
                    d: ZERO_C,
                }
            } else {
                CcPass {
                    a: ZERO_C,
                    b: ZERO_C,
                    c: ZERO_C,
                    d: 1,
                }
            };
            let opaque = CcPass {
                a: ZERO_A,
                b: ZERO_A,
                c: ZERO_A,
                d: 6,
            };
            let x = if alpha_panel { 48 } else { 16 };
            let y = 8 + row as u32 * 8;
            commands.extend([
                gdp_set_cycle_type_f3d(0),
                (0xb900_031d, 0x0f0a_4000),
                gdp_set_combine_lerp(color, opaque, color, opaque),
                (
                    0xe400_0000 | ((x + 16) * 4) << 12 | ((y + 3) * 4),
                    (x * 4) << 12 | (y * 4),
                ),
                (0xb400_0000, 0),
                (0xb300_0000, 0x0400_0400),
                gdp_pipe_sync(),
            ]);
        }
    }
    commands.extend([(0xe900_0000, 0), gsp_enddl_f3d()]);
    b.list("main", &commands);
    b.finish("main")
}
fn tlut_mutation() -> Built {
    let mut b = DlBuilder::new();
    let mut commands = prologue(&mut b);
    let indices = b.bytes(8, &[0; 32]);
    commands.extend([
        gsp_setothermode_h_f3d(14, 2, 2 << 14),
        gdp_set_texture_image(2, 1, 8, indices),
        gdp_set_tile(2, 1, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
        (0xf400_0000, (7 << 24) | (28 << 12) | 12),
        gdp_set_tile(2, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 28, 12),
    ]);
    for (draw, (destination, entries)) in [
        (256, vec![0xf8, 1]),
        (256, vec![7, 0xc1]),
        (257, vec![0, 0x3f]),
        (257, vec![7, 0xc1]),
        (511, vec![0xff, 0xff, 0, 0x3f]),
    ]
    .into_iter()
    .enumerate()
    {
        palette(&mut b, &mut commands, destination, &entries);
        if draw == 4 {
            // Destination 512 wraps to byte zero; reload index zero after the TLUT write.
            commands.extend([
                gdp_set_texture_image(2, 1, 8, indices),
                gdp_set_tile(2, 1, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                (0xf400_0000, (7 << 24) | (28 << 12) | 12),
            ]);
            palette(&mut b, &mut commands, 256, &[0, 0x3f]);
        }
        rect(&mut commands, 16 + draw as u32 * 16, 24, 8, 4);
    }
    commands.extend([(0xe900_0000, 0), gsp_enddl_f3d()]);
    b.list("main", &commands);
    b.finish("main")
}
fn roles() -> Built {
    let mut b = DlBuilder::new();
    let mut commands = prologue(&mut b);
    let matrix = b.bytes(
        8,
        &mtx_to_bytes([
            [1.0 / 128.0, 0., 0., 0.],
            [0., 1.0 / 128.0, 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ]),
    );
    let vp = b.viewport(Vp {
        vscale: [512, 512, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    commands.extend([
        gsp_matrix_f3d(matrix, true, true, false),
        gsp_viewport_f3d(vp),
        gdp_set_cycle_type_f3d(0),
        (0xb900_031d, 0x0f0a_4000),
        gsp_setothermode_h_f3d(16, 1, 1 << 16),
        (0xfc12_7e24, 0xffff_f9fc),
    ]);
    for (index, (w, h, value)) in [(8, 4, 37), (3, 5, 91), (5, 2, 173), (2, 2, 211)]
        .into_iter()
        .enumerate()
    {
        let source = b.bytes(8, &[value; 40]);
        commands.extend([
            gdp_set_texture_image(4, 1, 8, source),
            gdp_set_tile(4, 1, 1, index as u32 * 16, 7, 0, 2, 0, 0, 2, 0, 0),
            (0xf400_0000, (7 << 24) | (28 << 12) | ((h - 1) * 4)),
            gdp_set_tile(
                4,
                1,
                1,
                index as u32 * 16,
                index as u32,
                0,
                2,
                0,
                0,
                2,
                0,
                0,
            ),
            gdp_set_tile_size(index as u32, 0, 0, (w - 1) * 4, (h - 1) * 4),
        ]);
    }
    for (draw, base) in [0, 1, 2].into_iter().enumerate() {
        let x = -24 + draw as i16 * 16;
        let vertices =
            b.vertices(
                &[(x, -8), (x + 16, -8), (x + 16, 8), (x, 8)].map(|(x, y)| VtxColored {
                    x,
                    y,
                    z: 0,
                    flag: 0,
                    s: 0,
                    t: 0,
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                }),
            );
        commands.extend([
            gsp_texture_f3d(65535, 65535, 1, base, true),
            gsp_vertex_f3d(0, 4, vertices),
            gsp_1triangle_f3d(0, 1, 2),
            gsp_1triangle_f3d(0, 2, 3),
            gdp_pipe_sync(),
        ]);
    }
    commands.extend([(0xe900_0000, 0), gsp_enddl_f3d()]);
    b.list("main", &commands);
    b.finish("main")
}
fn built(name: &str) -> Built {
    match name {
        "tmem-layouts" => layouts(),
        "tmem-tlut-mutation" => tlut_mutation(),
        _ => roles(),
    }
}
fn fixture(name: &str) -> crate::capture::Fixture {
    let b = built(name);
    super::capture_fixture::make_image(
        b.rdram,
        b.entry,
        crate::Microcode::F3d,
        320,
        240,
        Provenance {
            decomp_revision: "authored T1 independent TMEM witnesses".into(),
            source_symbols: name.into(),
            command_vector: "F3D IMAGE LoadTile, TLUT and ordered draws; CPU literal checks in tmem_fixtures".into(),
            synthetic_data: "asymmetric literal channels, non-halving roles, repeated indices and palette mutation".into(),
        },
    )
}
#[test]
fn tmem_image_inputs_have_literal_draw_snapshots() {
    for name in semantics::FIXTURES {
        let b = built(name);
        let result = crate::hle::interpret(
            RdramImage::new(&b.rdram),
            b.entry.into(),
            crate::hle::gbi::GbiUcode::F3d,
            DataFormat::Fixed,
            None,
        );
        assert!(result.diags.is_empty(), "{name}: {:?}", result.diags);
        if name == "tmem-layouts" {
            for (mat, f) in result.scene.materials.iter().zip(
                semantics::formats()
                    .into_iter()
                    .flat_map(|f| [f.clone(), f]),
            ) {
                assert_eq!((mat.tex_w, mat.tex_h), (16, 3));
                assert_eq!(mat.texture.decode().len(), 16 * 3 * 4);
                for (i, pixel) in mat.texture.decode().as_chunks::<4>().0.iter().enumerate() {
                    assert_eq!(*pixel, f.pixels[i % 4], "{} pixel{i}", f.name);
                }
            }
            assert_eq!(result.scene.materials.len(), 18);
        } else if name == "tmem-tlut-mutation" {
            let materials: Vec<_> = result
                .scene
                .framebuffer_pairs
                .iter()
                .flat_map(|p| &p.ops)
                .filter_map(|op| {
                    if let crate::scene::SceneOp::TexRect { material_index, .. } = op {
                        Some(&result.scene.materials[*material_index as usize])
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(materials.len(), 5);
            for (material, color) in materials.into_iter().zip(semantics::TLUT_COLORS) {
                assert_eq!(material.texture.decode(), color.repeat(32));
            }
        } else {
            for (i, mat) in result.scene.materials.iter().enumerate() {
                let widths = [8, 3, 5, 2];
                let heights = [4, 5, 2, 2];
                let values = [37, 91, 173, 211];
                assert_eq!(mat.mip_levels.len(), 2);
                assert_eq!(mat.texture.decode(), mat.mip_levels[0].texture.decode());
                for (j, level) in mat.mip_levels.iter().enumerate() {
                    assert_eq!((level.w, level.h), (widths[i + j], heights[i + j]));
                    assert_eq!(
                        level.texture.decode(),
                        vec![values[i + j]; (level.w * level.h * 4) as usize]
                    );
                }
            }
            assert_eq!(result.scene.materials.len(), 3);
        }
        let encoded = fixture(name).to_bytes().unwrap();
        assert_eq!(
            crate::capture::Fixture::from_bytes(&encoded)
                .unwrap()
                .to_bytes()
                .unwrap(),
            encoded
        );
    }
}
#[test]
fn tmem_captures_keep_one_rgba32_target_layout() {
    use crate::{capture::ReplayHardware, Hardware};

    for name in semantics::FIXTURES {
        let encoded = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(format!("{name}.f3dcap")),
        )
        .unwrap();
        assert_eq!(encoded, fixture(name).to_bytes().unwrap(), "{name}");
        let capture = crate::capture::Fixture::from_bytes(&encoded).unwrap();
        assert_eq!(capture.frame.vi.unwrap().status, 3);
        assert_eq!(capture.tasks.len(), 1);
        let task = &capture.tasks[0];
        let hardware = ReplayHardware::new(task, capture.frame.vi).unwrap();
        let result = crate::hle::interpret(
            hardware.rdram(),
            task.entry,
            task.microcode.into(),
            task.data_format,
            None,
        );
        hardware.check().unwrap();
        assert!(result.diags.is_empty(), "{name}: {:?}", result.diags);
        assert!(!result.scene.framebuffer_pairs.is_empty());
        for pair in &result.scene.framebuffer_pairs {
            let image = pair.color_image;
            assert_eq!(
                (image.addr, image.fmt, image.siz, image.width),
                (0x0010_0000, 0, 3, 320),
                "{name}: target at epoch {}",
                pair.color_image_epoch
            );
        }
    }
}

#[test]
#[ignore = "writes T1 IMAGE captures to FAST3D_WRITE_FIXTURES"]
fn write_tmem_fixtures() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    for name in semantics::FIXTURES {
        std::fs::write(
            dir.join(format!("{name}.f3dcap")),
            fixture(name).to_bytes().unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn tmem_browser_expectations_reject_alpha_and_background_mutations() {
    for name in semantics::FIXTURES {
        let expected = semantics::expected_pixels(name);
        semantics::assert_pixels(name, &expected);
        for offset in [3, (24 * 320 + 16) * 4 + 3] {
            let mut changed = expected.clone();
            changed[offset] ^= 1;
            assert!(std::panic::catch_unwind(|| semantics::assert_pixels(name, &changed)).is_err());
        }
    }
}
