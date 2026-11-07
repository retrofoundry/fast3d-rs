use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub representation: u8,
    pub output: [u32; 2],
    pub logical: [u32; 2],
    pub fmt: u8,
    pub siz: u8,
    pub base: u16,
    pub line: u16,
    pub palette: u8,
    pub tlut: u8,
}

#[derive(Clone, Debug)]
pub struct Witness {
    pub recipe: Recipe,
    pub physical: Vec<(u16, u8)>,
    pub linear: Vec<u8>,
    pub palette: Vec<(u16, u8)>,
}

pub fn vectors() -> Value {
    serde_json::from_str(include_str!("../../../tools/tmem/vectors.json")).unwrap()
}

pub fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn pairs(text: &str) -> Vec<(u16, u8)> {
    let bytes = unhex(text);
    assert_eq!(bytes.len() % 3, 0);
    bytes
        .as_chunks::<3>()
        .0
        .iter()
        .map(|pair| (u16::from_le_bytes([pair[0], pair[1]]), pair[2]))
        .collect()
}

pub fn witness(row: &Value) -> Witness {
    let r = &row["recipe"];
    let number = |field: &str| r[field].as_u64().unwrap();
    let extent = |field: &str| {
        [
            u32::try_from(r[field][0].as_u64().unwrap()).unwrap(),
            u32::try_from(r[field][1].as_u64().unwrap()).unwrap(),
        ]
    };
    Witness {
        recipe: Recipe {
            representation: u8::try_from(number("representation")).unwrap(),
            output: extent("output"),
            logical: extent("logical"),
            fmt: u8::try_from(number("fmt")).unwrap(),
            siz: u8::try_from(number("siz")).unwrap(),
            base: u16::try_from(number("base")).unwrap(),
            line: u16::try_from(number("line")).unwrap(),
            palette: u8::try_from(number("palette")).unwrap(),
            tlut: u8::try_from(number("tlut")).unwrap(),
        },
        physical: pairs(row["physical_pairs_hex"].as_str().unwrap()),
        linear: unhex(row["linear_hex"].as_str().unwrap()),
        palette: pairs(row["palette_pairs_hex"].as_str().unwrap()),
    }
}

fn write_pairs(out: &mut Vec<u8>, pairs: &[(u16, u8)]) {
    let mut pairs = pairs.to_vec();
    pairs.sort_unstable_by_key(|&(address, _)| address);
    for pair in pairs.windows(2) {
        assert!(pair[0].0 != pair[1].0 || pair[0].1 == pair[1].1);
    }
    pairs.dedup();
    out.extend_from_slice(&u32::try_from(pairs.len()).unwrap().to_le_bytes());
    for (address, value) in pairs {
        assert!(address < 4096);
        out.extend_from_slice(&address.to_le_bytes());
        out.push(value);
    }
}

pub fn serialize(witness: &Witness) -> Vec<u8> {
    let mut recipe = witness.recipe.clone();
    if recipe.fmt != 2 {
        recipe.palette = 0;
        recipe.tlut = 0;
        assert!(witness.palette.is_empty());
    } else if recipe.siz == 1 {
        recipe.palette = 0;
    }
    match recipe.representation {
        0 | 1 => assert!(witness.linear.is_empty() && witness.palette.is_empty()),
        2 => assert!(witness.physical.is_empty()),
        _ => panic!("unknown fixture representation"),
    }
    let mut out = b"fast3d-tmem-key\0".to_vec();
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[recipe.representation, 0]);
    for dimension in recipe.output.into_iter().chain(recipe.logical) {
        out.extend_from_slice(&dimension.to_le_bytes());
    }
    out.extend_from_slice(&[recipe.fmt, recipe.siz]);
    out.extend_from_slice(&recipe.base.to_le_bytes());
    out.extend_from_slice(&recipe.line.to_le_bytes());
    out.extend_from_slice(&[recipe.palette, recipe.tlut]);
    write_pairs(&mut out, &witness.physical);
    out.extend_from_slice(&u32::try_from(witness.linear.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&witness.linear);
    write_pairs(&mut out, &witness.palette);
    out
}

#[derive(Debug, PartialEq, Eq)]
pub struct SpecKey {
    pub bucket: u64,
    pub canonical_preimage: Vec<u8>,
}

pub fn key_with_hash(witness: &Witness, hash: impl FnOnce(&[u8]) -> u64) -> SpecKey {
    let canonical_preimage = serialize(witness);
    SpecKey {
        bucket: hash(&canonical_preimage),
        canonical_preimage,
    }
}
