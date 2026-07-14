//! N64 texture-format decoders → RGBA8. Authority: libultra/gbi. TMEM is linear.
//!
//! Per-format decode formulas (nibble order + intensity replication) are documented on each
//! decode function below. Nibble order for 4-bit formats: even column = high nibble.

/// Format + size descriptor for N64 texture tile.
///
/// `fmt` matches the GBI `G_IM_FMT_*` constants (0=RGBA, 2=CI, 3=IA, 4=I).
/// `siz` matches `G_IM_SIZ_*` constants (0=4b, 1=8b, 2=16b, 3=32b).
pub struct FormatInfo {
    pub fmt: u8,
    pub siz: u8,
}

impl FormatInfo {
    /// Number of TMEM bytes occupied by a `w × h` texture in this format.
    ///
    /// Dispatches only on `siz` — all formats with the same `siz` share the same byte count:
    /// - siz=0 (4b):  ceil(w*h / 2)  — 2 texels per byte  (I4, IA4)
    /// - siz=1 (8b):  w*h             — 1 byte per texel   (I8, IA8)
    /// - siz=2 (16b): w*h*2           — 2 bytes per texel  (RGBA16, IA16)
    /// - siz=3 (32b): w*h*4           — 4 bytes per texel  (RGBA32)
    pub fn tmem_bytes(&self, w: u32, h: u32) -> usize {
        let texels = (w * h) as usize;
        match self.siz {
            0 => texels.div_ceil(2),
            1 => texels,
            3 => texels * 4,
            _ => texels * 2,
        }
    }

    /// Decode TMEM bytes to RGBA8 (`w × h × 4` bytes).
    ///
    /// Dispatches on `(fmt, siz)`:
    /// - `(4, 1)` → I8 intensity
    /// - `(4, 0)` → I4 packed nibbles
    /// - `(3, 2)` → IA16 intensity+alpha
    /// - `(3, 1)` → IA8 intensity+alpha
    /// - `(3, 0)` → IA4 intensity+alpha
    /// - `(0, 2)` → RGBA16 (delegates to `combiner::decode_rgba16` for byte-identity)
    /// - `(0, 3)` → RGBA32 (direct 8-bit-per-channel copy)
    /// - other    → warn + RGBA16 fallback (NOT silent)
    ///
    /// `tlut`/`palette`/`tlut_fmt` carry TLUT state for CI formats (Task 3); non-CI ignore them.
    pub fn decode(
        &self,
        src: &[u8],
        w: u32,
        h: u32,
        tlut: &[u8],
        palette: u8,
        tlut_fmt: u8,
    ) -> Vec<u8> {
        match (self.fmt, self.siz) {
            (4, 1) => decode_i8(src, w, h),
            (4, 0) => decode_i4(src, w, h),
            (3, 2) => decode_ia16(src, w, h),
            (3, 1) => decode_ia8(src, w, h),
            (3, 0) => decode_ia4(src, w, h),
            (0, 2) => crate::hle::combiner::decode_rgba16(src),
            (0, 3) => decode_rgba32(src, w, h),
            (2, 1) => decode_ci8(src, w, h, tlut, tlut_fmt),
            (2, 0) => decode_ci4(src, w, h, tlut, palette, tlut_fmt),
            (f, s) => {
                eprintln!("texdec: unimplemented format (fmt={f}, siz={s}); decoding as RGBA16");
                crate::hle::combiner::decode_rgba16(src)
            }
        }
    }
}

/// Decode I8 (8-bit intensity) TMEM bytes to RGBA8.
///
/// Each source byte is a full 8-bit intensity value. Output is `[I, I, I, I]` per texel —
/// intensity is replicated to ALL FOUR channels, **including alpha** (N64 I-format semantics).
///
/// I8 expansion: the 8-bit intensity value broadcasts to all four RGBA components, so
/// alpha = intensity (not opaque).
pub fn decode_i8(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n.min(src.len()) {
        let v = src[i];
        out[i * 4..i * 4 + 4].copy_from_slice(&[v, v, v, v]);
    }
    out
}

