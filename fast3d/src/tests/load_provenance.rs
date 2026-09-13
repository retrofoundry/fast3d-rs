use super::dl_builder::DlBuilder;
use crate::hle::interpret_rdram;
use n64_gbi::encode::*;

fn load(via_tile: bool, addr: u32, base: u32, small: bool) -> Vec<(u32, u32)> {
    let mut commands = vec![
        gdp_set_texture_image(4, 1, 16, addr),
        gdp_set_tile(4, 1, u32::from(via_tile), base, 7, 0, 2, 0, 0, 2, 0, 0),
    ];
    commands.push(if via_tile {
        (
            0xf400_0000,
            (7 << 24) | (8 << 12) | if small { 0 } else { 8 },
        )
    } else {
        gdp_load_block(7, 0, 0, if small { 7 } else { 23 }, 2048)
    });
    commands
}

fn two_loads(
    first_tile: bool,
    second_tile: bool,
    base: u32,
    small: bool,
) -> crate::hle::InterpResult {
    let mut dl = DlBuilder::new();
    let first = dl.bytes(8, &(0..48).map(|i| 0x10 + i).collect::<Vec<u8>>());
    let second = dl.bytes(8, &[0xee; 48]);
    let mut commands = vec![
        gdp_set_color_image(0, 2, 32, 0x1000),
        gdp_set_other_mode_h(20, 2, 2 << 20),
    ];
    commands.extend(load(first_tile, first, 0, false));
    commands.extend([
        gdp_set_tile(4, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 8, 8),
    ]);
    commands.extend(gsp_texture_rectangle(
        0, 0, 12, 12, 0, 0, 0, 4096, 1024, false,
    ));
    commands.extend(load(second_tile, second, base, small));
    commands.extend(gsp_texture_rectangle(
        0, 0, 12, 12, 0, 0, 0, 4096, 1024, false,
    ));
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpret_rdram(&built.rdram, built.entry)
}

fn intensities(bytes: &[u8]) -> Vec<u8> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[0])
        .collect()
}

fn rect_texture(result: &crate::hle::InterpResult, index: usize) -> Vec<u8> {
    let crate::scene::SceneOp::TexRect { material_index, .. } =
        result.scene.framebuffer_pairs[0].ops[index]
    else {
        panic!("expected rectangle")
    };
    result.scene.materials[material_index as usize]
        .texture
        .decode()
        .into_owned()
}

#[test]
fn load_provenance_disjoint_block_preserves_loadtile() {
    let result = two_loads(true, false, 32, true);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let expected = [16, 17, 18, 32, 33, 34, 48, 49, 50];
    let bank = result
        .rdp
        .tmem_bank
        .sample_tile(&result.rdp.tiles[0], 0)
        .unwrap();
    assert_eq!(intensities(&bank), expected);
    assert_eq!(
        intensities(&result.scene.materials[0].texture.decode()),
        expected
    );
    assert_eq!(intensities(&rect_texture(&result, 1)), expected);
}

#[test]
fn load_provenance_disjoint_loads_preserve_both_layouts() {
    for first in [false, true] {
        for second in [false, true] {
            let result = two_loads(first, second, 32, true);
            assert!(
                result.diags.is_empty(),
                "{first}/{second}: {:?}",
                result.diags
            );
            let expected = if first {
                vec![16, 17, 18, 32, 33, 34, 48, 49, 50]
            } else {
                (16..25).collect()
            };
            assert_eq!(
                intensities(&rect_texture(&result, 1)),
                expected,
                "{first}/{second}"
            );
        }
    }
}

#[test]
fn load_provenance_overwriting_loads_replace_both_layouts() {
    for first in [false, true] {
        for second in [false, true] {
            let result = two_loads(first, second, 0, false);
            assert!(
                result.diags.is_empty(),
                "{first}/{second}: {:?}",
                result.diags
            );
            assert_eq!(
                intensities(&rect_texture(&result, 1)),
                [0xee; 9],
                "{first}/{second}"
            );
        }
    }
}

#[test]
fn load_provenance_missing_block_bytes_drops_draw() {
    for second in [false, true] {
        let result = two_loads(false, second, 1, true);
        assert_eq!(result.dropped_runs, 1, "{second}: {:?}", result.diags);
        assert_eq!(result.diags.len(), 1);
        assert_eq!(
            result.diags[0].kind,
            crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 }
        );
        assert_eq!(result.summary(false).errors, 1);
        assert_eq!(result.scene.materials.len(), 1);
    }
}

#[test]
fn load_provenance_partial_base_overwrite_cannot_repoint_surviving_rows() {
    let mut state = two_loads(true, false, 0, true).rdp;
    state.tiles[0].height = 2;
    let mut diags = Vec::new();
    assert!(crate::hle::combiner::build_rect_material(
        &state,
        &Default::default(),
        0,
        &mut diags,
        0
    )
    .is_none());
    assert_eq!(
        diags[0].kind,
        crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 }
    );
}

