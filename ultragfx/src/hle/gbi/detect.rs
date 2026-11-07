//! Microcode hash lookup. Production records await a ROM consumer with a verified hash recipe.
use super::GbiUcode;

/// Returns `None` until verified microcode hashes are available.
pub fn detect_from_ucode_hash(hash: u64) -> Option<GbiUcode> {
    lookup_hash(hash, &[])
}

fn lookup_hash(hash: u64, records: &[(u64, GbiUcode)]) -> Option<GbiUcode> {
    records.iter().find(|(h, _)| *h == hash).map(|(_, u)| *u)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_RECORDS: &[(u64, GbiUcode)] = &[
        (0xF3D2_0000_0000_0001, GbiUcode::F3dex2),
        (0xF3D0_0000_0000_0003, GbiUcode::F3d),
    ];

    #[test]
    fn fixture_hashes_resolve_in_test_table() {
        assert_eq!(
            lookup_hash(0xF3D2_0000_0000_0001, FIXTURE_RECORDS),
            Some(GbiUcode::F3dex2)
        );
        assert_eq!(
            lookup_hash(0xF3D0_0000_0000_0003, FIXTURE_RECORDS),
            Some(GbiUcode::F3d)
        );
    }

    #[test]
    fn unknown_hash_is_none() {
        assert_eq!(lookup_hash(0xDEAD_BEEF, FIXTURE_RECORDS), None);
    }
}
