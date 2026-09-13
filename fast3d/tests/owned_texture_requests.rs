#![cfg(feature = "profiling")]

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn production_requests_reproduce_frozen_preimages_on_this_target() {
    let document: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/tmem/vectors.json")).unwrap();
    let unhex = |text: &str| -> Vec<u8> {
        text.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    };
    let prototype = fast3d::profiling::authored_requests().remove(0);
    for row in document["vectors"].as_array().unwrap() {
        let recipe = &row["recipe"];
        let n = |name: &str| recipe[name].as_u64().unwrap();
        let mut trace = prototype.clone();
        trace.tile.fmt = n("fmt") as u8;
        trace.tile.siz = n("siz") as u8;
        trace.tile.width = recipe["logical"][0].as_u64().unwrap() as u16;
        trace.tile.height = recipe["logical"][1].as_u64().unwrap() as u16;
        trace.tile.tmem_addr = n("base") as u16;
        trace.tile.line = n("line") as u16;
        trace.tile.palette = n("palette") as u8;
        trace.tile.masks = if n("representation") == 1 { 10 } else { 0 };
        trace.tlut = n("tlut") as u8;
        trace.representation = match n("representation") {
            0 => fast3d::profiling::Representation::Tile,
            1 => fast3d::profiling::Representation::Lookup,
            _ => fast3d::profiling::Representation::Linear,
        };
        trace.bank.bytes = vec![0; 4096];
        trace.bank.sources = vec![[0; 2]; 4096];
        trace.bank.blocks.clear();
        for field in ["physical_pairs_hex", "palette_pairs_hex"] {
            for pair in unhex(row[field].as_str().unwrap()).as_chunks::<3>().0 {
                trace.bank.bytes[usize::from(u16::from_le_bytes([pair[0], pair[1]]))] = pair[2];
            }
        }
        let linear = unhex(row["linear_hex"].as_str().unwrap());
        if !linear.is_empty() {
            let base = usize::from(trace.tile.tmem_addr) * 8;
            for (offset, &byte) in linear.iter().enumerate() {
                let address = (base + offset) & 4095;
                trace.bank.bytes[address] = byte;
                trace.bank.sources[address] = [1, offset as u16];
            }
            trace
                .bank
                .blocks
                .push((base as u16, linear.len() as u16, true));
        }
        trace.rejection = None;
        let (key, witness) = trace.encoded_identity().unwrap();
        assert_eq!(
            witness,
            unhex(row["canonical_preimage_hex"].as_str().unwrap()),
            "{}",
            row["name"]
        );
        assert_eq!(
            key,
            u64::from_str_radix(row["xxh3_64"].as_str().unwrap(), 16).unwrap()
        );
    }
}
