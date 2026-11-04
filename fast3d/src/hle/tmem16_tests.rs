use super::{
    combiner::build_material,
    interp::{interpret, interpret_rdram},
    rdp::{Rdp, TileDescriptor},
    rsp::Rsp,
    texture_request::{prepare, EncodedTextureRequest, Representation, TextureSource},
};
use crate::{
    inspect::WalkTermination,
    profiling::{Mode, Recorder},
    tests::dl_builder::DlBuilder,
    DataFormat, Microcode, RdramImage,
};
use n64_gbi::encode::*;

const CRYSTAL: [[u8; 4]; 16] = [
    [0, 0, 0, 17],
    [17, 17, 17, 34],
    [34, 34, 34, 51],
    [51, 51, 51, 68],
    [136, 136, 136, 153],
    [153, 153, 153, 170],
    [170, 170, 170, 187],
    [187, 187, 187, 204],
    [153, 153, 153, 0],
    [153, 153, 153, 17],
    [153, 153, 153, 34],
    [153, 153, 153, 51],
    [153, 153, 153, 136],
    [153, 153, 153, 153],
    [153, 153, 153, 170],
    [153, 153, 153, 187],
];

fn crystal() -> Rdp {
    let mut dl = DlBuilder::new();
    let wider = dl.bytes(8, &(0x80..0xa0).collect::<Vec<_>>());
    let narrow = dl.bytes(
        8,
        &[
            0x01, 0x12, 0x23, 0x34, 0x45, 0x56, 0x67, 0x78, 0x89, 0x9a, 0xab, 0xbc, 0xcd, 0xde,
            0xef, 0xf0,
        ],
    );
    let mut commands = gdp_load_texture_block(3, 1, 8, 4, wider, 2, 0, 2, 0);
    commands.extend(gdp_load_texture_block(3, 1, 4, 4, narrow, 2, 0, 2, 0));
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    let result = interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.termination, WalkTermination::End);
    result.rdp
}

fn i8(width: u16, height: u16) -> TileDescriptor {
    TileDescriptor {
        fmt: 4,
        siz: 1,
        width,
        height,
        line: 1,
        ..Default::default()
    }
}

fn encoded(source: &TextureSource) -> &EncodedTextureRequest {
    let TextureSource::Encoded(request) = source else {
        panic!("expected encoded TMEM request")
    };
    request
}

#[test]
fn crystal_physical_texels_include_retained_wider_load_bytes() {
    let rdp = crystal();
    let mut addresses = Vec::new();
    let pixels = rdp
        .tmem_bank
        .sample_tile_observed(&rdp.tiles[0], 0, &mut |address| addresses.push(address))
        .unwrap();
    assert_eq!(
        addresses,
        [0, 1, 2, 3, 12, 13, 14, 15, 16, 17, 18, 19, 28, 29, 30, 31]
    );
    assert_eq!(pixels, CRYSTAL.concat());
    assert_eq!(
        &rdp.tmem_bank.encoded_bank()[16..32],
        &[
            0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x9c, 0x9d, 0x9e, 0x9f, 0x98, 0x99,
            0x9a, 0x9b,
        ]
    );
}

#[test]
#[ignore = "S2: retire the mixed-load LinearCompat refusal"]
fn crystal_request_decodes_literal_physical_texels() {
    let rdp = crystal();
    let binding = prepare(&rdp, &rdp.tiles[0], 0, &Recorder::default()).unwrap();
    assert_eq!(
        encoded(&binding.source).recipe().representation,
        Representation::Tile
    );
    assert_eq!(binding.source.decode().as_ref(), CRYSTAL.concat());
}

