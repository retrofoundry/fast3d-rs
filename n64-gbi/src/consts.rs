#![allow(dead_code)]

pub mod rdp {
    // RDP opcodes (top byte of w0, bits [31:24]).
    pub const G_NOOP: u8 = 0x00;
    pub const G_SETTIMG: u8 = 0xFD;
    pub const G_SETCOMBINE: u8 = 0xFC;
    pub const G_SETENVCOLOR: u8 = 0xFB;
    pub const G_SETPRIMCOLOR: u8 = 0xFA;
    pub const G_SETPRIMDEPTH: u8 = 0xEE;
    pub const G_SETCONVERT: u8 = 0xEC;
    pub const G_SETKEYR: u8 = 0xEB;
    pub const G_SETKEYGB: u8 = 0xEA;
    pub const G_MDSFT_TEXTCONV: u32 = 9;
    pub const G_TC_CONV: u32 = 0 << G_MDSFT_TEXTCONV;
    pub const G_TC_FILTCONV: u32 = 5 << G_MDSFT_TEXTCONV;
    pub const G_TC_FILT: u32 = 6 << G_MDSFT_TEXTCONV;
    pub const G_MDSFT_COMBKEY: u32 = 8;
    pub const G_CK_NONE: u32 = 0;
    pub const G_CK_KEY: u32 = 1 << G_MDSFT_COMBKEY;
    pub const G_CCMUX_CENTER: u32 = 6;
    pub const G_CCMUX_SCALE: u32 = 6;
    pub const G_CCMUX_K4: u32 = 7;
    pub const G_CCMUX_K5: u32 = 15;
    pub const G_SETTILE: u8 = 0xF5;
    pub const G_LOADBLOCK: u8 = 0xF3;
    pub const G_LOADTILE: u8 = 0xF4;
    pub const G_LOADTLUT: u8 = 0xF0;
    // --- other_mode_l render-flag bit masks (libultra gbi.h) ---
    pub const AC_NONE: u32 = 0;
    pub const AC_THRESHOLD: u32 = 1;
    pub const AC_DITHER: u32 = 3;
    pub const G_ZS_PIXEL: u32 = 0;
    pub const G_ZS_PRIM: u32 = 4;
    pub const AA_EN: u32 = 0x0008;
    pub const Z_CMP: u32 = 0x0010;
    pub const Z_UPD: u32 = 0x0020;
    pub const IM_RD: u32 = 0x0040;
    pub const CLR_ON_CVG: u32 = 0x0080;
    pub const ZMODE_MASK: u32 = 0x0C00;
    pub const ZMODE_OPA: u32 = 0x0000;
    pub const ZMODE_INTER: u32 = 0x0400;
    pub const ZMODE_XLU: u32 = 0x0800;
    pub const ZMODE_DEC: u32 = 0x0C00;
    pub const CVG_X_ALPHA: u32 = 0x1000;
    pub const ALPHA_CVG_SEL: u32 = 0x2000;
    pub const FORCE_BL: u32 = 0x4000;
    // --- blender selector codes (§4.2) ---
    pub const CLR_IN: u32 = 0;
    pub const CLR_MEM: u32 = 1;
    pub const CLR_BL: u32 = 2;
    pub const CLR_FOG: u32 = 3;
    pub const A_IN: u32 = 0;
    pub const A_FOG: u32 = 1;
    pub const A_SHADE: u32 = 2;
    pub const A_0: u32 = 3;
    pub const B_1MA: u32 = 0;
    pub const B_A_MEM: u32 = 1;
    pub const B_1: u32 = 2;
    pub const B_0: u32 = 3;
    /// GBL_c1(p,a,m,b) = p<<30 | a<<26 | m<<22 | b<<18 (cycle-1 blender mux).
    pub const fn gbl_c1(p: u32, a: u32, m: u32, b: u32) -> u32 {
        (p << 30) | (a << 26) | (m << 22) | (b << 18)
    }
    /// GBL_c2(p,a,m,b) = p<<28 | a<<24 | m<<20 | b<<16 (cycle-2 blender mux).
    pub const fn gbl_c2(p: u32, a: u32, m: u32, b: u32) -> u32 {
        (p << 28) | (a << 24) | (m << 20) | (b << 16)
    }
    // CVG_DST coverage-destination modes (libultra gbi.h) — needed for the exact §4.5 low words.
    pub const CVG_DST_WRAP: u32 = 0x0100;
    pub const CVG_DST_FULL: u32 = 0x0200;
    pub const CVG_DST_SAVE: u32 = 0x0300;
    // --- G_RM_* render-mode presets (libultra gbi.h, low word verified against spec §4.5) ---
    pub const G_RM_OPA_SURF: u32 = IM_RD | FORCE_BL | gbl_c1(CLR_IN, A_0, CLR_IN, B_1); // low 0x4040
    pub const G_RM_OPA_SURF2: u32 = gbl_c2(CLR_IN, A_0, CLR_IN, B_1);
    pub const G_RM_AA_ZB_OPA_SURF: u32 = AA_EN
        | Z_CMP
        | Z_UPD
        | IM_RD
        | ALPHA_CVG_SEL
        | ZMODE_OPA
        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x2078 (NO CLR_ON_CVG)
    pub const G_RM_AA_ZB_OPA_SURF2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    pub const G_RM_AA_ZB_XLU_SURF: u32 = AA_EN
        | Z_CMP
        | IM_RD
        | CVG_DST_WRAP
        | CLR_ON_CVG
        | FORCE_BL
        | ZMODE_XLU
        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x49D8 (INCLUDES CVG_DST_WRAP)
    pub const G_RM_AA_ZB_XLU_SURF2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    pub const G_RM_AA_ZB_TEX_EDGE: u32 = AA_EN
        | Z_CMP
        | Z_UPD
        | IM_RD
        | CVG_X_ALPHA
        | ALPHA_CVG_SEL
        | ZMODE_OPA
        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x3078 (NO CLR_ON_CVG)
    pub const G_RM_AA_ZB_TEX_EDGE2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    pub const G_RM_CLD_SURF: u32 =
        IM_RD | CVG_DST_SAVE | FORCE_BL | ZMODE_OPA | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x4340 (ZMODE_OPA, no Z bits)
    pub const G_RM_CLD_SURF2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    pub const G_RM_AA_ZB_OPA_DECAL: u32 = AA_EN
        | Z_CMP
        | IM_RD
        | CVG_DST_WRAP
        | ALPHA_CVG_SEL
        | ZMODE_DEC
        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x2D58
    pub const G_RM_AA_ZB_OPA_DECAL2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    pub const G_RM_AA_ZB_XLU_DECAL: u32 = AA_EN
        | Z_CMP
        | IM_RD
        | CVG_DST_WRAP
        | CLR_ON_CVG
        | FORCE_BL
        | ZMODE_DEC
        | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA); // low 0x4DD8
    pub const G_RM_AA_ZB_XLU_DECAL2: u32 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    /// G_RM_FOG_SHADE_A: cycle-1 fog blender (CLR_FOG*A_SHADE + CLR_IN*(1-A_SHADE)).
    /// Used as mode1 in gsDPSetRenderMode with a 2-cycle OPA_SURF mode2.
    pub const G_RM_FOG_SHADE_A: u32 = gbl_c1(CLR_FOG, A_SHADE, CLR_IN, B_1MA);
    // othermode_H TextureLUT field (bits [15:14]).
    pub const G_MDSFT_TEXTLUT: u32 = 14;
    pub const G_TT_NONE: u8 = 0;
    pub const G_TT_RGBA16: u8 = 2;
    pub const G_TT_IA16: u8 = 3;
    pub const G_SETTILESIZE: u8 = 0xF2;
    pub const G_RDPLOADSYNC: u8 = 0xE6;
    pub const G_RDPPIPESYNC: u8 = 0xE7;
    pub const G_RDPTILESYNC: u8 = 0xE8;
    pub const G_RDPFULLSYNC: u8 = 0xE9;
    pub const G_RDPSETOTHERMODE: u8 = 0xEF;
    /// G_SETFOGCOLOR (0xF8): sets the scene-global fog RGBA color. Deferred from A1 to C1.
    pub const G_SETFOGCOLOR: u8 = 0xF8;
    /// G_SETBLENDCOLOR (0xF9): sets the blend-color register RGBA8 (used by CLR_BL blender selector
    /// and THRESHOLD alpha-compare; Phase D wires the HLE handler and renderer uniform).
    pub const G_SETBLENDCOLOR: u8 = 0xF9;
    // --- 2D / framebuffer RDP opcodes ---
    pub const G_TEXRECT: u8 = 0xE4;
    pub const G_TEXRECTFLIP: u8 = 0xE5;
    pub const G_FILLRECT: u8 = 0xF6;
    pub const G_SETFILLCOLOR: u8 = 0xF7;
    pub const G_SETSCISSOR: u8 = 0xED;
    pub const G_SETCIMG: u8 = 0xFF;
    pub const G_SETZIMG: u8 = 0xFE;
    pub const G_RDPHALF_1: u8 = 0xE1;
    pub const G_RDPHALF_2: u8 = 0xF1;
    /// G_CYC_COPY (2): copy-mode cycle type (SETOTHERMODE_H CYC field value).
    pub const G_CYC_COPY: u32 = 2;
}

