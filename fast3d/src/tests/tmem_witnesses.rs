use crate::hle::{rdp::TileDescriptor, tmem::Tmem};

use super::readback_export;

const C5: [u8; 32] = [
    0, 8, 16, 24, 33, 41, 49, 57, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 198, 206, 214, 222, 231, 239, 247, 255,
];
const C3: [u8; 8] = [0, 36, 73, 109, 146, 182, 219, 255];

fn tile(fmt: u8, siz: u8, width: u16, height: u16, line: u16, base: u16) -> TileDescriptor {
    TileDescriptor {
        fmt,
        siz,
        width,
        height,
        line,
        tmem_addr: base,
        cms: 2,
        cmt: 2,
        ..Default::default()
    }
}

fn bank(bytes: &[u8]) -> Tmem {
    let mut profile = Tmem::default().profile_bank(false);
    profile.bytes[..bytes.len()].copy_from_slice(bytes);
    Tmem::from_profile(&profile)
}

fn export(id: &str, actual: &[u8], expected: &[u8], width: u32, height: u32, input: &[u8]) {
    assert_eq!(actual, expected, "{id}");
    readback_export::write_row(
        "FAST3D_DECODE_OUTPUT",
        id,
        "decode",
        "cpu-oracle",
        "none",
        actual,
        width,
        height,
        input,
        None,
    );
    if let Some(dir) = std::env::var_os("FAST3D_DECODE_OUTPUT") {
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{id}.expected.bin")),
            expected,
        )
        .unwrap();
    }
}

fn check(id: &str, tmem: &Tmem, tile: &TileDescriptor, mode: u8, expected: &[u8]) {
    let actual = tmem.sample_tile(tile, mode).unwrap();
    let mut input = tmem.profile_bank(false).bytes;
    input.extend([tile.fmt, tile.siz, tile.palette, mode]);
    for v in [tile.width, tile.height, tile.line, tile.tmem_addr] {
        input.extend(v.to_le_bytes());
    }
    export(
        id,
        &actual,
        expected,
        tile.width.into(),
        tile.height.into(),
        &input,
    );
}

#[test]
fn tmem_decode_formats_independent_literals() {
    let mut encoded = Vec::new();
    let mut expected = Vec::new();
    for channel in 0..3 {
        for value in 0..32u16 {
            for alpha in 0..2u16 {
                let mut channels = [3u16, 17, 29];
                channels[channel] = value;
                encoded.extend(
                    ((channels[0] << 11) | (channels[1] << 6) | (channels[2] << 1) | alpha)
                        .to_be_bytes(),
                );
                expected.extend([
                    C5[channels[0] as usize],
                    C5[channels[1] as usize],
                    C5[channels[2] as usize],
                    if alpha == 0 { 0 } else { 255 },
                ]);
            }
        }
    }
    check(
        "format-rgba16",
        &bank(&encoded),
        &tile(0, 2, 192, 1, 48, 0),
        0,
        &expected,
    );
    for (fmt, siz, name) in [
        (4, 0, "i4"),
        (4, 1, "i8"),
        (3, 0, "ia4"),
        (3, 1, "ia8"),
        (3, 2, "ia16"),
    ] {
        let mut bytes = Vec::new();
        let mut pixels = Vec::new();
        match siz {
            0 => {
                for value in 0..16u8 {
                    if value.is_multiple_of(2) {
                        bytes.push(value * 16 + value + 1);
                    }
                    pixels.extend(if fmt == 4 {
                        [value * 17; 4]
                    } else {
                        [
                            C3[(value / 2) as usize],
                            C3[(value / 2) as usize],
                            C3[(value / 2) as usize],
                            if value.is_multiple_of(2) { 0 } else { 255 },
                        ]
                    });
                }
            }
            1 => {
                for value in 0..=255u8 {
                    bytes.push(value);
                    pixels.extend(if fmt == 4 {
                        [value; 4]
                    } else {
                        [
                            (value / 16) * 17,
                            (value / 16) * 17,
                            (value / 16) * 17,
                            (value % 16) * 17,
                        ]
                    });
                }
            }
            _ => {
                for value in 0..=255u8 {
                    bytes.extend([value, 93, 171, value]);
                    pixels.extend([value, value, value, 93, 171, 171, 171, value]);
                }
            }
        }
        check(
            &format!("format-{name}"),
            &bank(&bytes),
            &tile(
                fmt,
                siz,
                (pixels.len() / 4) as u16,
                1,
                (bytes.len().div_ceil(8)) as u16,
                0,
            ),
            0,
            &pixels,
        );
    }
    let mut bytes = vec![0; 4096];
    bytes[..8].copy_from_slice(&[1, 37, 82, 129, 170, 215, 253, 9]);
    bytes[2048..2056].copy_from_slice(&[203, 17, 29, 233, 101, 3, 19, 0]);
    check(
        "format-rgba32",
        &bank(&bytes),
        &tile(0, 3, 4, 1, 1, 0),
        0,
        &[
            1, 37, 203, 17, 82, 129, 29, 233, 170, 215, 101, 3, 253, 9, 19, 0,
        ],
    );
    for siz in [0, 1] {
        for mode in 0..4 {
            let mut bytes = vec![0; 4096];
            bytes[..2].copy_from_slice(if siz == 0 { &[0x0f, 0] } else { &[0, 255] });
            for (address, pair) in [
                (2048, [0x19, 0x63]),
                (2168, [0xa7, 0xb0]),
                (3968, [0x19, 0x63]),
                (4088, [0xa7, 0xb0]),
            ] {
                bytes[address..address + 2].copy_from_slice(&pair);
            }
            let expected = match mode {
                2 => vec![24, 41, 140, 255, 165, 247, 198, 0],
                3 => vec![25, 25, 25, 99, 167, 167, 167, 176],
                _ => vec![0; 8],
            };
            let mut descriptor = tile(2, siz, 2, 1, 1, 0);
            descriptor.palette = 15;
            check(
                &format!("format-ci{}-mode{mode}", if siz == 0 { 4 } else { 8 }),
                &bank(&bytes),
                &descriptor,
                mode,
                &expected,
            );
        }
    }
}

