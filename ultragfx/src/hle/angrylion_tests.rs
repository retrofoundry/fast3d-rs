use super::{rdp::TileDescriptor, tmem::BankDecoder};

pub(crate) fn decode_vectors() -> Vec<(String, crate::profiling::Request, Vec<u8>)> {
    use crate::profiling::{Bank, Representation, Request, TRACE_VERSION};
    let mut vectors = Vec::new();
    let mut push =
        |name: String, tile: TileDescriptor, tlut, bank: [u8; 4096], pixels: Vec<u32>| {
            vectors.push((
                name,
                Request {
                    version: TRACE_VERSION,
                    representation: Representation::Tile,
                    role: "angrylion".into(),
                    extent: [tile.width.into(), tile.height.into()],
                    tile,
                    tlut,
                    bank: Bank {
                        bytes: bank.to_vec(),
                    },
                    reachable_bytes: 0,
                    read_operations: 0,
                    palette_bytes: 0,
                    output: Vec::new(),
                    rejection: None,
                },
                pixels.into_iter().flat_map(u32::to_be_bytes).collect(),
            ));
        };
    for tlut in [0, 1] {
        for (siz, pixels) in [
            (0, [0xabababab, 0xacacacac]),
            (1, [0xbcbcbcbc, 0xdededede]),
            (2, [0xbcdebcde, 0]),
            (3, [0xbcdebcde, 0]),
        ] {
            let mut bank = [0; 4096];
            bank[0] = 0x12;
            bank[0x800..0x802].copy_from_slice(&[0xbc, 0xde]);
            push(
                format!("angrylion-ci-off-{siz}-{tlut}"),
                TileDescriptor {
                    fmt: 2,
                    siz,
                    tmem_addr: 0x100,
                    palette: 10,
                    width: 2,
                    height: 1,
                    ..Default::default()
                },
                tlut,
                bank,
                pixels.to_vec(),
            );
        }
    }
    let rgba = [
        [0xa5297b00, 0xa54a73ff, 0xa56b7300, 0xa58c6bff],
        [0x105ab5ff, 0x319c29ff, 0x52d6a5ff, 0x7b1018ff],
        [0x105ab5ff, 0x52d6a5ff, 0x9c4a94ff, 0xdec684ff],
        [0xa5297b00, 0xa56b7300, 0xa5ad6b00, 0xa5ef6300],
    ];
    let ia = [
        [0xa1a1a15e, 0xa2a2a25d, 0xa3a3a35c, 0xa4a4a45b],
        [0x121212ed, 0x343434cb, 0x565656a9, 0x78787887],
        [0x121212ed, 0x565656a9, 0x9a9a9a65, 0xdedede21],
        [0xa1a1a15e, 0xa3a3a35c, 0xa5a5a55a, 0xa7a7a758],
    ];
    for fmt in 0..8 {
        for siz in 0..4 {
            for tlut in [2, 3] {
                let mut bank = [0; 4096];
                bank[..8].copy_from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
                for i in 0..256usize {
                    for lane in 0..4 {
                        bank[2048 + i * 8 + lane * 2..2050 + i * 8 + lane * 2]
                            .copy_from_slice(&[i as u8, i as u8 ^ 255]);
                    }
                }
                let row = match (fmt, siz) {
                    (1, 0) => 3,
                    (_, 0) => 0,
                    (1, _) | (_, 1) => 1,
                    _ => 2,
                };
                push(
                    format!("angrylion-tlut-{fmt}-{siz}-{tlut}"),
                    TileDescriptor {
                        fmt,
                        siz,
                        line: 1,
                        palette: 10,
                        width: 4,
                        height: 1,
                        ..Default::default()
                    },
                    tlut,
                    bank,
                    if tlut == 2 { rgba[row] } else { ia[row] }.to_vec(),
                );
            }
        }
    }
    let mut bank = [0; 4096];
    bank[..8].copy_from_slice(&[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17]);
    push(
        "angrylion-zero-line".into(),
        TileDescriptor {
            fmt: 4,
            siz: 1,
            width: 4,
            height: 3,
            ..Default::default()
        },
        0,
        bank,
        vec![
            0x10101010, 0x11111111, 0x12121212, 0x13131313, 0x14141414, 0x15151515, 0x16161616,
            0x17171717, 0x10101010, 0x11111111, 0x12121212, 0x13131313,
        ],
    );
    let mut bank = [0; 4096];
    bank[0] = 0x11;
    bank[2048] = 0x99;
    let mut pixels = vec![0; 257];
    pixels[0] = 0x11111111;
    pixels[256] = 0x11111111;
    push(
        "angrylion-t256".into(),
        TileDescriptor {
            fmt: 4,
            siz: 1,
            line: 1,
            width: 1,
            height: 257,
            ..Default::default()
        },
        0,
        bank,
        pixels,
    );
    let mut bank = [0; 4096];
    bank[2048..2056].copy_from_slice(&[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255]);
    push(
        "angrylion-palette-lanes".into(),
        TileDescriptor {
            fmt: 2,
            siz: 1,
            width: 1,
            height: 1,
            ..Default::default()
        },
        3,
        bank,
        vec![0x101010ff, 0x303030ff, 0x505050ff, 0x707070ff],
    );
    vectors
}

