//! Hardware-faithful, byte-addressable TMEM.
//!
//! Models the RDP's 4 KiB texture memory as a flat byte array and reproduces the exact
//! LoadBlock write path and per-tile sample path of the N64 RDP,
//! decoding to RGBA8 on the CPU.
//!
//! The odd-line word swap: on an odd TMEM line the two 4-byte halves of every 8-byte word are
//! swapped. Both the write path (LoadBlock) and the read path (sample) key this swap on the SAME
//! address bit (`0x4`) via the shared [`Tmem::swap_odd_line`] helper, so for a well-formed
//! LoadBlock the two swaps CANCEL — a texel written on an odd line is read back byte-for-byte.
//!
//! Supported formats: the six non-paletted RGBA16 (0,2), I8 (4,1), I4 (4,0), IA16 (3,2),
//! IA8 (3,1), IA4 (3,0); the paletted CI4 (2,0)/CI8 (2,1) whose palette (TLUT) lives in the upper
//! 2 KiB of this same array (loaded via [`Tmem::write_tlut`]); and RGBA32 (0,3), the dual-bank
//! format whose 32-bit texel is split R,G → low bank / B,A → high bank across the two 2 KiB halves.

use crate::hle::rdp::TileDescriptor;
use crate::hle::texdec::{decode_ia16_entry, decode_rgba16_entry};

/// Total TMEM size in bytes (4 KiB).
pub const TMEM_BYTES: usize = 0x1000;
/// Address mask for 8-bit-addressed (non-RGBA32) TMEM: the full 4 KiB.
pub const MASK8: usize = 0xFFF;
/// Address mask for 16-bit-addressed (RGBA32 / TLUT) TMEM: the low 2 KiB bank.
pub const MASK16: usize = 0x7FF;
/// Base of the palette / TLUT region within TMEM.
pub const PALETTE_BASE: usize = 0x800;
/// The DXT accumulator threshold: crossing it advances one virtual row and flips the swap.
pub const DXT_SWAP: u32 = 0x800;

/// The address bit toggled by the odd-line 32-bit word swap.
///
/// Flipping this bit within an 8-byte-aligned word exchanges its two 4-byte halves. The
/// write path expresses this as `tmemXorMask ^= 0x4`; the read path expresses it as
/// `wordIndex ^ 1` (which, since a word is 4 bytes and rows are 8-byte-aligned, is the same
/// `^ 0x4` on the byte address). Keeping both on one bit is what makes the swaps cancel.
const SWAP_BIT: usize = 0x4;

/// Byte-addressable RDP texture memory (4 KiB).
#[derive(Clone, Debug, PartialEq)]
pub struct Tmem {
    bytes: Box<[u8; TMEM_BYTES]>,
}

impl Default for Tmem {
    fn default() -> Self {
        Tmem {
            bytes: Box::new([0u8; TMEM_BYTES]),
        }
    }
}

impl Tmem {
    /// The odd-line 32-bit word swap, shared by the write and read paths. When `odd`, flips bit
    /// `0x4` to exchange the two 4-byte halves of the enclosing 8-byte word; routing both paths
    /// through this one helper is what makes the swaps cancel for a well-formed LoadBlock.
    #[inline]
    fn swap_odd_line(addr: usize, odd: bool) -> usize {
        if odd {
            addr ^ SWAP_BIT
        } else {
            addr
        }
    }

    /// Hardware-faithful LoadBlock write.
    ///
    /// Copies `word_count` contiguous 8-byte words from `src` (already in texture byte order —
    /// fast3d's RDRAM accessor does not need the `^3` byte-swap) into TMEM starting at
    /// `dst_tmem_addr_words << 3`. A `dxt` accumulator advances one word per step; each time it
    /// crosses [`DXT_SWAP`] the destination advances by `stride = line_words << 3` and the odd-line
    /// swap flips (`xor ^= 0x4`). For a well-formed LoadBlock the load tile's `line` is 0, so
    /// `stride` is 0 and the crossing only toggles the swap; the general formula is kept so a
    /// non-zero load line degrades correctly.
    ///
    /// RGBA32 (`siz == 3`) splits its 32-bit texel across the two 2 KiB banks: each 8-byte word
    /// holds two texels; R,G land in the low bank and
    /// B,A in the high bank (`| `[`PALETTE_BASE`]), so `tmem_addr` advances 4 (not 8) and is masked
    /// with [`MASK16`]. The swap runs through the same helper BEFORE the high-bank OR. Every other
    /// format is 8-bytes-per-word and takes the verbatim copy path.
    pub fn write_block(
        &mut self,
        src: &[u8],
        dst_tmem_addr_words: usize,
        line_words: usize,
        dxt: u32,
        word_count: usize,
        siz: u8,
    ) {
        let rgba32 = siz == 3;
        let mask = if rgba32 { MASK16 } else { MASK8 };
        let advance = if rgba32 { 4 } else { 8 };
        // Per-texel RDRAM byte offsets feeding the low bank (R,G of each of the two texels) and the
        // high bank (B,A of each).
        const LOW_SRC: [usize; 4] = [0, 1, 4, 5];
        const HIGH_SRC: [usize; 4] = [2, 3, 6, 7];

        let mut tmem_addr = (dst_tmem_addr_words << 3) & mask;
        let stride = line_words << 3;
        let mut odd = false;
        let mut dxt_counter: u32 = 0;
        let mut tex = 0usize;

        for _ in 0..word_count {
            if rgba32 {
                // loadWord<true, false>: split R,G → low bank, B,A → high bank (dst | 0x800).
                for i in 0..4 {
                    let dst = Self::swap_odd_line(tmem_addr + i, odd) & mask;
                    self.bytes[dst] = src.get(tex + LOW_SRC[i]).copied().unwrap_or(0);
                    self.bytes[dst | PALETTE_BASE] =
                        src.get(tex + HIGH_SRC[i]).copied().unwrap_or(0);
                }
            } else {
                // loadWord<false, false>: copy the whole 8-byte word, applying the odd-line swap.
                for i in 0..8 {
                    let dst = Self::swap_odd_line(tmem_addr + i, odd) & mask;
                    self.bytes[dst] = src.get(tex + i).copied().unwrap_or(0);
                }
            }

            // loadWordStep, BLOCK branch: advance the DXT accumulator, crossing rows.
            dxt_counter = dxt_counter.wrapping_add(dxt);
            while dxt_counter >= DXT_SWAP {
                tmem_addr = (tmem_addr + stride) & mask;
                dxt_counter -= DXT_SWAP;
                odd = !odd;
            }
            tex += 8;
            tmem_addr = (tmem_addr + advance) & mask;
        }
    }

