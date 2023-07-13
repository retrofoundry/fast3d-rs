use fast3d::gbi::defines::{AlphaCombinerMux, ColorCombinerMux, GWords, Gfx, Mtx, Viewport, Vtx};
use fast3d::models::color_combiner::CombineParams;

/// At the moment these commands are of F3DEX2. In the future we should add more.
/// They're only here for the example testing.

// old hacky way of doing this of fixing bowtie hangs
const BOWTIE_VAL: u32 = 0;

// this is for non hardware version 1.0
pub const G_CD_ENABLE: u32 = 1 << G_MDSFT_COLORDITHER;
pub const G_CD_DISABLE: u32 = 0 << G_MDSFT_COLORDITHER;

// G_MTX: parameter flags
pub const G_MTX_PROJECTION: u32 = 0x04;
pub const G_MTX_LOAD: u32 = 0x02;
pub const G_MTX_NOPUSH: u32 = 0x00;
pub const G_MTX_MODELVIEW: u32 = 0x00;
pub const G_MTX_PUSH: u32 = 0x01;

// G_SETOTHERMODE_H gSetCycleType
pub const G_CYC_1CYCLE: u32 = 0 << G_MDSFT_CYCLETYPE;
pub const G_CYC_FILL: u32 = 3 << G_MDSFT_CYCLETYPE;

// G_SETOTHERMODE_L sft: shift count
const G_MDSFT_RENDERMODE: u32 = 3;
const G_MDSFT_ALPHACOMPARE: u32 = 0;
const G_MDSFT_ZSRCSEL: u32 = 2;
const G_MDSFT_BLENDER: u32 = 16;

// G_SETOTHERMODE_H sft: shift count
const G_MDSFT_BLENDMASK: u32 = 0; /* unsupported */
const G_MDSFT_ALPHADITHER: u32 = 4;
const G_MDSFT_RGBDITHER: u32 = 6;

const G_MDSFT_COMBKEY: u32 = 8;
const G_MDSFT_TEXTCONV: u32 = 9;
const G_MDSFT_TEXTFILT: u32 = 12;
const G_MDSFT_TEXTLUT: u32 = 14;
const G_MDSFT_TEXTLOD: u32 = 16;
const G_MDSFT_TEXTDETAIL: u32 = 17;
const G_MDSFT_TEXTPERSP: u32 = 19;
const G_MDSFT_CYCLETYPE: u32 = 20;
const G_MDSFT_COLORDITHER: u32 = 22; /* unsupported in HW 2.0 */
const G_MDSFT_PIPELINE: u32 = 23;

// G_SETOTHERMODE_L gSetRenderMode
const AA_EN: u32 = 0x8;
const Z_CMP: u32 = 0x10;
const Z_UPD: u32 = 0x20;
const IM_RD: u32 = 0x40;
const CLR_ON_CVG: u32 = 0x80;
const CVG_DST_CLAMP: u32 = 0;
const CVG_DST_WRAP: u32 = 0x100;
const CVG_DST_FULL: u32 = 0x200;
const CVG_DST_SAVE: u32 = 0x300;
const ZMODE_OPA: u32 = 0;
const ZMODE_INTER: u32 = 0x400;
const ZMODE_XLU: u32 = 0x800;
const ZMODE_DEC: u32 = 0xc00;
const CVG_X_ALPHA: u32 = 0x1000;
const ALPHA_CVG_SEL: u32 = 0x2000;
const FORCE_BL: u32 = 0x4000;
const TEX_EDGE: u32 = 0x0000; /* used to be 0x8000 */

const G_BL_CLR_IN: u32 = 0;
const G_BL_CLR_MEM: u32 = 1;
const G_BL_CLR_BL: u32 = 2;
const G_BL_CLR_FOG: u32 = 3;
const G_BL_1MA: u32 = 0;
const G_BL_A_MEM: u32 = 1;
const G_BL_A_IN: u32 = 0;
const G_BL_A_FOG: u32 = 1;
const G_BL_A_SHADE: u32 = 2;
const G_BL_1: u32 = 2;
const G_BL_0: u32 = 3;

