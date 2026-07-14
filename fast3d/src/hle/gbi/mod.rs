//! Microcode-variant descriptor: the opcode→handler table (Task 1), plus symbolic
//! constants (Task 2) and the data format (Task 4). Selected by an M-free `GbiUcode`
//! so consumers pick a ucode with no backend in scope.
use crate::hle::interp::{unknown, Handler};
use crate::hle::mem::{GbiDataFormat, Rdram};

mod detect;
pub(crate) mod f3dex2;
mod f3dex2e;
pub use detect::detect_from_ucode_hash;

/// Per-ucode symbolic constants. `Copy`, M-free. Every field except `mtx_param_xor`
/// is sourced FROM `crate::hle::consts::*`. F3DEX2 and F3DEX2E are IDENTICAL here (every SP
/// constant is gated on `#ifdef F3DEX_GBI_2`, never `_2E`); the struct is the seam
/// where a future F3D/S2DEX module supplies different values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GbiConstants {
    pub g_dl: u8,
    pub g_enddl: u8,
    pub mtx_param_xor: u8,
    pub g_mtx_projection: u8,
    pub g_mtx_load: u8,
    pub g_mtx_push: u8,
    pub g_mv_viewport: u8,
    pub g_mv_light: u8,
    pub g_mw_segment: u8,
    pub g_mw_perspnorm: u8,
    pub g_mw_clip: u8,
    pub g_mw_numlight: u8,
    pub g_mw_fog: u8,
    /// Geometry-mode G_FOG bit. Read at the scene-fog site (interp.rs). NOTE: the per-render-mode
    /// fog flag in `blender::decode_render_mode` intentionally keeps the `crate::hle::consts::G_FOG`
    /// literal (ucode-invariant, public-API exemption) — keep the two in lockstep.
    pub g_fog_geom: u32,
    pub g_clipping: u32,
    pub g_lighting: u32,
    pub g_texture_gen: u32,
    pub g_texture_gen_linear: u32,
    pub g_cull_front: u32,
    pub g_cull_back: u32,
    pub g_cull_both: u32,
}

/// Identity of a microcode variant. M-free.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GbiUcode {
    /// Authentic fixed-point F3DEX2 — the web/RdramImage default.
    #[default]
    F3dex2,
    /// GBI_FLOATS PC ports (sm64, wafel): float matrices + float verts.
    F3dex2e,
}

impl GbiUcode {
    pub(crate) fn install<M: Rdram>(self, table: &mut [Handler<M>; 256]) {
        match self {
            GbiUcode::F3dex2 => f3dex2::install_f3dex2(table),
            GbiUcode::F3dex2e => f3dex2e::install_f3dex2e(table),
        }
    }

    pub fn constants(self) -> GbiConstants {
        match self {
            GbiUcode::F3dex2 | GbiUcode::F3dex2e => f3dex2::F3DEX2_CONSTS,
        }
    }

    pub fn data_format(self) -> GbiDataFormat {
        match self {
            GbiUcode::F3dex2 => GbiDataFormat::Fixed,
            GbiUcode::F3dex2e => GbiDataFormat::Float,
        }
    }
}

/// The live, M-parametric descriptor. CRATE-INTERNAL (not re-exported): built only
/// inside `interpret`. Re-exporting it would leak the `pub(crate)` `Handler`/`Ctx`.
pub(crate) struct Gbi<M: Rdram> {
    pub(crate) table: [Handler<M>; 256],
    pub(crate) consts: GbiConstants,
    pub(crate) data_format: GbiDataFormat,
}