pub mod rsp_f3dex2 {
    // RSP F3DEX2 opcodes (top byte of w0, bits [31:24]).
    pub const G_VTX: u8 = 0x01;
    pub const G_MODIFYVTX: u8 = 0x02;
    pub const G_CULLDL: u8 = 0x03;
    pub const G_BRANCH_Z: u8 = 0x04;
    pub const G_TRI1: u8 = 0x05;
    pub const G_TRI2: u8 = 0x06;
    pub const G_QUAD: u8 = 0x07;
    pub const G_LINE3D: u8 = 0x08;
    pub const G_SPECIAL_3: u8 = 0xD3;
    pub const G_SPECIAL_2: u8 = 0xD4;
    pub const G_SPECIAL_1: u8 = 0xD5;
    pub const G_DMA_IO: u8 = 0xD6;
    pub const G_LOAD_UCODE: u8 = 0xDD;
    pub const G_SPNOOP: u8 = 0xE0;
    pub const G_GEOMETRYMODE: u8 = 0xD9;
    pub const G_MTX: u8 = 0xDA;
    pub const G_MOVEMEM: u8 = 0xDC;
    pub const G_ENDDL: u8 = 0xDF;
    pub const G_TEXTURE: u8 = 0xD7;
    pub const G_SETOTHERMODE_H: u8 = 0xE3;
    /// G_SETOTHERMODE_L (0xE2): RSP bit-field write into other_mode_l (sibling of G_SETOTHERMODE_H).
    pub const G_SETOTHERMODE_L: u8 = 0xE2;
    /// G_MOVEWORD sub-type selecting the fog word (gSPFogPosition).
    pub const G_MW_FOG: u8 = 0x08;
    /// Byte-offset within the fog word.
    pub const G_MWO_FOG: u8 = 0x00;
    pub const G_DL: u8 = 0xDE;
    pub const G_MOVEWORD: u8 = 0xDB;
    pub const G_POPMTX: u8 = 0xD8;
    /// G_MOVEWORD sub-type selecting the segment table (F3DEX2 microcode).
    pub const G_MW_SEGMENT: u8 = 0x06;
    /// G_MOVEWORD sub-type selecting the perspective-normalize coefficient (libultra gbi.h).
    pub const G_MW_PERSPNORM: u8 = 0x0E;
    /// G_MOVEWORD sub-type selecting the clip ratio (libultra gbi.h).
    pub const G_MW_CLIP: u8 = 0x04;