// G_SETIMG fmt: set image formats
pub const G_IM_FMT_RGBA: u32 = 0;
pub const G_IM_FMT_YUV: u32 = 1;
pub const G_IM_FMT_CI: u32 = 2;
pub const G_IM_FMT_IA: u32 = 3;
pub const G_IM_FMT_I: u32 = 4;

// G_SETIMG siz: set image pixel size
pub const G_IM_SIZ_4b: u32 = 0;
pub const G_IM_SIZ_8b: u32 = 1;
pub const G_IM_SIZ_16b: u32 = 2;
pub const G_IM_SIZ_32b: u32 = 3;
pub const G_IM_SIZ_DD: u32 = 5;

// flags for G_SETGEOMETRYMODE
pub const G_SHADE: u32 = 0x00000004;
pub const G_SHADING_SMOOTH: u32 = 0x00200000; // value unique to f3dex2
pub const G_CULL_BACK: u32 = 0x00000400; // value unique to f3dex2
pub const G_CULL_BOTH: u32 = 0x00000600; // value unique to f3dex2
pub const G_FOG: u32 = 0x00010000;
pub const G_LIGHTING: u32 = 0x00020000;
pub const G_TEXTURE_GEN: u32 = 0x00040000;
pub const G_TEXTURE_GEN_LINEAR: u32 = 0x00080000;
pub const G_LOD: u32 = 0x00100000;

/* G_SETOTHERMODE_L gSetAlphaCompare */
pub const G_AC_NONE: u32 = 0 << G_MDSFT_ALPHACOMPARE;
pub const G_AC_THRESHOLD: u32 = 1 << G_MDSFT_ALPHACOMPARE;
pub const G_AC_DITHER: u32 = 3 << G_MDSFT_ALPHACOMPARE;

// Commands
const G_MTX: u32 = 0xda;
const G_RDPPIPESYNC: u32 = 0xe7;
const G_RDPFULLSYNC: u32 = 0xe9;
const G_SETOTHERMODE_H: u32 = 0xe3;
const G_SETOTHERMODE_L: u32 = 0xe2;
const G_GEOMETRYMODE: u32 = 0xd9;
const G_DL: u32 = 0xde;

const G_VTX: u32 = 0x01;
const G_TRI1: u32 = 0x05;
const G_MOVEMEM: u32 = 0xdc;
const G_TEXTURE: u32 = 0xd7;
const G_SETSCISSOR: u32 = 0xed;
const G_SETCOMBINE: u32 = 0xfc;
const G_SETCIMG: u32 = 0xff;
const G_SETFILLCOLOR: u32 = 0xf7;
const G_FILLRECT: u32 = 0xf6;

// flags to inhibit pushing of the display list
const G_DL_PUSH: u32 = 0;
const G_DL_NOPUSH: u32 = 1;

// viewport
pub const G_MAXZ: u32 = 0x03ff;

// movemem indices
const G_MV_VIEWPORT: u32 = 8;

/* G_SETOTHERMODE_H gPipelineMode */
pub const G_PM_1PRIMITIVE: u32 = 1 << G_MDSFT_PIPELINE;
pub const G_PM_NPRIMITIVE: u32 = 0 << G_MDSFT_PIPELINE;

// G_SETSCISSOR: interlace mode
pub const G_SC_NON_INTERLACE: u32 = 0;
pub const G_SC_ODD_INTERLACE: u32 = 3;
pub const G_SC_EVEN_INTERLACE: u32 = 2;

/* G_SETOTHERMODE_H gSetTextureLOD */
pub const G_TL_TILE: u32 = 0 << G_MDSFT_TEXTLOD;
pub const G_TL_LOD: u32 = 1 << G_MDSFT_TEXTLOD;

/* G_SETOTHERMODE_H gSetTextureLUT */
pub const G_TT_NONE: u32 = 0 << G_MDSFT_TEXTLUT;
pub const G_TT_RGBA16: u32 = 2 << G_MDSFT_TEXTLUT;
pub const G_TT_IA16: u32 = 3 << G_MDSFT_TEXTLUT;

/* G_SETOTHERMODE_H gSetTexturePersp */
pub const G_TP_NONE: u32 = 0 << G_MDSFT_TEXTPERSP;
pub const G_TP_PERSP: u32 = 1 << G_MDSFT_TEXTPERSP;