/// Decode I4 (4-bit intensity, 2 texels per byte) TMEM bytes to RGBA8.
///
/// Nibble order: **even column = high nibble** (`byte >> 4`), odd column = low nibble (`byte & 0xF`).
/// Each 4-bit value is replicated to 8 bits: `v8 = (v4 << 4) | v4`. Output = `[v8, v8, v8, v8]` —
/// intensity replicated to all four channels including alpha (N64 I-format).
///
/// Nibble order (even column = high nibble):
///   `oddColumn=(texelInt.x & 1)`, `pixelShift=select_uint(oddColumn, 0, 4)`,
///   `pixelValue4bit=(pixelValue0 >> pixelShift) & 0xF`.
/// I4 expansion: `(i4 << 4) | i4`, broadcast to RGBA.
///
/// TODO: row-align nibbles for odd-width textures (N64 TMEM starts each row on a byte boundary;
/// this flat-stream packing only matches hardware for even widths — all current test scenes are even).
pub fn decode_i4(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let byte = src.get(i / 2).copied().unwrap_or(0);
        // Even column (i%2==0) → high nibble; odd column (i%2==1) → low nibble.
        let v4 = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
        let v8 = (v4 << 4) | v4;
        out[i * 4..i * 4 + 4].copy_from_slice(&[v8, v8, v8, v8]);
    }
    out
}

/// Decode IA16 (16-bit intensity+alpha, big-endian word) TMEM bytes to RGBA8.
///
/// Each 2-byte big-endian word: high byte = 8-bit intensity, low byte = 8-bit alpha.
/// Output = `[I, I, I, A]` per texel — intensity replicated to RGB, alpha is the explicit
/// second byte. IA formats are distinct from I formats: alpha is NOT the intensity.
///
/// IA16 expansion: `i=(ia16>>8)&0xFF`, `a=ia16&0xFF`.
pub fn decode_ia16(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let base = i * 2;
        let intensity = src.get(base).copied().unwrap_or(0);
        let alpha = src.get(base + 1).copied().unwrap_or(0);
        out[i * 4..i * 4 + 4].copy_from_slice(&[intensity, intensity, intensity, alpha]);
    }
    out
}

/// Decode IA8 (4-bit intensity + 4-bit alpha, 1 byte/texel) TMEM bytes to RGBA8.
///
/// Each byte: high nibble = 4-bit intensity, low nibble = 4-bit alpha.
/// Both nibbles are expanded 4→8 bits via replication: `v8 = (v4 << 4) | v4`.
/// Output = `[I8, I8, I8, A8]` — alpha is the explicit low nibble, NOT the intensity.
///
/// IA8 expansion:
///   `i=(ia8>>4)&0xF; a=(ia8>>0)&0xF; i=(i<<4)|i; a=(a<<4)|a`.
pub fn decode_ia8(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let v = src.get(i).copied().unwrap_or(0);
        let i4 = (v >> 4) & 0xF;
        let a4 = v & 0xF;
        let i8 = (i4 << 4) | i4;
        let a8 = (a4 << 4) | a4;
        out[i * 4..i * 4 + 4].copy_from_slice(&[i8, i8, i8, a8]);
    }
    out
}

/// Decode IA4 (3-bit intensity + 1-bit alpha, 2 texels per byte) TMEM bytes to RGBA8.
///
/// Nibble order: **even column = high nibble** (`byte >> 4`), odd column = low nibble (`byte & 0xF`).
/// Each 4-bit nibble: bits [3:1] = 3-bit intensity, bit [0] = 1-bit alpha.
/// Intensity expansion: `i = nibble & 0x0E; i8 = (i << 4) | (i << 1) | (i >> 2)`.
/// Alpha: `a = (nibble & 1) ? 255 : 0`.
///
/// IA4 alpha is the EXPLICIT 1-bit alpha, NOT the intensity (contrast with I4 which replicates I to A).
///
/// IA4 expansion:
///   `i = ia4 & 0b1110; i = (i << 4) | (i << 1) | (i >> 2);`.
/// Nibble order: same as I4 (even column = high nibble).
///
/// TODO: row-align nibbles for odd-width textures (same caveat as `decode_i4`).
pub fn decode_ia4(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let byte = src.get(i / 2).copied().unwrap_or(0);
        // Even column (i%2==0) → high nibble; odd column (i%2==1) → low nibble.
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
        // Intensity: `i = ia4 & 0b1110; i = (i<<4)|(i<<1)|(i>>2)`.
        let i_raw = nibble & 0x0E;
        let i8 = (i_raw << 4) | (i_raw << 1) | (i_raw >> 2);
        let a = if nibble & 1 != 0 { 255 } else { 0 };
        out[i * 4..i * 4 + 4].copy_from_slice(&[i8, i8, i8, a]);
    }
    out
}