#[test]
fn zero_loads_are_written_even_when_bank_bytes_do_not_change() {
    let mut rdp = Rdp::default();
    let tile = i8(4, 1);
    let before = *rdp.tmem_bank.encoded_bank();
    assert!(!rdp.tmem_bank.tile_has_load(&tile));
    rdp.tmem_bank.write_block(&[0; 8], 0, 0, 0, 1, 1);
    assert_eq!(rdp.tmem_bank.encoded_bank(), &before);
    assert!(rdp.tmem_bank.tile_has_load(&tile));
    let untouched = TileDescriptor {
        tmem_addr: 1,
        ..tile.clone()
    };
    assert!(!rdp.tmem_bank.tile_has_load(&untouched));
    let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
    assert_eq!(binding.source.decode().as_ref(), [[0; 4]; 4].concat());
}

#[test]
fn zero_load_prevents_the_load_free_framebuffer_shortcut() {
    for loaded in [false, true] {
        let mut dl = DlBuilder::new();
        let zeroes = dl.bytes(8, &[0; 8]);
        let mut commands = vec![
            gdp_set_color_image(0, 2, 4, 0x10000),
            gdp_set_scissor(0, 0, 0, 16, 4),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(0xf801f801),
            gdp_fill_rectangle(0, 0, 12, 0),
            gdp_set_color_image(0, 2, 4, 0x20000),
            gdp_set_cycle_type(2),
        ];
        if loaded {
            commands.extend(gdp_load_texture_block(0, 2, 4, 1, zeroes, 2, 0, 2, 0));
        }
        commands.extend([
            gdp_set_texture_image(0, 2, 4, 0x10000),
            gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
            gdp_set_tile_size(0, 0, 0, 12, 0),
        ]);
        commands.extend(gsp_texture_rectangle(
            0, 0, 16, 4, 0, 0, 0, 4096, 1024, false,
        ));
        commands.push(gsp_enddl());
        dl.list("main", &commands);
        let built = dl.finish("main");
        let result = interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let (material_index, fb_source) = result
            .scene
            .framebuffer_pairs
            .iter()
            .flat_map(|pair| &pair.ops)
            .find_map(|op| match op {
                crate::scene::SceneOp::TexRect {
                    material_index,
                    fb_source,
                    ..
                } => Some((*material_index, *fb_source)),
                _ => None,
            })
            .unwrap();
        assert_eq!(fb_source, (!loaded).then_some(0x10000));
        if loaded {
            assert_eq!(
                result.scene.materials[material_index as usize]
                    .texture
                    .decode()
                    .as_ref(),
                [[0; 4]; 4].concat()
            );
        }
    }
}

fn partial_overwrite() -> (Rdp, TileDescriptor) {
    let mut rdp = Rdp::default();
    rdp.tmem_bank.write_block(
        &[
            16, 17, 18, 19, 20, 21, 22, 23, 32, 33, 34, 35, 36, 37, 38, 39, 48, 49, 50, 51, 52, 53,
            54, 55,
        ],
        0,
        0,
        2048,
        3,
        1,
    );
    rdp.tmem_bank.write_tile(&[0; 8], 1, 1, 1, 1, 8, 1);
    (rdp, i8(3, 4))
}

const PARTIAL: [[u8; 4]; 12] = [
    [16; 4], [17; 4], [18; 4], [0; 4], [0; 4], [0; 4], [48; 4], [49; 4], [50; 4], [0; 4], [0; 4],
    [0; 4],
];

#[test]
fn partial_overwrite_keeps_stale_rows_and_initial_zeroes() {
    let (rdp, tile) = partial_overwrite();
    assert_eq!(
        rdp.tmem_bank.sample_tile(&tile, 0).unwrap(),
        PARTIAL.concat()
    );
    assert!(rdp.tmem_bank.tile_has_load(&TileDescriptor {
        height: 1,
        tmem_addr: 1,
        ..tile.clone()
    }));
    assert!(!rdp.tmem_bank.tile_has_load(&TileDescriptor {
        height: 1,
        tmem_addr: 3,
        ..tile
    }));
}