/* G_SETOTHERMODE_H gSetTextureDetail */
pub const G_TD_CLAMP: u32 = 0 << G_MDSFT_TEXTDETAIL;
pub const G_TD_SHARPEN: u32 = 1 << G_MDSFT_TEXTDETAIL;
pub const G_TD_DETAIL: u32 = 2 << G_MDSFT_TEXTDETAIL;

/* G_SETOTHERMODE_H gSetTextureFilter */
pub const G_TF_POINT: u32 = 0 << G_MDSFT_TEXTFILT;
pub const G_TF_AVERAGE: u32 = 3 << G_MDSFT_TEXTFILT;
pub const G_TF_BILERP: u32 = 2 << G_MDSFT_TEXTFILT;

/* G_SETOTHERMODE_H gSetTextureConvert */
pub const G_TC_CONV: u32 = 0 << G_MDSFT_TEXTCONV;
pub const G_TC_FILTCONV: u32 = 5 << G_MDSFT_TEXTCONV;
pub const G_TC_FILT: u32 = 6 << G_MDSFT_TEXTCONV;

/* G_SETOTHERMODE_H gSetCombineKey */
pub const G_CK_NONE: u32 = 0 << G_MDSFT_COMBKEY;
pub const G_CK_KEY: u32 = 1 << G_MDSFT_COMBKEY;

/* G_SETOTHERMODE_H gSetColorDither */
pub const G_CD_MAGICSQ: u32 = 0 << G_MDSFT_RGBDITHER;
pub const G_CD_BAYER: u32 = 1 << G_MDSFT_RGBDITHER;
pub const G_CD_NOISE: u32 = 2 << G_MDSFT_RGBDITHER;

// color combiner stuff
const SHADE: u32 = 0x04;

pub const G_CC_SHADE: CombineParams = CombineParams::SHADE;

// shift helpers

fn shiftl(value: u32, shift: u32, width: u32) -> usize {
    ((value & ((0x01 << width) - 1)) << shift) as usize
}

// colors

const fn GBL_c1(m1a: u32, m1b: u32, m2a: u32, m2b: u32) -> u32 {
    (m1a) << 30 | (m1b) << 26 | (m2a) << 22 | (m2b) << 18
}

const fn GBL_c2(m1a: u32, m1b: u32, m2a: u32, m2b: u32) -> u32 {
    (m1a) << 28 | (m1b) << 24 | (m2a) << 20 | (m2b) << 16
}

#[allow(non_snake_case)]
const fn RM_AA_OPA_SURF(clk: u8) -> u32 {
    AA_EN
        | IM_RD
        | CVG_DST_CLAMP
        | ZMODE_OPA
        | ALPHA_CVG_SEL
        | match clk {
            1 => GBL_c1(G_BL_CLR_IN, G_BL_A_IN, G_BL_CLR_MEM, G_BL_A_MEM),
            2 => GBL_c2(G_BL_CLR_IN, G_BL_A_IN, G_BL_CLR_MEM, G_BL_A_MEM),
            _ => 0, // This should really panic.. but in a const we can't do that.
        }
}

#[allow(non_snake_case)]
const fn RM_OPA_SURF(clk: u8) -> u32 {
    CVG_DST_CLAMP
        | FORCE_BL
        | ZMODE_OPA
        | match clk {
            1 => GBL_c1(G_BL_CLR_IN, G_BL_0, G_BL_CLR_IN, G_BL_1),
            2 => GBL_c2(G_BL_CLR_IN, G_BL_0, G_BL_CLR_IN, G_BL_1),
            _ => 0, // This should really panic.. but in a const we can't do that.
        }
}

pub const G_RM_AA_OPA_SURF: u32 = RM_AA_OPA_SURF(1);
pub const G_RM_AA_OPA_SURF2: u32 = RM_AA_OPA_SURF(2);

pub const G_RM_OPA_SURF: u32 = RM_OPA_SURF(1);
pub const G_RM_OPA_SURF2: u32 = RM_OPA_SURF(2);