/// Decode RGBA32 (8-bit per channel, 4 bytes/texel) TMEM bytes to RGBA8.
///
/// Each 4-byte texel is stored `R G B A` in memory (RT64 `RGBA32ToFloat4`:
/// `(r<<24)|(g<<16)|(b<<8)|a`). All four channels are already 8-bit, so decode is a direct
/// copy with **no expansion** — the one N64 texture format that needs no bit-replication.
///
/// fast3d's linear-TMEM model decodes the LoadBlock'd source bytes directly, so it sidesteps
/// RGBA32's authentic dual-TMEM-bank split (RG in the low bank, BA in the high bank) — RT64
/// needs that split only because it models real TMEM banks; here the source bytes are contiguous.
pub fn decode_rgba32(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let base = i * 4;
        let r = src.get(base).copied().unwrap_or(0);
        let g = src.get(base + 1).copied().unwrap_or(0);
        let b = src.get(base + 2).copied().unwrap_or(0);
        let a = src.get(base + 3).copied().unwrap_or(0);
        out[base..base + 4].copy_from_slice(&[r, g, b, a]);
    }
    out
}

/// RGBA16 TLUT entry (5/5/5/1 big-endian) → RGBA8. Matches `combiner::decode_rgba16` expand exactly:
/// `(c5 << 3) | (c5 >> 2)`.
fn decode_rgba16_entry(v: u16) -> [u8; 4] {
    let r5 = ((v >> 11) & 0x1F) as u8;
    let g5 = ((v >> 6) & 0x1F) as u8;
    let b5 = ((v >> 1) & 0x1F) as u8;
    let a = if v & 1 != 0 { 255 } else { 0 };
    [
        (r5 << 3) | (r5 >> 2),
        (g5 << 3) | (g5 >> 2),
        (b5 << 3) | (b5 >> 2),
        a,
    ]
}

/// IA16 TLUT entry → RGBA8 (intensity in RGB, explicit alpha):
/// `i=(ia16>>8)&0xFF`, `a=ia16&0xFF`.
fn decode_ia16_entry(v: u16) -> [u8; 4] {
    let i = (v >> 8) as u8;
    let a = (v & 0xFF) as u8;
    [i, i, i, a]
}

/// Decode CI8 (8-bit index per texel) via TLUT to RGBA8.
///
/// Each source byte is a palette index. `tlut_offset = index << 3`. Entry is a
/// big-endian u16 at `tlut[off..off+2]`. Out-of-range index → 0 (zero-pad safety net).
/// Decodes by `tlut_fmt`: 2=RGBA16 entry, 3=IA16 entry, else → transparent black.
///
/// The `palette` field (CI4 sub-palette select) is IGNORED for CI8 — CI8 has no sub-palette.
pub fn decode_ci8(src: &[u8], w: u32, h: u32, tlut: &[u8], tlut_fmt: u8) -> Vec<u8> {
    let n = (w * h) as usize;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let index = src.get(i).copied().unwrap_or(0) as usize;
        let off = index << 3;
        let entry = match (tlut.get(off), tlut.get(off + 1)) {
            (Some(&hi), Some(&lo)) => u16::from_be_bytes([hi, lo]),
            _ => 0, // out-of-range index -> 0 (silent zero-pad safety net)
        };
        let px = match tlut_fmt {
            2 => decode_rgba16_entry(entry),
            3 => decode_ia16_entry(entry),
            _ => [0, 0, 0, 0],
        };
        out[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    out
}

/// Decode CI4 (4-bit index, 2 texels/byte) via banked TLUT to RGBA8.
///
/// Banking formula:
///   `paletteAddress = RDP_TMEM_PALETTE + (palette << 7) + (pixelValue4bit << 3)`
/// Nibble order (even column = high nibble, same as I4):
///   `pixelShift = select_uint(oddColumn, 0, 4); pixelValue4bit = (pixelValue0 >> pixelShift) & 0xF`
/// Entry load (big-endian u16):
///   `paletteValue = loadTLUT(paletteAddress+1) | (loadTLUT(paletteAddress) << 8)`
///
/// `tlut` is the **stride-8 expanded** TMEM buffer (Task 5 accuracy fix: load_tlut DMA-expands
/// packed RDRAM → stride-8, so entry `i` lives at byte `i*8`). Both `(palette<<7)` and
/// `(index<<3)` index directly into this stride-8 buffer — no adjustment needed.
/// Out-of-range index → 0 (zero-pad safety net, same as decode_ci8).
pub fn decode_ci4(src: &[u8], w: u32, h: u32, tlut: &[u8], palette: u8, tlut_fmt: u8) -> Vec<u8> {
    let n = (w * h) as usize;
    let base = (palette as usize) << 7;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let byte = src.get(i / 2).copied().unwrap_or(0);
        // Even column (i%2==0) → high nibble; odd column (i%2==1) → low nibble.
        let index = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F } as usize;
        let off = base + (index << 3);
        let entry = match (tlut.get(off), tlut.get(off + 1)) {
            (Some(&hi), Some(&lo)) => u16::from_be_bytes([hi, lo]),
            _ => 0, // out-of-range index → 0 (silent zero-pad safety net)
        };
        let px = match tlut_fmt {
            2 => decode_rgba16_entry(entry),
            3 => decode_ia16_entry(entry),
            _ => [0, 0, 0, 0],
        };
        out[i * 4..i * 4 + 4].copy_from_slice(&px);
    }
    out
}

