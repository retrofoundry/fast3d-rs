//! Public microcode selector (spec §3.6). Mirrors the crate-internal `crate::hle::gbi::GbiUcode`.

use crate::hle::gbi::{detect_from_ucode_hash, GbiUcode};

/// Which N64 graphics microcode a display list targets. An explicit per-`process_dl` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Microcode {
    /// Authentic fixed-point F3DEX2 (web / RdramImage default).
    #[default]
    F3dex2,
    /// GBI_FLOATS PC ports (sm64/helix, wafel).
    F3dex2e,
}

impl From<Microcode> for GbiUcode {
    fn from(m: Microcode) -> Self {
        match m {
            Microcode::F3dex2 => GbiUcode::F3dex2,
            Microcode::F3dex2e => GbiUcode::F3dex2e,
        }
    }
}

impl From<GbiUcode> for Microcode {
    fn from(u: GbiUcode) -> Self {
        match u {
            GbiUcode::F3dex2 => Microcode::F3dex2,
            GbiUcode::F3dex2e => Microcode::F3dex2e,
        }
    }
}

/// Table lookup for a real-ROM consumer that has already hashed a microcode image (spec §3.6/§8.4;
/// no live caller today — reuses the internal `detect_from_ucode_hash` fixture table).
pub fn detect_microcode(ucode_hash: u64) -> Option<Microcode> {
    detect_from_ucode_hash(ucode_hash).map(Microcode::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_roundtrips_both_directions() {
        assert_eq!(GbiUcode::from(Microcode::F3dex2), GbiUcode::F3dex2);
        assert_eq!(GbiUcode::from(Microcode::F3dex2e), GbiUcode::F3dex2e);
        assert_eq!(Microcode::from(GbiUcode::F3dex2e), Microcode::F3dex2e);
        assert_eq!(Microcode::default(), Microcode::F3dex2);
    }

    #[test]
    fn detect_resolves_known_and_rejects_unknown() {
        // Reuses the populated fixture table (hle/gbi/detect.rs), NOT an always-None stub.
        assert_eq!(
            detect_microcode(0xF3D2_0000_0000_0001),
            Some(Microcode::F3dex2)
        );
        assert_eq!(
            detect_microcode(0xF3D2_E000_0000_0002),
            Some(Microcode::F3dex2e)
        );
        assert_eq!(detect_microcode(0xDEAD_BEEF), None);
    }
}
