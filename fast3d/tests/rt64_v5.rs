use fast3d::replacement::{
    DecodeRecipe, EncodedInput, ReplacementTarget, Representation, TextureKeyScheme,
};
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn pinned_cpp_streams_and_digests_match_production() {
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/tmem/rt64-v5-vectors.json")).unwrap();
    assert_eq!(
        document["provenance"]["pin"],
        "43373749dac9bbc1b653e6a02aed40a9e1783bed"
    );
    let vectors = document["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 82);
    let mut eligible = 0;
    let mut excluded = 0;
    for row in vectors {
        let number = |name: &str| row[name].as_u64().unwrap() as u32;
        let mut bank = std::array::from_fn(|i| (i * 37 + (i / 256) * 11 + 19) as u8);
        for item in row["overrides"].as_array().unwrap() {
            bank[item[0].as_u64().unwrap() as usize] = item[1].as_u64().unwrap() as u8;
        }
        let mut recipe = DecodeRecipe {
            representation: Representation::Tile,
            output: [number("width"), number("height")],
            logical: [number("width"), number("height")],
            fmt: number("fmt") as u8,
            siz: number("siz") as u8,
            base: number("base") as u16,
            line: number("line") as u16,
            palette: number("palette") as u8,
            tlut: number("tlut") as u8,
            palette_lanes: false,
        };
        if recipe.tlut >= 2 {
            recipe.palette_lanes = row["spans"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|span| span[0].as_u64().unwrap() >= 2048)
                .any(|span| {
                    let start = span[0].as_u64().unwrap() as usize;
                    let count = span[1].as_u64().unwrap() as usize;
                    bank[start..start + count]
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .any(|slot| {
                            slot.as_chunks::<2>()
                                .0
                                .iter()
                                .any(|lane| lane != &slot[..2])
                        })
                });
        }
        let lanes = recipe.palette_lanes;
        let target = ReplacementTarget::new(
            recipe,
            EncodedInput::Tmem(Arc::new(bank)),
            TextureKeyScheme::Rt64V5,
        );
        if lanes {
            assert_eq!(
                target,
                Err(fast3d::replacement::ReplacementError::IneligibleScheme),
                "{}",
                row["name"]
            );
            excluded += 1;
            continue;
        }
        let target = target.unwrap();
        eligible += 1;
        assert_eq!(target.key().scheme, TextureKeyScheme::Rt64V5);
        assert_eq!(
            target.witness(),
            unhex(row["stream"].as_str().unwrap()),
            "{}",
            row["name"]
        );
        assert_eq!(
            target.key().xxh3_64,
            u64::from_str_radix(row["xxh3_64"].as_str().unwrap(), 16).unwrap(),
            "{}",
            row["name"]
        );
    }
    eprintln!("frozen v5 streams: {eligible} eligible, {excluded} excluded for palette lanes");
    assert_eq!(eligible + excluded, 82);
}