// ── unit tests ────────────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i8_replicates_intensity_to_all_channels_incl_alpha() {
        // 2×1: bytes 0x00, 0x80 → [0,0,0,0], [128,128,128,128] (alpha = intensity, N64 I-format)
        let out = decode_i8(&[0x00, 0x80], 2, 1);
        assert_eq!(out, vec![0, 0, 0, 0, 128, 128, 128, 128]);
    }

    #[test]
    fn i4_high_nibble_is_even_column_multirow() {
        // 2×2, 1 byte/row: row0=0xF0 → [255..],[0..]; row1=0x0F → [0..],[255..]
        // Row-distinct so a 4-byte odd-row word swap would show up here. Alpha = intensity.
        let out = decode_i4(&[0xF0, 0x0F], 2, 2);
        assert_eq!(&out[0..4], &[255, 255, 255, 255]); // r0c0 high nibble 0xF
        assert_eq!(&out[4..8], &[0, 0, 0, 0]); // r0c1 low  nibble 0x0
        assert_eq!(&out[8..12], &[0, 0, 0, 0]); // r1c0 high nibble 0x0
        assert_eq!(&out[12..16], &[255, 255, 255, 255]); // r1c1 low  nibble 0xF
    }

    #[test]
    fn rgba16_dispatch_is_byte_identical() {
        // The refactor MUST NOT change RGBA16 output: FormatInfo(0,2) == standalone decode_rgba16.
        let src: Vec<u8> = (0u8..8).collect(); // 2×2 RGBA16 (w*h*2 = 8 bytes)
        let src = &src[..8];
        assert_eq!(
            FormatInfo { fmt: 0, siz: 2 }.decode(src, 2, 2, &[], 0, 2),
            crate::hle::combiner::decode_rgba16(src)
        );
    }

    #[test]
    fn format_info_tmem_bytes() {
        assert_eq!(FormatInfo { fmt: 4, siz: 1 }.tmem_bytes(4, 4), 16); // I8:  w*h
        assert_eq!(FormatInfo { fmt: 4, siz: 0 }.tmem_bytes(4, 4), 8); //  I4:  ceil(w*h/2)
        assert_eq!(FormatInfo { fmt: 0, siz: 2 }.tmem_bytes(4, 4), 32); // RGBA16: w*h*2
        assert_eq!(FormatInfo { fmt: 0, siz: 3 }.tmem_bytes(4, 4), 64); // RGBA32: w*h*4
    }

    #[test]
    fn rgba32_direct_copy_and_dispatch() {
        // RGBA32 is 8-bit-per-channel: decode is an identity copy, R G B A order preserved.
        // 2×2 with per-texel-distinct bytes so a channel/texel swap would show up.
        let src: Vec<u8> = (0u8..16).collect(); // w*h*4 = 16 bytes
        assert_eq!(decode_rgba32(&src, 2, 2), src);
        // Dispatch (0,3) routes to decode_rgba32 (not the RGBA16 fallback).
        assert_eq!(
            FormatInfo { fmt: 0, siz: 3 }.decode(&src, 2, 2, &[], 0, 0),
            src
        );
    }

    #[test]
    fn rgba32_short_src_zero_pads() {
        // Contract: output is always w*h*4 bytes even when src is short (zero-padded tail).
        let out = decode_rgba32(&[0xAA, 0xBB], 1, 1);
        assert_eq!(out, vec![0xAA, 0xBB, 0x00, 0x00]);
    }

    #[test]
    fn ia16_splits_intensity_and_alpha() {
        // big-endian word 0x80FF -> i=0x80, a=0xFF
        // i=(ia16>>8)&0xFF, a=ia16&0xFF
        let out = decode_ia16(&[0x80, 0xFF], 1, 1);
        assert_eq!(out, vec![0x80, 0x80, 0x80, 0xFF]);
    }

    #[test]
    fn ia8_expands_4bit_nibbles() {
        // 0xF0 -> i4=0xF->i8=0xFF, a4=0x0->a8=0x00
        // i=(ia8>>4)&0xF; a=(ia8>>0)&0xF; i=(i<<4)|i; a=(a<<4)|a.
        assert_eq!(decode_ia8(&[0xF0], 1, 1), vec![255, 255, 255, 0]);
    }

    #[test]
    fn ia8_expands_4bit_nibbles_both_channels() {
        // 0x8A -> i4=0x8->i8=0x88=136, a4=0xA->a8=0xAA=170
        assert_eq!(decode_ia8(&[0x8A], 1, 1), vec![0x88, 0x88, 0x88, 0xAA]);
    }

    #[test]
    fn ia4_3bit_intensity_1bit_alpha() {
        // texel 0xF (nibble): i = 0xF & 0xE = 0xE; i8 = (0xE<<4)|(0xE<<1)|(0xE>>2) = 0xE0|0x1C|0x3 = 0xFF.
        // alpha = 0xF & 1 = 1 -> 255.
        // i=ia4&0b1110; i=(i<<4)|(i<<1)|(i>>2).
        let out = decode_ia4(&[0xF0], 2, 1); // texel0 = high nibble 0xF, texel1 = low nibble 0x0
        assert_eq!(out[3], 255); // alpha bit set on texel0
        assert!(out[0] > 200); // near-max intensity (255)
                               // texel1 (nibble 0x0): i=0, alpha=0
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn ia4_alpha_zero_when_bit_clear() {
        // nibble 0xE: i=0xE&0xE=0xE -> i8=0xFF; alpha=0xE&1=0 -> 0
        let out = decode_ia4(&[0xE0], 2, 1); // texel0 = high nibble 0xE
        assert_eq!(out[0], 255); // intensity max
        assert_eq!(out[3], 0); // alpha clear
    }

    #[test]
    fn ia4_multirow_swizzle_canary() {
        // 2×2: row0=0xF0 -> t0=0xF(i8=255,a=255), t1=0x0(i8=0,a=0)
        //       row1=0x0F -> t2=0x0(i8=0,a=0), t3=0xF(i8=255,a=255)
        // Row-distinct so an odd-row word swap would scramble the bands.
        let out = decode_ia4(&[0xF0, 0x0F], 2, 2);
        assert_eq!(&out[0..4], &[255, 255, 255, 255]); // r0c0 high=0xF
        assert_eq!(&out[4..8], &[0, 0, 0, 0]); // r0c1 low=0x0
        assert_eq!(&out[8..12], &[0, 0, 0, 0]); // r1c0 high=0x0
        assert_eq!(&out[12..16], &[255, 255, 255, 255]); // r1c1 low=0xF
    }

    #[test]
    fn ia16_multirow_swizzle_canary() {
        // 2×2 IA16: row0=[0xFF,0xFF,0x00,0x00] row1=[0x00,0x00,0xFF,0xFF]
        // Row0: t0=[255,255,255,255], t1=[0,0,0,0]
        // Row1: t2=[0,0,0,0], t3=[255,255,255,255]
        let out = decode_ia16(&[0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF], 2, 2);
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        assert_eq!(&out[8..12], &[0, 0, 0, 0]);
        assert_eq!(&out[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn ia_alpha_is_not_intensity() {
        // IA formats: alpha channel is the EXPLICIT alpha, NOT the intensity.
        // IA16: i=0x80, a=0x40 -> [128, 128, 128, 64] (alpha != intensity)
        let out = decode_ia16(&[0x80, 0x40], 1, 1);
        assert_eq!(out, vec![0x80, 0x80, 0x80, 0x40]);
        // IA8: 0xF8 -> i4=0xF->i8=0xFF, a4=0x8->a8=0x88 (alpha != intensity)
        let out8 = decode_ia8(&[0xF8], 1, 1);
        assert_eq!(out8, vec![0xFF, 0xFF, 0xFF, 0x88]);
    }

    #[test]
    fn format_info_ia_tmem_bytes() {
        assert_eq!(FormatInfo { fmt: 3, siz: 2 }.tmem_bytes(4, 4), 32); // IA16: w*h*2
        assert_eq!(FormatInfo { fmt: 3, siz: 1 }.tmem_bytes(4, 4), 16); // IA8:  w*h
        assert_eq!(FormatInfo { fmt: 3, siz: 0 }.tmem_bytes(4, 4), 8); //  IA4:  ceil(w*h/2)
    }

    #[test]
    fn ci8_rgba16_entry_lookup() {
        // TLUT: entry 0 = 0x0000 (transparent black), entry 1 = 0xF801 (red, a=1) at offset 1<<3=8.
        let mut tlut = vec![0u8; 16];
        tlut[8] = 0xF8; // entry index 1, high byte
        tlut[9] = 0x01; // entry index 1, low byte
                        // 2x1 CI8: indices [0, 1]
        let out = decode_ci8(&[0, 1], 2, 1, &tlut, 2 /*RGBA16*/);
        assert_eq!(&out[0..4], &[0, 0, 0, 0]); // index 0 -> 0x0000
        assert_eq!(&out[4..8], &[255, 0, 0, 255]); // index 1 -> red, a1=1 -> 255
    }

    #[test]
    fn ci8_ia16_entry_lookup() {
        // TT=IA16: entry 1 = 0x80FF -> i=0x80, a=0xFF
        let mut tlut = vec![0u8; 16];
        tlut[8] = 0x80;
        tlut[9] = 0xFF;
        let out = decode_ci8(&[1], 1, 1, &tlut, 3 /*IA16*/);
        assert_eq!(out, vec![0x80, 0x80, 0x80, 0xFF]);
    }

    #[test]
    fn ci4_high_nibble_even_column() {
        // 2×1 CI4, src byte 0x10: texel0 = high nibble = 1, texel1 = low nibble = 0.
        // TLUT bank0 entry1: offset (0<<7)+(1<<3)=8. 0xF801:
        //   r5=31→(31<<3)|(31>>2)=255, g5=0, b5=0, a=1→255 → [255,0,0,255].
        // Entry 0 = 0x0000 → [0,0,0,0].
        // Nibble select, palette banking, and big-endian u16 entry load.
        let mut tlut = vec![0u8; 256];
        tlut[8] = 0xF8;
        tlut[9] = 0x01; // bank0 entry1 = red, offset (0<<7)+(1<<3)=8
        let out = decode_ci4(&[0x10], 2, 1, &tlut, 0, 2);
        assert_eq!(&out[0..4], &[255, 0, 0, 255]); // texel0 idx1 (high nibble) -> red
        assert_eq!(&out[4..8], &[0, 0, 0, 0]); // texel1 idx0 -> 0x0000
    }

    #[test]
    fn ci4_palette_bank_offset() {
        // Proof of palette banking: same index 1 in bank0 vs bank1 yields different colours.
        // bank0 idx1 offset = (0<<7)+(1<<3) = 8.  0xF801 → [255,0,0,255] (red).
        // bank1 idx1 offset = (1<<7)+(1<<3) = 136. 0x07C1:
        //   r5=0, g5=bits[10:6]=11111=31→255, b5=0, a=1→255 → [0,255,0,255] (green).
        // Palette banking formula.
        let mut tlut = vec![0u8; 256];
        tlut[8] = 0xF8;
        tlut[9] = 0x01; // bank0 idx1 = red
        tlut[(1 << 7) + 8] = 0x07;
        tlut[(1 << 7) + 9] = 0xC1; // bank1 idx1 = green
        let p0 = decode_ci4(&[0x10], 2, 1, &tlut, 0, 2);
        let p1 = decode_ci4(&[0x10], 2, 1, &tlut, 1, 2);
        assert_eq!(&p0[0..3], &[255, 0, 0]);
        assert_eq!(&p1[0..3], &[0, 255, 0]);
        assert_ne!(p0, p1);
    }
}