    // G_MOVEMEM index for the viewport (F3DEX2_G_MV_VIEWPORT).
    pub const G_MV_VIEWPORT: u8 = 0x08;
    /// F3DEX2 G_MOVEMEM index for lights (per-light DMEM stride 0x18; slots 0/1 = LookAt).
    pub const G_MV_LIGHT: u8 = 0x0A;
    /// G_MOVEWORD index/offset for the directional light count (gSPNumLights(n) => n*24).
    pub const G_MW_NUMLIGHT: u8 = 0x02;
    /// G_MOVEWORD byte-offset within the numlight word (always 0x00 for F3DEX2).
    pub const G_MWO_NUMLIGHT: u8 = 0x00;

    pub const G_MWO_POINT_RGBA: u8 = 0x10;
    pub const G_MWO_POINT_ST: u8 = 0x14;
    pub const G_MWO_POINT_XYSCREEN: u8 = 0x18;
    pub const G_MWO_POINT_ZSCREEN: u8 = 0x1C;

    // G_MTX param bits (post-XOR logical values; F3DEX2 microcode).
    pub const G_MTX_MODELVIEW: u8 = 0x00;
    pub const G_MTX_PROJECTION: u8 = 0x04;
    pub const G_MTX_MUL: u8 = 0x00;
    pub const G_MTX_LOAD: u8 = 0x02;
    pub const G_MTX_NOPUSH: u8 = 0x00;
    pub const G_MTX_PUSH: u8 = 0x01;

