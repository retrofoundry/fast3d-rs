use crate::capture::{Fixture, Provenance};
use crate::hle::combiner::{AlphaIn, ColorIn};
use crate::hle::{gbi::GbiUcode, AlphaCompare, BlendClass, ZMode};
use crate::{DataFormat, RdramImage};
use n64_gbi::encode::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Surface {
    Shadow,
    Water,
    Foliage,
}

impl Surface {
    fn region(self) -> [usize; 4] {
        match self {
            Self::Shadow => [144, 104, 32, 32],
            Self::Water => [128, 88, 64, 64],
            Self::Foliage => [144, 88, 32, 64],
        }
    }

    fn provenance(self) -> Provenance {
        let (symbols, state, payload) = match self {
            Self::Shadow => (
                "src/game/shadow.c: make_shadow_vertex_at_xyz, add_shadow_to_display_list; bin/segment2.c: dl_shadow_begin, dl_shadow_square, dl_shadow_4_verts, dl_shadow_end",
                "AA_ZB_XLU_DECAL/DECAL2, MODULATEIA, mirrored 16x16 IA8, shade alpha 128",
                "Solid black IA8 quarter-square, white vertices with alpha 128; 32x32 decal coplanar with the opaque floor",
            ),
            Self::Water => (
                "src/game/moving_texture.c: movtex_make_quad_vertex, movtex_gen_from_quad, movtex_change_texture_format; bin/segment2.c: dl_waterbox_rgba16_begin, dl_waterbox_end",
                "AA_ZB_XLU_SURF/SURF2, MODULATERGBA (texture times shade, including alpha), wrapped 32x32 RGBA16, shade alpha 128",
                "Opaque blue/green RGBA16 half-tile bands wrapped twice across a 64x64 quad; white vertices with alpha 128 above the opaque floor; env alpha 17 deliberately differs from shade alpha",
            ),
            Self::Foliage => (
                "actors/tree/model.inc.c: tree_seg3_dl_0302FEE8, tree_seg3_dl_0302FE88; actors/tree/geo.inc.c: bubbly_tree_geo",
                "AA_ZB_TEX_EDGE/TEX_EDGE2, DECALRGBA, clamped 32x64 RGBA16, coverage-select alpha, no alpha compare",
                "Green RGBA16 tapered billboard with transparent diagonal holes; 32x64 quad above the opaque floor, white unlit vertices",
            ),
        };
        Provenance {
            decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
            source_symbols: format!("{symbols}; src/game/rendering_graph_node.c: renderModeTable_1Cycle, renderModeTable_2Cycle; src/game/game_init.c: init_rdp"),
            command_vector: format!("F3D {state}; one-cycle, perspective correction, BILERP inherited from init_rdp"),
            synthetic_data: format!("{payload}. All textures, geometry, ST coordinates, matrices, viewport and addresses are synthetic; pixel-centered sampling and a full-frame shade-colored [64,128,192] floor in a cleared 320x240 framebuffer. No ROM bytes"),
        }
    }

    pub(super) fn fixture(self) -> Fixture {
        super::capture_fixture::make(memory(self), 0x3000, 320, 240, self.provenance())
    }
}

fn quad(bytes: &mut [u8], address: usize, region: [usize; 4], z: i16, rgba: [u8; 4]) {
    let [left, top, width, height] = region.map(|n| n as i16);
    for (i, (x, y, s, t)) in [
        (left, top + height, -16, height * 32 - 16),
        (
            left + width,
            top + height,
            width * 32 - 16,
            height * 32 - 16,
        ),
        (left + width, top, width * 32 - 16, -16),
        (left, top, -16, -16),
    ]
    .into_iter()
    .enumerate()
    {
        bytes[address + i * 16..address + (i + 1) * 16].copy_from_slice(
            &VtxColored {
                x: x - 160,
                y: 120 - y,
                z,
                flag: 0,
                s,
                t,
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            }
            .to_bytes(),
        );
    }
}