#[test]
#[ignore = "S2: route partially overwritten block loads through physical TMEM"]
fn partial_overwrite_request_decodes_literal_physical_texels() {
    let (rdp, tile) = partial_overwrite();
    let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
    assert_eq!(binding.source.decode().as_ref(), PARTIAL.concat());
}

fn odd_nibbles() -> (Rdp, TileDescriptor) {
    let mut rdp = Rdp::default();
    rdp.tmem_bank.write_block(
        &[
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde,
            0xf0, 0x12, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34,
        ],
        0,
        0,
        2048,
        3,
        0,
    );
    (rdp, TileDescriptor { siz: 0, ..i8(3, 3) })
}

const ODD_NIBBLES: [[u8; 4]; 9] = [
    [17; 4], [34; 4], [51; 4], [51; 4], [68; 4], [85; 4], [85; 4], [102; 4], [119; 4],
];

#[test]
fn odd_width_four_bit_physical_rows_restart_nibble_parity() {
    let (rdp, tile) = odd_nibbles();
    assert_eq!(
        rdp.tmem_bank.sample_tile(&tile, 0).unwrap(),
        ODD_NIBBLES.concat()
    );
}

#[test]
#[ignore = "S2: retire linear nibble parity across odd-width rows"]
fn odd_width_four_bit_request_restarts_nibble_parity() {
    let (rdp, tile) = odd_nibbles();
    let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
    assert_eq!(binding.source.decode().as_ref(), ODD_NIBBLES.concat());
}

#[test]
fn dxt_rounding_changes_the_swap_at_the_third_word() {
    let tile = TileDescriptor {
        line: 3,
        ..i8(24, 2)
    };
    for (dxt, expected) in [
        (
            683,
            [
                24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
                45, 46, 47,
            ],
        ),
        (
            682,
            [
                28, 29, 30, 31, 24, 25, 26, 27, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
                45, 46, 47,
            ],
        ),
    ] {
        let mut rdp = Rdp::default();
        rdp.tmem_bank
            .write_block(&(0..48).collect::<Vec<_>>(), 0, 0, dxt, 6, 1);
        let physical = rdp.tmem_bank.sample_tile(&tile, 0).unwrap();
        assert_eq!(
            &physical[..96],
            [
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                23
            ]
            .map(|v| [v; 4])
            .concat()
        );
        assert_eq!(
            &physical[96..],
            expected.map(|v| [v; 4]).concat(),
            "dxt={dxt}"
        );
        let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
        assert_eq!(binding.source.decode().as_ref(), physical);
    }
}

#[test]
fn lookup_threshold_is_strict_and_mirroring_does_not_disable_it() {
    let mut rdp = Rdp::default();
    rdp.tmem_bank
        .write_block(&[17, 34, 51, 68, 85, 102, 119, 136], 0, 0, 0, 1, 1);
    for (mask, mode, representation) in [
        (2, 0, Representation::LinearCompat),
        (3, 0, Representation::Lookup4096x4),
        (3, 1, Representation::Lookup4096x4),
        (3, 2, Representation::LinearCompat),
    ] {
        let tile = TileDescriptor {
            masks: mask,
            cms: mode,
            ..i8(4, 1)
        };
        let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
        assert_eq!(
            encoded(&binding.source).recipe().representation,
            representation
        );
        let pixels = binding.source.decode();
        assert_eq!(&pixels[..16], [[17; 4], [34; 4], [51; 4], [68; 4]].concat());
        if representation == Representation::Lookup4096x4 {
            assert_eq!(
                &pixels[16..32],
                [[85; 4], [102; 4], [119; 4], [136; 4]].concat()
            );
        }
    }
}

