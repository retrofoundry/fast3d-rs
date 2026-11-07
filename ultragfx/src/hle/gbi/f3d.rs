//! Original F3D microcode constants and Phase 0 dispatch-table skeleton.
use crate::hle::consts::rsp_f3d;
use crate::hle::gbi::GbiConstants;
use crate::hle::interp::Handler;
use crate::hle::mem::Rdram;

pub(crate) const F3D_CONSTS: GbiConstants = GbiConstants {
    g_dl: rsp_f3d::G_DL,
    g_enddl: rsp_f3d::G_ENDDL,
    mtx_param_xor: 0x00,
    g_mtx_projection: rsp_f3d::G_MTX_PROJECTION,
    g_mtx_load: rsp_f3d::G_MTX_LOAD,
    g_mtx_push: rsp_f3d::G_MTX_PUSH,
    g_mv_viewport: rsp_f3d::G_MV_VIEWPORT,
    g_mv_light: rsp_f3d::G_MV_LIGHT,
    g_mw_segment: rsp_f3d::G_MW_SEGMENT,
    g_mw_perspnorm: rsp_f3d::G_MW_PERSPNORM,
    g_mw_clip: rsp_f3d::G_MW_CLIP,
    g_mw_numlight: rsp_f3d::G_MW_NUMLIGHT,
    g_mw_fog: rsp_f3d::G_MW_FOG,
    g_fog_geom: rsp_f3d::G_FOG,
    g_clipping: rsp_f3d::G_CLIPPING,
    g_lighting: rsp_f3d::G_LIGHTING,
    g_texture_gen: rsp_f3d::G_TEXTURE_GEN,
    g_texture_gen_linear: rsp_f3d::G_TEXTURE_GEN_LINEAR,
    g_cull_front: rsp_f3d::G_CULL_FRONT,
    g_cull_back: rsp_f3d::G_CULL_BACK,
    g_cull_both: rsp_f3d::G_CULL_BOTH,
};

pub(crate) fn install_f3d<M: Rdram>(table: &mut [Handler<M>; 256]) {
    crate::hle::rdp::install_defaults(table);
    crate::hle::rsp_f3d::install_overrides(table);
}