impl<M: Rdram> Gbi<M> {
    pub(crate) fn new(ucode: GbiUcode) -> Self {
        let mut table = [unknown::<M> as Handler<M>; 256];
        ucode.install(&mut table);
        Gbi {
            table,
            consts: ucode.constants(),
            data_format: ucode.data_format(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hle::mem::RdramImage;

    #[test]
    fn f3dex2_consts_match_gbi_consts_source() {
        let c = GbiUcode::F3dex2.constants();
        assert_eq!(c.g_dl, crate::hle::consts::G_DL);
        assert_eq!(c.g_enddl, crate::hle::consts::G_ENDDL);
        assert_eq!(c.mtx_param_xor, 0x01); // the one allowed literal
        assert_eq!(c.g_mtx_projection, crate::hle::consts::G_MTX_PROJECTION);
        assert_eq!(c.g_mtx_load, crate::hle::consts::G_MTX_LOAD);
        assert_eq!(c.g_mtx_push, crate::hle::consts::G_MTX_PUSH);
        assert_eq!(c.g_mv_viewport, crate::hle::consts::G_MV_VIEWPORT);
        assert_eq!(c.g_mv_light, crate::hle::consts::G_MV_LIGHT);
        assert_eq!(c.g_mw_segment, crate::hle::consts::G_MW_SEGMENT);
        assert_eq!(c.g_mw_perspnorm, crate::hle::consts::G_MW_PERSPNORM);
        assert_eq!(c.g_mw_numlight, crate::hle::consts::G_MW_NUMLIGHT);
        assert_eq!(c.g_mw_fog, crate::hle::consts::G_MW_FOG);
        assert_eq!(c.g_fog_geom, crate::hle::consts::G_FOG);
        assert_eq!(c.g_clipping, crate::hle::consts::G_CLIPPING);
        assert_eq!(c.g_lighting, crate::hle::consts::G_LIGHTING);
        assert_eq!(c.g_texture_gen, crate::hle::consts::G_TEXTURE_GEN);
        assert_eq!(
            c.g_texture_gen_linear,
            crate::hle::consts::G_TEXTURE_GEN_LINEAR
        );
        assert_eq!(c.g_cull_front, crate::hle::consts::G_CULL_FRONT);
        assert_eq!(c.g_cull_back, crate::hle::consts::G_CULL_BACK);
        assert_eq!(c.g_cull_both, crate::hle::consts::G_CULL_BOTH);
        assert_eq!(c.g_mw_clip, crate::hle::consts::G_MW_CLIP);
    }

    #[test]
    fn f3dex2_and_f3dex2e_share_sp_constants() {
        assert_eq!(GbiUcode::F3dex2.constants(), GbiUcode::F3dex2e.constants());
    }

    #[test]
    fn ucode_data_format_mapping() {
        use crate::hle::mem::GbiDataFormat;
        assert_eq!(GbiUcode::F3dex2.data_format(), GbiDataFormat::Fixed);
        assert_eq!(GbiUcode::F3dex2e.data_format(), GbiDataFormat::Float);
    }

    /// Pre-refactor construction: exactly what `build_f3dex2_table` did inline.
    fn reference_f3dex2() -> [Handler<RdramImage<'static>>; 256] {
        let mut t = [unknown::<RdramImage<'static>> as Handler<RdramImage<'static>>; 256];
        crate::hle::rdp::install_defaults(&mut t);
        crate::hle::rsp_f3dex2::install_overrides(&mut t);
        t
    }

    // T-parity: Gbi::new(F3dex2) is byte-for-byte the pre-refactor table.
    #[test]
    fn f3dex2_table_matches_reference() {
        let built = Gbi::<RdramImage<'static>>::new(GbiUcode::F3dex2).table;
        let reference = reference_f3dex2();
        for i in 0..256 {
            assert_eq!(built[i] as usize, reference[i] as usize, "slot 0x{i:02X}");
        }
    }

    // T-compose: F3DEX2E's override slot is empty — its table equals F3DEX2's.
    // Goes red intentionally when the 2D slice adds a single-word override.
    #[test]
    fn f3dex2e_table_equals_f3dex2() {
        let a = Gbi::<RdramImage<'static>>::new(GbiUcode::F3dex2).table;
        let b = Gbi::<RdramImage<'static>>::new(GbiUcode::F3dex2e).table;
        for i in 0..256 {
            assert_eq!(a[i] as usize, b[i] as usize, "slot 0x{i:02X}");
        }
    }
}