#[test]
#[ignore = "S2: route equal-mask and clamped narrow block requests to physical tiles"]
fn lookup_threshold_keeps_literal_texels_after_routing_changes() {
    let mut rdp = Rdp::default();
    rdp.tmem_bank
        .write_block(&[17, 34, 51, 68, 85, 102, 119, 136], 0, 0, 0, 1, 1);
    for (mask, mode) in [(2, 0), (3, 2)] {
        let tile = TileDescriptor {
            masks: mask,
            cms: mode,
            ..i8(4, 1)
        };
        let binding = prepare(&rdp, &tile, 0, &Recorder::default()).unwrap();
        assert_eq!(
            encoded(&binding.source).recipe().representation,
            Representation::Tile
        );
        assert_eq!(
            binding.source.decode().as_ref(),
            [[17; 4], [34; 4], [51; 4], [68; 4]].concat()
        );
    }
}

#[test]
fn ci_identity_tracks_used_palette_entries_only() {
    for (siz, palette, base, indices) in [
        (1, 0, 256, [0, 1, 0, 1, 0, 0, 0, 0]),
        (0, 3, 304, [0x01, 0x01, 0, 0, 0, 0, 0, 0]),
    ] {
        let mut rdp = Rdp::default();
        let tile = TileDescriptor {
            fmt: 2,
            siz,
            palette,
            ..i8(4, 1)
        };
        rdp.tmem_bank.write_tile(&indices, 0, 1, 1, 1, 8, siz);
        rdp.tmem_bank
            .write_tlut(&[0xf8, 1, 7, 0xc1, 0, 0x3f], 3, base);
        let first = prepare(&rdp, &tile, 2, &Recorder::default()).unwrap();
        let initial = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
        ];
        assert_eq!(first.source.decode().as_ref(), initial.concat());
        rdp.tmem_bank.write_tlut(&[0xff, 0xff], 1, base + 2);
        let unused = prepare(&rdp, &tile, 2, &Recorder::default()).unwrap();
        assert_eq!(unused.source.decode().as_ref(), initial.concat());
        assert_eq!(
            encoded(&first.source).witness(),
            encoded(&unused.source).witness()
        );
        rdp.tmem_bank.write_tlut(&[0, 0x3f], 1, base + 1);
        let used = prepare(&rdp, &tile, 2, &Recorder::default()).unwrap();
        assert_eq!(
            used.source.decode().as_ref(),
            [
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                [255, 0, 0, 255],
                [0, 0, 255, 255]
            ]
            .concat()
        );
        assert_ne!(
            encoded(&first.source).witness(),
            encoded(&used.source).witness()
        );
        drop(rdp);
        assert_eq!(first.source.decode().as_ref(), initial.concat());
    }
}

#[test]
fn texture0_texture1_lod_and_detail_have_literal_physical_texels() {
    let mut rdp = Rdp {
        texture_loaded: true,
        combine_l: 0x0088_7f10,
        combine_h: 0x88fc_fc7e,
        other_mode_h: (1 << 20) | (1 << 16) | (2 << 17),
        ..Default::default()
    };
    for (index, base, value) in [(0, 0, 112), (1, 4, 32), (2, 8, 64)] {
        rdp.tiles[index] = TileDescriptor {
            tmem_addr: base,
            ..i8(3, 2)
        };
        rdp.tmem_bank
            .write_tile(&[value; 16], base as usize, 1, 2, 1, 8, 1);
    }
    let profiling = Recorder::new(Mode::Trace);
    let mut rsp = Rsp::default();
    rsp.profiling = profiling.clone();
    rsp.set_texture(1, 1, true, 65535, 65535);
    let mut diags = Vec::new();
    let material = build_material(&rdp, &rsp, &mut diags, 0x80).unwrap();
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(material.texture.decode().as_ref(), [[32; 4]; 6].concat());
    assert_eq!(
        material.tex1.as_ref().unwrap().texture.decode().as_ref(),
        [[64; 4]; 6].concat()
    );
    assert_eq!(
        material.mip_levels[0].texture.decode().as_ref(),
        [[32; 4]; 6].concat()
    );
    assert_eq!(
        material.mip_levels[1].texture.decode().as_ref(),
        [[64; 4]; 6].concat()
    );
    assert_eq!(
        material
            .detail_tex
            .as_ref()
            .unwrap()
            .texture
            .decode()
            .as_ref(),
        [[112; 4]; 6].concat()
    );
    let snapshot = profiling.drain();
    assert_eq!(
        snapshot
            .requests
            .iter()
            .map(|request| request.role.as_str())
            .collect::<Vec<_>>(),
        ["texture0", "texture1", "lod0", "lod1", "detail"]
    );
}

