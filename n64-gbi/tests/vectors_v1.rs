use n64_gbi::consts::rdp::combine::{AlphaAbd, AlphaC, ColorA, ColorB, ColorC, ColorD};
use n64_gbi::consts::rdp::{CycleType, ImageFormat, ImageSize, G_TX_LOADTILE, G_TX_RENDERTILE};
use n64_gbi::encode::{
    command_words_to_be_bytes, gdp_load_tlut_cmd, gdp_set_combine_lerp, gdp_set_combine_lerp_typed,
    gsp_light, gsp_lookat, gsp_numlights, pack_rgba8, segmented_address, AlphaCombinePass,
    AmbientLight, CcPass, ColorCombinePass, DirectionalLight, LookAt, VtxNormal, ZERO_A, ZERO_C,
};
use n64_gbi::texel::{pack_4bit_pair, pack_4bit_row, pack_ia16, pack_ia4, pack_ia8, pack_rgba5551};
use n64_gbi::vectors::{v1, Literal, ProvenanceKind, VectorStatus};

fn assert_u32(literal: Literal, actual: u32) {
    assert_eq!(literal, Literal::U32(actual));
}

fn assert_bytes(literal: Literal, actual: &[u8]) {
    let Literal::Bytes(expected) = literal else {
        panic!("expected byte vector, got {literal:?}");
    };
    assert_eq!(actual, expected);
}

#[allow(deprecated)]
fn legacy_fast3d_load_tlut(tile: u32, lrt: u32) -> n64_gbi::encode::CommandWords {
    n64_gbi::encode::gdp_load_tlut(tile, lrt)
}

#[test]
fn vector_table_metadata_is_structurally_complete_unique_and_leaf_local() {
    let mut ids = std::collections::HashSet::new();
    assert_eq!(v1::VERSION, 1);

    for vector in v1::TABLE {
        assert!(ids.insert(vector.id), "duplicate vector id: {}", vector.id);
        assert!(!vector.provenance.source.is_empty());
        assert!(!vector.provenance.locator.is_empty());
        assert!(!vector.provenance.derivation.is_empty());
        assert!(
            !vector.provenance.source.contains("fast3d")
                && !vector.provenance.locator.contains("fast3d"),
            "downstream provenance leaked into {}",
            vector.id
        );
        assert_eq!(
            vector.status == VectorStatus::CharacterizationOnly,
            vector.provenance.kind == ProvenanceKind::LocalCharacterization,
            "status/provenance mismatch for {}",
            vector.id
        );
    }

    assert_eq!(
        v1::TABLE
            .iter()
            .filter(|vector| vector.status == VectorStatus::CharacterizationOnly)
            .count(),
        3
    );

    // Each tuple is one combiner slot's value in canary A and canary B, reduced to the narrowest
    // slot width. Pairwise uniqueness guarantees that transposing any two selector sources changes
    // at least one independently pinned command literal.
    let combiner_slot_signatures = [
        (7, 3),
        (6, 0),
        (0, 4),
        (6, 1),
        (5, 4),
        (6, 2),
        (6, 3),
        (7, 0),
        (6, 4),
        (7, 1),
        (7, 7),
        (7, 2),
        (1, 5),
        (2, 6),
        (3, 7),
        (4, 7),
    ];
    assert_eq!(
        combiner_slot_signatures
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        16,
        "combiner canaries must distinguish all 16 slots"
    );
}

