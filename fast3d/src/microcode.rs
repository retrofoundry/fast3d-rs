//! Public microcode selector (spec §3.6). Mirrors the crate-internal `crate::hle::gbi::GbiUcode`.

use crate::hle::gbi::{detect_from_ucode_hash, GbiUcode};

/// Which N64 graphics microcode a display list targets. An explicit per-`process_dl` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Microcode {
    /// Authentic F3DEX2 (web / RdramImage default). PC ports (sm64/helix, wafel) that
    /// emit `GBI_FLOATS` data also select this microcode and pair it with `DataFormat::Float`.
    #[default]
    F3dex2,
    /// Original F3D microcode.
    F3d,
}

impl From<Microcode> for GbiUcode {
    fn from(m: Microcode) -> Self {
        match m {
            Microcode::F3dex2 => GbiUcode::F3dex2,
            Microcode::F3d => GbiUcode::F3d,
        }
    }
}

impl From<GbiUcode> for Microcode {
    fn from(u: GbiUcode) -> Self {
        match u {
            GbiUcode::F3dex2 => Microcode::F3dex2,
            GbiUcode::F3d => Microcode::F3d,
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
        assert_eq!(GbiUcode::from(Microcode::F3d), GbiUcode::F3d);
        assert_eq!(Microcode::from(GbiUcode::F3dex2), Microcode::F3dex2);
        assert_eq!(Microcode::from(GbiUcode::F3d), Microcode::F3d);
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
            detect_microcode(0xF3D0_0000_0000_0003),
            Some(Microcode::F3d)
        );
        assert_eq!(detect_microcode(0xDEAD_BEEF), None);
    }
}