#[test]
#[ignore = "S2: all texture roles must admit retained physical crystal texels"]
fn crystal_literal_texels_reach_each_material_role() {
    for role in ["texture0", "texture1", "lod1", "detail"] {
        let mut rdp = crystal();
        rdp.tmem_bank.write_tile(&[85; 8], 32, 1, 1, 1, 8, 1);
        rdp.tiles[1] = TileDescriptor {
            tmem_addr: 32,
            ..i8(8, 1)
        };
        rdp.combine_l = 0x00ff_ffff;
        rdp.combine_h = 0xfffc_f279;
        let mut rsp = Rsp::default();
        rsp.set_texture(0, 0, true, 65535, 65535);
        match role {
            "texture1" => {
                rdp.tiles.swap(0, 1);
                rdp.combine_l = 0x0088_7f10;
                rdp.combine_h = 0x88fc_fc7e;
                rdp.other_mode_h = 1 << 20;
            }
            "lod1" => {
                rdp.tiles.swap(0, 1);
                rdp.other_mode_h = 1 << 16;
                rsp.texture_state.level = 1;
            }
            "detail" => {
                rdp.tiles[2] = rdp.tiles[1].clone();
                rdp.other_mode_h = (1 << 16) | (2 << 17);
                rsp.texture_state.tile = 1;
                rsp.texture_state.level = 1;
            }
            _ => {}
        }
        let mut diags = Vec::new();
        let material = build_material(&rdp, &rsp, &mut diags, 0x80)
            .unwrap_or_else(|| panic!("{role}: {diags:?}"));
        assert!(diags.is_empty(), "{role}: {diags:?}");
        let source = match role {
            "texture1" => &material.tex1.as_ref().unwrap().texture,
            "lod1" => &material.mip_levels[1].texture,
            "detail" => &material.detail_tex.as_ref().unwrap().texture,
            _ => &material.texture,
        };
        assert_eq!(
            encoded(source).recipe().representation,
            Representation::Tile
        );
        assert_eq!(source.decode().as_ref(), CRYSTAL.concat(), "{role}");
    }
}

#[test]
fn census_keeps_later_roles_after_the_first_texture_refuses() {
    let mut rdp = crystal();
    rdp.tiles[1] = rdp.tiles[0].clone();
    for (index, base, value) in [(0, 32, 80), (2, 33, 96)] {
        rdp.tiles[index] = TileDescriptor {
            tmem_addr: base,
            ..i8(8, 1)
        };
        rdp.tmem_bank
            .write_tile(&[value; 8], base as usize, 1, 1, 1, 8, 1);
    }
    rdp.combine_l = 0x0088_7f10;
    rdp.combine_h = 0x88fc_fc7e;
    rdp.other_mode_h = (1 << 20) | (1 << 16) | (2 << 17);
    let profiling = Recorder::new(Mode::Census);
    let mut rsp = Rsp::default();
    rsp.profiling = profiling.clone();
    rsp.set_texture(1, 1, true, 65535, 65535);
    let mut diags = Vec::new();
    assert!(build_material(&rdp, &rsp, &mut diags, 0x80).is_none());
    assert_eq!(
        diags,
        [crate::Diagnostic {
            at: 0x80,
            kind: crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 }
        }]
    );
    let snapshot = profiling.drain();
    assert_eq!(
        snapshot
            .census_attempts
            .iter()
            .map(|attempt| attempt.role.as_str())
            .collect::<Vec<_>>(),
        ["tex0", "tex1", "lod0", "lod1", "detail"]
    );
    for attempt in &snapshot.census_attempts {
        assert_eq!(attempt.pc, 0x80);
        let expected = match attempt.role.as_str() {
            "tex0" | "lod0" => {
                assert_eq!(
                    attempt.old_outcome,
                    "TextureBytesUnavailable { tmem_addr: 0 }"
                );
                CRYSTAL.concat()
            }
            "tex1" | "lod1" => {
                assert_eq!(attempt.old_outcome, "Tile");
                [[96; 4]; 8].concat()
            }
            "detail" => {
                assert_eq!(attempt.old_outcome, "Tile");
                [[80; 4]; 8].concat()
            }
            role => panic!("unexpected role {role}"),
        };
        assert_eq!(attempt.physical, expected, "{}", attempt.role);
    }
}

