//! Microcode-hash → GBI detection. SCAFFOLD: our consumers emit pre-decoded DLs, so there
//! is NO live caller. The streaming-hash DB lands with a
//! real-ROM consumer (sarchar/n64, wafel real-ROM). Until then: a pure (hash → ucode) table.
//!
//! No float entry exists by design: the RSP is fixed-point, so there is no float microcode image
//! to hash. `GBI_FLOATS` is a PC-port data-format choice set explicitly via `set_data_format`, not
//! a detectable microcode — do not add a "float microcode" fixture here.
use super::GbiUcode;

/// Map a known RSP-microcode hash to its GBI variant. `None` if unknown.
pub fn detect_from_ucode_hash(hash: u64) -> Option<GbiUcode> {
    KNOWN.iter().find(|(h, _)| *h == hash).map(|(_, u)| *u)
}

/// Hand-written fixtures. Replace with real microcode hashes when a real-ROM consumer exists.
const KNOWN: &[(u64, GbiUcode)] = &[
    (0xF3D2_0000_0000_0001, GbiUcode::F3dex2),
    (0xF3D0_0000_0000_0003, GbiUcode::F3d),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hashes_resolve() {
        assert_eq!(
            detect_from_ucode_hash(0xF3D2_0000_0000_0001),
            Some(GbiUcode::F3dex2)
        );
        assert_eq!(
            detect_from_ucode_hash(0xF3D0_0000_0000_0003),
            Some(GbiUcode::F3d)
        );
    }

    #[test]
    fn unknown_hash_is_none() {
        assert_eq!(detect_from_ucode_hash(0xDEAD_BEEF), None);
    }
}
