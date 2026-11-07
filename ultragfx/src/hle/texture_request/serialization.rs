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

    fn palette_entry(&mut self, index: u8, lanes: bool) {
        let address = (2048 + usize::from(index) * 8) & 4095;
        self.0[address / 64] |= if lanes { 255 } else { 3 } << (address & 63);
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

fn palette_indices(
    recipe: &DecodeRecipe,
    bank: &[u8; TMEM_BYTES],
    mut visit: impl FnMut(usize, u8),
) -> u64 {
    let mut reads = 0;
    let mut index = |address: usize, low: bool| {
        let byte = bank[address];
        reads += 1;
        if recipe.siz == 0 {
            visit(address, (recipe.palette << 4) | (byte >> 4));
            if low && recipe.fmt != 1 {
                visit(address, (recipe.palette << 4) | (byte & 15));
            }
        } else {
            visit(address, byte);
        }
    };
    match recipe.representation {
        Representation::Tile => {
            let width = recipe.output[0] as usize;
            let bytes = if recipe.fmt == 1 {
                width
            } else {
                (width << recipe.siz.min(2)).div_ceil(2)
            };
            let step = if recipe.siz >= 2 && recipe.fmt != 1 {
                2
            } else {
                1
            };
            for y in 0..recipe.output[1] as usize {
                let row = (y & 255) * usize::from(recipe.line) * 8;
                for x in (0..bytes).step_by(step) {
                    let address =
                        (usize::from(recipe.base) * 8 + ((row + x) ^ ((y & 1) * 4))) & 2047;
                    index(address, x + 1 < bytes || width & 1 == 0);
                }
            }
        }
        Representation::Lookup4096x4 => {
            for address in 0..2048 {
                index(address, true);
            }
        }
    }
    reads
}

pub(crate) fn palette_lanes(
    recipe: &DecodeRecipe,
    bank: &[u8; TMEM_BYTES],
    nonreplicated: &[u64; 4],
    profiling: &Recorder,
) -> bool {
    if recipe.tlut < 2 || nonreplicated.iter().all(|&bits| bits == 0) {
        return false;
    }
    let mut used = false;
    let reads = palette_indices(recipe, bank, |_, index| {
        used |= nonreplicated[usize::from(index) / 64] & (1 << (index & 63)) != 0;
    });
    profiling.count("tmem.palette_lane_index_reads", reads);
    used
}

fn select(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES], profiling: &Recorder) -> Selection {
    let mut selected = Selection::default();
    if recipe.tlut >= 2 {
        let reads = palette_indices(recipe, bank, |address, index| {
            selected.range(address, address + 1);
            selected.palette_entry(index, recipe.palette_lanes);
        });
        profiling.count("tmem.footprint_span_bytes", reads);
        profiling.count("tmem.palette_index_reads", reads);
        return selected;
    }
    let rgba32 = recipe.fmt == 0 && recipe.siz == 3;
    let size = if rgba32 { 2048 } else { TMEM_BYTES };
    let base = usize::from(recipe.base) * 8;
    let row_bytes = ((recipe.output[0] as usize) << recipe.siz.min(2)).div_ceil(2);
    let mut span_bytes = 0;
    match recipe.representation {
        Representation::Tile => {
            for y in 0..recipe.output[1] as usize {
                let start = base + (y & 255) * usize::from(recipe.line) * 8;
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
    }
    if rgba32 {
        for i in 0..32 {
            selected.0[i + 32] = selected.0[i];
        }
        span_bytes *= 2;
    }
    profiling.count("tmem.footprint_span_bytes", span_bytes as u64);
    selected
}

pub(super) fn matches(
    recipe: &DecodeRecipe,
    a: &[u8; TMEM_BYTES],
    b: &[u8; TMEM_BYTES],
    profiling: &Recorder,
) -> bool {
    let selected = select(recipe, a, profiling);
    let bytes = selected.count() as u64;
    profiling.count("tmem.encoded_bytes_compared", bytes);
    profiling.count("tmem.bytes_compared", bytes);
    selected.0.iter().enumerate().all(|(word, &bits)| {
        let mut bits = bits;
        while bits != 0 {
            let address = word * 64 + bits.trailing_zeros() as usize;
            if a[address] != b[address] {
                return false;
            }
            bits &= bits - 1;
        }
        true
    })
}

pub(super) fn serialize_profiled(
    recipe: &DecodeRecipe,
    bank: &[u8; TMEM_BYTES],
    profiling: &Recorder,
) -> Vec<u8> {
    let selected = select(recipe, bank, profiling);
    let physical_count = selected.count();
    let mut out = Vec::with_capacity(58 + physical_count * 3);
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
    selected.pairs(&mut out, bank, physical_count);
    out.extend_from_slice(&[0; 8]);
    profiling.count("tmem.preimage_bytes_constructed", out.len() as u64);
    out
}

#[cfg(test)]
pub(crate) fn serialize(recipe: &DecodeRecipe, bank: &[u8; TMEM_BYTES]) -> Vec<u8> {
    serialize_profiled(recipe, bank, &Recorder::default())
}

#[cfg(test)]
#[path = "serialization_tests.rs"]
mod tests;