#[test]
fn load_provenance_recovers_gapped_block_at_its_base() {
    let mut state = two_loads(false, true, 32, true).rdp;
    state.tmem_bank = Default::default();
    state
        .tmem_bank
        .write_block(&(16..40).collect::<Vec<_>>(), 0, 1, 2048, 3, 1);
    let material = crate::hle::combiner::build_rect_material(
        &state,
        &Default::default(),
        0,
        &mut Vec::new(),
        0,
    )
    .unwrap();
    assert_eq!(
        intensities(&material.texture.decode()),
        (16..25).collect::<Vec<_>>()
    );
}

#[test]
fn load_provenance_second_texture_and_lod_use_retained_linear_bytes() {
    for lod in [false, true] {
        let mut state = two_loads(false, true, 32, true).rdp;
        state.tiles[1] = state.tiles[0].clone();
        state.tiles[0].tmem_addr = 32;
        state.tiles[0].width = 8;
        state.tiles[0].height = 1;
        state.combine_l = 0x0088_7f10;
        state.combine_h = 0x88fc_fc7e;
        state.other_mode_h = (1 << 20) | if lod { 1 << 16 } else { 0 };
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.set_texture(0, u8::from(lod), true, 65535, 65535);
        let mut diags = Vec::new();
        let material = crate::hle::combiner::build_material(&state, &rsp, &mut diags, 0)
            .unwrap_or_else(|| panic!("{lod}: {diags:?}"));
        assert_eq!(
            intensities(&material.tex1.unwrap().texture.decode()),
            (16..25).collect::<Vec<_>>()
        );
        if lod {
            assert!(material.lod);
            assert_eq!(
                intensities(&material.mip_levels[1].texture.decode()),
                (16..25).collect::<Vec<_>>()
            );
        }
    }
}

fn narrow_state(base: u16) -> crate::hle::rdp::Rdp {
    let mut state = crate::hle::rdp::Rdp {
        other_mode_h: 2 << 20,
        ..Default::default()
    };
    state.tiles[0] = crate::hle::rdp::TileDescriptor {
        fmt: 4,
        siz: 1,
        width: 3,
        height: 3,
        line: 1,
        tmem_addr: base,
        cms: 2,
        cmt: 2,
        ..Default::default()
    };
    state
}

fn decode(state: &crate::hle::rdp::Rdp) -> Result<Vec<u8>, crate::DiagKind> {
    let mut diags = Vec::new();
    crate::hle::combiner::build_rect_material(state, &Default::default(), 0, &mut diags, 0x80)
        .map(|material| material.texture.decode().into_owned())
        .ok_or_else(|| diags[0].kind)
}