    /// Hardware-faithful LoadTile write.
    ///
    /// Copies a `words_per_row × row_count` region out of a strided RDRAM image into TMEM. Unlike
    /// LoadBlock there is NO DXT accumulator; the odd-line swap flips ONCE PER ROW (`loadRowStep`
    /// `tmemXorMask ^= 0x4`), i.e. keyed on `row & 1` through the same [`swap_odd_line`] helper the
    /// sampler uses, so it cancels against the read swap.
    ///
    /// Row `r` reads its words from `src[r * src_stride_bytes]` (`src_stride_bytes` = tex-image
    /// `bytesPerRow`), gathering a sub-rectangle from a wider image, and writes them to TMEM row
    /// `(dst_tmem_word << 3) + r * (line_words << 3)`. That padded `line_words << 3` is a GENUINE
    /// per-row stride (unlike LoadBlock's contiguous rows), which lets [`sample_tile`] read a
    /// LoadTile tile correctly even for sub-word widths via the render tile's `line`. RGBA32
    /// (`siz == 3`) uses the same dual-bank split as [`write_block`], with the per-row swap.
    #[allow(clippy::too_many_arguments)]
    pub fn write_tile(
        &mut self,
        src: &[u8],
        dst_tmem_word: usize,
        line_words: usize,
        row_count: usize,
        words_per_row: usize,
        src_stride_bytes: usize,
        siz: u8,
    ) {
        let rgba32 = siz == 3;
        let mask = if rgba32 { MASK16 } else { MASK8 };
        let advance = if rgba32 { 4 } else { 8 };
        const LOW_SRC: [usize; 4] = [0, 1, 4, 5];
        const HIGH_SRC: [usize; 4] = [2, 3, 6, 7];

        let tmem_start = (dst_tmem_word << 3) & mask;
        let tmem_stride = line_words << 3;

        for r in 0..row_count {
            let odd = (r & 1) == 1;
            let mut tmem_addr = (tmem_start + r * tmem_stride) & mask;
            let src_row = r * src_stride_bytes;
            for w in 0..words_per_row {
                let tex = src_row + w * 8;
                if rgba32 {
                    // loadWord<true, false>: split R,G → low bank, B,A → high bank (dst | 0x800).
                    for i in 0..4 {
                        let dst = Self::swap_odd_line(tmem_addr + i, odd) & mask;
                        self.bytes[dst] = src.get(tex + LOW_SRC[i]).copied().unwrap_or(0);
                        self.bytes[dst | PALETTE_BASE] =
                            src.get(tex + HIGH_SRC[i]).copied().unwrap_or(0);
                    }
                } else {
                    // loadWord<false, false>: copy the whole 8-byte word, applying the odd-line swap.
                    for i in 0..8 {
                        let dst = Self::swap_odd_line(tmem_addr + i, odd) & mask;
                        self.bytes[dst] = src.get(tex + i).copied().unwrap_or(0);
                    }
                }
                tmem_addr = (tmem_addr + advance) & mask;
            }
        }
    }

    /// Load packed BE halfwords, repeating each across its destination word.
    /// `dst_word` is a 64-bit TMEM word address; writes wrap at 4 KiB.
    pub fn write_tlut(&mut self, entries_be: &[u8], count: usize, dst_word: usize) {
        let base = (dst_word << 3) & MASK8;
        for i in 0..count {
            let entry = &entries_be[i * 2..i * 2 + 2];
            let addr = (base + i * 8) & MASK8;
            for halfword in self.bytes[addr..addr + 8].as_chunks_mut::<2>().0 {
                halfword.copy_from_slice(entry);
            }
        }
    }

    /// The palette (TLUT) region of TMEM — the upper 2 KiB starting at [`PALETTE_BASE`].
    ///
    /// Single source of truth for palette bytes: both the faithful CI sampler
    /// ([`sample_tile`](Self::sample_tile)) and the legacy linear `texdec::decode_ci*` fallback
    /// read from here. Entry `i` of palette 0 lives at byte `i*8` of this slice.
    pub fn palette(&self) -> &[u8] {
        &self.bytes[PALETTE_BASE..]
    }

    /// Read one TMEM byte at row-relative address `rel`, honoring the odd-line swap, masking the
    /// final address with `mask`. Mirrors `implLoadTMEM`: on an odd row the row-relative address
    /// has its word swapped (bit `0x4`) before adding `base`, then the sum is masked.
    ///
    /// `mask` is [`MASK8`] for the six non-paletted formats (whole 4 KiB) and [`MASK16`] for the
    /// CI **index** read (TLUT sampling addresses only the low 2 KiB bank, matching the RDP's
    /// `addressMask = usesTlut ? RDP_TMEM_MASK16 : RDP_TMEM_MASK8`).
    #[inline]
    fn load_byte_masked(&self, base: usize, rel: usize, odd_row: bool, mask: usize) -> u8 {
        let addr = (base + Self::swap_odd_line(rel, odd_row)) & mask;
        self.bytes[addr]
    }

