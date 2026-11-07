#![cfg(feature = "profiling")]

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn angrylion_requests_preserve_unaffected_frozen_preimages_on_this_target() {
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
    let prototype = ultragfx::profiling::authored_requests().remove(0);
    let mut unchanged = 0;
    let mut changed = 0;
    for row in document["vectors"].as_array().unwrap() {
        let recipe = &row["recipe"];
        let n = |name: &str| recipe[name].as_u64().unwrap();
        if n("representation") == 2 {
            continue;
        }
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
            0 => ultragfx::profiling::Representation::Tile,
            1 => ultragfx::profiling::Representation::Lookup,
            _ => unreachable!(),
        };
        trace.bank.bytes = vec![0; 4096];
        for field in ["physical_pairs_hex", "palette_pairs_hex"] {
            for pair in unhex(row[field].as_str().unwrap()).as_chunks::<3>().0 {
                trace.bank.bytes[usize::from(u16::from_le_bytes([pair[0], pair[1]]))] = pair[2];
            }
        }
        if trace.tile.fmt == 2 && trace.tlut >= 2 {
            for slot in trace.bank.bytes[2048..].as_chunks_mut::<8>().0 {
                let pair = [slot[0], slot[1]];
                for lane in slot[2..].as_chunks_mut::<2>().0 {
                    lane.copy_from_slice(&pair);
                }
            }
        }
        trace.extent = [
            recipe["output"][0].as_u64().unwrap() as u32,
            recipe["output"][1].as_u64().unwrap() as u32,
        ];
        trace.rejection = None;
        let (key, witness) = trace.encoded_identity().unwrap();
        let frozen = unhex(row["canonical_preimage_hex"].as_str().unwrap());
        let frozen_key = u64::from_str_radix(row["xxh3_64"].as_str().unwrap(), 16).unwrap();
        if (trace.tile.fmt == 2 && trace.tlut < 2) || (trace.tile.fmt != 2 && trace.tlut >= 2) {
            assert_ne!(witness, frozen, "{}", row["name"]);
            assert_ne!(key, frozen_key, "{}", row["name"]);
            changed += 1;
        } else {
            assert_eq!(witness, frozen, "{}", row["name"]);
            assert_eq!(key, frozen_key, "{}", row["name"]);
            unchanged += 1;
        }
    }
    assert_eq!((unchanged, changed), (9, 3));
}