    // Geometry-mode flags (F3DEX2 values).
    pub const G_SHADE: u32 = 0x0000_0004;
    pub const G_CULL_FRONT: u32 = 0x0000_0200;
    pub const G_CULL_BACK: u32 = 0x0000_0400;
    pub const G_CULL_BOTH: u32 = 0x0000_0600;
    pub const G_FOG: u32 = 0x0001_0000;
    pub const G_LIGHTING: u32 = 0x0002_0000;
    pub const G_SHADING_SMOOTH: u32 = 0x0020_0000;
    pub const G_CLIPPING: u32 = 0x0080_0000;
    /// Enable the Z-buffer (F3DEX2 gbi.h `G_ZBUFFER`).
    pub const G_ZBUFFER: u32 = 0x0000_0001;
    /// Enable texture-coordinate generation / spherical reflection mapping (F3DEX2 `G_TEXTURE_GEN`).
    pub const G_TEXTURE_GEN: u32 = 0x0004_0000;
    /// Linear texture-coordinate generation (F3DEX2 `G_TEXTURE_GEN_LINEAR`).
    pub const G_TEXTURE_GEN_LINEAR: u32 = 0x0008_0000;
}

pub mod rsp_f3d {
    // RSP F3D opcodes (top byte of w0, bits [31:24]).
    pub const G_SPNOOP: u8 = 0x00;
    pub const G_MTX: u8 = 0x01;
    pub const G_MOVEMEM: u8 = 0x03;
    pub const G_VTX: u8 = 0x04;
    pub const G_DL: u8 = 0x06;
    pub const G_SPRITE2D_BASE: u8 = 0x09;
    pub const G_RDPHALF_2: u8 = 0xB3;
    pub const G_RDPHALF_1: u8 = 0xB4;
    pub const G_QUAD: u8 = 0xB5;
    pub const G_CLEARGEOMETRYMODE: u8 = 0xB6;
    pub const G_SETGEOMETRYMODE: u8 = 0xB7;
    pub const G_ENDDL: u8 = 0xB8;
    pub const G_SETOTHERMODE_L: u8 = 0xB9;
    pub const G_SETOTHERMODE_H: u8 = 0xBA;
    pub const G_TEXTURE: u8 = 0xBB;
    pub const G_MOVEWORD: u8 = 0xBC;
    pub const G_POPMTX: u8 = 0xBD;
    pub const G_CULLDL: u8 = 0xBE;
    pub const G_TRI1: u8 = 0xBF;
    pub const G_RDPNOOP: u8 = 0xC0;

    // G_MTX parameter bits. Original F3D uses these directly, without an XOR.
    pub const G_MTX_MODELVIEW: u8 = 0x00;
    pub const G_MTX_PROJECTION: u8 = 0x01;
    pub const G_MTX_MUL: u8 = 0x00;
    pub const G_MTX_LOAD: u8 = 0x02;
    pub const G_MTX_NOPUSH: u8 = 0x00;
    pub const G_MTX_PUSH: u8 = 0x04;

    // Geometry-mode flags (original F3D values).
    pub const G_ZBUFFER: u32 = 0x0000_0001;
    pub const G_TEXTURE_ENABLE: u32 = 0x0000_0002;
    pub const G_SHADE: u32 = 0x0000_0004;
    pub const G_SHADING_SMOOTH: u32 = 0x0000_0200;
    pub const G_CULL_FRONT: u32 = 0x0000_1000;
    pub const G_CULL_BACK: u32 = 0x0000_2000;
    pub const G_CULL_BOTH: u32 = 0x0000_3000;
    pub const G_FOG: u32 = 0x0001_0000;
    pub const G_LIGHTING: u32 = 0x0002_0000;
    pub const G_TEXTURE_GEN: u32 = 0x0004_0000;
    pub const G_TEXTURE_GEN_LINEAR: u32 = 0x0008_0000;
    pub const G_POINT_LIGHTING: u32 = 0x0040_0000;
    pub const G_CLIPPING: u32 = 0x0080_0000;