#[test]
fn load_provenance_nonzero_wrapped_and_interior_bases() {
    for base in [32, 511] {
        for offset in [0, 1] {
            let mut state = narrow_state((base + offset) & 511);
            state.tmem_bank.write_block(
                &(16..48).collect::<Vec<_>>(),
                base as usize,
                0,
                2048,
                4,
                1,
            );
            for _ in 0..1024 {
                state.tmem_bank.write_block(&[0xee; 8], 64, 0, 0, 1, 1);
            }
            let first = 16 + offset as u8 * 8;
            assert_eq!(
                intensities(&decode(&state).unwrap()),
                (first..first + 9).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn load_provenance_overwrite_elsewhere_in_same_block_preserves_subregion() {
    let mut state = narrow_state(2);
    state
        .tmem_bank
        .write_block(&(16..80).collect::<Vec<_>>(), 0, 0, 2048, 8, 1);
    state.tmem_bank.write_tile(&[0xee; 8], 0, 1, 1, 1, 8, 1);
    assert_eq!(
        intensities(&decode(&state).unwrap()),
        (32..41).collect::<Vec<_>>()
    );
}

#[test]
fn load_provenance_self_overwrite_only_loses_overwritten_source_bytes() {
    let mut state = narrow_state(0);
    let mut source = vec![0xee; 4104];
    source[..24].copy_from_slice(&(16..40).collect::<Vec<_>>());
    state.tmem_bank.write_block(&source, 0, 0, 2048, 513, 1);
    assert_eq!(
        decode(&state),
        Err(crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 })
    );
    state.tiles[0].tmem_addr = 1;
    assert_eq!(
        intensities(&decode(&state).unwrap()),
        (24..33).collect::<Vec<_>>()
    );
}

#[test]
fn load_provenance_palette_and_rgba32_writes_invalidate_overlapping_bytes() {
    for base in [0, 256] {
        for palette in [false, true] {
            let mut state = narrow_state(base);
            state.tmem_bank.write_block(
                &(16..40).collect::<Vec<_>>(),
                base as usize,
                0,
                2048,
                3,
                1,
            );
            if palette {
                state.tmem_bank.write_tlut(&[0xee; 2], 1, base as usize + 1);
            } else {
                state.tmem_bank.write_block(&[0xee; 8], 1, 0, 0, 1, 3);
                assert_eq!(
                    intensities(&decode(&state).unwrap()),
                    (16..25).collect::<Vec<_>>()
                );
                state.tmem_bank.write_block(&[0xee; 16], 1, 0, 0, 2, 3);
            }
            assert_eq!(
                decode(&state),
                Err(crate::DiagKind::TextureBytesUnavailable { tmem_addr: base })
            );
        }
    }
}

#[test]
fn load_provenance_tile_padding_does_not_change_decode_path() {
    let mut state = narrow_state(0);
    state.tiles[0].line = 2;
    state
        .tmem_bank
        .write_tile(&(16..64).collect::<Vec<_>>(), 0, 2, 3, 1, 16, 1);
    state.tmem_bank.write_block(&[0xee; 8], 1, 0, 0, 1, 1);
    assert_eq!(
        intensities(&decode(&state).unwrap()),
        [16, 17, 18, 32, 33, 34, 48, 49, 50]
    );
}

#[test]
fn load_provenance_retained_ci_uses_live_palette_and_low_bank_mask() {
    let mut state = narrow_state(256);
    state.tiles[0].fmt = 2;
    state.other_mode_h |= 3 << 14;
    state.tmem_bank.write_block(&[0; 24], 0, 0, 2048, 3, 1);
    state.tmem_bank.write_tlut(&[0x70, 0x80], 1, 256);
    assert_eq!(decode(&state).unwrap(), [0x70, 0x70, 0x70, 0x80].repeat(9));
    state.tmem_bank.write_tile(&[0xee; 8], 64, 1, 1, 1, 8, 1);
    state.tmem_bank.write_tlut(&[0x90, 0xa0], 1, 256);
    assert_eq!(decode(&state).unwrap(), [0x90, 0x90, 0x90, 0xa0].repeat(9));
}

#[test]
fn load_provenance_missing_bytes_in_every_used_input_reject_draw() {
    for input in [0, 1, 2, 3] {
        let mut state = two_loads(false, true, 1, true).rdp;
        state.tmem_bank.write_tile(&[0x70; 8], 32, 1, 1, 1, 8, 1);
        state.tiles[1] = state.tiles[0].clone();
        state.tiles[1].tmem_addr = 32;
        state.tiles[1].width = 8;
        state.tiles[1].height = 1;
        state.combine_l = 0x00ff_ffff;
        state.combine_h = 0xfffc_f279;
        state.other_mode_h = 0;
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.set_texture(0, 0, true, 65535, 65535);
        if input == 1 {
            state.tiles.swap(0, 1);
            state.combine_l = 0x0088_7f10;
            state.combine_h = 0x88fc_fc7e;
            state.other_mode_h = 1 << 20;
        } else if input == 2 {
            state.tiles.swap(0, 1);
            state.other_mode_h = 1 << 16;
            rsp.texture_state.level = 1;
        } else if input == 3 {
            state.tiles[2] = state.tiles[1].clone();
            state.other_mode_h = (1 << 16) | (2 << 17);
            rsp.texture_state.tile = 1;
            rsp.texture_state.level = 1;
        }
        let mut diags = Vec::new();
        assert!(
            crate::hle::combiner::build_material(&state, &rsp, &mut diags, 0x80).is_none(),
            "input {input}"
        );
        assert_eq!(
            diags,
            [crate::Diagnostic {
                at: 0x80,
                kind: crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 }
            }]
        );
    }
}

#[test]
fn load_provenance_four_bit_rows_must_align_in_bits() {
    let mut state = narrow_state(0);
    state.tiles[0].siz = 0;
    state.tiles[0].width = 1;
    state.tiles[0].height = 9;
    state
        .tmem_bank
        .write_block(&[0x12, 0x34, 0x56, 0x78, 0x90, 0, 0, 0], 0, 0, 2048, 1, 0);
    state.tmem_bank.write_tile(&[0xee; 8], 64, 1, 1, 1, 8, 1);
    assert_eq!(
        intensities(&decode(&state).unwrap()),
        [17, 34, 51, 68, 85, 102, 119, 136, 153]
    );
}

#[test]
fn load_provenance_survives_task_boundaries_without_original_rdram() {
    for first in [false, true] {
        for second in [false, true] {
            let state = two_loads(first, second, 32, true).rdp;
            let mut dl = DlBuilder::new();
            let address = dl.bytes(8, &[0xee; 48]);
            let mut commands = load(second, address, 64, true);
            commands.extend(gsp_texture_rectangle(
                0, 0, 12, 12, 0, 0, 0, 4096, 1024, false,
            ));
            commands.push(gsp_enddl());
            dl.list("later", &commands);
            let built = dl.finish("later");
            let result = crate::hle::interp::interpret_with_state(
                crate::RdramImage::new(&built.rdram),
                u64::from(built.entry),
                crate::hle::gbi::GbiUcode::F3dex2,
                crate::DataFormat::Fixed,
                state,
                None,
            );
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            let expected = if first {
                vec![16, 17, 18, 32, 33, 34, 48, 49, 50]
            } else {
                (16..25).collect()
            };
            assert_eq!(intensities(&rect_texture(&result, 0)), expected);
        }
    }
}