#[test]
fn tmem_layout_addresses_and_footprint_aliases() {
    let descriptor = tile(4, 0, 5, 3, 3, 0);
    let addresses = [0, 1, 2, 28, 29, 30, 48, 49, 50];
    let mut bytes = vec![0xee; 4096];
    for (address, value) in addresses
        .into_iter()
        .zip([0x12, 0x34, 0x50, 0x67, 0x89, 0xa0, 0xbc, 0xde, 0xf0])
    {
        bytes[address] = value;
    }
    let expected: Vec<_> = [
        17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
    ]
    .into_iter()
    .flat_map(|v| [v; 4])
    .collect();
    let tmem = bank(&bytes);
    let mut reads = std::collections::BTreeSet::new();
    assert_eq!(
        tmem.sample_tile_observed(&descriptor, 0, &mut |a| {
            reads.insert(a);
        })
        .unwrap(),
        expected
    );
    assert_eq!(reads.into_iter().collect::<Vec<_>>(), addresses);
    check("layout-odd-5x3", &tmem, &descriptor, 0, &expected);
    for (address, pixel) in [(28, 5), (50, 14)] {
        let mut changed = bytes.clone();
        changed[address] ^= 0x10;
        let mut want = expected.clone();
        for channel in 0..4 {
            want[pixel * 4 + channel] ^= 17;
        }
        check(
            if address == 28 {
                "alias-odd-swap"
            } else {
                "alias-final-nibble"
            },
            &bank(&changed),
            &descriptor,
            0,
            &want,
        );
    }
    for address in [3, 2048] {
        let mut changed = bytes.clone();
        changed[address] ^= 0xff;
        assert_eq!(
            bank(&changed).sample_tile(&descriptor, 0).unwrap(),
            expected
        );
    }
    let mut split = vec![0; 4096];
    for (address, value) in [
        (2046, 1),
        (2047, 2),
        (4094, 3),
        (4095, 4),
        (0, 5),
        (1, 6),
        (2048, 7),
        (2049, 8),
    ] {
        split[address] = value;
    }
    let mut high = tile(0, 3, 5, 1, 2, 255);
    let expected = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
    check("layout-rgba32-wrap", &bank(&split), &high, 0, &expected);
    split[2049] = 91;
    let mut changed = expected;
    changed[19] = 91;
    check("alias-high-bank", &bank(&split), &high, 0, &changed);
    high = tile(4, 1, 1, 1, 0, 511);
    check(
        "layout-one-zero-line",
        &bank(&vec![93; 4096]),
        &high,
        3,
        &[93; 4],
    );
}

