use super::*;

fn reference_serialize(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES]) -> Vec<u8> {
    let tlut = recipe.tlut >= 2;
    let rgba32 = recipe.fmt == 0 && recipe.siz == 3;
    let mut selected = [false; TMEM_BYTES];
    let mut mark = |rel: usize, odd: bool, parity: usize| {
        let mask = if tlut || rgba32 { 2047 } else { 4095 };
        let address = |offset| {
            ((usize::from(recipe.base) * 8) + ((rel + offset) ^ if odd { 4 } else { 0 })) & mask
        };
        let a = address(0);
        selected[a] = true;
        if tlut {
            let index = if recipe.siz == 0 {
                let low = parity == 1 && recipe.fmt != 1;
                (usize::from(recipe.palette) << 4)
                    | usize::from((bank[a] >> if low { 0 } else { 4 }) & 15)
            } else {
                usize::from(bank[a])
            };
            let p = 2048 + index * 8;
            selected[p..p + if recipe.palette_lanes { 8 } else { 2 }].fill(true);
            return;
        }
        if recipe.siz >= 2 {
            selected[address(1)] = true;
        }
        if rgba32 {
            selected[a | 2048] = true;
            selected[address(1) | 2048] = true;
        }
    };
    match recipe.representation {
        Representation::Tile => {
            for y in 0..recipe.output[1] as usize {
                for x in 0..recipe.output[0] as usize {
                    let column = if tlut && recipe.fmt == 1 {
                        x
                    } else {
                        (x << recipe.siz.min(2)) >> 1
                    };
                    mark(
                        (y & 255) * usize::from(recipe.line) * 8 + column,
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
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"ultragfx-tmem-key\0");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&[recipe.representation as u8, recipe.semantic_flags()]);
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
    out.extend_from_slice(&[0; 8]);
    out
}

#[test]
fn spans_preserve_preimages_through_wrap_swap_overlap_and_palette_tails() {
    let bank = std::array::from_fn(|i| ((i * 37 + i / 17 + 11) & 255) as u8);
    let extents: Vec<_> = (0..=17)
        .chain([65, 2049])
        .map(|width| (Representation::Tile, [width, 3]))
        .chain([
            (Representation::Tile, [5, 258]),
            (Representation::Lookup4096x4, [4096, 4]),
        ])
        .collect();
    for fmt in 0..8 {
        for siz in 0..4 {
            for (tlut, palette_lanes) in [(0, false), (2, false), (3, true)] {
                if tlut < 2
                    && (crate::hle::texdec::FormatInfo { fmt, siz })
                        .validate()
                        .is_err()
                {
                    continue;
                }
                for base in [0, 1, 255, 256, 511] {
                    for line in [0, 1, 3, 255, 511] {
                        for &(representation, output) in &extents {
                            let recipe = DecodeRecipe {
                                palette_lanes,
                                representation,
                                output,
                                logical: output,
                                fmt,
                                siz,
                                base,
                                line,
                                palette: if (fmt == 2 || tlut >= 2) && siz == 0 {
                                    15
                                } else {
                                    0
                                },
                                tlut,
                            };
                            assert_eq!(
                                serialize(&recipe, &bank),
                                reference_serialize(&recipe, &bank),
                                "{recipe:?}"
                            );
                        }
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
            palette_lanes: false,
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
        let preimage = serialize(&recipe, &bank);
        assert_eq!(preimage.len(), 58);
        assert_eq!(&preimage[46..], &[0; 12]);
        assert_eq!(preimage, reference_serialize(&recipe, &bank));
    }
}
