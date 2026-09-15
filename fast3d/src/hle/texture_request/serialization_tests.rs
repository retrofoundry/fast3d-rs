use super::*;

fn reference_serialize(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES], linear: &[u8]) -> Vec<u8> {
    let mut selected = [false; TMEM_BYTES];
    let mut palette = [false; TMEM_BYTES];
    let mut mark = |rel: usize, odd: bool, parity: usize| {
        let mask = if recipe.fmt == 2 || recipe.siz == 3 {
            2047
        } else {
            4095
        };
        let address = |offset| {
            ((usize::from(recipe.base) * 8) + ((rel + offset) ^ if odd { 4 } else { 0 })) & mask
        };
        let a = address(0);
        selected[a] = true;
        if recipe.siz >= 2 {
            selected[address(1)] = true;
        }
        if recipe.siz == 3 {
            selected[a | 2048] = true;
            selected[address(1) | 2048] = true;
        }
        if recipe.fmt == 2 {
            let index = if recipe.siz == 0 {
                (bank[a] >> if parity == 0 { 4 } else { 0 }) & 15
            } else {
                bank[a]
            };
            let p = 2048 + usize::from(recipe.palette) * 128 + usize::from(index) * 8;
            selected[p & 4095] = true;
            selected[(p + 1) & 4095] = true;
        }
    };
    match recipe.representation {
        Representation::Tile => {
            for y in 0..recipe.output[1] as usize {
                for x in 0..recipe.output[0] as usize {
                    mark(
                        y * usize::from(recipe.line) * 8 + ((x << recipe.siz.min(2)) >> 1),
                        y & 1 != 0,
                        x & 1,
                    );
                }
            }
        }
        Representation::Lookup4096x4 => {
            for odd in [false, true] {
                for parity in 0..2 {
                    for rel in 0..TMEM_BYTES {
                        mark(rel, odd, parity);
                    }
                }
            }
        }
        Representation::LinearCompat => {
            if recipe.fmt == 2 {
                for x in 0..(recipe.logical[0] as usize * recipe.logical[1] as usize) {
                    let index = if recipe.siz == 0 {
                        (linear[x / 2] >> if x & 1 == 0 { 4 } else { 0 }) & 15
                    } else {
                        linear[x]
                    };
                    let p = 2048 + usize::from(recipe.palette) * 128 + usize::from(index) * 8;
                    palette[p & 4095] = true;
                    palette[(p + 1) & 4095] = true;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(
        56 + selected.iter().filter(|&&v| v).count() * 3
            + linear.len()
            + palette.iter().filter(|&&v| v).count() * 3,
    );
    out.extend_from_slice(b"fast3d-tmem-key\0");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[recipe.representation as u8, 0]);
    for dimension in recipe.output.into_iter().chain(recipe.logical) {
        out.extend_from_slice(&dimension.to_le_bytes());
    }
    out.extend_from_slice(&[recipe.fmt, recipe.siz]);
    out.extend_from_slice(&recipe.base.to_le_bytes());
    out.extend_from_slice(&recipe.line.to_le_bytes());
    out.extend_from_slice(&[recipe.palette, recipe.tlut]);
    let pairs = |out: &mut Vec<u8>, selected: &[bool; TMEM_BYTES]| {
        out.extend_from_slice(&(selected.iter().filter(|&&v| v).count() as u32).to_le_bytes());
        for (address, &included) in selected.iter().enumerate() {
            if included {
                out.extend_from_slice(&(address as u16).to_le_bytes());
                out.push(bank[address]);
            }
        }
    };
    pairs(&mut out, &selected);
    out.extend_from_slice(&(linear.len() as u32).to_le_bytes());
    out.extend_from_slice(linear);
    pairs(&mut out, &palette);
    out
}

#[test]
fn spans_preserve_preimages_through_wrap_swap_overlap_and_palette_tails() {
    let bank = std::array::from_fn(|i| ((i * 37 + i / 17 + 11) & 255) as u8);
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
        for base in [0, 1, 255, 256, 511] {
            for line in [0, 1, 3, 255, 511] {
                for width in (0..=17).chain([65, 2049]) {
                    for representation in [
                        Representation::Tile,
                        Representation::Lookup4096x4,
                        Representation::LinearCompat,
                    ] {
                        if representation == Representation::LinearCompat && siz == 3 {
                            continue;
                        }
                        let recipe = DecodeRecipe {
                            representation,
                            output: if representation == Representation::Lookup4096x4 {
                                [4096, 4]
                            } else {
                                [width, 3]
                            },
                            logical: [width, 3],
                            fmt,
                            siz,
                            base,
                            line,
                            palette: if fmt == 2 && siz == 0 { 15 } else { 0 },
                            tlut: if fmt == 2 { 3 } else { 0 },
                        };
                        let linear = if representation == Representation::LinearCompat {
                            (0..((width as usize * 3) << siz).div_ceil(2))
                                .map(|i| bank[i & 4095])
                                .collect()
                        } else {
                            Vec::new()
                        };
                        assert_eq!(
                            serialize(&recipe, &bank, &linear),
                            reference_serialize(&recipe, &bank, &linear),
                            "{recipe:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn zero_height_selects_no_physical_bytes_or_palette_entries() {
    for (fmt, siz) in [(0, 3), (2, 0), (2, 1), (4, 1)] {
        let recipe = DecodeRecipe {
            representation: Representation::Tile,
            output: [7, 0],
            logical: [7, 1],
            fmt,
            siz,
            base: 511,
            line: 511,
            palette: 0,
            tlut: 0,
        };
        let bank = [255; TMEM_BYTES];
        let preimage = serialize(&recipe, &bank, &[]);
        assert_eq!(preimage.len(), 56);
        assert_eq!(&preimage[44..], &[0; 12]);
        assert_eq!(preimage, reference_serialize(&recipe, &bank, &[]));
    }
}
