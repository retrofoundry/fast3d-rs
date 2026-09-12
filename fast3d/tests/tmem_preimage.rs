#[path = "common/tmem_preimage.rs"]
mod tmem_preimage;

use std::hash::Hasher;
use tmem_preimage::{key_with_hash, serialize, unhex, vectors, witness};
use twox_hash::XxHash3_64;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn literal_preimages_match_explicit_fields_and_selected_bytes() {
    for row in vectors()["vectors"].as_array().unwrap() {
        let expected = unhex(row["canonical_preimage_hex"].as_str().unwrap());
        assert_eq!(expected.len() as u64, row["preimage_length"]);
        assert_eq!(serialize(&witness(row)), expected, "{}", row["name"]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn default_seed_xxh3_matches_independent_c_vectors() {
    let document = vectors();
    for row in document["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .chain(document["probes"].as_array().unwrap())
    {
        let bytes = unhex(row["canonical_preimage_hex"].as_str().unwrap());
        let expected = u64::from_str_radix(row["xxh3_64"].as_str().unwrap(), 16).unwrap();
        assert_eq!(XxHash3_64::oneshot(&bytes), expected, "{}", row["name"]);
        let mut stream = XxHash3_64::with_seed(0);
        for chunk in bytes.chunks(7) {
            stream.write(chunk);
        }
        assert_eq!(stream.finish(), expected, "{}", row["name"]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unused_metadata_and_repeated_visits_have_one_canonical_witness() {
    let document = vectors();
    let rows = document["vectors"].as_array().unwrap();
    for group in document["equal_groups"].as_array().unwrap() {
        let canonical = rows.iter().find(|row| row["name"] == group[0]).unwrap();
        let expected = unhex(canonical["canonical_preimage_hex"].as_str().unwrap());
        for name in group.as_array().unwrap() {
            let row = rows.iter().find(|row| row["name"] == *name).unwrap();
            assert_eq!(serialize(&witness(row)), expected, "{name}");
        }
    }
    for row in rows {
        let mut input = witness(row);
        input.physical.reverse();
        input.physical.extend(input.physical.clone());
        input.palette.reverse();
        input.palette.extend(input.palette.clone());
        assert_eq!(
            serialize(&input),
            unhex(row["canonical_preimage_hex"].as_str().unwrap()),
            "{}",
            row["name"]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn raw_probes_change_each_stated_field_at_its_frozen_offset() {
    let document = vectors();
    for probe in document["probes"].as_array().unwrap() {
        let source = document["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == probe["source"])
            .unwrap();
        let mut bytes = unhex(source["canonical_preimage_hex"].as_str().unwrap());
        let offset = usize::try_from(probe["offset"].as_u64().unwrap()).unwrap();
        let replacement = unhex(probe["replacement_hex"].as_str().unwrap());
        bytes[offset..offset + replacement.len()].copy_from_slice(&replacement);
        assert_eq!(
            bytes,
            unhex(probe["canonical_preimage_hex"].as_str().unwrap()),
            "{}",
            probe["name"]
        );
        assert_ne!(probe["xxh3_64"], source["xxh3_64"], "{}", probe["name"]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn constant_hash_requires_exact_content_recipe_and_tlut_equality() {
    let document = vectors();
    let row = document["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "tile-ci8-tlut2")
        .unwrap();
    let original = witness(row);
    let mut content = original.clone();
    content.physical[0].1 ^= 1;
    let mut recipe = original.clone();
    recipe.recipe.line += 1;
    let mut interpretation = original.clone();
    interpretation.recipe.tlut = 3;
    let variants = [original.clone(), content, recipe, interpretation];
    let keys: Vec<_> = variants
        .iter()
        .map(|input| key_with_hash(input, |_| 0x4242))
        .collect();
    for (a, left) in keys.iter().enumerate() {
        for (b, right) in keys.iter().enumerate() {
            assert_eq!(left.bucket, right.bucket);
            assert_eq!(left == right, a == b, "variant {a} against {b}");
        }
    }
    let mut ignored_bank = original;
    ignored_bank.recipe.palette = 15;
    assert_eq!(key_with_hash(&ignored_bank, |_| 0x4242), keys[0]);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_stream_palette_and_layout_remain_distinct_in_one_bucket() {
    let document = vectors();
    let row = document["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "linear-ci4-bank15")
        .unwrap();
    let original = witness(row);
    let key = key_with_hash(&original, |_| 0);
    let mut stream = original.clone();
    stream.linear[0] ^= 1;
    let mut palette = original.clone();
    for (_, byte) in &mut palette.palette {
        *byte ^= 1;
    }
    let mut base = original.clone();
    base.recipe.base += 1;
    let mut line = original;
    line.recipe.line += 1;
    for different in [stream, palette, base, line] {
        let candidate = key_with_hash(&different, |_| 0);
        assert_eq!(candidate.bucket, key.bucket);
        assert_ne!(candidate.canonical_preimage, key.canonical_preimage);
        assert_ne!(candidate, key);
    }
}