    /// The non-paletted byte read: [`load_byte_masked`](Self::load_byte_masked) with [`MASK8`].
    #[inline]
    fn load_byte(&self, base: usize, rel: usize, odd_row: bool) -> u8 {
        self.load_byte_masked(base, rel, odd_row, MASK8)
    }

    /// The RGBA32 dual-bank byte read. Mirrors `implLoadTMEM(..., MASK16, orAddress)`: the address
    /// is confined to one 2 KiB bank ([`MASK16`]) after the odd-line swap, then the high-bank read
    /// ORs [`PALETTE_BASE`] (`orAddress = RDP_TMEM_BYTES >> 1`). `or_addr` is 0 for the R,G bytes in
    /// the low bank and [`PALETTE_BASE`] for the B,A bytes in the high bank.
    #[inline]
    fn load_byte_rgba32(&self, base: usize, rel: usize, odd_row: bool, or_addr: usize) -> u8 {
        let addr = ((base + Self::swap_odd_line(rel, odd_row)) & MASK16) | or_addr;
        self.bytes[addr]
    }

    /// Read a 16-bit big-endian palette entry DIRECTLY from the TLUT region and decode it by
    /// `tlut_fmt`. The palette read is a direct load of `paletteAddress & MASK8` — it applies
    /// NO odd-line swap (unlike the index read); `paddr` addresses the
    /// high byte, `paddr+1` the low byte. `tlut_fmt`: 2 => RGBA16 entry, 3 => IA16 entry, else
    /// transparent black (matching `texdec::decode_ci*`).
    #[inline]
    fn load_palette_entry(&self, paddr: usize, tlut_fmt: u8) -> [u8; 4] {
        let hi = self.bytes[paddr & MASK8];
        let lo = self.bytes[(paddr + 1) & MASK8];
        let entry = ((hi as u16) << 8) | lo as u16;
        match tlut_fmt {
            2 => decode_rgba16_entry(entry),
            3 => decode_ia16_entry(entry),
            _ => [0, 0, 0, 0],
        }
    }

