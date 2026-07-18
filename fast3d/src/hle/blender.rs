//! `other_mode_l` decoder → `RenderMode` (pure HLE, no GPU dependency).
//! Authority: libultra gbi.h G_RM_* + the N64 blender.

use crate::hle::consts::rdp::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendClass {
    #[default]
    Replace,
    AlphaOver,
    DualSrc,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZMode {
    #[default]
    Opa,
    Inter,
    Xlu,
    Decal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaCompare {
    #[default]
    None,
    Threshold,
    Dither,
}

/// Decoded `other_mode_l` (+ G_FOG geometry bit) for one run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderMode {
    pub blender_mux: u16,
    pub force_blend: bool,
    pub blend_class: BlendClass,
    pub fallback_class: BlendClass,
    /// True when the mode blends but is NOT the canonical M=CLR_MEM,B=1MA,A=A_IN lerp.
    /// The §4.4/§9 [IMP12] diagnostic is emitted by `snapshot_run` in A6 (pure decode here).
    pub non_canonical_blend: bool,
    pub z_test: bool,
    pub z_write: bool,
    pub z_mode: ZMode,
    pub fog: bool,
    pub alpha_compare: AlphaCompare,
    pub cvg_x_alpha: bool,
}

/// Decode the RDP othermode low word into a `RenderMode`. `geom` supplies the G_FOG bit.
pub fn decode_render_mode(other_mode_l: u32, other_mode_h: u32, geom: u32) -> RenderMode {
    let cycle_type = (other_mode_h >> 20) & 3;
    let blender_mux = ((other_mode_l >> 16) & 0xFFFF) as u16;
    let z_mode = match other_mode_l & ZMODE_MASK {
        ZMODE_OPA => ZMode::Opa,
        ZMODE_INTER => ZMode::Inter,
        ZMODE_XLU => ZMode::Xlu,
        _ => ZMode::Decal,
    };
    let alpha_compare = match other_mode_l & 0x3 {
        AC_THRESHOLD => AlphaCompare::Threshold,
        AC_DITHER => AlphaCompare::Dither,
        _ => AlphaCompare::None,
    };
    let force_blend = other_mode_l & FORCE_BL != 0;
    // A render mode is also a fog mode when its blender mux uses CLR_FOG as a
    // P or M source in either cycle (e.g. G_RM_FOG_SHADE_A has no G_FOG geom bit).
    let fog_from_mux = (blender_mux >> 14) & 3 == CLR_FOG as u16 // c1.p
        || (blender_mux >> 6) & 3 == CLR_FOG as u16              // c1.m
        || (blender_mux >> 12) & 3 == CLR_FOG as u16             // c2.p
        || (blender_mux >> 4) & 3 == CLR_FOG as u16; // c2.m
    let (blend_class, fallback_class, non_canonical_blend) =
        classify_blend(blender_mux, force_blend, cycle_type);
    RenderMode {
        blender_mux,
        force_blend,
        blend_class,
        fallback_class,
        non_canonical_blend,
        z_test: other_mode_l & Z_CMP != 0,
        z_write: other_mode_l & Z_UPD != 0,
        z_mode,
        // NOTE: `crate::hle::consts::G_FOG` is retained here (not threaded via GbiConstants) because this
        // is public API consumed cross-crate by the renderer, and the geom-mode G_FOG bit is
        // ucode-invariant across F3D/F3DEX/F3DEX2. Kept in lockstep with the scene-fog read
        // at interp.rs (`gbi.consts.g_fog_geom`). A future ucode that redefines G_FOG must update both.
        fog: (geom & crate::hle::consts::G_FOG != 0) || fog_from_mux,
        alpha_compare,
        cvg_x_alpha: other_mode_l & CVG_X_ALPHA != 0,
    }
}

/// `usesAlphaBlendCycle` predicate.
/// `all_inputs`: false → blends only if P==CLR_MEM;
/// true → if (P==CLR_MEM && A!=0) or (M==CLR_MEM && B!=0).
fn uses_alpha_blend(p: u32, a: u32, m: u32, b: u32, all_inputs: bool) -> bool {
    if !all_inputs {
        p == CLR_MEM
    } else {
        (p == CLR_MEM && a != A_0) || (m == CLR_MEM && b != B_0)
    }
}

/// Classify the blender mux into `(blend_class, fallback_class, non_canonical_blend)`.
///
/// Keyed on the framebuffer (last active) cycle:
/// `combinerCycleCount`: 2CYCLE→2, 1CYCLE→1, COPY/FILL→0.
/// `blendCycleCount = FORCE_BL ? cc : max(cc-1, 0)`.
/// The last cycle is cycle-2 when 2CYCLE, else cycle-1.
/// `all_inputs = forceBlend` for the last cycle.
/// `non_canonical_blend` is true when the mode blends but is NOT the canonical
/// `M=CLR_MEM, B=1MA, A=A_IN` lerp (§4.4/§9 [IMP12]; diagnostic emitted by A6).
fn classify_blend(mux: u16, force_blend: bool, cycle_type: u32) -> (BlendClass, BlendClass, bool) {
    let bi = mux as u32;
    // combinerCycleCount: 2CYCLE→2, 1CYCLE→1, COPY/FILL→0.
    let cc = match cycle_type {
        1 => 2u32,
        0 => 1,
        _ => 0,
    };
    // Examine the framebuffer (last) blend cycle: cycle-2 for 2CYCLE, else cycle-1.
    let (p, a, m, b, second) = if cc >= 2 {
        ((bi >> 12) & 3, (bi >> 8) & 3, (bi >> 4) & 3, bi & 3, true)
    } else {
        (
            (bi >> 14) & 3,
            (bi >> 10) & 3,
            (bi >> 6) & 3,
            (bi >> 2) & 3,
            false,
        )
    };
    // allInputs = forceBlend for the last cycle (second=true); for the first/only cycle
    // in 1CYCLE mode, forceBlend is also used as the gate.
    let all_inputs = if second {
        force_blend
    } else {
        (cc >= 2) || force_blend
    };
    if uses_alpha_blend(p, a, m, b, all_inputs) {
        // DualSrc primary; fallback AlphaOver (lossless for canonical M=CLR_MEM,B=1MA,A=A_IN;
        // additive and other non-canonical modes are clamped — still translucent, not opaque).
        let canonical = p == CLR_IN && a == A_IN && m == CLR_MEM && b == B_1MA;
        (BlendClass::DualSrc, BlendClass::AlphaOver, !canonical)
    } else {
        (BlendClass::Replace, BlendClass::Replace, false)
    }
}

#[cfg(test)]
// `gbl_c*`/render-mode consts are `const fn`/consts now living in-crate (were in the external
// `gbi_consts` crate pre-merge); clippy can now see through them and flags the byte-identical
// `mux as u32` guards as no-ops. Preserve the tests verbatim and suppress the merge-induced lint.
#[allow(clippy::unnecessary_cast)]
mod tests {
    use super::*;
    use crate::hle::consts::rdp::{
        gbl_c1, gbl_c2, AC_DITHER, AC_THRESHOLD, A_0, A_IN, B_1, B_1MA, B_A_MEM, CLR_IN, CLR_MEM,
        FORCE_BL,
    };

    fn rm(l: u32) -> RenderMode {
        // other_mode_h with 1-cycle (bits 20-21 = 0); geom = 0 (no fog).
        decode_render_mode(l, 0, 0)
    }

    #[test]
    fn aa_zb_opa_surf_depth_flags() {
        // flags 0x2078 = AA_EN|Z_CMP|Z_UPD|IM_RD|ALPHA_CVG_SEL, ZMODE_OPA.
        let r = rm(0x2078);
        assert!(r.z_test && r.z_write);
        assert_eq!(r.z_mode, ZMode::Opa);
        assert!(!r.cvg_x_alpha);
        assert!(!r.force_blend);
        assert_eq!(r.alpha_compare, AlphaCompare::None);
    }

    #[test]
    fn aa_zb_xlu_surf_is_xlu_no_zwrite_forced() {
        // flags 0x49D8: FORCE_BL|ZMODE_XLU|Z_CMP|... , no Z_UPD.
        let r = rm(0x49D8);
        assert!(r.z_test && !r.z_write);
        assert_eq!(r.z_mode, ZMode::Xlu);
        assert!(r.force_blend);
    }

    #[test]
    fn aa_zb_opa_decal_is_decal_no_zwrite() {
        // flags 0x2D58: ZMODE_DEC (0xC00), Z_CMP, no Z_UPD.
        let r = rm(0x2D58);
        assert_eq!(r.z_mode, ZMode::Decal);
        assert!(r.z_test && !r.z_write);
    }

    #[test]
    fn tex_edge_sets_cvg_x_alpha() {
        // flags 0x3078 = 0x2078 + CVG_X_ALPHA(0x1000).
        assert!(rm(0x3078).cvg_x_alpha);
    }

    #[test]
    fn threshold_alpha_compare_bit() {
        assert_eq!(
            rm(0x2078 | AC_THRESHOLD).alpha_compare,
            AlphaCompare::Threshold
        );
        assert_eq!(rm(0x2078 | AC_DITHER).alpha_compare, AlphaCompare::Dither);
    }

    #[test]
    fn fog_from_geom_bit() {
        assert!(decode_render_mode(0x2078, 0, crate::hle::consts::G_FOG).fog);
        assert!(!decode_render_mode(0x2078, 0, 0).fog);
    }

    #[test]
    fn fog_from_mux_clr_fog_source() {
        // G_RM_FOG_SHADE_A low word 0xC8000000: mux=0xC800, c1.p=CLR_FOG.
        // No geom G_FOG bit, 2-cycle other_mode_h — still a fog mode.
        assert!(decode_render_mode(0xC800_0000, 1 << 20, 0).fog);
        // A non-fog mode (mux has no CLR_FOG P/M source) stays false.
        assert!(!decode_render_mode(0x2078, 1 << 20, 0).fog);
    }

    #[test]
    fn blender_mux_is_high_word() {
        assert_eq!(decode_render_mode(0x00AB_0010, 0, 0).blender_mux, 0x00AB);
    }

    #[test]
    fn opa_surf_trap_is_replace_not_blend() {
        // G_RM_AA_ZB_OPA_SURF: P=CLR_IN (not CLR_MEM), FORCE_BL off → NOT a blend.
        let mux =
            (gbl_c1(CLR_IN, A_IN, CLR_MEM, B_A_MEM) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_A_MEM)) >> 16;
        let r = decode_render_mode(((mux as u32) << 16) | 0x2078, 0, 0);
        assert_eq!(r.blend_class, BlendClass::Replace);
        assert_eq!(r.fallback_class, BlendClass::Replace);
    }

    #[test]
    fn xlu_surf_is_dualsrc_with_alphaover_fallback() {
        // G_RM_AA_ZB_XLU_SURF: M=CLR_MEM, B=1MA, FORCE_BL on → blend.
        let mux =
            (gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)) >> 16;
        let r = decode_render_mode(((mux as u32) << 16) | 0x49D8, 0, 0);
        assert_eq!(r.blend_class, BlendClass::DualSrc);
        assert_eq!(r.fallback_class, BlendClass::AlphaOver);
    }

    #[test]
    fn opa_surf_no_force_is_replace() {
        // G_RM_OPA_SURF: GBL (CLR_IN,0,CLR_IN,1), FORCE_BL on but M=CLR_IN → Replace.
        let mux = (gbl_c1(CLR_IN, A_0, CLR_IN, B_1) | gbl_c2(CLR_IN, A_0, CLR_IN, B_1)) >> 16;
        let r = decode_render_mode(((mux as u32) << 16) | FORCE_BL, 0, 0);
        assert_eq!(r.blend_class, BlendClass::Replace);
    }

    #[test]
    fn all_presets_match_spec_table() {
        use crate::hle::consts::rdp::*;
        // (m1, m2, spec_low_word, z_test, z_write, z_mode, cvg_x_alpha, blend_class)
        type PresetRow = (u32, u32, u32, bool, bool, ZMode, bool, BlendClass);
        let cases: &[PresetRow] = &[
            (
                G_RM_OPA_SURF,
                G_RM_OPA_SURF2,
                0x4040,
                false,
                false,
                ZMode::Opa,
                false,
                BlendClass::Replace,
            ),
            (
                G_RM_AA_ZB_OPA_SURF,
                G_RM_AA_ZB_OPA_SURF2,
                0x2078,
                true,
                true,
                ZMode::Opa,
                false,
                BlendClass::Replace,
            ),
            (
                G_RM_AA_ZB_XLU_SURF,
                G_RM_AA_ZB_XLU_SURF2,
                0x49D8,
                true,
                false,
                ZMode::Xlu,
                false,
                BlendClass::DualSrc,
            ),
            (
                G_RM_AA_ZB_TEX_EDGE,
                G_RM_AA_ZB_TEX_EDGE2,
                0x3078,
                true,
                true,
                ZMode::Opa,
                true,
                BlendClass::Replace,
            ),
            (
                G_RM_CLD_SURF,
                G_RM_CLD_SURF2,
                0x4340,
                false,
                false,
                ZMode::Opa,
                false,
                BlendClass::DualSrc,
            ),
            (
                G_RM_AA_ZB_OPA_DECAL,
                G_RM_AA_ZB_OPA_DECAL2,
                0x2D58,
                true,
                false,
                ZMode::Decal,
                false,
                BlendClass::Replace,
            ),
            (
                G_RM_AA_ZB_XLU_DECAL,
                G_RM_AA_ZB_XLU_DECAL2,
                0x4DD8,
                true,
                false,
                ZMode::Decal,
                false,
                BlendClass::DualSrc,
            ),
        ];
        for &(m1, m2, low, zt, zw, zm, cva, bc) in cases {
            assert_eq!(
                (m1 | m2) & 0xFFFF,
                low,
                "§4.5 low word for {m1:#x} must match exactly"
            );
            let r = decode_render_mode(m1 | m2, 0, 0);
            assert_eq!(r.z_test, zt, "z_test for {m1:#x}");
            assert_eq!(r.z_write, zw, "z_write for {m1:#x}");
            assert_eq!(r.z_mode, zm, "z_mode for {m1:#x}");
            assert_eq!(r.cvg_x_alpha, cva, "cvg_x_alpha for {m1:#x}");
            assert_eq!(r.blend_class, bc, "blend_class for {m1:#x}");
        }
    }

    #[test]
    fn fog_shade_a_2cycle_uses_cycle2_selectors() {
        // §4.5 8th preset (IMP14): synthetic 2-cycle fog mux — cycle-1 = fog premultiply
        // (P=CLR_FOG,A=A_SHADE,M=CLR_IN,B=1MA), cycle-2 = framebuffer blend (CLR_IN,A_IN,CLR_MEM,1MA).
        // cycle-1 (P=CLR_FOG,M=CLR_IN) is NOT a blend; cycle-2 (M=CLR_MEM,B=1MA) IS — so asserting
        // DualSrc proves classify keyed on the cycle-2 (framebuffer) selectors, not cycle-1.
        use crate::hle::consts::rdp::{
            gbl_c1, gbl_c2, A_IN, A_SHADE, B_1MA, CLR_FOG, CLR_IN, CLR_MEM,
        };
        let mux =
            (gbl_c1(CLR_FOG, A_SHADE, CLR_IN, B_1MA) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)) >> 16;
        let omh = 1u32 << 20; // other_mode_h cycle_type = G_CYC_2CYCLE
                              // low word 0x49D8 carries FORCE_BL → the 2nd cycle evaluates all inputs.
        let r = decode_render_mode(
            ((mux as u32) << 16) | 0x49D8,
            omh,
            crate::hle::consts::G_FOG,
        );
        assert!(r.fog, "G_FOG geom bit drives fog");
        assert_eq!(r.blend_class, BlendClass::DualSrc);
        assert!(!r.non_canonical_blend);
    }

    #[test]
    fn additive_blend_is_flagged_non_canonical_canonical_lerp_is_not() {
        // Canonical XLU lerp (M=CLR_MEM,B=1MA,A=A_IN) → blended but NOT flagged.
        let canon =
            (gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA) | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)) >> 16;
        let rc = decode_render_mode(((canon as u32) << 16) | 0x49D8, 0, 0);
        assert_eq!(rc.blend_class, BlendClass::DualSrc);
        assert!(
            !rc.non_canonical_blend,
            "canonical lerp must not be flagged"
        );
        // Additive (M=CLR_MEM, B=1 → P+M) → blended AND flagged (AlphaOver fallback is lossy).
        let add = (gbl_c1(CLR_IN, A_0, CLR_MEM, B_1) | gbl_c2(CLR_IN, A_0, CLR_MEM, B_1)) >> 16;
        let ra = decode_render_mode(((add as u32) << 16) | FORCE_BL, 0, 0);
        assert_eq!(ra.blend_class, BlendClass::DualSrc);
        assert!(
            ra.non_canonical_blend,
            "additive blend must be flagged for the §4.4 diag"
        );
    }
}