fn memory(surface: Surface) -> Vec<u8> {
    let mut bytes = vec![0; 0x4000];
    bytes[..64].copy_from_slice(&mtx_to_bytes([
        [1.0 / 256.0, 0.0, 0.0, 0.0],
        [0.0, 1.0 / 256.0, 0.0, 0.0],
        [0.0, 0.0, 1.0 / 128.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    bytes[64..80].copy_from_slice(
        &Vp {
            vscale: [1024, 1024, 511, 0],
            vtrans: [640, 480, 511, 0],
        }
        .to_bytes(),
    );
    let floor_z = if matches!(surface, Surface::Shadow) {
        0
    } else {
        32
    };
    quad(
        &mut bytes,
        0x100,
        [0, 0, 320, 240],
        floor_z,
        [64, 128, 192, 255],
    );
    let alpha = if matches!(surface, Surface::Foliage) {
        255
    } else {
        128
    };
    quad(
        &mut bytes,
        0x140,
        surface.region(),
        0,
        [255, 255, 255, alpha],
    );
    let (fmt, size, width, height, line, cmt, maskt, cms, masks, load_count, dxt) = match surface {
        Surface::Shadow => (3, 1, 16, 16, 2, 1, 4, 1, 4, 127, 1024),
        Surface::Water => (0, 2, 32, 32, 8, 0, 5, 0, 5, 1023, 256),
        Surface::Foliage => (0, 2, 32, 64, 8, 2, 6, 2, 5, 2047, 256),
    };
    for t in 0..height {
        for s in 0..width {
            let index = (t * width + s) as usize;
            match surface {
                Surface::Shadow => bytes[0x1000 + index] = 15,
                Surface::Water | Surface::Foliage => {
                    let word: u16 = match surface {
                        Surface::Water => {
                            if s < 16 {
                                0x003f
                            } else {
                                0x07c1
                            }
                        }
                        _ => 0x07c0 | u16::from(s >= t / 4 && s < 32 - t / 4 && (s + t) % 7 != 0),
                    };
                    bytes[0x1000 + index * 2..0x1002 + index * 2]
                        .copy_from_slice(&word.to_be_bytes());
                }
            }
        }
    }
    let mut words = vec![
        gsp_matrix_f3d(0, true, true, false),
        gsp_viewport_f3d(64),
        (0xba00_1402, 0),
        (0xba00_1301, 0x0008_0000),
        (0xba00_0c02, 0x2000),
        (0xb900_0002, 0),
        (0xb700_0000, 0x0020_0005),
        (0xb900_031d, 0x0055_2078),
        (0xfcff_ffff, 0xfffe_793c),
        gsp_vertex_f3d(0, 4, 0x100),
        gsp_1triangle_f3d(0, 1, 2),
        gsp_1triangle_f3d(0, 2, 3),
        (0xe700_0000, 0),
        (0xb600_0000, 0x0002_2000),
        (
            0xb900_031d,
            match surface {
                Surface::Shadow => 0x0050_4dd8,
                Surface::Water => 0x0050_49d8,
                Surface::Foliage => 0x0055_3078,
            },
        ),
        match surface {
            Surface::Foliage => (0xfcff_ffff, 0xfffc_f279),
            _ => (0xfc12_1824, 0xff33_ffff),
        },
        (0xfb00_0000, 0xffff_ff11),
        gsp_texture_f3d(65535, 65535, 0, 0, true),
        gdp_set_texture_image(fmt, 2, 1, 0x1000),
        gdp_set_tile(fmt, 2, 0, 0, 7, 0, cmt, maskt, 0, cms, masks, 0),
        gdp_load_sync(),
        gdp_load_block(7, 0, 0, load_count, dxt),
        (0xe700_0000, 0),
        (0xe800_0000, 0),
        gdp_set_tile(fmt, size, line, 0, 0, 0, cmt, maskt, 0, cms, masks, 0),
        gdp_set_tile_size(0, 0, 0, (width - 1) * 4, (height - 1) * 4),
        gsp_vertex_f3d(0, 4, 0x140),
        gsp_1triangle_f3d(0, 1, 2),
        gsp_1triangle_f3d(0, 2, 3),
        gsp_texture_f3d(65535, 65535, 0, 0, false),
        (0xe700_0000, 0),
        (0xb700_0000, 0x0002_2000),
        (0xfcff_ffff, 0xfffe_793c),
    ];
    words.push(gsp_enddl_f3d());
    for (i, (w0, w1)) in words.into_iter().enumerate() {
        bytes[0x3000 + i * 8..0x3004 + i * 8].copy_from_slice(&w0.to_be_bytes());
        bytes[0x3004 + i * 8..0x3008 + i * 8].copy_from_slice(&w1.to_be_bytes());
    }
    bytes
}

#[test]
fn sm64_surface_commands_preserve_alpha_depth_and_tiles() {
    for surface in [Surface::Shadow, Surface::Water, Surface::Foliage] {
        let bytes = memory(surface);
        let result = crate::hle::interpret(
            RdramImage::new(&bytes),
            0x3000,
            GbiUcode::F3d,
            DataFormat::Fixed,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let scene = &result.scene;
        assert_eq!(scene.draw_runs.len(), 2);
        let floor = &scene.render_modes[scene.draw_runs[0].render_mode_index as usize];
        assert!(floor.z_test && floor.z_write);
        let draw = &scene.draw_runs[1];
        let material = &scene.materials[draw.material_index as usize];
        let mode = &scene.render_modes[draw.render_mode_index as usize];
        assert_eq!(mode.alpha_compare, AlphaCompare::None);
        assert!(mode.z_test);
        assert_eq!(material.cycle_type, 0);
        assert!(material.tex_enable);
        let foliage = matches!(surface, Surface::Foliage);
        assert_eq!(mode.cvg_x_alpha, foliage);
        assert_eq!(mode.z_write, foliage);
        assert_eq!(
            mode.fallback_class,
            if foliage {
                BlendClass::Replace
            } else {
                BlendClass::AlphaOver
            }
        );
        assert_eq!(
            mode.z_mode,
            match surface {
                Surface::Shadow => ZMode::Decal,
                Surface::Water => ZMode::Xlu,
                Surface::Foliage => ZMode::Opa,
            }
        );
        for cycle in [&material.selectors.cyc0, &material.selectors.cyc1] {
            if foliage {
                assert_eq!(cycle.cd, ColorIn::Texel0);
                assert_eq!(cycle.ad, AlphaIn::Texel0);
            } else {
                assert_eq!(cycle.ca, ColorIn::Texel0);
                assert_eq!(cycle.cc, ColorIn::Shade);
                assert_eq!(cycle.aa, AlphaIn::Texel0);
                assert_eq!(cycle.ac, AlphaIn::Shade);
            }
        }
        assert_eq!(scene.cn[4] >> 24, if foliage { 255 } else { 128 });
        assert_eq!(material.env[3], 17);
        assert_eq!(
            (material.tex_w, material.tex_h),
            match surface {
                Surface::Shadow => (16, 16),
                Surface::Water => (32, 32),
                Surface::Foliage => (32, 64),
            }
        );
        assert_eq!(
            scene.raw_pos[0][2] == scene.raw_pos[4][2],
            matches!(surface, Surface::Shadow)
        );
    }
}

fn assert_surface(surface: Surface) {
    let fixture = surface.fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert_eq!((output.width, output.height), (320, 240));
    assert_eq!(output.rgba8.len(), 320 * 240 * 4);
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    assert!(output
        .summaries
        .iter()
        .all(|s| s.renderable && s.errors == 0));
    for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % 320, i / 320);
        let mut expected = [64, 128, 192, 255];
        let [left, top, width, height] = surface.region();
        let inside = (left..left + width).contains(&x) && (top..top + height).contains(&y);
        let mut blended = false;
        if inside {
            let (s, t) = (x - left, y - top);
            match surface {
                Surface::Shadow => {
                    expected = [32, 64, 96, 128];
                    blended = true;
                }
                Surface::Water => {
                    expected = if s % 32 < 16 {
                        [32, 64, 224, 128]
                    } else {
                        [32, 192, 96, 128]
                    };
                    blended = true;
                }
                Surface::Foliage if s >= t / 4 && s < 32 - t / 4 && (s + t) % 7 != 0 => {
                    expected = [0, 255, 0, 255]
                }
                _ => {}
            }
        }
        let tolerance = u8::from(blended);
        assert!(
            pixel
                .iter()
                .zip(expected)
                .all(|(&a, b)| a.abs_diff(b) <= tolerance),
            "{surface:?} ({x},{y}): {pixel:?}, expected {expected:?}"
        );
    }
}

#[test]
fn fixture_sm64_shadow_decal() {
    assert_surface(Surface::Shadow);
}

#[test]
fn fixture_sm64_water_translucency() {
    assert_surface(Surface::Water);
}

#[test]
fn fixture_sm64_cutout_foliage() {
    assert_surface(Surface::Foliage);
}

fn write(surface: Surface, filename: &str) {
    super::capture_fixture::write(
        memory(surface),
        0x3000,
        320,
        240,
        filename,
        surface.provenance(),
    );
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_shadow_decal_fixture() {
    write(Surface::Shadow, "sm64-shadow-decal.f3dcap");
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_water_translucency_fixture() {
    write(Surface::Water, "sm64-water-translucency.f3dcap");
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_sm64_cutout_foliage_fixture() {
    write(Surface::Foliage, "sm64-cutout-foliage.f3dcap");
}