fn process_task(
    interpreter: &mut crate::profiling::CpuInterpreter,
    mut dl: DlBuilder,
    mut commands: Vec<(u32, u32)>,
) -> Vec<crate::Diagnostic> {
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpreter
        .process(
            RdramImage::new(&built.rdram),
            u64::from(built.entry),
            Microcode::F3dex2,
            DataFormat::Fixed,
        )
        .1
}

#[test]
fn census_preserves_task_history_and_rolls_back_failed_guest_reads() {
    let mut interpreter = crate::profiling::CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Census);
    let mut first = DlBuilder::new();
    let source = first.bytes(8, &[17, 18, 19, 20, 21, 22, 23, 24]);
    let mut commands = vec![gdp_set_color_image(0, 2, 8, 0x10000), gdp_set_cycle_type(2)];
    commands.extend(gdp_load_texture_block(4, 1, 8, 1, source, 2, 0, 2, 0));
    let draw = gsp_texture_rectangle(0, 0, 32, 4, 0, 0, 0, 4096, 1024, false);
    commands.extend(draw);
    assert!(process_task(&mut interpreter, first, commands).is_empty());
    let initial = interpreter.recorder.drain();
    assert_eq!(initial.census_attempts.len(), 1);
    let expected = [
        [17; 4], [18; 4], [19; 4], [20; 4], [21; 4], [22; 4], [23; 4], [24; 4],
    ]
    .concat();
    assert_eq!(initial.census_attempts[0].physical, expected);

    let mut failed = DlBuilder::new();
    let source = failed.bytes(8, &[91; 8]);
    let mut commands = gdp_load_texture_block(4, 1, 8, 1, source, 2, 0, 2, 0);
    commands.extend(draw);
    commands.extend([
        gdp_set_texture_image(4, 1, 8, 0x1fff_ffff),
        gdp_load_block(7, 0, 0, 7, 0),
    ]);
    let diags = process_task(&mut interpreter, failed, commands);
    assert!(
        diags
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, crate::DiagKind::MemoryRead { .. })),
        "{diags:?}"
    );
    let rejected = interpreter.recorder.drain();
    assert_eq!(rejected.census_attempts.len(), 1);
    assert_eq!(rejected.census_attempts[0].physical, [[91; 4]; 8].concat());

    assert!(process_task(&mut interpreter, DlBuilder::new(), draw.to_vec()).is_empty());
    let retained = interpreter.recorder.drain();
    assert!(retained.census_loads.is_empty());
    assert_eq!(retained.census_attempts.len(), 1);
    assert_eq!(retained.census_attempts[0].physical, expected);
    assert_eq!(
        retained.census_attempts[0].bank,
        initial.census_attempts[0].bank
    );
}