#[allow(non_snake_case)]
pub const fn GPACK_RGBA5551(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (((r as u32) << 8) & 0xf800)
        | (((g as u32) << 3) & 0x7c0)
        | (((b as u32) >> 2) & 0x3e)
        | ((a as u32) >> 0x1)
}

#[allow(non_snake_case)]
pub fn gsDPPipeSync() -> Gfx {
    gsDPNoParam(G_RDPPIPESYNC)
}

#[allow(non_snake_case)]
pub fn gsDPFullSync() -> Gfx {
    gsDPNoParam(G_RDPFULLSYNC)
}

#[allow(non_snake_case)]
pub fn gsSPMatrix(matrix: *mut Mtx, params: u32) -> Gfx {
    gsDma2p(
        G_MTX,
        matrix as usize,
        std::mem::size_of::<[u32; 16]>() as u32,
        params ^ G_MTX_PUSH,
        0,
    )
}

#[allow(non_snake_case)]
pub fn gsSPEndDisplayList() -> Gfx {
    Gfx {
        words: GWords {
            w0: 0xdf000000,
            w1: 0x0,
        },
    }
}

#[allow(non_snake_case)]
pub fn gsDPSetCycleType(cycle_type: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_CYCLETYPE, 2, cycle_type as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetRenderMode(c0: u32, c1: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_L, G_MDSFT_RENDERMODE, 29, (c0 | c1) as usize)
}

#[allow(non_snake_case)]
pub fn gsSPSetGeometryMode(word: u32) -> Gfx {
    gsSPGeometryMode(0, word)
}

#[allow(non_snake_case)]
pub fn gsSPVertex(vertices: &[Vtx], count: u16, write_index: u16) -> Gfx {
    let mut word: usize = 0x0;

    word |= shiftl(G_VTX, 24, 8);
    word |= shiftl(count as u32, 12, 8);
    word |= shiftl((write_index + count) as u32, 1, 7);

    let address = vertices.as_ptr() as usize;
    Gfx {
        words: GWords {
            w0: word,
            w1: address,
        },
    }
}

#[allow(non_snake_case)]
pub fn gsSP1Triangle(v0: u8, v1: u8, v2: u8, flag: u8) -> Gfx {
    let mut word: usize = 0x0;

    word |= shiftl(G_TRI1, 24, 8);
    word |= gsSP1Triangle_w1f(v0, v1, v2, flag);

    Gfx {
        words: GWords { w0: word, w1: 0 },
    }
}

#[allow(non_snake_case)]
pub fn gsDPSetColorImage(format: u32, size: u32, width: u32, address: usize) -> Gfx {
    gsSetImage(G_SETCIMG, format, size, width, address)
}

#[allow(non_snake_case)]
pub fn gsDPSetFillColor(color: u32) -> Gfx {
    gsDPSetColor(G_SETFILLCOLOR, color)
}

#[allow(non_snake_case)]
pub fn gsDPFillRectangle(ulx: u32, uly: u32, lrx: u32, lry: u32) -> Gfx {
    let mut w0: usize = 0x0;
    let mut w1: usize = 0x0;

    w0 |= shiftl(G_FILLRECT, 24, 8);
    w0 |= shiftl(lrx, 14, 10);
    w0 |= shiftl(lry, 2, 10);

    w1 |= shiftl(ulx, 14, 10);
    w1 |= shiftl(uly, 2, 10);

    Gfx {
        words: GWords { w0, w1 },
    }
}

#[allow(non_snake_case)]
pub fn gsSPDisplayList(display_list: &[Gfx]) -> Gfx {
    let address = display_list.as_ptr() as usize;
    gsDma1p(G_DL, address, 0, G_DL_PUSH)
}

#[allow(non_snake_case)]
pub fn gsSPViewport(viewport: &Viewport) -> Gfx {
    gsDma2p(
        G_MOVEMEM,
        viewport as *const Viewport as usize,
        std::mem::size_of::<Viewport>() as u32,
        G_MV_VIEWPORT,
        0,
    )
}

#[allow(non_snake_case)]
pub fn gsSPClearGeometryMode(word: u32) -> Gfx {
    gsSPGeometryMode(word, 0)
}

