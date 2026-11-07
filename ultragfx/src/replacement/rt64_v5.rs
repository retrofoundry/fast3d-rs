use super::{
    DecodeRecipe, EncodedInput, ReplacementTarget, Representation, TextureKey, TextureKeyScheme,
};
use crate::{hle::tile_sampling::TileSampling, profiling::Recorder};
use std::ops::Range;

fn spans(recipe: &DecodeRecipe) -> Option<Vec<Range<usize>>> {
    let [width, height] = recipe.output;
    if recipe.representation != Representation::Tile
        || recipe.logical != recipe.output
        || width == 0
        || height == 0
        || width > 4096
        || height > 256
        || recipe.palette_lanes
        || recipe.line == 0
        || recipe.line > 511
        || recipe.base > 511
        || !matches!(
            (recipe.fmt, recipe.siz),
            (0, 2 | 3) | (2, 0 | 1) | (3, 0..=2) | (4, 0 | 1)
        )
        || (recipe.siz == 0 && width & 1 != 0)
        || (recipe.fmt == 2 && !matches!(recipe.tlut, 2 | 3))
        || (recipe.fmt != 2 && recipe.tlut != 0)
        || recipe.palette > 15
        || ((recipe.fmt != 2 || recipe.siz != 0) && recipe.palette != 0)
    {
        return None;
    }
    let rgba32 = recipe.fmt == 0 && recipe.siz == 3;
    let bank_size = if rgba32 || recipe.fmt == 2 {
        2048
    } else {
        4096
    };
    let base = (usize::from(recipe.base) * 8) & (bank_size - 1);
    let row = ((width as usize) << recipe.siz.min(2)) >> 1;
    let stride = usize::from(recipe.line) * 8;
    if row > stride {
        return None;
    }
    let mut spans = Vec::new();
    for bank in 0..if rgba32 { 2 } else { 1 } {
        let bank_offset = bank * bank_size;
        if row == stride {
            let end = base + stride * height as usize;
            if end > bank_size {
                return None;
            }
            spans.push(base + bank_offset..end + bank_offset);
            continue;
        }
        for y in 0..height as usize {
            let start = base + y * stride;
            let full = row & !7;
            let tail = row & 7;
            let end = if y & 1 == 0 || tail == 0 {
                start + row
            } else {
                start + full + 4 + tail.min(4)
            };
            if end > bank_size {
                return None;
            }
            let start = start + bank_offset;
            if y & 1 == 0 {
                spans.push(start..start + row);
            } else {
                if full != 0 {
                    spans.push(start..start + full);
                }
                if tail > 4 {
                    spans.push(start + full..start + full + tail - 4);
                }
                if tail != 0 {
                    spans.push(start + full + 4..start + full + 4 + tail.min(4));
                }
            }
        }
    }
    Some(spans)
}

pub(crate) fn input_spans(recipe: &DecodeRecipe, bank: &[u8; 4096]) -> Option<Vec<Range<usize>>> {
    let mut spans = spans(recipe)?;
    let mut indices = [0u64; 4];
    for span in &spans {
        let bytes = &bank[span.clone()];
        if recipe.fmt == 2 {
            for &byte in bytes {
                if recipe.siz == 0 {
                    indices[0] |= (1 << (byte & 15)) | (1 << (byte >> 4));
                } else {
                    indices[usize::from(byte >> 6)] |= 1 << (byte & 63);
                }
            }
        }
    }
    if recipe.fmt == 2 {
        let palette = 2048 + usize::from(recipe.palette) * 128;
        for (word, mut bits) in indices.into_iter().enumerate() {
            while bits != 0 {
                let index = word * 64 + bits.trailing_zeros() as usize;
                spans.push(palette + index * 8..palette + index * 8 + 8);
                bits &= bits - 1;
            }
        }
    }
    Some(spans)
}

pub(crate) fn allowed(sampling: &TileSampling, recipe: &DecodeRecipe) -> bool {
    sampling.palette[1] == u32::from(recipe.tlut)
        && (0..2).all(|axis| {
            // rt64 hashes min(clamped tile extent, mask period) texels, not the tile extent.
            let mask = sampling.shift_mask[axis + 2];
            let extent = recipe.output[axis];
            mask == 0
                || 1 << mask == extent
                || (sampling.modes[axis] & 2 != 0 && 1 << mask > extent)
        })
}

pub(super) fn alias(
    recipe: &DecodeRecipe,
    input: &EncodedInput,
    profiling: &Recorder,
) -> Option<ReplacementTarget> {
    let EncodedInput::Tmem(bank) = input;
    let spans = input_spans(recipe, bank)?;
    if recipe.fmt == 2
        && spans.iter().filter(|span| span.start >= 2048).any(|span| {
            bank[span.clone()]
                .as_chunks::<2>()
                .0
                .iter()
                .any(|lane| lane != &bank[span.start..span.start + 2])
        })
    {
        return None;
    }
    let mut stream = Vec::new();
    for span in spans {
        stream.extend_from_slice(&bank[span]);
    }
    stream.extend_from_slice(&(recipe.output[0] as u16).to_le_bytes());
    stream.extend_from_slice(&(recipe.output[1] as u16).to_le_bytes());
    // rt64 hashes OtherMode::textLUT(), which is `H & (3 << G_MDSFT_TEXTLUT)` and keeps the
    // field in place, so the stream carries the mode shifted by 14 rather than 0/2/3.
    stream.extend_from_slice(&(u32::from(recipe.tlut) << 14).to_le_bytes());
    stream.extend_from_slice(&recipe.line.to_le_bytes());
    stream.extend_from_slice(&[recipe.siz, recipe.fmt]);
    profiling.count("replacement.v5_hashes", 1);
    profiling.count("replacement.v5_bytes_hashed", stream.len() as u64);
    Some(ReplacementTarget {
        key: TextureKey {
            scheme: TextureKeyScheme::Rt64V5,
            xxh3_64: twox_hash::XxHash3_64::oneshot(&stream),
        },
        witness: stream.into(),
        extent: recipe.output,
        representation: recipe.representation,
        palette_lanes: false,
    })
}

#[cfg(test)]
mod tests;