#[test]
fn tmem_lookup_all_planes_wrap_independent_literals() {
    let mut bytes = vec![0x12; 4096];
    bytes[0] = 0xab;
    bytes[4095] = 0xcd;
    let descriptor = tile(4, 0, 1, 1, 0, 511);
    let mut expected = Vec::new();
    let mut relative = bytes.clone();
    relative.rotate_left(4088);
    for odd in [false, true] {
        let mut plane = relative.clone();
        if odd {
            for word in plane.as_chunks_mut::<8>().0 {
                word.rotate_left(4);
            }
        }
        for low in [false, true] {
            for &v in &plane {
                expected.extend([if low { v % 16 * 17 } else { v / 16 * 17 }; 4]);
            }
        }
    }
    let actual = bank(&bytes).sampling_lookup(&descriptor, 0).unwrap();
    export("layout-lookup-wrap", &actual, &expected, 4096, 4, &bytes);
    bytes[0] = 0x9b;
    let changed = bank(&bytes).sampling_lookup(&descriptor, 0).unwrap();
    let differences: Vec<_> = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(changed.as_chunks::<4>().0)
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect();
    assert_eq!(differences, [8, 8204]);
    let mut want = expected;
    for pixel in [8, 8204] {
        want[pixel * 4..pixel * 4 + 4].fill(153);
    }
    export("alias-wrapped-lookup", &changed, &want, 4096, 4, &bytes);
}

#[test]
fn tmem_provenance_equal_banks_distinct_linear_streams() {
    let descriptor = tile(4, 1, 3, 3, 1, 0);
    let mut streams = Vec::new();
    for dxt in [0, 2048] {
        let mut tmem = Tmem::default();
        tmem.write_block(&[0; 24], 0, 0, dxt, 3, 1);
        let mut profile = tmem.profile_bank(true);
        for (i, b) in profile.bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        tmem = Tmem::from_profile(&profile);
        streams.push(tmem.linear_bytes(&descriptor, 16).unwrap());
    }
    assert_eq!(
        streams[0],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );
    assert_eq!(
        streams[1],
        [0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15, 8, 9, 10, 11]
    );
    assert_ne!(streams[0], streams[1]);
}

#[test]
fn tmem_poison_then_reload_checks_rejection_before_reuse() {
    let mut state = crate::hle::rdp::Rdp {
        other_mode_h: 2 << 20,
        ..Default::default()
    };
    state.tiles[0] = tile(4, 1, 1, 1, 1, 0);
    state.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    let get = |state: &crate::hle::rdp::Rdp| {
        let mut diags = Vec::new();
        let material = crate::hle::combiner::build_rect_material(
            state,
            &Default::default(),
            0,
            &mut diags,
            0x88,
        );
        (material, diags)
    };
    assert_eq!(get(&state).0.unwrap().texture, [61; 4]);
    let diagnostic = crate::Diagnostic {
        at: 0x40,
        kind: crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 },
    };
    let before = state.tmem_bank.profile_bank(false).bytes;
    state.tmem_bank.reject_load(diagnostic, |tmem| {
        tmem.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1)
    });
    assert_eq!(before, state.tmem_bank.profile_bank(false).bytes);
    assert!(get(&state).0.is_none());
    assert_eq!(get(&state).1, [diagnostic]);
    state.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    assert_eq!(get(&state).0.unwrap().texture, [61; 4]);
    state.tiles[0].cms = 0;
    state.tiles[0].masks = 4;
    state.tmem_bank.reject_load(diagnostic, |tmem| {
        tmem.write_tile(&[0; 8], 1, 1, 1, 1, 8, 1)
    });
    assert!(get(&state).0.is_none());
}

#[test]
fn tmem_tlut_order_reach_and_rgba32_overlap() {
    let mut tmem = Tmem::default();
    tmem.write_tile(&[0; 8], 0, 1, 1, 1, 8, 1);
    let descriptor = tile(2, 1, 1, 1, 1, 0);
    tmem.write_tlut(&[0xf8, 1], 1, 256);
    let first = tmem.sample_tile(&descriptor, 2).unwrap();
    tmem.write_tlut(&[7, 0xc1], 1, 256);
    let second = tmem.sample_tile(&descriptor, 2).unwrap();
    tmem.write_tlut(&[7, 0xc1], 1, 257);
    assert_eq!(tmem.sample_tile(&descriptor, 2).unwrap(), second);
    tmem.write_block(&[0, 0, 0, 0x3f, 0, 0, 0, 1], 0, 0, 0, 1, 3);
    assert_eq!(tmem.sample_tile(&descriptor, 2).unwrap(), [0, 0, 255, 255]);
    assert_eq!(first, [255, 0, 0, 255]);
    assert_eq!(second, [0, 255, 0, 255]);
    tmem.write_tlut(&[0x12, 0x34, 0x56, 0x78], 2, 511);
    let bytes = tmem.profile_bank(false).bytes;
    assert_eq!(&bytes[4088..], [0x12, 0x34].repeat(4));
    assert_eq!(&bytes[..8], [0x56, 0x78].repeat(4));
    for palette in 0..16 {
        let mut tmem = Tmem::default();
        tmem.write_tile(&[0; 8], 0, 1, 1, 1, 8, 1);
        tmem.write_tlut(&[palette * 13, 255], 1, 256 + usize::from(palette) * 16);
        let mut ci4 = tile(2, 0, 1, 1, 1, 0);
        ci4.palette = palette;
        assert_eq!(
            tmem.sample_tile(&ci4, 3).unwrap(),
            [palette * 13, palette * 13, palette * 13, 255]
        );
    }
}