#[allow(non_snake_case)]
pub fn gsSPTexture(s: u16, t: u16, level: u8, tile: u8, on: bool) -> Gfx {
    let mut word: usize = 0x0;

    word |= shiftl(G_TEXTURE, 24, 8);
    word |= shiftl(BOWTIE_VAL, 16, 8);
    word |= shiftl(level as u32, 11, 3);
    word |= shiftl(tile as u32, 8, 3);
    word |= shiftl(on as u32, 1, 7);

    let mut w1: usize = 0x0;
    w1 |= shiftl(s as u32, 16, 16);
    w1 |= shiftl(t as u32, 0, 16);

    Gfx {
        words: GWords { w0: word, w1 },
    }
}

#[allow(non_snake_case)]
pub fn gsDPPipelineMode(mode: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_PIPELINE, 1, mode as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetScissor(mode: u32, ulx: u32, uly: u32, lrx: u32, lry: u32) -> Gfx {
    let mut w0: usize = 0x0;
    w0 |= shiftl(G_SETSCISSOR, 24, 8);
    w0 |= shiftl((ulx as f32 * 4.0) as u32, 12, 12);
    w0 |= shiftl((uly as f32 * 4.0) as u32, 0, 12);

    let mut w1: usize = 0x0;
    w1 |= shiftl(mode, 24, 2);
    w1 |= shiftl((lrx as f32 * 4.0) as u32, 12, 12);
    w1 |= shiftl((lry as f32 * 4.0) as u32, 0, 12);

    Gfx {
        words: GWords { w0, w1 },
    }
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureLOD(lod: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTLOD, 1, lod as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureLUT(lut: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTLUT, 2, lut as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureDetail(detail: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTDETAIL, 2, detail as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetTexturePersp(persp: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTPERSP, 1, persp as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureFilter(filter: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTFILT, 2, filter as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureConvert(convert: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_TEXTCONV, 3, convert as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetCombineMode(combine: CombineParams) -> Gfx {
    gsDPSetCombineLERP(combine)
}

#[allow(non_snake_case)]
pub fn gsDPSetCombineKey(key: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_COMBKEY, 1, key as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetAlphaCompare(compare: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_L, G_MDSFT_ALPHACOMPARE, 2, compare as usize)
}

#[allow(non_snake_case)]
pub fn gsDPSetColorDither(dither: u32) -> Gfx {
    gsSPSetOtherMode(G_SETOTHERMODE_H, G_MDSFT_COLORDITHER, 1, dither as usize)
}

// Inner Helpers

#[allow(non_snake_case)]
fn gsDPNoParam(command: u32) -> Gfx {
    Gfx {
        words: GWords {
            w0: shiftl(command, 24, 8),
            w1: 0x0,
        },
    }
}

#[allow(non_snake_case)]
fn gsDma1p(command: u32, segment: usize, length: u32, params: u32) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(params, 16, 8);
    word |= shiftl(length, 0, 16);

    Gfx {
        words: GWords {
            w0: word,
            w1: segment,
        },
    }
}

#[allow(non_snake_case)]
fn gsDma2p(command: u32, address: usize, length: u32, index: u32, offset: u32) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl((length - 1) / 8, 19, 5);
    word |= shiftl(offset / 8, 8, 8);
    word |= shiftl(index, 0, 8);

    Gfx {
        words: GWords {
            w0: word,
            w1: address,
        },
    }
}

#[allow(non_snake_case)]
fn gsSPSetOtherMode(command: u32, shift: u32, length: u32, data: usize) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(32 - shift - length, 8, 8);
    word |= shiftl(length - 1, 0, 8);

    Gfx {
        words: GWords { w0: word, w1: data },
    }
}

#[allow(non_snake_case)]
fn gsSPGeometryMode(clear_bits: u32, set_bits: u32) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(G_GEOMETRYMODE, 24, 8);
    word |= shiftl(!clear_bits, 0, 24);

    Gfx {
        words: GWords {
            w0: word,
            w1: set_bits as usize,
        },
    }
}

#[allow(non_snake_case)]
fn gsSP1Triangle_w1f(v0: u8, v1: u8, v2: u8, flag: u8) -> usize {
    if flag == 0 {
        return gsSP1Triangle_w1(v0, v1, v2);
    } else if flag == 1 {
        return gsSP1Triangle_w1(v1, v2, v0);
    }

    gsSP1Triangle_w1(v2, v0, v1)
}

#[allow(non_snake_case)]
fn gsSP1Triangle_w1(v0: u8, v1: u8, v2: u8) -> usize {
    let mut word: usize = 0x0;
    word |= shiftl((v0 * 2) as u32, 16, 8);
    word |= shiftl((v1 * 2) as u32, 8, 8);
    word |= shiftl((v2 * 2) as u32, 0, 8);

    word
}

#[allow(non_snake_case)]
fn gsSetImage(command: u32, format: u32, size: u32, width: u32, address: usize) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(format, 21, 3);
    word |= shiftl(size, 19, 2);
    word |= shiftl(width - 1, 0, 12);

    Gfx {
        words: GWords {
            w0: word,
            w1: address,
        },
    }
}

#[allow(non_snake_case)]
fn gsDPSetColor(command: u32, color: u32) -> Gfx {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);

    Gfx {
        words: GWords {
            w0: word,
            w1: color as usize,
        },
    }
}

#[allow(non_snake_case)]
fn gsDPSetCombineLERP(combine: CombineParams) -> Gfx {
    let mut w0: usize = 0x0;
    w0 |= shiftl(G_SETCOMBINE, 24, 8);
    w0 |= shiftl(
        GCCc0w0(combine.c0.a, combine.c0.c, combine.a0.a, combine.a0.c)
            | GCCc1w0(combine.c1.a, combine.c1.c),
        0,
        24,
    );

    let mut w1: usize = 0x0;
    w1 |= shiftl(
        GCCc0w1(combine.c0.b, combine.c0.d, combine.a0.b, combine.a0.d)
            | GCCc1w1(
                combine.c1.b,
                combine.a1.a,
                combine.a1.c,
                combine.c1.d,
                combine.a1.b,
                combine.a1.d,
            ),
        0,
        24,
    );

    Gfx {
        words: GWords { w0, w1 },
    }
}

#[allow(non_snake_case)]
fn GCCc0w0(
    saRGB0: ColorCombinerMux,
    mRGB0: ColorCombinerMux,
    saA0: AlphaCombinerMux,
    mA0: AlphaCombinerMux,
) -> u32 {
    shiftl(saRGB0.bits(), 20, 4) as u32
        | shiftl(mRGB0.bits(), 15, 5) as u32
        | shiftl(saA0.bits(), 12, 3) as u32
        | shiftl(mA0.bits(), 9, 3) as u32
}

#[allow(non_snake_case)]
fn GCCc1w0(saRGB1: ColorCombinerMux, mRGB1: ColorCombinerMux) -> u32 {
    shiftl(saRGB1.bits(), 5, 4) as u32 | shiftl(mRGB1.bits(), 0, 5) as u32
}

#[allow(non_snake_case)]
fn GCCc0w1(
    sbRGB0: ColorCombinerMux,
    aRGB0: ColorCombinerMux,
    sbA0: AlphaCombinerMux,
    aA0: AlphaCombinerMux,
) -> u32 {
    shiftl(sbRGB0.bits(), 28, 4) as u32
        | shiftl(aRGB0.bits(), 15, 3) as u32
        | shiftl(sbA0.bits(), 12, 3) as u32
        | shiftl(aA0.bits(), 9, 3) as u32
}

#[allow(non_snake_case)]
fn GCCc1w1(
    sbRGB1: ColorCombinerMux,
    saA1: AlphaCombinerMux,
    mA1: AlphaCombinerMux,
    aRGB1: ColorCombinerMux,
    sbA1: AlphaCombinerMux,
    aA1: AlphaCombinerMux,
) -> u32 {
    shiftl(sbRGB1.bits(), 24, 4) as u32
        | shiftl(saA1.bits(), 21, 3) as u32
        | shiftl(mA1.bits(), 18, 3) as u32
        | shiftl(aRGB1.bits(), 6, 3) as u32
        | shiftl(sbA1.bits(), 3, 3) as u32
        | shiftl(aA1.bits(), 0, 3) as u32
}
