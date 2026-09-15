use super::{DecodeRecipe, Recorder, Representation, TMEM_BYTES};

struct Selection([u64; TMEM_BYTES / 64]);

impl Default for Selection {
    fn default() -> Self {
        Self([0; TMEM_BYTES / 64])
    }
}

impl Selection {
    fn range(&mut self, mut start: usize, end: usize) {
        while start < end {
            let bit = start & 63;
            let count = (end - start).min(64 - bit);
            self.0[start / 64] |= (u64::MAX >> (64 - count)) << bit;
            start += count;
        }
    }

    fn wrapped_range(&mut self, start: usize, count: usize, size: usize) {
        if count >= size {
            self.range(0, size);
        } else {
            let start = start & (size - 1);
            let end = start + count;
            self.range(start, end.min(size));
            if end > size {
                self.range(0, end - size);
            }
        }
    }

    fn palette_entry(&mut self, palette: u8, index: u8) {
        let address = (2048 + usize::from(palette) * 128 + usize::from(index) * 8) & 4095;
        self.0[address / 64] |= 3 << (address & 63);
    }

    fn count(&self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }

    fn pairs(&self, out: &mut Vec<u8>, bank: &[u8; TMEM_BYTES], count: usize) {
        out.extend_from_slice(&(count as u32).to_le_bytes());
        let start = out.len();
        out.resize(start + count * 3, 0);
        let mut remaining = &mut out[start..];
        for (word, &bits) in self.0.iter().enumerate() {
            let mut offset = 0;
            let mut bits = bits;
            while bits != 0 {
                let skip = bits.trailing_zeros() as usize;
                offset += skip;
                bits >>= skip;
                let count = bits.trailing_ones() as usize;
                let address = word * 64 + offset;
                let (run, rest) = remaining.split_at_mut(count * 3);
                remaining = rest;
                for (i, (pair, &byte)) in run
                    .as_chunks_mut::<3>()
                    .0
                    .iter_mut()
                    .zip(&bank[address..address + count])
                    .enumerate()
                {
                    let address = (address + i) as u16;
                    let [lo, hi] = address.to_le_bytes();
                    *pair = [lo, hi, byte];
                }
                offset += count;
                bits = bits.checked_shr(count as u32).unwrap_or(0);
            }
        }
    }
}

pub(super) fn serialize_profiled(
    recipe: &DecodeRecipe,
    bank: &[u8; TMEM_BYTES],
    linear: &[u8],
    profiling: &Recorder,
) -> Vec<u8> {
    let mut selected = Selection::default();
    let mut palette = Selection::default();
    let size = if recipe.fmt == 2 || recipe.siz == 3 {
        2048
    } else {
        TMEM_BYTES
    };
    let base = usize::from(recipe.base) * 8;
    let row_bytes = ((recipe.output[0] as usize) << recipe.siz.min(2)).div_ceil(2);
    let mut span_bytes = 0;
    match recipe.representation {
        Representation::Tile => {
            for y in 0..recipe.output[1] as usize {
                let start = base + y * usize::from(recipe.line) * 8;
                if y & 1 == 0 {
                    selected.wrapped_range(start, row_bytes, size);
                } else {
                    // XOR 4 preserves complete words; only the final partial word moves.
                    let whole = row_bytes & !7;
                    let tail = row_bytes & 7;
                    selected.wrapped_range(start, whole, size);
                    selected.wrapped_range(start + whole + 4, tail.min(4), size);
                    selected.wrapped_range(start + whole, tail.saturating_sub(4), size);
                }
                span_bytes += row_bytes;
            }
        }
        Representation::Lookup4096x4 => {
            selected.range(0, size);
            span_bytes = size;
        }
        Representation::LinearCompat => {}
    }
    if recipe.siz == 3 {
        for i in 0..32 {
            selected.0[i + 32] = selected.0[i];
        }
        span_bytes *= 2;
    }

    let mut palette_reads = 0;
    if recipe.fmt == 2 {
        let mut indices = |byte: u8, low: bool| {
            palette_reads += 1;
            if recipe.siz == 0 {
                palette.palette_entry(recipe.palette, byte >> 4);
                if low {
                    palette.palette_entry(recipe.palette, byte & 15);
                }
            } else {
                palette.palette_entry(recipe.palette, byte);
            }
        };
        match recipe.representation {
            Representation::Tile => {
                for y in 0..recipe.output[1] as usize {
                    let row = y * usize::from(recipe.line) * 8;
                    for x in 0..row_bytes {
                        let address = (base + ((row + x) ^ ((y & 1) * 4))) & 2047;
                        indices(
                            bank[address],
                            x + 1 < row_bytes || recipe.output[0] & 1 == 0,
                        );
                    }
                }
            }
            Representation::Lookup4096x4 => {
                for &byte in &bank[..2048] {
                    indices(byte, true);
                }
            }
            Representation::LinearCompat => {
                let pixels = recipe.logical[0] as usize * recipe.logical[1] as usize;
                let bytes = (pixels << recipe.siz).div_ceil(2);
                for (i, &byte) in linear[..bytes].iter().enumerate() {
                    indices(byte, i + 1 < bytes || pixels & 1 == 0);
                }
            }
        }
        if recipe.representation != Representation::LinearCompat {
            for (selected, palette) in selected.0.iter_mut().zip(&mut palette.0) {
                *selected |= *palette;
                *palette = 0;
            }
        }
    }

    let physical_count = selected.count();
    let palette_count = palette.count();
    let mut out = Vec::with_capacity(56 + physical_count * 3 + linear.len() + palette_count * 3);
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
    selected.pairs(&mut out, bank, physical_count);
    out.extend_from_slice(&(linear.len() as u32).to_le_bytes());
    out.extend_from_slice(linear);
    palette.pairs(&mut out, bank, palette_count);
    profiling.count("tmem.preimage_bytes_constructed", out.len() as u64);
    profiling.count("tmem.footprint_span_bytes", span_bytes as u64);
    profiling.count("tmem.palette_index_reads", palette_reads);
    out
}

#[cfg(any(test, feature = "profiling"))]
pub(crate) fn serialize(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES], linear: &[u8]) -> Vec<u8> {
    serialize_profiled(recipe, bank, linear, &Recorder::default())
}

#[cfg(test)]
#[path = "serialization_tests.rs"]
mod tests;