    /// Decode a whole tile from TMEM to RGBA8 (`width * height * 4` bytes), row-major.
    ///
    /// For texel `(x, y)`:
    /// `rel = y * (line << 3) + ((x << tmemShift) >> 1)`, `tmemShift = {4b:0, 8b:1, 16b:2}`,
    /// with the odd-row swap applied per byte via [`load_byte`](Self::load_byte). The decode
    /// expansions are byte-identical to `texdec`.
    ///
    /// `tlut_fmt` (othermode TT: 0=NONE, 2=RGBA16, 3=IA16) is consulted only by the CI4/CI8
    /// arms, which read a palette index from the low bank and resolve it against the TLUT region.
    pub fn sample_tile(&self, tile: &TileDescriptor, tlut_fmt: u8) -> Vec<u8> {
        let w = tile.width as usize;
        let h = tile.height as usize;
        let base = (tile.tmem_addr as usize) << 3;
        let stride = (tile.line as usize) << 3;
        let palette = tile.palette as usize;
        // log2 of the pixel stride in half-bytes: 4b->0, 8b->1, 16b->2. RGBA32 (siz 3) is also 2,
        // not 3: a 32-bit RGBA texel occupies only 16 bits per bank, so its pixel stride is 16 bits.
        let tmem_shift = match tile.siz {
            0 => 0,
            1 => 1,
            _ => 2,
        };

        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let odd = (y & 1) == 1;
            for x in 0..w {
                let rel = y * stride + ((x << tmem_shift) >> 1);
                let px =
                    self.decode_texel(tile.fmt, tile.siz, base, rel, odd, x, tlut_fmt, palette);
                let o = (y * w + x) * 4;
                out[o..o + 4].copy_from_slice(&px);
            }
        }
        out
    }

    pub fn sampling_lookup(&self, tile: &TileDescriptor, tlut_fmt: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(TMEM_BYTES * 4 * 4);
        for odd in [false, true] {
            for parity in 0..2 {
                for rel in 0..TMEM_BYTES {
                    out.extend_from_slice(&self.decode_texel(
                        tile.fmt,
                        tile.siz,
                        usize::from(tile.tmem_addr) * 8,
                        rel,
                        odd,
                        parity,
                        tlut_fmt,
                        usize::from(tile.palette),
                    ));
                }
            }
        }
        out
    }

    /// Decode a single texel to RGBA8, reproducing the `texdec` expansions exactly.
    ///
    /// `tlut_fmt` / `palette` are used only by the CI4/CI8 arms.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn decode_texel(
        &self,
        fmt: u8,
        siz: u8,
        base: usize,
        rel: usize,
        odd_row: bool,
        x: usize,
        tlut_fmt: u8,
        palette: usize,
    ) -> [u8; 4] {
        match (fmt, siz) {
            // RGBA16: big-endian 5/5/5/1 word, channel bit-replicated (c<<3)|(c>>2).
            (0, 2) => {
                let hi = self.load_byte(base, rel, odd_row);
                let lo = self.load_byte(base, rel + 1, odd_row);
                decode_rgba16_entry(((hi as u16) << 8) | lo as u16)
            }
            // RGBA32: dual-bank. R,G are the two bytes at `rel`/`rel+1` in the LOW bank; B,A the two
            // bytes at the SAME relative address in the HIGH bank (`| PALETTE_BASE`). No expansion —
            // these are already 8-bit channels. Assembles the 32-bit texel as
            // `(r<<24)|(g<<16)|(b<<8)|a`, i.e. the byte order [r, g, b, a].
            (0, 3) => {
                let r = self.load_byte_rgba32(base, rel, odd_row, 0);
                let g = self.load_byte_rgba32(base, rel + 1, odd_row, 0);
                let b = self.load_byte_rgba32(base, rel, odd_row, PALETTE_BASE);
                let a = self.load_byte_rgba32(base, rel + 1, odd_row, PALETTE_BASE);
                [r, g, b, a]
            }
            // CI4: 4-bit palette index (even column = high nibble). The index read honors the
            // odd-line swap and is confined to the low 2 KiB bank (MASK16); the palette entry is
            // then read directly (no swap) from a 32-entry sub-palette selected by `palette<<7`.
            (2, 0) => {
                let byte = self.load_byte_masked(base, rel, odd_row, MASK16);
                let index = if x & 1 == 0 { byte >> 4 } else { byte & 0x0F } as usize;
                let paddr = PALETTE_BASE + (palette << 7) + (index << 3);
                self.load_palette_entry(paddr, tlut_fmt)
            }
            // CI8: 8-bit palette index into the full table (palette ignored). Same swap/mask
            // discipline as CI4 on the index read; palette entry read directly.
            (2, 1) => {
                let index = self.load_byte_masked(base, rel, odd_row, MASK16) as usize;
                let paddr = PALETTE_BASE + (index << 3);
                self.load_palette_entry(paddr, tlut_fmt)
            }
            // I8: 8-bit intensity broadcast to all four channels (alpha = intensity).
            (4, 1) => {
                let v = self.load_byte(base, rel, odd_row);
                [v, v, v, v]
            }
            // I4: even column = high nibble; 4->8 by replication, broadcast incl. alpha.
            (4, 0) => {
                let byte = self.load_byte(base, rel, odd_row);
                let v4 = if x & 1 == 0 { byte >> 4 } else { byte & 0x0F };
                let v8 = (v4 << 4) | v4;
                [v8, v8, v8, v8]
            }
            // IA16: high byte intensity, low byte explicit alpha.
            (3, 2) => {
                let i = self.load_byte(base, rel, odd_row);
                let a = self.load_byte(base, rel + 1, odd_row);
                [i, i, i, a]
            }
            // IA8: high nibble intensity, low nibble alpha, each 4->8 by replication.
            (3, 1) => {
                let v = self.load_byte(base, rel, odd_row);
                let i4 = (v >> 4) & 0xF;
                let a4 = v & 0xF;
                let i8 = (i4 << 4) | i4;
                let a8 = (a4 << 4) | a4;
                [i8, i8, i8, a8]
            }
            // IA4: even column = high nibble; 3-bit intensity + 1-bit explicit alpha.
            (3, 0) => {
                let byte = self.load_byte(base, rel, odd_row);
                let nib = if x & 1 == 0 { byte >> 4 } else { byte & 0x0F };
                let i_raw = nib & 0x0E;
                let i8 = (i_raw << 4) | (i_raw << 1) | (i_raw >> 2);
                let a = if nib & 1 != 0 { 255 } else { 0 };
                [i8, i8, i8, a]
            }
            // Formats outside this phase (RGBA32, YUV, ...) are handled elsewhere; decode as
            // RGBA16 rather than fail silently, matching texdec's fallback.
            (f, s) => {
                eprintln!("tmem::sample_tile: unimplemented format (fmt={f}, siz={s}); decoding as RGBA16");
                let hi = self.load_byte(base, rel, odd_row);
                let lo = self.load_byte(base, rel + 1, odd_row);
                decode_rgba16_entry(((hi as u16) << 8) | lo as u16)
            }
        }
    }

    /// Raw byte access into TMEM (for tests / diagnostics).
    #[cfg(test)]
    fn raw(&self, i: usize) -> u8 {
        self.bytes[i]
    }

    /// Raw byte write into TMEM (for hand-computed tests that place bytes at exact addresses).
    #[cfg(test)]
    fn raw_set(&mut self, i: usize, v: u8) {
        self.bytes[i] = v;
    }
}

