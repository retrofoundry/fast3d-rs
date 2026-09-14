use super::*;

#[test]
fn texture_decode_shader_has_a_valid_bounded_compute_entry() {
    let module =
        wgpu::naga::front::wgsl::parse_str(SHADER).expect("texture decode WGSL must parse");
    wgpu::naga::valid::Validator::new(
        wgpu::naga::valid::ValidationFlags::all(),
        wgpu::naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("texture decode WGSL must validate with baseline capabilities");
    assert_eq!(module.entry_points.len(), 1);
    let entry = &module.entry_points[0];
    assert_eq!(entry.stage, wgpu::naga::ShaderStage::Compute);
    assert_eq!(entry.workgroup_size, [8, 8, 1]);
}

#[test]
fn texture_decode_packs_raw_bytes_and_explicit_palette_offset() {
    use crate::hle::texture_request::{
        DecodeRecipe, EncodedInput, EncodedTextureRequest, Representation,
    };
    let bank = std::sync::Arc::new(std::array::from_fn(|i| i as u8));
    let recipe = DecodeRecipe {
        representation: Representation::LinearCompat,
        output: [3, 1],
        logical: [3, 1],
        fmt: 2,
        siz: 1,
        base: 511,
        line: 3,
        palette: 7,
        tlut: 3,
    };
    let request = EncodedTextureRequest::from_test_parts(
        EncodedInput::LinearCompat {
            bytes: [0x12, 0x34, 0x56].into(),
            palette_bank: bank.clone(),
        },
        recipe.clone(),
    );
    let input = PackedInput::new(&request);
    assert_eq!(input.bytes.len(), 4100);
    assert_eq!(&input.bytes[..8], &[0x12, 0x34, 0x56, 0, 0, 1, 2, 3]);
    assert_eq!(&input.bytes[4..], &bank[..]);
    let params = DecodeParams::new(&request, &input);
    assert_eq!(
        params.words,
        [[3, 1, 3, 1], [2, 2, 1, 4088], [24, 7, 3, 3], [4, 0, 0, 0]]
    );
    assert_eq!(&params.bytes()[28..32], &[0xf8, 0x0f, 0, 0]);
    assert_eq!(required_input_bytes(&request), 4100);
    let request = EncodedTextureRequest::from_test_parts(
        EncodedInput::Tmem(bank.clone()),
        DecodeRecipe {
            representation: Representation::Tile,
            ..recipe
        },
    );
    let input = PackedInput::new(&request);
    assert_eq!(input.bytes.as_ptr(), bank.as_ptr());
    assert_eq!(input.bytes.as_ref(), &bank[..]);
    assert_eq!(required_input_bytes(&request), 4096);
}

#[test]
fn texture_decode_component_vectors_match_the_existing_cpu_oracle() {
    for vector in super::vectors::component_vectors()
        .into_iter()
        .chain(super::vectors::literal_layout_vectors())
    {
        assert_eq!(
            super::vectors::oracle(&vector.request),
            vector.expected,
            "{}",
            vector.name
        );
    }
}

#[test]
fn texture_decode_uniform_matches_wgsl_offsets() {
    let module = wgpu::naga::front::wgsl::parse_str(SHADER).unwrap();
    let params = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("DecodeParams"))
        .unwrap()
        .1;
    let wgpu::naga::TypeInner::Struct { members, span } = &params.inner else {
        panic!("uniform must be a struct")
    };
    assert_eq!(*span, PARAM_BYTES as u32);
    assert_eq!(
        members.iter().map(|v| v.offset).collect::<Vec<_>>(),
        [0, 16, 32, 48]
    );
}

#[test]
fn texture_decode_readback_vectors_are_valid_diagnostic_inputs() {
    let mut vectors = super::vectors::component_vectors();
    vectors.extend(super::vectors::layout_vectors());
    vectors.extend(super::vectors::literal_layout_vectors());
    for vector in vectors {
        let encoded = vector
            .request
            .encoded_request()
            .unwrap_or_else(|error| panic!("{}: {error:?}", vector.name));
        assert_eq!(encoded.decode(), vector.expected, "{}", vector.name);
    }
}

#[test]
fn texture_decode_vectors_match_the_frozen_artifact_inventory() {
    let inventory: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/tmem/gpu-decode-inventory.json"
    ))
    .unwrap();
    assert_eq!(inventory["version"], 1);
    let rows = inventory["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1909);
    let mut vectors = super::vectors::component_vectors();
    vectors.extend(super::vectors::layout_vectors());
    vectors.extend(super::vectors::literal_layout_vectors());
    assert_eq!(vectors.len(), 1909);
    vectors.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    let actual: Vec<_> = vectors
        .iter()
        .map(|vector| {
            serde_json::json!({
                "id": vector.name,
                "width": vector.request.extent[0],
                "height": vector.request.extent[1],
            })
        })
        .collect();
    assert_eq!(&actual, rows);
}

#[test]
fn warp_readbacks_cover_formats_modes_and_addressing_edges() {
    use crate::profiling::Representation;

    let selected = super::vectors::readback_vectors(true);
    assert!(
        selected.len() <= 100,
        "WARP must bound live decode resources"
    );
    for (fmt, siz) in [
        (0, 2),
        (0, 3),
        (2, 0),
        (2, 1),
        (3, 0),
        (3, 1),
        (3, 2),
        (4, 0),
        (4, 1),
    ] {
        for mode in if fmt == 2 {
            &[0, 1, 2, 3][..]
        } else {
            &[3][..]
        } {
            for representation in [Representation::Tile, Representation::Lookup] {
                assert!(
                    selected.iter().any(|v| {
                        let r = &v.request;
                        r.tile.fmt == fmt
                            && r.tile.siz == siz
                            && r.tlut == *mode
                            && r.representation == representation
                            && r.tile.tmem_addr == 511
                            && r.tile.line == 3
                            && r.tile.palette == 15
                    }),
                    "missing high-bank/wrapped layout {fmt}/{siz}/{mode}/{representation:?}"
                );
            }
        }
        if siz != 3 {
            assert!(
                selected.iter().any(|v| {
                    let r = &v.request;
                    r.tile.fmt == fmt
                        && r.tile.siz == siz
                        && r.representation == Representation::Linear
                        && r.extent == [3, 3]
                }),
                "missing odd-width linear format {fmt}/{siz}"
            );
        }
    }
    for literal in super::vectors::literal_layout_vectors() {
        let vector = selected.iter().find(|v| v.name == literal.name).unwrap();
        assert_eq!(vector.expected, literal.expected);
    }
    for vector in selected {
        assert_eq!(
            super::vectors::oracle(&vector.request),
            vector.expected,
            "{}",
            vector.name
        );
    }
    assert_eq!(super::vectors::readback_vectors(false).len(), 1909);
}
