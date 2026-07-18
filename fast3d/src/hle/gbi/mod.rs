//! Microcode-variant descriptor: the opcode→handler table (Task 1) plus symbolic
//! constants (Task 2). Selected by an M-free `GbiUcode` so consumers pick a ucode with
//! no backend in scope. The vertex/matrix data format (fixed vs float) is orthogonal to
//! the microcode and is supplied by the caller, not derived here.
use crate::hle::interp::{unknown, Handler};
use crate::hle::mem::{GbiDataFormat, Rdram};

mod detect;
mod f3d;
pub(crate) mod f3dex2;
pub use detect::detect_from_ucode_hash;

/// Per-ucode symbolic constants. `Copy`, M-free. Every field except `mtx_param_xor`
/// is sourced from the selected microcode's constants module; original F3D supplies its
/// distinct values through the same seam as F3DEX2.
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
    /// Authentic F3DEX2 — the web/RdramImage default.
    #[default]
    F3dex2,
    /// Original F3D microcode.
    F3d,
}

impl GbiUcode {
    pub(crate) fn install<M: Rdram>(self, table: &mut [Handler<M>; 256]) {
        match self {
            GbiUcode::F3dex2 => f3dex2::install_f3dex2(table),
            GbiUcode::F3d => f3d::install_f3d(table),
        }
    }