#[test]
fn tmem_roles_keep_independent_pixels_after_sources_are_destroyed() {
    let make = |state: &crate::hle::rdp::Rdp, rsp: &crate::hle::rsp::Rsp| {
        let mut diagnostics = Vec::new();
        let result =
            crate::hle::combiner::build_material(state, rsp, &mut diagnostics, 0x80).unwrap();
        assert!(diagnostics.is_empty());
        result
    };
    let mut state = crate::hle::rdp::Rdp {
        texture_loaded: true,
        combine_l: 0x0088_7f10,
        combine_h: 0x88fc_fc7e,
        other_mode_h: 1 << 20,
        ..Default::default()
    };
    for (i, (width, height, value)) in [(8, 4, 37), (3, 5, 91), (5, 2, 173), (2, 2, 211)]
        .into_iter()
        .enumerate()
    {
        state.tiles[i] = tile(4, 1, width, height, 1, i as u16 * 16);
        state
            .tmem_bank
            .write_tile(&[value; 40], i * 16, 1, usize::from(height), 1, 8, 1);
    }
    let mut rsp = crate::hle::rsp::Rsp::default();
    rsp.set_texture(0, 0, true, 65535, 65535);
    let first = make(&state, &rsp);
    assert_eq!(first.texture, vec![37; 8 * 4 * 4]);
    assert_eq!(first.tex1.as_ref().unwrap().texture, vec![91; 3 * 5 * 4]);
    state.tmem_bank.write_tile(&[137; 40], 16, 1, 5, 1, 8, 1);
    let second = make(&state, &rsp);
    assert_eq!(first.texture, second.texture);
    assert_eq!(second.tex1.as_ref().unwrap().texture, vec![137; 3 * 5 * 4]);
    state.other_mode_h |= (1 << 16) | (2 << 17);
    rsp.set_texture(1, 2, true, 65535, 65535);
    let lod = make(&state, &rsp);
    assert_eq!(
        lod.mip_levels
            .iter()
            .map(|v| (v.w, v.h))
            .collect::<Vec<_>>(),
        [(3, 5), (5, 2), (2, 2)]
    );
    assert_eq!(lod.texture, lod.mip_levels[0].texture);
    assert_eq!(
        lod.detail_tex.as_ref().unwrap().texture,
        vec![37; 8 * 4 * 4]
    );
    state.tmem_bank.write_tile(&[61; 40], 0, 1, 4, 1, 8, 1);
    let detail_changed = make(&state, &rsp);
    assert_eq!(lod.texture, detail_changed.texture);
    for (a, b) in lod.mip_levels.iter().zip(&detail_changed.mip_levels) {
        assert_eq!(a.texture, b.texture);
    }
    assert_eq!(
        detail_changed.detail_tex.as_ref().unwrap().texture,
        vec![61; 8 * 4 * 4]
    );
    state.tmem_bank.write_tile(&[199; 40], 32, 1, 2, 1, 8, 1);
    let level_changed = make(&state, &rsp);
    assert_eq!(
        detail_changed.mip_levels[0].texture,
        level_changed.mip_levels[0].texture
    );
    assert_ne!(
        detail_changed.mip_levels[1].texture,
        level_changed.mip_levels[1].texture
    );
    assert_eq!(
        detail_changed.mip_levels[2].texture,
        level_changed.mip_levels[2].texture
    );
    drop(state);
    drop(rsp);
    assert_eq!(first.tex1.unwrap().texture, vec![91; 3 * 5 * 4]);
    assert_eq!(lod.detail_tex.unwrap().texture, vec![37; 8 * 4 * 4]);
}