    // G_MOVEMEM selectors (original F3D values).
    pub const G_MV_VIEWPORT: u8 = 0x80;
    pub const G_MV_LOOKATY: u8 = 0x82;
    pub const G_MV_LOOKATX: u8 = 0x84;
    pub const G_MV_LIGHT: u8 = 0x86;
    pub const G_MV_L0: u8 = 0x86;
    pub const G_MV_L1: u8 = 0x88;
    pub const G_MV_L2: u8 = 0x8A;
    pub const G_MV_L3: u8 = 0x8C;
    pub const G_MV_L4: u8 = 0x8E;
    pub const G_MV_L5: u8 = 0x90;
    pub const G_MV_L6: u8 = 0x92;
    pub const G_MV_L7: u8 = 0x94;
    pub const G_MV_TXTATT: u8 = 0x96;
    pub const G_MV_MATRIX_2: u8 = 0x98;
    pub const G_MV_MATRIX_3: u8 = 0x9A;
    pub const G_MV_MATRIX_4: u8 = 0x9C;
    pub const G_MV_MATRIX_1: u8 = 0x9E;

    // G_MOVEWORD selectors (original F3D values).
    pub const G_MW_MATRIX: u8 = 0x00;
    pub const G_MW_NUMLIGHT: u8 = 0x02;
    pub const G_MW_CLIP: u8 = 0x04;
    pub const G_MW_SEGMENT: u8 = 0x06;
    pub const G_MW_FOG: u8 = 0x08;
    pub const G_MW_LIGHTCOL: u8 = 0x0A;
    pub const G_MW_POINTS: u8 = 0x0C;
    pub const G_MW_PERSPNORM: u8 = 0x0E;
}

pub use rdp::*;
pub use rsp_f3dex2::*;

#[cfg(test)]
mod tests {
    #[test]
    fn submodules_and_glob_agree() {
        assert_eq!(super::rdp::G_SETTIMG, 0xFD);
        assert_eq!(super::rdp::G_SETCOMBINE, 0xFC);
        assert_eq!(super::rsp_f3dex2::G_TEXTURE, 0xD7);
        assert_eq!(super::rsp_f3dex2::G_SETOTHERMODE_H, 0xE3);
        assert_eq!(super::G_VTX, 0x01);
        assert_eq!(super::G_ENDDL, 0xDF);
        assert_eq!(super::G_NOOP, 0x00);
        assert_eq!(super::G_SETTIMG, 0xFD);
    }

    #[test]
    fn f3d_constants_match_original_gbi_values() {
        use super::rsp_f3d::*;

        assert_eq!(G_SPNOOP, 0x00);
        assert_eq!(G_MTX, 0x01);
        assert_eq!(G_MOVEMEM, 0x03);
        assert_eq!(G_VTX, 0x04);
        assert_eq!(G_DL, 0x06);
        assert_eq!(G_SPRITE2D_BASE, 0x09);
        assert_eq!(G_RDPHALF_2, 0xB3);
        assert_eq!(G_RDPHALF_1, 0xB4);
        assert_eq!(G_QUAD, 0xB5);
        assert_eq!(G_CLEARGEOMETRYMODE, 0xB6);
        assert_eq!(G_SETGEOMETRYMODE, 0xB7);
        assert_eq!(G_ENDDL, 0xB8);
        assert_eq!(G_SETOTHERMODE_L, 0xB9);
        assert_eq!(G_SETOTHERMODE_H, 0xBA);
        assert_eq!(G_TEXTURE, 0xBB);
        assert_eq!(G_MOVEWORD, 0xBC);
        assert_eq!(G_POPMTX, 0xBD);
        assert_eq!(G_CULLDL, 0xBE);
        assert_eq!(G_TRI1, 0xBF);
        assert_eq!(G_RDPNOOP, 0xC0);

        assert_eq!(G_MTX_PROJECTION, 0x01);
        assert_eq!(G_MTX_LOAD, 0x02);
        assert_eq!(G_MTX_PUSH, 0x04);
        assert_eq!(G_CULL_FRONT, 0x0000_1000);
        assert_eq!(G_CULL_BACK, 0x0000_2000);
        assert_eq!(G_CULL_BOTH, 0x0000_3000);

        assert_eq!(G_MV_VIEWPORT, 0x80);
        assert_eq!(G_MV_LIGHT, 0x86);
        assert_eq!(G_MV_MATRIX_1, 0x9E);
        assert_eq!(G_MW_SEGMENT, 0x06);
        assert_eq!(G_MW_FOG, 0x08);
        assert_eq!(G_MW_POINTS, 0x0C);
        assert_eq!(G_MW_PERSPNORM, 0x0E);
    }
}