    pub fn constants(self) -> GbiConstants {
        match self {
            GbiUcode::F3dex2 => f3dex2::F3DEX2_CONSTS,
            GbiUcode::F3d => f3d::F3D_CONSTS,
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
    pub(crate) fn new(ucode: GbiUcode, data_format: GbiDataFormat) -> Self {
        let mut table = [unknown::<M> as Handler<M>; 256];
        ucode.install(&mut table);
        Gbi {
            table,
            consts: ucode.constants(),
            data_format,
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
    fn f3d_consts_match_original_microcode() {
        use crate::hle::consts::rsp_f3d as f3d;

        let c = GbiUcode::F3d.constants();
        assert_eq!(c.g_dl, f3d::G_DL);
        assert_eq!(c.g_enddl, f3d::G_ENDDL);
        assert_eq!(c.mtx_param_xor, 0x00);
        assert_eq!(c.g_mtx_projection, f3d::G_MTX_PROJECTION);
        assert_eq!(c.g_mtx_load, f3d::G_MTX_LOAD);
        assert_eq!(c.g_mtx_push, f3d::G_MTX_PUSH);
        assert_eq!(c.g_mv_viewport, f3d::G_MV_VIEWPORT);
        assert_eq!(c.g_mv_light, f3d::G_MV_LIGHT);
        assert_eq!(c.g_mw_segment, f3d::G_MW_SEGMENT);
        assert_eq!(c.g_mw_perspnorm, f3d::G_MW_PERSPNORM);
        assert_eq!(c.g_mw_clip, f3d::G_MW_CLIP);
        assert_eq!(c.g_mw_numlight, f3d::G_MW_NUMLIGHT);
        assert_eq!(c.g_mw_fog, f3d::G_MW_FOG);
        assert_eq!(c.g_fog_geom, f3d::G_FOG);
        assert_eq!(c.g_clipping, f3d::G_CLIPPING);
        assert_eq!(c.g_lighting, f3d::G_LIGHTING);
        assert_eq!(c.g_texture_gen, f3d::G_TEXTURE_GEN);
        assert_eq!(c.g_texture_gen_linear, f3d::G_TEXTURE_GEN_LINEAR);
        assert_eq!(c.g_cull_front, f3d::G_CULL_FRONT);
        assert_eq!(c.g_cull_back, f3d::G_CULL_BACK);
        assert_eq!(c.g_cull_both, f3d::G_CULL_BOTH);

        assert_eq!(c.g_dl, 0x06);
        assert_eq!(c.g_cull_front, 0x0000_1000);
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
        let built = Gbi::<RdramImage<'static>>::new(GbiUcode::F3dex2, GbiDataFormat::Fixed).table;
        let reference = reference_f3dex2();
        for i in 0..256 {
            assert_eq!(built[i] as usize, reference[i] as usize, "slot 0x{i:02X}");
        }
    }

    #[test]
    fn f3d_table_installs_phase3_handlers() {
        use crate::hle::consts::{rdp, rsp_f3d};

        let built = Gbi::<RdramImage<'static>>::new(GbiUcode::F3d, GbiDataFormat::Fixed).table;
        let f3dex2 = Gbi::<RdramImage<'static>>::new(GbiUcode::F3dex2, GbiDataFormat::Fixed).table;
        let unknown = unknown::<RdramImage<'static>> as Handler<RdramImage<'static>>;

        for op in [
            rdp::G_SETCOMBINE,
            rdp::G_SETTILE,
            rdp::G_SETTILESIZE,
            rdp::G_LOADBLOCK,
            rdp::G_LOADTILE,
            rdp::G_LOADTLUT,
            rdp::G_SETPRIMCOLOR,
            rdp::G_SETENVCOLOR,
            rdp::G_SETFOGCOLOR,
            rdp::G_SETBLENDCOLOR,
            rdp::G_RDPLOADSYNC,
            rdp::G_RDPPIPESYNC,
            rdp::G_RDPTILESYNC,
            rdp::G_RDPFULLSYNC,
            rdp::G_RDPSETOTHERMODE,
            rdp::G_SETCIMG,
            rdp::G_SETZIMG,
            rdp::G_SETSCISSOR,
            rdp::G_SETFILLCOLOR,
            rdp::G_RDPHALF_1,
            rdp::G_RDPHALF_2,
        ] {
            assert_ne!(
                built[op as usize] as usize, unknown as usize,
                "slot 0x{op:02X}"
            );
            assert_eq!(
                built[op as usize] as usize, f3dex2[op as usize] as usize,
                "shared RDP slot 0x{op:02X}"
            );
        }

        let no_op = built[rsp_f3d::G_SPNOOP as usize] as usize;
        for op in [
            rsp_f3d::G_MTX,
            rsp_f3d::G_VTX,
            rsp_f3d::G_TRI1,
            rsp_f3d::G_QUAD,
            rsp_f3d::G_SETGEOMETRYMODE,
            rsp_f3d::G_CLEARGEOMETRYMODE,
            rsp_f3d::G_MOVEMEM,
            rsp_f3d::G_SETOTHERMODE_L,
            rsp_f3d::G_SETOTHERMODE_H,
            rsp_f3d::G_TEXTURE,
            rsp_f3d::G_MOVEWORD,
            rsp_f3d::G_POPMTX,
        ] {
            assert_ne!(
                built[op as usize] as usize, no_op,
                "implemented slot 0x{op:02X}"
            );
            assert_ne!(
                built[op as usize] as usize, unknown as usize,
                "implemented slot 0x{op:02X}"
            );
        }

        for op in [
            rsp_f3d::G_SPNOOP,
            rsp_f3d::G_DL,
            rsp_f3d::G_SPRITE2D_BASE,
            rsp_f3d::G_RDPHALF_2,
            rsp_f3d::G_RDPHALF_1,
            rsp_f3d::G_ENDDL,
            rsp_f3d::G_CULLDL,
            rsp_f3d::G_RDPNOOP,
        ] {
            assert_eq!(built[op as usize] as usize, no_op, "stub slot 0x{op:02X}");
        }

        assert_ne!(built[rdp::G_SETTIMG as usize] as usize, unknown as usize);
        assert_ne!(built[rdp::G_SETTIMG as usize] as usize, no_op);

        for op in [0x05, 0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF] {
            assert_eq!(
                built[op] as usize, unknown as usize,
                "F3DEX2-only slot 0x{op:02X}"
            );
        }
    }
}