#[test]
fn every_v1_literal_matches_its_protocol_primitive() {
    for vector in v1::TABLE {
        match vector.id {
            "image-format-rgba" => assert_u32(vector.literal, ImageFormat::Rgba.bits()),
            "image-format-yuv" => assert_u32(vector.literal, ImageFormat::Yuv.bits()),
            "image-format-ci" => assert_u32(vector.literal, ImageFormat::Ci.bits()),
            "image-format-ia" => assert_u32(vector.literal, ImageFormat::Ia.bits()),
            "image-format-i" => assert_u32(vector.literal, ImageFormat::I.bits()),
            "image-size-4" => assert_u32(vector.literal, ImageSize::Bits4.bits()),
            "image-size-8" => assert_u32(vector.literal, ImageSize::Bits8.bits()),
            "image-size-16" => assert_u32(vector.literal, ImageSize::Bits16.bits()),
            "image-size-32" => assert_u32(vector.literal, ImageSize::Bits32.bits()),
            "tile-render" => assert_u32(vector.literal, G_TX_RENDERTILE),
            "tile-load" => assert_u32(vector.literal, G_TX_LOADTILE),
            "cycle-selector-one" => assert_u32(vector.literal, CycleType::OneCycle.selector()),
            "cycle-selector-two" => assert_u32(vector.literal, CycleType::TwoCycle.selector()),
            "cycle-selector-copy" => assert_u32(vector.literal, CycleType::Copy.selector()),
            "cycle-selector-fill" => assert_u32(vector.literal, CycleType::Fill.selector()),
            "cycle-bits-one" => assert_u32(vector.literal, CycleType::OneCycle.other_mode_h_bits()),
            "cycle-bits-two" => assert_u32(vector.literal, CycleType::TwoCycle.other_mode_h_bits()),
            "cycle-bits-copy" => assert_u32(vector.literal, CycleType::Copy.other_mode_h_bits()),
            "cycle-bits-fill" => assert_u32(vector.literal, CycleType::Fill.other_mode_h_bits()),
            "combiner-color-a" => assert_bytes(
                vector.literal,
                &[
                    ColorA::Combined as u8,
                    ColorA::Texel0 as u8,
                    ColorA::Texel1 as u8,
                    ColorA::Primitive as u8,
                    ColorA::Shade as u8,
                    ColorA::Environment as u8,
                    ColorA::One as u8,
                    ColorA::Noise as u8,
                    ColorA::Zero as u8,
                ],
            ),
            "combiner-color-b" => assert_bytes(
                vector.literal,
                &[
                    ColorB::Combined as u8,
                    ColorB::Texel0 as u8,
                    ColorB::Texel1 as u8,
                    ColorB::Primitive as u8,
                    ColorB::Shade as u8,
                    ColorB::Environment as u8,
                    ColorB::Center as u8,
                    ColorB::K4 as u8,
                    ColorB::Zero as u8,
                ],
            ),
            "combiner-color-c" => assert_bytes(
                vector.literal,
                &[
                    ColorC::Combined as u8,
                    ColorC::Texel0 as u8,
                    ColorC::Texel1 as u8,
                    ColorC::Primitive as u8,
                    ColorC::Shade as u8,
                    ColorC::Environment as u8,
                    ColorC::Scale as u8,
                    ColorC::CombinedAlpha as u8,
                    ColorC::Texel0Alpha as u8,
                    ColorC::Texel1Alpha as u8,
                    ColorC::PrimitiveAlpha as u8,
                    ColorC::ShadeAlpha as u8,
                    ColorC::EnvironmentAlpha as u8,
                    ColorC::LodFraction as u8,
                    ColorC::PrimLodFraction as u8,
                    ColorC::K5 as u8,
                    ColorC::Zero as u8,
                ],
            ),
            "combiner-color-d" => assert_bytes(
                vector.literal,
                &[
                    ColorD::Combined as u8,
                    ColorD::Texel0 as u8,
                    ColorD::Texel1 as u8,
                    ColorD::Primitive as u8,
                    ColorD::Shade as u8,
                    ColorD::Environment as u8,
                    ColorD::One as u8,
                    ColorD::Zero as u8,
                ],
            ),
            "combiner-alpha-abd" => assert_bytes(
                vector.literal,
                &[
                    AlphaAbd::Combined as u8,
                    AlphaAbd::Texel0 as u8,
                    AlphaAbd::Texel1 as u8,
                    AlphaAbd::Primitive as u8,
                    AlphaAbd::Shade as u8,
                    AlphaAbd::Environment as u8,
                    AlphaAbd::One as u8,
                    AlphaAbd::Zero as u8,
                ],
            ),
            "combiner-alpha-c" => assert_bytes(
                vector.literal,
                &[
                    AlphaC::LodFraction as u8,
                    AlphaC::Texel0 as u8,
                    AlphaC::Texel1 as u8,
                    AlphaC::Primitive as u8,
                    AlphaC::Shade as u8,
                    AlphaC::Environment as u8,
                    AlphaC::PrimLodFraction as u8,
                    AlphaC::Zero as u8,
                ],
            ),
            "vtx-normal-layout" => assert_bytes(
                vector.literal,
                &VtxNormal {
                    x: 0x0102,
                    y: 0x0304,
                    z: 0x0506,
                    flag: 0x0708,
                    s: 0x090a,
                    t: 0x0b0c,
                    nx: -1,
                    ny: -128,
                    nz: 127,
                    a: 0xdd,
                }
                .to_bytes(),
            ),
            "directional-light-layout" => {
                let bytes = DirectionalLight {
                    color: [0x11, 0x22, 0x33],
                    pad1: 0x44,
                    color_copy: [0x55, 0x66, 0x77],
                    pad2: 0x88,
                    direction: [-1, -128, 127],
                    pad3: 0x99,
                    alignment_bytes: [0xaa, 0xbb, 0xcc, 0xdd],
                }
                .to_bytes();
                assert_bytes(vector.literal, &bytes[..12]);
            }
            "ambient-light-layout" => assert_bytes(
                vector.literal,
                &AmbientLight {
                    color: [0x11, 0x22, 0x33],
                    pad1: 0x44,
                    color_copy: [0x55, 0x66, 0x77],
                    pad2: 0x88,
                }
                .to_bytes(),
            ),
            "lookat-gdspdef" => {
                let bytes = LookAt {
                    x: DirectionalLight {
                        color: [0, 0, 0],
                        pad1: 0,
                        color_copy: [0, 0, 0],
                        pad2: 0,
                        direction: [127, 0, -127],
                        pad3: 0,
                        alignment_bytes: [0; 4],
                    },
                    y: DirectionalLight {
                        color: [0, 0x80, 0],
                        pad1: 0,
                        color_copy: [0, 0x80, 0],
                        pad2: 0,
                        direction: [0, 127, -128],
                        pad3: 0,
                        alignment_bytes: [0; 4],
                    },
                }
                .to_bytes();
                let mut named_fields = [0; 24];
                named_fields[..12].copy_from_slice(&bytes[..12]);
                named_fields[12..].copy_from_slice(&bytes[16..28]);
                assert_bytes(vector.literal, &named_fields);
            }
            "f3dex2-numlights-2" => {
                assert_eq!(vector.literal, Literal::Words(gsp_numlights(2)))
            }
            "f3dex2-light-1" => {
                assert_eq!(vector.literal, Literal::Words(gsp_light(1, 0x0123_4567)))
            }
            "f3dex2-lookat" => {
                let words = gsp_lookat(0x0123_4567);
                let mut bytes = Vec::with_capacity(16);
                bytes.extend_from_slice(&command_words_to_be_bytes(words[0]));
                bytes.extend_from_slice(&command_words_to_be_bytes(words[1]));
                assert_bytes(vector.literal, &bytes);
            }
            "load-tlut-3-entries" => assert_eq!(
                vector.literal,
                Literal::Words(gdp_load_tlut_cmd(G_TX_LOADTILE, 2))
            ),
            "combine-slot-canary" => assert_eq!(
                vector.literal,
                Literal::Words(gdp_set_combine_lerp_typed(
                    ColorCombinePass {
                        a: ColorA::Noise,
                        b: ColorB::Center,
                        c: ColorC::Texel0Alpha,
                        d: ColorD::One,
                    },
                    AlphaCombinePass {
                        a: AlphaAbd::Environment,
                        b: AlphaAbd::One,
                        c: AlphaC::PrimLodFraction,
                        d: AlphaAbd::Zero,
                    },
                    ColorCombinePass {
                        a: ColorA::One,
                        b: ColorB::K4,
                        c: ColorC::K5,
                        d: ColorD::Zero,
                    },
                    AlphaCombinePass {
                        a: AlphaAbd::Texel0,
                        b: AlphaAbd::Texel1,
                        c: AlphaC::Primitive,
                        d: AlphaAbd::Shade,
                    },
                ))
            ),
            "combine-slot-canary-b" => assert_eq!(
                vector.literal,
                Literal::Words(gdp_set_combine_lerp_typed(
                    ColorCombinePass {
                        a: ColorA::Primitive,
                        b: ColorB::Combined,
                        c: ColorC::EnvironmentAlpha,
                        d: ColorD::Texel0,
                    },
                    AlphaCombinePass {
                        a: AlphaAbd::Shade,
                        b: AlphaAbd::Texel1,
                        c: AlphaC::Primitive,
                        d: AlphaAbd::Combined,
                    },
                    ColorCombinePass {
                        a: ColorA::Shade,
                        b: ColorB::Texel0,
                        c: ColorC::Zero,
                        d: ColorD::Texel1,
                    },
                    AlphaCombinePass {
                        a: AlphaAbd::Environment,
                        b: AlphaAbd::One,
                        c: AlphaC::Zero,
                        d: AlphaAbd::Zero,
                    },
                ))
            ),
            "command-be-light-1" => assert_bytes(
                vector.literal,
                &command_words_to_be_bytes(gsp_light(1, 0x0123_4567)),
            ),
            "rgba8-12345678" => assert_u32(vector.literal, pack_rgba8(0x12, 0x34, 0x56, 0x78)),
            "segmented-6-123456" => assert_u32(vector.literal, segmented_address(6, 0x0012_3456)),
            "rgba5551-f821" => assert_bytes(vector.literal, &pack_rgba5551(31, 0, 16, 1)),
            "ia16-12ab" => assert_bytes(vector.literal, &pack_ia16(0x12, 0xab)),
            "ia8-a3" => assert_u32(vector.literal, u32::from(pack_ia8(0x0a, 3))),
            "ia4-b" => assert_u32(vector.literal, u32::from(pack_ia4(5, 1))),
            "four-bit-pair-1a" => assert_u32(vector.literal, u32::from(pack_4bit_pair(1, 0x0a))),
            "odd-row-zero-pad" => {
                let actual: Vec<u8> = pack_4bit_row(&[1, 2, 3])
                    .chain(pack_4bit_row(&[0x0a, 0x0b, 0x0c]))
                    .collect();
                assert_bytes(vector.literal, &actual);
            }
            "legacy-tlut-count-3" => assert_eq!(
                vector.literal,
                Literal::Words(legacy_fast3d_load_tlut(G_TX_LOADTILE, (3 - 1) << 2))
            ),
            "legacy-combine-golden" => {
                let color = CcPass {
                    a: 1,
                    b: ZERO_C,
                    c: 4,
                    d: ZERO_C,
                };
                let alpha = CcPass {
                    a: ZERO_A,
                    b: ZERO_A,
                    c: ZERO_A,
                    d: 4,
                };
                assert_eq!(
                    vector.literal,
                    Literal::Words(gdp_set_combine_lerp(color, alpha, color, alpha))
                );
            }
            unknown => panic!("vector has no primitive assertion: {unknown}"),
        }
    }
}

#[test]
fn segmented_address_masks_offset_to_the_wire_field() {
    assert_eq!(segmented_address(6, 0xff12_3456), 0x0612_3456);
    assert_eq!(segmented_address(0xf6, 0x0012_3456), 0x0612_3456);
}

#[test]
fn four_bit_row_iterator_reports_its_exact_output_size() {
    assert_eq!(pack_4bit_row(&[]).len(), 0);
    assert_eq!(pack_4bit_row(&[1]).len(), 1);
    assert_eq!(pack_4bit_row(&[1, 2, 3]).len(), 2);

    assert_eq!(pack_rgba5551(0x3f, 0x20, 0x30, 3), [0xf8, 0x21]);
    assert_eq!(pack_ia8(0x1a, 0x13), 0xa3);
    assert_eq!(pack_ia4(0x0d, 3), 0x0b);
    assert_eq!(pack_4bit_pair(0x11, 0x1a), 0x1a);
    assert_eq!(pack_4bit_row(&[0x11, 0x1a]).collect::<Vec<_>>(), [0x1a]);
}
