use n64_gbi::encode::*;

fn interpret_commands(commands: impl IntoIterator<Item = (u32, u32)>) -> crate::hle::InterpResult {
    let rdram: Vec<u8> = commands
        .into_iter()
        .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
        .collect();
    crate::hle::interpret_rdram(&rdram, 0)
}

#[test]
fn roundtrip_known_f3dex2_stubs() {
    use crate::{DiagKind, Severity};

    let r = interpret_commands([
        gsp_spnoop(),
        gsp_line3d(1, 3),
        gsp_dma_io(true, 0x120, 0xffff_ffff, 32),
        gsp_enddl(),
    ]);
    assert_eq!(r.commands, 4);
    assert_eq!(r.dropped_runs, 1);
    assert_eq!(r.diags.len(), 2);
    assert_eq!(
        r.diags[0].kind,
        DiagKind::UnsupportedCommand {
            opcode: 0x08,
            w0: 0x0802_0600,
            w1: 0
        }
    );
    assert_eq!(r.diags[1].kind.severity(), Severity::Warn);

    let load = interpret_commands(
        gsp_load_ucode(0xffff_fff0, 0xffff_ffe0, 2048)
            .into_iter()
            .chain([gsp_line3d(0, 1), gsp_enddl()]),
    );
    assert_eq!(load.commands, 2);
    assert_eq!(load.dropped_runs, 0);
    assert_eq!(
        load.diags[0].kind,
        DiagKind::UnsupportedMicrocodeLoad {
            w0: 0xdd00_07ff,
            w1: 0xffff_fff0,
            data_address: Some(0xffff_ffe0),
        }
    );
}

#[test]
fn roundtrip_fill_rectangle() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_fill_color(0xCAFECAFE),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pairs = &r.scene.framebuffer_pairs;
    assert_eq!(pairs.len(), 1, "expected 1 framebuffer pair");
    let pair = &pairs[0];

    assert_eq!(pair.color_image.fmt, 0, "fmt should be RGBA(0)");
    assert_eq!(pair.color_image.siz, 2, "siz should be 16b(2)");
    assert_eq!(pair.color_image.width, 320);
    assert_eq!(pair.color_image.addr, 0x00100000);

    assert_eq!(pair.ops.len(), 1);
    match &pair.ops[0] {
        crate::hle::SceneOp::FillRect { rect, color_raw } => {
            assert_eq!(*color_raw, 0xCAFECAFE);
            assert_eq!(rect.ulx, 0);
            assert_eq!(rect.uly, 0);
            assert_eq!(rect.lrx, 320);
            assert_eq!(rect.lry, 240);
        }
        other => panic!("expected FillRect, got {:?}", other),
    }
}

#[test]
fn roundtrip_texture_rectangle() {
    let r = interpret_commands(
        [gdp_set_color_image(0, 2, 320, 0x00100000)]
            .into_iter()
            .chain(gsp_texture_rectangle(
                0, 0, 1280, 960, 0, 44, 52, 1024, 512, false,
            ))
            .chain([gsp_enddl()]),
    );
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pairs = &r.scene.framebuffer_pairs;
    assert_eq!(pairs.len(), 1);
    match &pairs[0].ops[0] {
        crate::hle::SceneOp::TexRect {
            rect,
            uls,
            ult,
            dsdx,
            dtdy,
            flip,
            ..
        } => {
            assert_eq!(rect.lrx, 1280);
            assert_eq!(rect.lry, 960);
            assert_eq!(rect.ulx, 0);
            assert_eq!(rect.uly, 0);
            assert_eq!(*uls, 44);
            assert_eq!(*ult, 52);
            assert_eq!(*dsdx, 1024);
            assert_eq!(*dtdy, 512);
            assert!(!*flip);
        }
        other => panic!("expected TexRect, got {:?}", other),
    }
}

#[test]
fn roundtrip_texture_rectangle_flip() {
    let r = interpret_commands(
        [gdp_set_color_image(0, 2, 320, 0x00100000)]
            .into_iter()
            .chain(gsp_texture_rectangle(
                0, 0, 1280, 960, 0, 11, 13, 1024, 512, true,
            ))
            .chain([gsp_enddl()]),
    );
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    match &r.scene.framebuffer_pairs[0].ops[0] {
        crate::hle::SceneOp::TexRect {
            flip,
            uls,
            ult,
            dsdx,
            dtdy,
            ..
        } => {
            assert!(*flip, "flip should be true for TextureRectangleFlip");
            assert_eq!(*uls, 11);
            assert_eq!(*ult, 13);
            assert_eq!(*dsdx, 1024);
            assert_eq!(*dtdy, 512);
        }
        other => panic!("expected TexRect (flip), got {:?}", other),
    }
}

