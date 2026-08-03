//! Pure packing of already-selected N64 texel components and four-bit indices.

/// Pack final 5/5/5/1 components into one big-endian RGBA16 texel.
///
/// Only the low 5/5/5/1 bits are encoded. Callers remain responsible for deciding whether values
/// outside those wire fields should be rejected before packing.
pub const fn pack_rgba5551(r5: u8, g5: u8, b5: u8, a1: u8) -> [u8; 2] {
    let packed = (((r5 & 0x1f) as u16) << 11)
        | (((g5 & 0x1f) as u16) << 6)
        | (((b5 & 0x1f) as u16) << 1)
        | (a1 & 1) as u16;
    packed.to_be_bytes()
}

/// Pack final 8-bit intensity and alpha components into one big-endian IA16 texel.
pub const fn pack_ia16(i8: u8, a8: u8) -> [u8; 2] {
    [i8, a8]
}

/// Pack final four-bit intensity and alpha components into one IA8 texel.
///
/// Only the low nibble of each component is encoded.
pub const fn pack_ia8(i4: u8, a4: u8) -> u8 {
    ((i4 & 0x0f) << 4) | (a4 & 0x0f)
}

/// Pack final three-bit intensity and one-bit alpha components into one IA4 nibble.
///
/// Only the low three intensity bits and low alpha bit are encoded.
pub const fn pack_ia4(i3: u8, a1: u8) -> u8 {
    ((i3 & 0x07) << 1) | (a1 & 1)
}

/// Pack the even-column nibble high and the odd-column nibble low.
///
/// Only the low nibble of each input is encoded.
pub const fn pack_4bit_pair(even: u8, odd: u8) -> u8 {
    ((even & 0x0f) << 4) | (odd & 0x0f)
}

/// Pack one row of already-selected four-bit texels or indices.
///
/// Packing restarts at the high nibble for every call. An odd-length row emits a final byte whose
/// unused low nibble is zero; this deterministic zero-fill is part of this function's contract.
pub fn pack_4bit_row(texels: &[u8]) -> impl ExactSizeIterator<Item = u8> + '_ {
    texels.chunks(2).map(|pair| {
        let odd = pair.get(1).copied().unwrap_or(0);
        pack_4bit_pair(pair[0], odd)
    })
}
