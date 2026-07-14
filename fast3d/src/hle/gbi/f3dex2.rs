//! F3DEX2 microcode install: the authentic fixed-point GBI table body.
use crate::hle::gbi::GbiConstants;
use crate::hle::interp::Handler;
use crate::hle::mem::Rdram;

pub(crate) const F3DEX2_CONSTS: GbiConstants = GbiConstants {
    g_dl: crate::hle::consts::G_DL,
    g_enddl: crate::hle::consts::G_ENDDL,
    mtx_param_xor: 0x01, // F3DEX2 push-bit inversion — the one documented literal
    g_mtx_projection: crate::hle::consts::G_MTX_PROJECTION,
    g_mtx_load: crate::hle::consts::G_MTX_LOAD,
    g_mtx_push: crate::hle::consts::G_MTX_PUSH,
    g_mv_viewport: crate::hle::consts::G_MV_VIEWPORT,
    g_mv_light: crate::hle::consts::G_MV_LIGHT,
    g_mw_segment: crate::hle::consts::G_MW_SEGMENT,
    g_mw_perspnorm: crate::hle::consts::G_MW_PERSPNORM,
    g_mw_clip: crate::hle::consts::G_MW_CLIP,
    g_mw_numlight: crate::hle::consts::G_MW_NUMLIGHT,
    g_mw_fog: crate::hle::consts::G_MW_FOG,
    g_fog_geom: crate::hle::consts::G_FOG,
    g_clipping: crate::hle::consts::G_CLIPPING,
    g_lighting: crate::hle::consts::G_LIGHTING,
    g_texture_gen: crate::hle::consts::G_TEXTURE_GEN,
    g_texture_gen_linear: crate::hle::consts::G_TEXTURE_GEN_LINEAR,
    g_cull_front: crate::hle::consts::G_CULL_FRONT,
    g_cull_back: crate::hle::consts::G_CULL_BACK,
    g_cull_both: crate::hle::consts::G_CULL_BOTH,
};

/// Install the F3DEX2 opcode→handler table: RDP defaults first, then RSP overrides (overrides win).
pub(crate) fn install_f3dex2<M: Rdram>(table: &mut [Handler<M>; 256]) {
    crate::hle::rdp::install_defaults(table);
    crate::hle::rsp_f3dex2::install_overrides(table);
}