// ── round-trip property tests ───────────────────────────────────────────────────────────────────
//
// Every test synthesizes a well-formed LoadBlock (load tile line = 0, CALC_DXT dxt), writes it,
// then samples through the render tile — proving the write/read swaps cancel and stride is right.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_lookup_matches_tmem_all_supported_formats() {
        let mut tmem = Tmem::default();
        tmem.bytes.copy_from_slice(&pattern(4096));
        for (fmt, siz) in [
            (0, 2),
            (0, 3),
            (4, 0),
            (4, 1),
            (3, 0),
            (3, 1),
            (3, 2),
            (2, 0),
            (2, 1),
        ] {
            for (base, palette, tlut) in [(0, 0, 2), (255, 7, 3), (511, 15, 2)] {
                let tile = TileDescriptor {
                    fmt,
                    siz,
                    tmem_addr: base,
                    palette,
                    ..Default::default()
                };
                let lookup = tmem.sampling_lookup(&tile, tlut);
                assert_eq!(lookup.len(), 65536);
                for odd in 0..2 {
                    for parity in 0..2 {
                        for rel in 0..4096 {
                            let offset = ((odd * 2 + parity) * 4096 + rel) * 4;
                            let expected = tmem.decode_texel(
                                fmt,
                                siz,
                                usize::from(base) * 8,
                                rel + 4096,
                                odd != 0,
                                parity,
                                tlut,
                                usize::from(palette),
                            );
                            assert_eq!(lookup[offset..offset + 4], expected,
                                "fmt {fmt}/{siz}, base {base}, rel {rel}, odd {odd}, parity {parity}");
                        }
                    }
                }
            }
        }
    }

    /// libultra CALC_DXT for a texture whose row occupies `line_words` 64-bit words:
    /// `dxt = ceil(2048 / line_words)`, i.e. `(2048 + n - 1) / n`.
    fn dxt_for(line_words: usize) -> u32 {
        DXT_SWAP.div_ceil(line_words as u32)
    }

    /// Render-tile `line` (64-bit words) for a `width`-texel row in size `siz`.
    /// `line_bytes = (width << siz) >> 1` (ceil for 4-bit), `line = (line_bytes + 7) >> 3`.
    fn render_line_words(width: usize, siz: u8) -> usize {
        let line_bytes = ((width << siz) + 1) >> 1; // +1 gives ceil for 4-bit odd widths
        (line_bytes + 7) >> 3
    }

    fn tile(
        fmt: u8,
        siz: u8,
        width: usize,
        height: usize,
        line: usize,
        tmem_addr: usize,
    ) -> TileDescriptor {
        TileDescriptor {
            width: width as u16,
            height: height as u16,
            fmt,
            siz,
            line: line as u16,
            tmem_addr: tmem_addr as u16,
            ..Default::default()
        }
    }

    // A tiny deterministic PRNG so the pattern has no accidental structure.
    fn pattern(n: usize) -> Vec<u8> {
        let mut s: u32 = 0x1234_5678;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn calc_dxt_is_ceil_2048_over_line_words() {
        assert_eq!(dxt_for(8), 256); // width-32 RGBA16
        assert_eq!(dxt_for(1), 2048); // one word per row
        assert_eq!(dxt_for(3), 683); // ceil(2048/3) = 683
    }

    #[test]
    fn roundtrip_rgba16_width32_byte_identical_to_flat_decode() {
        // width % 4 == 0 (the load-bearing invariant precondition): zero padding, swaps cancel,
        // sample output MUST equal the old flat decode byte-for-byte. Height 4 exercises odd rows.
        let (w, h, siz) = (32usize, 4usize, 2u8);
        let line = render_line_words(w, siz); // 8
        let dxt = dxt_for(line); // 256
        let word_count = w * h * 2 / 8; // 32
        let src = pattern(w * h * 2); // 256 bytes, contiguous RGBA16

        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0 /* load line */, dxt, word_count, siz);
        let got = tmem.sample_tile(&tile(0, siz, w, h, line, 0), 0);

        let expected = crate::hle::combiner::decode_rgba16(&src);
        assert_eq!(
            got, expected,
            "RGBA16 width%4==0 must be byte-identical to flat decode"
        );
    }

    #[test]
    fn write_block_physically_swaps_odd_line_halves() {
        // width-4 RGBA16: one 64-bit word per row, dxt=2048 flips the swap every row.
        // Row 0 (even) is stored verbatim; row 1 (odd) has its two 4-byte halves swapped in TMEM.
        let src: Vec<u8> = vec![
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, // row 0 (even)
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, // row 1 (odd)
        ];
        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0, 2048, 2, 2);

        // Row 0 verbatim.
        for (i, &b) in src[..8].iter().enumerate() {
            assert_eq!(tmem.raw(i), b, "even row must be verbatim at byte {i}");
        }
        // Row 1: halves swapped -> [14 15 16 17 10 11 12 13].
        let expect_row1 = [0x14, 0x15, 0x16, 0x17, 0x10, 0x11, 0x12, 0x13];
        for (i, &b) in expect_row1.iter().enumerate() {
            assert_eq!(tmem.raw(8 + i), b, "odd row half-swap wrong at byte {i}");
        }

        // ...and sampling recovers the original texels (the swap cancels on read).
        let got = tmem.sample_tile(&tile(0, 2, 4, 2, render_line_words(4, 2), 0), 0);
        assert_eq!(got, crate::hle::combiner::decode_rgba16(&src));
    }

    #[test]
    fn roundtrip_i4_odd_width_stride_padded() {
        // Odd width (5) 4-bit format. Row occupies 3 bytes but the render tile stride is one
        // 64-bit word (8 bytes), so the source is stride-padded to 8 bytes/row. dxt=2048 toggles
        // the swap every row; height 3 exercises even/odd/even.
        let (w, h, siz) = (5usize, 3usize, 0u8);
        let line = render_line_words(w, siz); // (ceil(5/2)=3 -> (3+7)>>3 = 1) word
        assert_eq!(line, 1);
        let line_bytes = ((w << siz) + 1) >> 1; // 3
        let dxt = dxt_for(line); // 2048
        let stride_bytes = line << 3; // 8
        let word_count = h; // one word per row

        // Build a stride-padded source: each row's first `line_bytes` bytes carry the nibbles,
        // the rest is padding. Nibble (row r, byte b): high = r*16 + b*2, low = r*16 + b*2 + 1.
        let mut src = vec![0u8; word_count * 8];
        for r in 0..h {
            for b in 0..line_bytes {
                let hi = ((r * 16 + b * 2) & 0xF) as u8;
                let lo = ((r * 16 + b * 2 + 1) & 0xF) as u8;
                src[r * stride_bytes + b] = (hi << 4) | lo;
            }
        }

        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0, dxt, word_count, siz);
        let got = tmem.sample_tile(&tile(4, siz, w, h, line, 0), 0);

        // Independently expand every texel from the source nibbles (even column = high nibble).
        for y in 0..h {
            for x in 0..w {
                let byte = src[y * stride_bytes + (x >> 1)];
                let v4 = if x & 1 == 0 { byte >> 4 } else { byte & 0x0F };
                let v8 = (v4 << 4) | v4;
                let o = (y * w + x) * 4;
                assert_eq!(
                    &got[o..o + 4],
                    &[v8, v8, v8, v8],
                    "I4 texel ({x},{y}) mismatch"
                );
            }
        }
    }

    #[test]
    fn roundtrip_ia16_multirow_swap_cancels() {
        // IA16, width 8 (%4==0), height 3: 16-bit format, non-zero dxt (256/row... line=2),
        // odd rows toggle the swap, and IA16's intensity/alpha split must survive the round-trip.
        let (w, h, siz) = (8usize, 3usize, 2u8);
        let line = render_line_words(w, siz); // (8*4)>>1 = 16 bytes -> (16+7)>>3 = 2
        assert_eq!(line, 2);
        let dxt = dxt_for(line); // ceil(2048/2)=1024
        let word_count = w * h * 2 / 8; // 6
        let src = pattern(w * h * 2);

        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0, dxt, word_count, siz);
        let got = tmem.sample_tile(&tile(3, siz, w, h, line, 0), 0);

        // IA16: [i, i, i, a] with i = src[2k], a = src[2k+1] in contiguous order.
        for k in 0..w * h {
            let i = src[k * 2];
            let a = src[k * 2 + 1];
            assert_eq!(
                &got[k * 4..k * 4 + 4],
                &[i, i, i, a],
                "IA16 texel {k} mismatch"
            );
        }
    }

    #[test]
    fn roundtrip_i8_nonzero_tmem_base() {
        // I8, width 8, height 3, loaded at a non-zero TMEM word address to prove `base` handling.
        let (w, h, siz) = (8usize, 3usize, 1u8);
        let line = render_line_words(w, siz); // 8 bytes -> 1 word
        assert_eq!(line, 1);
        let dxt = dxt_for(line); // 2048
        let word_count = w * h / 8; // 3
        let base_words = 16; // load at TMEM byte 128
        let src = pattern(w * h);

        let mut tmem = Tmem::default();
        tmem.write_block(&src, base_words, 0, dxt, word_count, siz);
        let got = tmem.sample_tile(&tile(4, siz, w, h, line, base_words), 0);

        for k in 0..w * h {
            let v = src[k];
            assert_eq!(
                &got[k * 4..k * 4 + 4],
                &[v, v, v, v],
                "I8 texel {k} mismatch"
            );
        }
    }

    #[test]
    fn roundtrip_rgba16_width12_dxt_drift_swap_cancels() {
        // A NON-power-of-2 line where the DXT accumulator drifts per row. width 12 RGBA16 →
        // line_bytes = 24 = 3 words/row, dxt = ceil(2048/3) = 683. 3*683 = 2049 overshoots 2048 by 1
        // each row, so the remainder drifts and the swap toggles at non-uniform accumulator points
        // rather than on a clean reset. width % 4 == 0 keeps the layout aligned, so the write and read
        // swaps must still cancel exactly — proving cancellation survives dxt drift.
        let (w, h, siz) = (12usize, 4usize, 2u8);
        let line = render_line_words(w, siz); // 3
        assert_eq!(line, 3, "width-12 RGBA16 must be 3 words/row");
        let dxt = dxt_for(line); // 683
        assert_eq!(
            dxt, 683,
            "ceil(2048/3) must be 683 (non-power-of-2, drifting)"
        );
        let word_count = w * h * 2 / 8; // 12
        let src = pattern(w * h * 2); // 96 bytes, contiguous RGBA16

        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0 /* load line */, dxt, word_count, siz);
        let got = tmem.sample_tile(&tile(0, siz, w, h, line, 0), 0);

        // The drifting accumulator must still land the swap on every row boundary, so the result is
        // byte-identical to the plain linear decode of the contiguous source.
        let expected = crate::hle::combiner::decode_rgba16(&src);
        assert_eq!(
            got, expected,
            "RGBA16 width-12 (dxt=683 drift) must recover the source byte-for-byte"
        );
    }

    #[test]
    fn roundtrip_ci8_faithful_palette_via_write_tlut() {
        // CI8, width 8 (line_bytes = 8 → word-aligned), height 2 (even + odd row). Index data is
        // LoadBlock-written into the low bank; the palette is written into the upper half via
        // write_tlut. Sampling must read the index (with the odd-row swap) and resolve it against
        // the TLUT (no swap), recovering the RGBA16 palette colors.
        let (w, h, siz) = (8usize, 2usize, 1u8);
        let line = render_line_words(w, siz); // 1
        let dxt = dxt_for(line); // 2048
        let word_count = w * h / 8; // 2
                                    // Indices 0..15 across the 16 texels.
        let indices: Vec<u8> = (0..(w * h) as u8).collect();

        // Palette: entry i = RGBA16 with r5 = i, so decoding is distinctive per index.
        let mut pal = vec![0u8; (w * h) * 2];
        for i in 0..w * h {
            let v: u16 = ((i as u16 & 0x1F) << 11) | 1; // r5=i, a=1
            pal[i * 2] = (v >> 8) as u8;
            pal[i * 2 + 1] = (v & 0xFF) as u8;
        }

        let mut tmem = Tmem::default();
        tmem.write_block(&indices, 0, 0, dxt, word_count, siz);
        tmem.write_tlut(&pal, w * h, PALETTE_BASE >> 3); // dst_word 0x100 → base 0x800

        let got = tmem.sample_tile(&tile(2, siz, w, h, line, 0), 2 /* RGBA16 TLUT */);

        for k in 0..w * h {
            let v: u16 = ((k as u16 & 0x1F) << 11) | 1;
            let want = decode_rgba16_entry(v);
            assert_eq!(
                &got[k * 4..k * 4 + 4],
                &want,
                "CI8 texel {k} palette mismatch"
            );
        }
    }

    // ── RGBA32 dual-bank ────────────────────────────────────────────────────────────────────────

    /// The N64 packs the 32-bit RGBA texel as `(r<<24)|(g<<16)|(b<<8)|a`. The CPU path
    /// keeps the four bytes as `[r, g, b, a]`; this re-derives the channels from that packed word to
    /// prove the byte order matches the hardware exactly.
    fn rgba32_pack(px: [u8; 4]) -> u32 {
        ((px[0] as u32) << 24) | ((px[1] as u32) << 16) | ((px[2] as u32) << 8) | px[3] as u32
    }

    #[test]
    fn write_block_splits_rgba32_across_banks() {
        // One 8-byte RDRAM word = two RGBA32 texels. Even line (row 0): R,G land verbatim in the low
        // bank at 0..3; B,A land verbatim in the HIGH bank (| 0x800) at 0x800..0x803. Proves the
        // dual-bank offset map low={0,1,4,5}, high={2,3,6,7}.
        let src: Vec<u8> = vec![
            0x10, 0x11, 0x12, 0x13, // texel0: R=10 G=11 B=12 A=13
            0x20, 0x21, 0x22, 0x23, // texel1: R=20 G=21 B=22 A=23
        ];
        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0, 0, 1, 3); // 1 word, siz=3, dxt=0 (no swap)

        // Low bank: R0 G0 R1 G1.
        assert_eq!(tmem.raw(0), 0x10, "R0 → low[0]");
        assert_eq!(tmem.raw(1), 0x11, "G0 → low[1]");
        assert_eq!(tmem.raw(2), 0x20, "R1 → low[2]");
        assert_eq!(tmem.raw(3), 0x21, "G1 → low[3]");
        // High bank (| PALETTE_BASE): B0 A0 B1 A1.
        assert_eq!(tmem.raw(PALETTE_BASE), 0x12, "B0 → high[0]");
        assert_eq!(tmem.raw(PALETTE_BASE + 1), 0x13, "A0 → high[1]");
        assert_eq!(tmem.raw(PALETTE_BASE + 2), 0x22, "B1 → high[2]");
        assert_eq!(tmem.raw(PALETTE_BASE + 3), 0x23, "A1 → high[3]");
    }

    #[test]
    fn rgba32_2x2_hand_computed_dual_bank_decode() {
        // Hand-place a 2×2 RGBA32 tile at exactly the addresses sample_tile reads (render line = 1 →
        // stride 8), with distinct per-channel bytes, then assert the decode and cross-check the
        // RGBA32 channel packing. The odd row (y=1) is placed at the SWAPPED low/high addresses
        // (rel ^ 0x4) so the read swap recovers it — hand-computed, no round-trip.
        let (w, h) = (2usize, 2usize);
        let stride = 8usize; // render tile line = 1
        let texels: [[u8; 4]; 4] = [
            [0x11, 0x22, 0x33, 0x44], // (0,0)
            [0x55, 0x66, 0x77, 0x88], // (1,0)
            [0x99, 0xAA, 0xBB, 0xCC], // (0,1)
            [0xDD, 0xEE, 0x0F, 0x1E], // (1,1)
        ];

        let mut tmem = Tmem::default();
        for y in 0..h {
            let odd = (y & 1) == 1;
            for x in 0..w {
                let rel = y * stride + 2 * x; // (x << 2) >> 1
                let lo = if odd { rel ^ SWAP_BIT } else { rel } & MASK16;
                let [r, g, b, a] = texels[y * w + x];
                tmem.raw_set(lo, r);
                tmem.raw_set(lo + 1, g);
                tmem.raw_set(lo | PALETTE_BASE, b);
                tmem.raw_set((lo + 1) | PALETTE_BASE, a);
            }
        }

        let got = tmem.sample_tile(&tile(0, 3, w, h, 1, 0), 0);
        for k in 0..w * h {
            let want = texels[k];
            let px: [u8; 4] = got[k * 4..k * 4 + 4].try_into().unwrap();
            assert_eq!(px, want, "RGBA32 2×2 texel {k} decode mismatch");
            // Cross-check the RGBA32 packed channel order.
            let packed = rgba32_pack(want);
            assert_eq!((packed >> 24) & 0xFF, want[0] as u32, "r");
            assert_eq!((packed >> 16) & 0xFF, want[1] as u32, "g");
            assert_eq!((packed >> 8) & 0xFF, want[2] as u32, "b");
            assert_eq!(packed & 0xFF, want[3] as u32, "a");
        }
    }

    #[test]
    fn roundtrip_rgba32_multirow_swap_cancels() {
        // width 4 (2 words/row), height 3 (even/odd/even → the odd-line swap toggles). A well-formed
        // LoadBlock (load line = 0, dxt = ceil(2048/2) = 1024) writes the RG-low / BA-high split; the
        // render tile (line = 1 → stride 8 = the 2-bytes/texel low-bank row footprint) samples it.
        // The write split + write swap and the read split + read swap all cancel, so the sampled
        // buffer is byte-identical to the RDRAM source (RGBA32 has no channel expansion).
        let (w, h) = (4usize, 3usize);
        let render_line = 1usize; // 2*w bytes low-bank footprint / 8 = w/4 = 1
        let word_count = w * h / 2; // 6 words (2 texels/word)
        let dxt = DXT_SWAP / 2; // 1024: cross once per 2-word row → swap toggles every row

        // Distinct per-channel bytes across all 12 texels (48 bytes 10..=57).
        let mut src = vec![0u8; w * h * 4];
        for (t, chunk) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            for (c, b) in chunk.iter_mut().enumerate() {
                *b = (10 + t * 4 + c) as u8;
            }
        }

        let mut tmem = Tmem::default();
        tmem.write_block(&src, 0, 0 /* load line */, dxt, word_count, 3);
        let got = tmem.sample_tile(&tile(0, 3, w, h, render_line, 0), 0);

        assert_eq!(
            got, src,
            "RGBA32 dual-bank round-trip must recover the source byte-for-byte"
        );
        // And the recovered channels pack in RGBA32 channel order.
        for t in 0..w * h {
            let px: [u8; 4] = got[t * 4..t * 4 + 4].try_into().unwrap();
            let packed = rgba32_pack(px);
            let expect = ((src[t * 4] as u32) << 24)
                | ((src[t * 4 + 1] as u32) << 16)
                | ((src[t * 4 + 2] as u32) << 8)
                | src[t * 4 + 3] as u32;
            assert_eq!(packed, expect, "RGBA32 texel {t} packed word mismatch");
        }
    }

    // ── LoadTile / write_tile ─────────────────────────────────────────────────────────────────

    #[test]
    fn write_tile_roundtrip_strided_source_padded_dest_per_row_swap() {
        // RGBA16, 4 texels/row (1 word = 8 bytes consumed), 3 rows → the per-row odd-line swap
        // toggles even/odd/even. TWO independent paddings are exercised at once:
        //   * SOURCE stride 12 (4 gap bytes after each row's 8 payload bytes) — proves the strided
        //     gather reads row r at src[r*12] and never touches the gap.
        //   * TMEM stride 16 (line_words = 2, though a row is only 8 bytes) — proves write_tile lays
        //     down a GENUINE padded per-row stride, and the render tile (line = 2) samples it back.
        let (w, h) = (4usize, 3usize);
        let words_per_row = 1usize; // (w-1)>>2 + 1 = 1 for RGBA16
        let line_words = 2usize; // TMEM stride 16 > 8 consumed bytes/row (padded)
        let src_stride = 12usize; // source stride 12 > 8 consumed bytes/row (padded)

        // Distinct RGBA16 texel per (row, col); bytes 8..12 of each source row are gap (0xEE) that
        // MUST NOT reach TMEM.
        let mut src = vec![0xEEu8; h * src_stride];
        for r in 0..h {
            for t in 0..w {
                let texel: u16 = (((r * w + t) as u16) << 6) | 1; // distinct, a-bit = 1
                let o = r * src_stride + t * 2;
                src[o] = (texel >> 8) as u8;
                src[o + 1] = (texel & 0xFF) as u8;
            }
        }

        let mut tmem = Tmem::default();
        tmem.write_tile(&src, 0, line_words, h, words_per_row, src_stride, 2);

        // Physical TMEM: row 0 (even) verbatim at 0..8; row 1 (odd) with its two 4-byte halves
        // swapped at 16..24 (padded stride 16); row 2 (even) verbatim at 32..40.
        for (i, &want) in src[..8].iter().enumerate() {
            assert_eq!(tmem.raw(i), want, "row0 verbatim byte {i}");
        }
        let r1 = &src[src_stride..src_stride + 8];
        let expect_r1 = [r1[4], r1[5], r1[6], r1[7], r1[0], r1[1], r1[2], r1[3]];
        for (i, &want) in expect_r1.iter().enumerate() {
            assert_eq!(tmem.raw(16 + i), want, "row1 odd-swap byte {i}");
        }
        for i in 0..8 {
            assert_eq!(
                tmem.raw(32 + i),
                src[2 * src_stride + i],
                "row2 verbatim byte {i}"
            );
        }
        // No source gap byte (0xEE) leaked into any written TMEM word.
        for base in [0usize, 16, 32] {
            for i in 0..8 {
                assert_ne!(tmem.raw(base + i), 0xEE, "gap byte leaked at {}", base + i);
            }
        }

        // Sample through a render tile whose line == the padded LOAD stride (line_words = 2). The
        // write swap and read swap cancel per row, so recovery is byte-identical to a per-row linear
        // RGBA16 decode of the payload bytes (gap excluded).
        let got = tmem.sample_tile(&tile(0, 2, w, h, line_words, 0), 0);
        for r in 0..h {
            let payload = &src[r * src_stride..r * src_stride + w * 2];
            let want = crate::hle::combiner::decode_rgba16(payload);
            assert_eq!(
                &got[r * w * 4..r * w * 4 + w * 4],
                &want[..],
                "sampled row {r} mismatch"
            );
        }
    }

    #[test]
    fn write_tile_roundtrip_rgba32_dual_bank_per_row_swap() {
        // RGBA32 LoadTile: width 2 (1 word = 2 texels/row), 3 rows → per-row swap toggles. Proves
        // write_tile's dual-bank split (R,G → low / B,A → high) plus the per-row odd-line swap all
        // cancel against sample_tile's dual-bank read swap. Source is contiguous (stride = 8).
        let (w, h) = (2usize, 3usize);
        let words_per_row = 1usize; // (w-1)>>(4-3) + 1 = 1
        let line_words = 1usize; // low-bank footprint 2*w = 4 bytes; stride 8 (1 word)
        let src_stride = 8usize; // 2 texels * 4 bytes, contiguous

        // Distinct per-channel bytes across all 6 texels.
        let mut src = vec![0u8; h * src_stride];
        for (t, chunk) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            for (c, b) in chunk.iter_mut().enumerate() {
                *b = (0x10 + t * 4 + c) as u8;
            }
        }

        let mut tmem = Tmem::default();
        tmem.write_tile(&src, 0, line_words, h, words_per_row, src_stride, 3);
        let got = tmem.sample_tile(&tile(0, 3, w, h, line_words, 0), 0);

        assert_eq!(
            got, src,
            "RGBA32 LoadTile dual-bank round-trip must recover the source byte-for-byte"
        );
    }
}
