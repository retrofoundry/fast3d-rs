use super::dl_builder::DlBuilder;
use crate::diag::{DiagKind, Diagnostic};
use crate::hle::interpret_rdram;
use crate::hle::rdp::TileDescriptor;
use crate::hle::texdec::FormatInfo;
use crate::hle::tmem::Tmem;
use n64_gbi::encode::*;

fn textured_rectangles(formats: &[(u8, u8)]) -> crate::hle::InterpResult {
    let mut dl = DlBuilder::new();
    let tex = dl.bytes(8, &[0xf8; 128]);
    let mut commands = vec![
        gdp_set_color_image(0, 2, 32, 0x1000),
        gdp_set_other_mode_h(20, 2, 2 << 20),
        gdp_set_texture_image(0, 2, 8, tex),
        gdp_set_tile(0, 2, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0),
        gdp_load_block(7, 0, 0, 63, 512),
    ];
    for &(fmt, siz) in formats {
        commands.push(gdp_set_tile(
            u32::from(fmt),
            u32::from(siz),
            1,
            0,
            0,
            0,
            2,
            0,
            0,
            2,
            0,
            0,
        ));
        commands.push(gdp_set_tile_size(0, 0, 0, 12, 4));
        commands.extend(gsp_texture_rectangle(
            0, 0, 12, 4, 0, 0, 0, 1024, 1024, false,
        ));
    }
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpret_rdram(&built.rdram, built.entry)
}

#[test]
fn unsupported_format_emits_once_at_draw() {
    let result = textured_rectangles(&[(1, 2), (1, 2), (5, 0), (5, 0)]);
    assert_eq!(
        result.diags,
        [
            Diagnostic {
                at: 200,
                kind: DiagKind::UnsupportedTextureFormat { fmt: 1, siz: 2 }
            },
            Diagnostic {
                at: 280,
                kind: DiagKind::UnsupportedTextureFormat { fmt: 5, siz: 0 }
            },
        ]
    );
    assert_eq!(result.dropped_runs, 4);
    assert_eq!(result.summary(false).errors, 2);
    assert_eq!(result.summary(false).warns, 0);
    assert!(result.scene.framebuffer_pairs.is_empty());
    assert!(result.scene.materials.is_empty());
}

#[test]
fn unsupported_format_never_decodes_rgba16() {
    let supported = [
        (0, 2),
        (0, 3),
        (2, 0),
        (2, 1),
        (3, 0),
        (3, 1),
        (3, 2),
        (4, 0),
        (4, 1),
    ];
    let mut encoded_unsupported = 0;
    for fmt in 0..=u8::MAX {
        for siz in 0..=u8::MAX {
            if supported.contains(&(fmt, siz)) {
                continue;
            }
            encoded_unsupported += usize::from(fmt < 8 && siz < 4);
            let error = DiagKind::UnsupportedTextureFormat { fmt, siz };
            assert_eq!(
                FormatInfo { fmt, siz }.decode(&[0xf8, 1], 1, 1, &[], 0, 0),
                Err(error)
            );
            let tile = TileDescriptor {
                fmt,
                siz,
                width: 1,
                height: 1,
                ..Default::default()
            };
            assert_eq!(Tmem::default().sample_tile(&tile, 0), Err(error));
            assert_eq!(Tmem::default().sampling_lookup(&tile, 0), Err(error));
            let empty = TileDescriptor {
                width: 0,
                height: 0,
                ..tile
            };
            assert_eq!(Tmem::default().sample_tile(&empty, 0), Err(error));
        }
    }
    assert_eq!(encoded_unsupported, 23);
    assert_eq!(
        FormatInfo { fmt: 0, siz: 3 }.decode(&[0xf8, 1], 1, 1, &[], 0, 0),
        Err(DiagKind::UnsupportedTextureFormat { fmt: 0, siz: 3 })
    );
}

#[test]
fn unsupported_format_inventory_rejects_each_draw() {
    for fmt in 0..8 {
        for siz in 0..4 {
            if (FormatInfo { fmt, siz }).validate().is_ok() {
                continue;
            }
            let result = textured_rectangles(&[(fmt, siz)]);
            assert_eq!(
                result.diags,
                [Diagnostic {
                    at: 200,
                    kind: DiagKind::UnsupportedTextureFormat { fmt, siz }
                }]
            );
            assert_eq!(result.dropped_runs, 1);
            assert!(result.scene.materials.is_empty());
        }
    }
}

#[test]
fn supported_fixtures_do_not_use_texture_fallbacks() {
    for fixture in super::fixtures::FIXTURES {
        let (bytes, entry) = super::fixtures::fixture(fixture.name);
        let result = interpret_rdram(bytes, entry as u32);
        assert!(
            !result.diags.iter().any(|diag| matches!(
                diag.kind,
                DiagKind::UnsupportedTextureFormat { .. }
                    | DiagKind::UnsupportedCommandParameters { opcode: 0xf3 }
            )),
            "{}: {:?}",
            fixture.name,
            result.diags
        );
    }
}

#[test]
fn unsupported_format_triangles_drop_and_recover() {
    let mut dl = DlBuilder::new();
    let tex = dl.bytes(8, &[0xf8; 16]);
    let vertices = dl.bytes(16, &[0; 48]);
    let prefix = [
        gsp_vertex(0, 3, vertices),
        (0xfc127e24, 0xfffff9fc),
        gdp_set_render_mode(0x00442078, 0),
        gdp_set_texture_image(0, 2, 4, tex),
        gdp_set_tile(0, 2, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0),
        gdp_load_block(7, 0, 0, 7, 0),
        gdp_set_tile(1, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 12, 4),
        gsp_texture(0xffff, 0xffff, 0, 0, true),
    ];
    let mut commands = prefix.to_vec();
    commands.extend([
        gsp_1triangle(0, 1, 2),
        gsp_1triangle(0, 1, 2),
        gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gsp_1triangle(0, 1, 2),
        gsp_enddl(),
    ]);
    dl.list("main", &commands);
    let built = dl.finish("main");
    for _ in 0..2 {
        let result = interpret_rdram(&built.rdram, built.entry);
        assert_eq!(
            result.diags,
            [Diagnostic {
                at: u64::from(built.entry) + prefix.len() as u64 * 8,
                kind: DiagKind::UnsupportedTextureFormat { fmt: 1, siz: 2 },
            }]
        );
        assert_eq!(result.dropped_runs, 2);
        assert_eq!(result.scene.draw_runs.len(), 1);
        assert_eq!(result.scene.materials.len(), 1);
    }
    let mut setters = DlBuilder::new();
    setters.list(
        "main",
        &[
            gdp_set_tile(7, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            gsp_enddl(),
        ],
    );
    let built = setters.finish("main");
    assert!(interpret_rdram(&built.rdram, built.entry).diags.is_empty());
}