#[test]
fn s2dex_generated_zero_loads_reach_the_encoded_texture_boundary() {
    let (mut bytes, _) = super::s2dex_bg_tests::fixture(8, 2);
    bytes[0x1000..].fill(0);
    bytes[0x400..0x402].copy_from_slice(&[0xf8, 1]);
    let result = interpret(
        RdramImage::new(&bytes),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.termination, WalkTermination::End);
    assert!(result.rdp.tmem_bank.tile_has_load(&result.rdp.tiles[0]));
    assert_eq!(result.rdp.tiles[0].masks, 15);
    assert!(!result.scene.materials.is_empty());
    for material in &result.scene.materials {
        assert_eq!(
            encoded(&material.texture).recipe().representation,
            Representation::Lookup4096x4
        );
        let pixels = material.texture.decode();
        assert!(!pixels.is_empty());
        for pixel in pixels.as_chunks::<4>().0 {
            assert_eq!(*pixel, [255, 0, 0, 255]);
        }
    }
    for op in result
        .scene
        .framebuffer_pairs
        .iter()
        .flat_map(|pair| &pair.ops)
    {
        if let crate::scene::SceneOp::TexRect { fb_source, .. } = op {
            assert_eq!(*fb_source, None);
        }
    }
}

#[test]
fn census_framebuffer_metadata_keeps_the_latest_color_writer_over_depth() {
    let mut interpreter = crate::profiling::CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Census);
    for depth in [0x10000, 0x30000] {
        let diags = process_task(
            &mut interpreter,
            DlBuilder::new(),
            vec![
                gdp_set_color_image(0, 2, 4, 0x10000),
                gdp_set_depth_image(depth),
                gdp_set_scissor(0, 0, 0, 16, 4),
                gdp_set_cycle_type(3),
                gdp_set_fill_color(0xf801f801),
                gdp_fill_rectangle(0, 0, 12, 0),
            ],
        );
        assert!(diags.is_empty(), "{diags:?}");
    }
    interpreter.recorder.drain();
    let mut commands = vec![gdp_set_color_image(0, 2, 4, 0x20000), gdp_set_cycle_type(2)];
    commands.extend(gdp_load_texture_block(0, 2, 4, 1, 0x10000, 2, 0, 2, 0));
    commands.extend(gsp_texture_rectangle(
        0, 0, 16, 4, 0, 0, 0, 4096, 1024, false,
    ));
    let diags = process_task(&mut interpreter, DlBuilder::new(), commands);
    assert!(diags.is_empty(), "{diags:?}");
    let attempts = interpreter.recorder.drain().census_attempts;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].old_outcome, "Framebuffer");
}

#[test]
fn census_framebuffer_metadata_excludes_copy_only_destinations() {
    let mut interpreter = crate::profiling::CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Census);
    assert!(process_task(
        &mut interpreter,
        DlBuilder::new(),
        vec![
            gdp_set_color_image(0, 2, 4, 0x10000),
            gdp_set_scissor(0, 0, 0, 16, 4),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(0xf801f801),
            gdp_fill_rectangle(0, 0, 12, 0),
        ]
    )
    .is_empty());
    let mut commands = vec![gdp_set_color_image(0, 2, 4, 16)];
    commands.extend(gdp_load_texture_block(0, 2, 4, 1, 0x10000, 2, 0, 2, 0));
    assert!(process_task(&mut interpreter, DlBuilder::new(), commands).is_empty());
    interpreter.recorder.drain();
    let mut dl = DlBuilder::new();
    let source = dl.bytes(8, &[0xf8, 1, 0xf8, 1, 0xf8, 1, 0xf8, 1]);
    assert_eq!(source, 16);
    let mut commands = vec![gdp_set_color_image(0, 2, 4, 0x20000), gdp_set_cycle_type(2)];
    commands.extend(gdp_load_texture_block(0, 2, 4, 1, source, 2, 0, 2, 0));
    commands.extend(gsp_texture_rectangle(
        0, 0, 16, 4, 0, 0, 0, 4096, 1024, false,
    ));
    let diags = process_task(&mut interpreter, dl, commands);
    assert!(diags.is_empty(), "{diags:?}");
    let attempts = interpreter.recorder.drain().census_attempts;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].old_outcome, "Tile");
    assert_eq!(attempts[0].physical, [[255, 0, 0, 255]; 4].concat());
}