#[test]
fn roundtrip_set_color_and_depth_image() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_depth_image(0x00200000),
        gdp_set_fill_color(0xFFFFFFFF),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pair = &r.scene.framebuffer_pairs[0];
    assert_eq!(pair.color_image.addr, 0x00100000);
    assert_eq!(pair.depth_image, Some(0x00200000));
}

#[test]
fn roundtrip_set_scissor() {
    let r = interpret_commands([
        gdp_set_color_image(0, 2, 320, 0x00100000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_set_fill_color(0xCAFECAFE),
        gdp_fill_rectangle(0, 0, 1280, 960),
        gsp_enddl(),
    ]);
    assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
    let pair = &r.scene.framebuffer_pairs[0];

    assert_eq!(pair.active_scissor.lrx, 320);
    assert_eq!(pair.active_scissor.lry, 240);
    assert_eq!(pair.active_scissor.mode, 0);

    assert_eq!(pair.ops.len(), 1);
    assert!(matches!(pair.ops[0], crate::hle::SceneOp::FillRect { .. }));
}

#[test]
fn tlut_count_and_destination_roundtrip() {
    let mut bytes: Vec<_> = [
        gdp_set_texture_image(0, 2, 1, 0x40),
        gdp_set_tile(0, 2, 0, 0x1fe, 3, 0, 0, 0, 0, 0, 0, 0),
        gdp_load_tlut(3, 2),
        gsp_enddl(),
    ]
    .into_iter()
    .flat_map(|(w0, w1)| [w0.to_be_bytes(), w1.to_be_bytes()].concat())
    .collect();
    bytes.resize(0x40, 0);
    bytes.extend([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
    let result = crate::hle::interpret_rdram(&bytes, 0);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.rdp.tiles[3].tmem_addr, 0x1fe);
    assert_eq!(
        &result.rdp.tmem_bank.palette()[0x7f0..],
        &[
            0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x56, 0x78, 0x56, 0x78, 0x56, 0x78,
            0x56, 0x78,
        ]
    );
    let tile = crate::hle::rdp::TileDescriptor {
        fmt: 3,
        siz: 2,
        width: 8,
        height: 1,
        ..Default::default()
    };
    assert_eq!(
        result.rdp.tmem_bank.sample_tile(&tile, 0).unwrap(),
        [
            0x9a, 0x9a, 0x9a, 0xbc, 0x9a, 0x9a, 0x9a, 0xbc, 0x9a, 0x9a, 0x9a, 0xbc, 0x9a, 0x9a,
            0x9a, 0xbc, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
}

fn modifyvtx_scene(commands: &[(u32, u32)], geometry: u32) -> crate::hle::Scene {
    use super::dl_builder::DlBuilder;
    use n64_gbi::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};

    let mut b = DlBuilder::new();
    let vertices = b.vertices(
        &[VtxColored {
            x: 1,
            y: -2,
            z: 3,
            flag: 0,
            s: 64,
            t: -96,
            r: 10,
            g: 20,
            b: 30,
            a: 40,
        }; 4],
    );
    let mut dl = vec![
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        gdp_set_combine_lerp(
            CcPass {
                a: 0,
                b: 0,
                c: 0,
                d: 4,
            },
            CcPass {
                a: 0,
                b: 0,
                c: 0,
                d: 4,
            },
            CcPass {
                a: 0,
                b: 0,
                c: 0,
                d: 4,
            },
            CcPass {
                a: 0,
                b: 0,
                c: 0,
                d: 4,
            },
        ),
        gsp_set_geometrymode(geometry),
        gsp_texture(32768, 16384, 0, 0, true),
        gsp_vertex(4, 4, vertices),
    ];
    dl.extend_from_slice(commands);
    dl.push(gsp_enddl());
    b.list("main", &dl);
    let built = b.finish("main");
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene
}

const MODIFY_TRI: (u32, u32) = (0x0508_0a0c, 0);

#[test]
fn modifyvtx_four_attributes() {
    for (command, first, second) in [
        (0x0210_0008, 0x1122_3344, 0xaabb_ccdd),
        (0x0214_0008, 0xffe0_0050, 0x0030_ffb0),
        (0x0218_0008, 0xfffc_0081, 0x0051_fff8),
        (0x021c_0008, 0x03ff_8000, 0x8000_4000),
    ] {
        let scene = modifyvtx_scene(
            &[
                MODIFY_TRI,
                (command, first),
                MODIFY_TRI,
                (command, second),
                MODIFY_TRI,
            ],
            0,
        );
        assert_eq!(scene.indices, [0, 1, 2, 4, 1, 2, 5, 1, 2]);
        assert_eq!(scene.cn[0], 0x281e_140a);
        assert_eq!(scene.raw_st[0], [64.0, -96.0]);
        assert_eq!(scene.modify_flags[0], 0);
        assert_eq!(scene.modify_screen[0], [0.0; 4]);
        assert_eq!(scene.raw_pos, [[1.0, -2.0, 3.0]; 6]);
        match command {
            0x0210_0008 => {
                assert_eq!([scene.cn[4], scene.cn[5]], [0x4433_2211, 0xddcc_bbaa]);
            }
            0x0214_0008 => {
                assert_eq!(scene.raw_st[4..], [[-1.0, 2.5], [1.5, -2.5]]);
                for i in [4, 5] {
                    assert_eq!(
                        scene.texcoord_table[scene.texcoord_index[i] as usize],
                        [1.0, 1.0]
                    );
                }
            }
            0x0218_0008 => {
                assert_eq!(scene.modify_flags[4..], [1, 1]);
                assert_eq!(
                    scene.modify_screen[4..],
                    [[-1.0, 32.25, 0.0, 0.0], [20.25, -2.0, 0.0, 0.0]]
                );
            }
            0x021c_0008 => {
                assert_eq!(scene.modify_flags[4..], [2, 2]);
                assert_eq!(
                    scene.modify_screen[4..],
                    [[0.0, 0.0, 1023.5, 0.0], [0.0, 0.0, 32768.25, 0.0]]
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn modifyvtx_used_vertex_is_copied() {
    let scene = modifyvtx_scene(
        &[
            gsp_modifyvertex(4, 0x10, 0x1122_3344),
            gsp_modifyvertex(4, 0x14, 0x0020_0040),
            MODIFY_TRI,
            gsp_modifyvertex(4, 0x10, 0xaabb_ccdd),
            gsp_modifyvertex(4, 0x14, 0x0060_0080),
            MODIFY_TRI,
        ],
        0,
    );
    assert_eq!(scene.raw_pos.len(), 5);
    assert_eq!(scene.indices, [0, 1, 2, 4, 1, 2]);
    assert_eq!([scene.cn[0], scene.cn[4]], [0x4433_2211, 0xddcc_bbaa]);
    assert_eq!([scene.raw_st[0], scene.raw_st[4]], [[1.0, 2.0], [3.0, 4.0]]);
    assert_eq!(scene.mtx_index[0], scene.mtx_index[4]);
    assert_eq!(scene.viewport_index[0], scene.viewport_index[4]);
    assert_eq!(scene.texcoord_index[0], scene.texcoord_index[4]);
}

#[test]
fn modifyvtx_preserves_previous_screen_changes() {
    let scene = modifyvtx_scene(
        &[
            MODIFY_TRI,
            (0x0218_0008, 0x0081_0102),
            MODIFY_TRI,
            (0x021c_0008, 0x0200_8000),
            MODIFY_TRI,
            (0x0210_0008, 0xaabb_ccdd),
            MODIFY_TRI,
            (0x0214_0008, 0xffe0_0040),
            MODIFY_TRI,
            (0x0218_0008, 0x0100_0200),
            MODIFY_TRI,
            (0x021c_0008, 0x0100_0000),
            MODIFY_TRI,
        ],
        0,
    );
    assert_eq!(
        scene
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t[0])
            .collect::<Vec<_>>(),
        [0, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(scene.modify_flags[4..], [1, 3, 3, 3, 3, 3]);
    assert_eq!(scene.modify_screen[4], [32.25, 64.5, 0.0, 0.0]);
    for i in 5..=7 {
        assert_eq!(scene.modify_screen[i], [32.25, 64.5, 512.5, 0.0]);
    }
    assert_eq!(scene.modify_screen[8], [64.0, 128.0, 512.5, 0.0]);
    assert_eq!(scene.modify_screen[9], [64.0, 128.0, 256.0, 0.0]);
    for i in 6..=9 {
        assert_eq!(scene.cn[i], 0xddcc_bbaa);
    }
    for i in 7..=9 {
        assert_eq!(scene.raw_st[i], [-1.0, 2.0]);
    }
}

#[test]
fn modifyvtx_invalid_slot_or_attr() {
    use super::dl_builder::DlBuilder;
    use crate::{DiagKind, Diagnostic, Severity};

    let mut b = DlBuilder::new();
    let vertices = b.vertices(&[VtxColored {
        x: 0,
        y: 0,
        z: 0,
        flag: 0,
        s: 0,
        t: 0,
        r: 1,
        g: 2,
        b: 3,
        a: 4,
    }]);
    let commands = [
        (0x0210_0000, 0xffff_ffff),
        gsp_vertex(7, 1, vertices),
        (0x0210_0000, 0xffff_ffff),
        (0x0210_000c, 0xffff_ffff),
        (0x0210_0010, 0xffff_ffff),
        (0x0210_0200, 0xffff_ffff),
        (0x0210_2469, 0xffff_ffff),
        (0x0210_fffe, 0xffff_ffff),
        (0x0211_000e, 0xffff_ffff),
        (0x0210_000f, 0x1122_3344),
        gsp_enddl(),
    ];
    b.list("main", &commands);
    let built = b.finish("main");
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    let expected: Vec<_> = [
        (0, 0, 0x10),
        (2, 0, 0x10),
        (3, 6, 0x10),
        (4, 8, 0x10),
        (5, 256, 0x10),
        (6, 0x1234, 0x10),
        (7, 0x7fff, 0x10),
        (8, 7, 0x11),
    ]
    .into_iter()
    .map(|(command, index, attribute)| Diagnostic {
        at: u64::from(built.entry) + command * 8,
        kind: DiagKind::InvalidModifyVertex { index, attribute },
    })
    .collect();
    assert_eq!(result.diags, expected);
    assert!(result
        .diags
        .iter()
        .all(|d| d.kind.severity() == Severity::Error));
    assert_eq!(result.commands, 11);
    assert_eq!(result.dropped_runs, 0);
    assert_eq!(result.scene.cn, [0x4433_2211]);
    assert_eq!(result.scene.raw_pos.len(), 1);
    assert_eq!(
        result.diags[0].kind.to_string(),
        "invalid MODIFYVTX slot or attribute: index=0, attribute=0x10"
    );
}

#[test]
fn modify_rgba_disables_fog_and_lighting() {
    use n64_gbi::consts::{G_FOG, G_LIGHTING};
    let scene = modifyvtx_scene(
        &[MODIFY_TRI, (0x0210_0008, 0x1234_5678), MODIFY_TRI],
        G_FOG | G_LIGHTING,
    );
    assert_ne!(scene.fog[0], 0);
    assert_ne!(scene.light_count[0], 0);
    assert_eq!(scene.cn[0], 0x281e_140a);
    assert_eq!(scene.cn[4], 0x7856_3412);
    assert_eq!(
        [scene.fog[4], scene.light_index[4], scene.light_count[4]],
        [0, 0, 0]
    );
    for i in 1..=3 {
        assert_eq!(scene.fog[i], scene.fog[0]);
        assert_eq!(scene.light_count[i], scene.light_count[0]);
    }
}

#[test]
fn modify_st_disables_texgen() {
    use n64_gbi::consts::{G_LIGHTING, G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR};
    for (extra, mode) in [(0, 1), (G_TEXTURE_GEN_LINEAR, 2)] {
        let scene = modifyvtx_scene(
            &[MODIFY_TRI, (0x0214_0008, 0xffd0_0050), MODIFY_TRI],
            G_LIGHTING | G_TEXTURE_GEN | extra,
        );
        assert_eq!(scene.texgen_mode[0..4], [mode; 4]);
        assert_eq!(scene.texgen_mode[4], 0);
        assert_eq!(scene.lookat_index[4], 0);
        assert_eq!(scene.raw_st[0], [64.0, -96.0]);
        assert_eq!(scene.raw_st[4], [-1.5, 2.5]);
        assert_eq!(
            scene.texcoord_table[scene.texcoord_index[4] as usize],
            [1.0, 1.0]
        );
        assert_eq!(scene.light_count[4], scene.light_count[0]);
    }
}

#[test]
fn quad_is_encoded_tri2() {
    for geometry in [0, 0x200, 0x400, 0x600] {
        let quad = modifyvtx_scene(&[(0x0708_0a0c, 0x000e_0c0a)], geometry);
        let tri2 = modifyvtx_scene(&[(0x0608_0a0c, 0x000e_0c0a)], geometry);
        assert_eq!(quad, tri2);
        let expected = match geometry {
            0x200 => vec![2, 1, 0, 1, 2, 3],
            0x600 => vec![],
            _ => vec![0, 1, 2, 3, 2, 1],
        };
        assert_eq!(quad.indices, expected);
    }
    let scene = modifyvtx_scene(&[gsp_quad(4, 5, 6, 7)], 0);
    assert_eq!(scene.indices, [0, 1, 2, 0, 2, 3]);
}