#[test]
fn angrylion_literal_fetch_vectors() {
    for (name, request, expected) in decode_vectors() {
        let bank = request.bank.bytes.as_slice().try_into().unwrap();
        let actual = BankDecoder::new(bank).sample_tile(&request.tile, request.tlut);
        assert_eq!(
            actual.unwrap_or_else(|e| panic!("{name}: {e:?}")),
            &expected[..usize::from(request.tile.width) * usize::from(request.tile.height) * 4],
            "{name}"
        );
    }
}

#[test]
fn angrylion_requests_keep_nonci_tlut_and_all_used_palette_lanes() {
    use super::{
        rdp::Rdp,
        texture_request::{prepare, TextureSource},
        tmem::Tmem,
    };
    for (name, request, expected) in decode_vectors() {
        if name == "angrylion-t256" {
            continue;
        }
        let rdp = Rdp {
            tmem_bank: Tmem::from_profile(&request.bank),
            ..Default::default()
        };
        let binding = prepare(&rdp, &request.tile, request.tlut, &Default::default()).unwrap();
        let actual = binding.source.decode();
        assert_eq!(&actual[..expected.len()], expected, "{name}");
    }
    let mut bank = vec![0; 4096];
    bank[2048..2056].copy_from_slice(&[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255]);
    let rdp = Rdp {
        tmem_bank: Tmem::from_profile(&crate::profiling::Bank { bytes: bank }),
        ..Default::default()
    };
    let tile = TileDescriptor {
        fmt: 2,
        siz: 1,
        width: 1,
        height: 1,
        ..Default::default()
    };
    let binding = prepare(&rdp, &tile, 3, &Default::default()).unwrap();
    assert_eq!(binding.sampling.allocation_extent(), [1, 4]);
    assert_eq!(
        binding.source.decode().as_ref(),
        [
            0x10, 0x10, 0x10, 255, 0x30, 0x30, 0x30, 255, 0x50, 0x50, 0x50, 255, 0x70, 0x70, 0x70,
            255
        ]
    );
    let TextureSource::Encoded(request) = binding.source else {
        panic!("encoded")
    };
    let witness = request.witness();
    for address in 2048u16..2056 {
        assert!(
            witness[50..witness.len() - 8]
                .as_chunks::<3>()
                .0
                .iter()
                .any(|p| p[..2] == address.to_le_bytes()),
            "missing lane byte {address}"
        );
    }
}

#[test]
fn angrylion_flat_component_helpers_keep_tlut_dispatch() {
    for (name, request, expected) in decode_vectors() {
        if !name.starts_with("angrylion-tlut-") && !name.starts_with("angrylion-ci-off-") {
            continue;
        }
        let base = usize::from(request.tile.tmem_addr) * 8;
        let actual = super::texdec::FormatInfo {
            fmt: request.tile.fmt,
            siz: request.tile.siz,
        }
        .decode(
            &request.bank.bytes[base..],
            request.tile.width.into(),
            1,
            &request.bank.bytes[2048..],
            request.tile.palette,
            request.tlut,
        )
        .unwrap();
        assert_eq!(actual, expected, "{name}");
    }
}