#[test]
fn tmem_loadtile_and_dxt_literals() {
    let source: Vec<u8> = (0..64).collect();
    let descriptor = tile(4, 1, 5, 3, 1, 0);
    let mut tiled = Tmem::default();
    tiled.write_tile(&source, 0, 1, 3, 1, 16, 1);
    let expected: Vec<_> = [0, 1, 2, 3, 4, 16, 17, 18, 19, 20, 32, 33, 34, 35, 36]
        .into_iter()
        .flat_map(|v| [v; 4])
        .collect();
    check("layout-loadtile-padded", &tiled, &descriptor, 0, &expected);
    for (dxt, name, expected) in [
        (
            0,
            "layout-block-dxt0",
            [0, 1, 2, 3, 4, 12, 13, 14, 15, 8, 16, 17, 18, 19, 20],
        ),
        (
            683,
            "layout-block-dxt683",
            [0, 1, 2, 3, 4, 12, 13, 14, 15, 8, 16, 17, 18, 19, 20],
        ),
    ] {
        let mut tmem = Tmem::default();
        tmem.write_block(&source, 0, 0, dxt, 8, 1);
        let mut expected = expected.to_vec();
        expected.extend(if dxt == 0 {
            [28, 29, 30, 31, 24]
        } else {
            [24, 25, 26, 27, 28]
        });
        check(
            name,
            &tmem,
            &tile(4, 1, 5, 4, 1, 0),
            0,
            &expected
                .into_iter()
                .flat_map(|v| [v; 4])
                .collect::<Vec<_>>(),
        );
        let fourth = tile(4, 1, 5, 4, 1, 0);
        let pixels = tmem.sample_tile(&fourth, 0).unwrap();
        let row: Vec<_> = pixels[60..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect();
        assert_eq!(
            row,
            if dxt == 0 {
                vec![28, 29, 30, 31, 24]
            } else {
                vec![24, 25, 26, 27, 28]
            }
        );
    }
}

#[test]
fn tmem_zero_line_and_nonci_tlut_do_not_alias_low_bank() {
    let mut bytes = vec![193; 4096];
    bytes[4088] = 1;
    bytes[4092] = 5;
    let descriptor = tile(4, 1, 1, 3, 0, 511);
    let expected = [1, 1, 1, 1, 5, 5, 5, 5, 1, 1, 1, 1];
    check(
        "layout-zero-line-nonci-tlut",
        &bank(&bytes),
        &descriptor,
        0,
        &expected,
    );
    for mode in [1, 2, 3] {
        assert_eq!(
            bank(&bytes).sample_tile(&descriptor, mode).unwrap(),
            expected
        );
    }
}

#[test]
fn tmem_gpu_source_poison_rejects_draw_then_guest_reload_restores_it() {
    use n64_gbi::encode::*;
    let mut b = super::dl_builder::DlBuilder::new();
    let source = b.bytes(8, &[0xf8, 1].repeat(4));
    let mut commands = vec![
        gdp_set_color_image(0, 2, 32, 0x10000),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(0x0001_0001),
        gdp_fill_rectangle(0, 0, 124, 124),
        gdp_set_color_image(0, 2, 32, 0x20000),
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 4, source),
        gdp_set_tile(0, 2, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
        gdp_load_block(7, 0, 0, 3, 0),
        gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 12, 0),
    ];
    commands.extend(gsp_texture_rectangle(
        0, 0, 16, 4, 0, 0, 0, 4096, 1024, false,
    ));
    commands.push(gdp_set_texture_image(0, 2, 4, 0x10000));
    let poison_index = commands.len();
    commands.push(gdp_load_block(7, 0, 0, 3, 0));
    commands.extend(gsp_texture_rectangle(
        16, 0, 32, 4, 0, 0, 0, 4096, 1024, false,
    ));
    commands.extend([
        gdp_set_texture_image(0, 2, 4, source),
        gdp_load_block(7, 0, 0, 3, 0),
    ]);
    commands.extend(gsp_texture_rectangle(
        32, 0, 48, 4, 0, 0, 0, 4096, 1024, false,
    ));
    commands.push(gsp_enddl());
    b.list("main", &commands);
    let built = b.finish("main");
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert_eq!(
        result.diags,
        [crate::Diagnostic {
            at: u64::from(built.entry) + poison_index as u64 * 8,
            kind: crate::DiagKind::UnsupportedFramebufferAccess {
                address: 0x10000,
                reason: crate::FramebufferAccess::TextureLoad
            }
        }]
    );
    assert_eq!(result.dropped_runs, 1);
    let textures: Vec<_> = result
        .scene
        .framebuffer_pairs
        .iter()
        .flat_map(|p| &p.ops)
        .filter_map(|op| {
            if let crate::scene::SceneOp::TexRect { material_index, .. } = op {
                Some(&result.scene.materials[*material_index as usize].texture)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(textures.len(), 2);
    for pixels in textures {
        assert_eq!(*pixels, [255, 0, 0, 255].repeat(4));
    }
}
