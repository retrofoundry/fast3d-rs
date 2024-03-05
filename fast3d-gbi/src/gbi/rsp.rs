use crate::defines::color_combiner::CombineParams;
use crate::defines::{
    ComponentSize, GfxCommand, ImageFormat, OtherModeH, OtherModeL, TextureShift, TextureTile,
    Vertex, WrapMode,
};
use crate::gbi::{
    gsDPLoadSync, gsDPPipeSync, rdp,
    rdp::{gsDPSetTextureImage, gsDPSetTile},
    shiftl, BOWTIE_VAL, CALC_DXT, G_TEXTURE_IMAGE_FRAC,
};

#[cfg(feature = "f3dex2")]
use crate::defines::f3dex2::OpCode;

#[cfg(not(feature = "f3dex2"))]
use crate::defines::f3d::OpCode;
use crate::rdp::{gsDPLoadBlock, gsDPSetTileSize};

#[allow(non_snake_case)]
pub fn gsDPPipelineMode(mode: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::PIPELINE.bits(),
        1,
        mode as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetCycleType(cycle_type: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::CYCLE_TYPE.bits(),
        2,
        cycle_type as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTexturePersp(enable: bool) -> GfxCommand {
    const G_MDSFT_TEXTPERSP: u32 = 17;

    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_PERSP.bits(),
        1,
        if enable { 1 << G_MDSFT_TEXTPERSP } else { 0 } as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureDetail(detail: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_DETAIL.bits(),
        2,
        detail as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureLOD(lod: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_LOD.bits(),
        1,
        lod as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureLUT(lut: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_LUT.bits(),
        2,
        lut as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureFilter(filter: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_FILT.bits(),
        2,
        filter as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureConvert(convert: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::TEXT_CONV.bits(),
        3,
        convert as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPLoadTextureBlock(
    tex_image: usize,
    format: ImageFormat,
    size: ComponentSize,
    width: u32,
    height: u32,
    pal: u32,
    cm_s: WrapMode,
    cm_t: WrapMode,
    mask_s: u32,
    mask_t: u32,
    shift_s: TextureShift,
    shift_t: TextureShift,
) -> Vec<GfxCommand> {
    vec![
        gsDPSetTextureImage(format, size.load_block(), 1, tex_image),
        gsDPSetTile(
            format,
            size.load_block(),
            0,
            0,
            TextureTile::LOADTILE,
            0,
            cm_t,
            mask_t,
            shift_t,
            cm_s,
            mask_s,
            shift_s,
        ),
        gsDPLoadSync(),
        gsDPLoadBlock(
            TextureTile::LOADTILE,
            0,
            0,
            (((width * height) + size.increment()) >> size.shift()) - 1,
            CALC_DXT(width, size.bytes()),
        ),
        gsDPPipeSync(),
        gsDPSetTile(
            format,
            size,
            ((width * size.line_bytes()) + 7) >> 3,
            0,
            TextureTile::RENDERTILE,
            pal,
            cm_t,
            mask_t,
            shift_t,
            cm_s,
            mask_s,
            shift_s,
        ),
        gsDPSetTileSize(
            TextureTile::RENDERTILE,
            0,
            0,
            (width - 1) << G_TEXTURE_IMAGE_FRAC,
            (height - 1) << G_TEXTURE_IMAGE_FRAC,
        ),
    ]
}

#[allow(non_snake_case)]
pub fn gsDPSetCombineMode(combine: CombineParams) -> GfxCommand {
    rdp::gsDPSetCombineLERP(combine)
}

#[allow(non_snake_case)]
pub fn gsDPSetCombineKey(enable: bool) -> GfxCommand {
    const G_MDSFT_COMBKEY: u32 = 8;
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::COMB_KEY.bits(),
        1,
        if enable { 1 << G_MDSFT_COMBKEY } else { 0 } as usize,
    )
}

#[cfg(not(feature = "hardware_version_1"))]
#[allow(non_snake_case)]
pub fn gsDPSetColorDither(dither: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::RGB_DITHER.bits(),
        1,
        dither as usize,
    )
}

#[cfg(feature = "hardware_version_1")]
#[allow(non_snake_case)]
pub fn gsDPSetColorDither(dither: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_H.bits(),
        OtherModeH::Shift::COLOR_DITHER.bits(),
        1,
        dither as usize,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetRenderMode(c0: u32, c1: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_L.bits(),
        OtherModeL::Shift::RENDER_MODE.bits(),
        29,
        (c0 | c1) as usize,
    )
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
pub fn gsSPSetGeometryMode(bits: u32) -> GfxCommand {
    gsSPGeometryMode(0, bits)
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
pub fn gsSPClearGeometryMode(bits: u32) -> GfxCommand {
    gsSPGeometryMode(bits, 0)
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
pub fn gsSPVertex(vertices: &[Vertex], count: u16, write_index: u16) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(OpCode::VTX.bits() as u32, 24, 8);
    word |= shiftl(count as u32, 12, 8);
    word |= shiftl((write_index + count) as u32, 1, 7);

    let address = vertices.as_ptr() as usize;
    GfxCommand::new(word, address)
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
pub fn gsSP1Triangle(v0: u8, v1: u8, v2: u8, flag: u8) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(OpCode::TRI1.bits() as u32, 24, 8);
    word |= gsSP1Triangle_w1f(v0, v1, v2, flag);

    GfxCommand::new(word, 0)
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
pub fn gsSPTexture(s: u16, t: u16, level: u8, tile: TextureTile, on: bool) -> GfxCommand {
    let mut word: usize = 0x0;

    word |= shiftl(OpCode::TEXTURE.bits() as u32, 24, 8);
    word |= shiftl(BOWTIE_VAL, 16, 8);
    word |= shiftl(level as u32, 11, 3);
    word |= shiftl(tile.bits() as u32, 8, 3);
    word |= shiftl(on as u32, 1, 7);

    let mut w1: usize = 0x0;
    w1 |= shiftl(s as u32, 16, 16);
    w1 |= shiftl(t as u32, 0, 16);

    GfxCommand::new(word, w1)
}

#[allow(non_snake_case)]
pub fn gsDPSetAlphaCompare(compare: u32) -> GfxCommand {
    gsSPSetOtherMode(
        OpCode::SETOTHERMODE_L.bits(),
        OtherModeL::Shift::ALPHA_COMPARE.bits(),
        2,
        compare as usize,
    )
}

// MARK: - Private Helpers

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
fn gsSPSetOtherMode(command: u8, shift: u32, length: u32, data: usize) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command as u32, 24, 8);
    word |= shiftl(32 - shift - length, 8, 8);
    word |= shiftl(length - 1, 0, 8);

    GfxCommand::new(word, data)
}

#[cfg(not(feature = "f3dex2"))]
fn gsSPSetOtherMode(command: u32, shift: u32, length: u32, data: usize) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(shift, 8, 8);
    word |= shiftl(length, 0, 8);

    GfxCommand::new(word, data)
}

#[cfg(feature = "f3dex2")]
#[allow(non_snake_case)]
fn gsSPGeometryMode(clear_bits: u32, set_bits: u32) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(OpCode::GEOMETRYMODE.bits() as u32, 24, 8);
    word |= shiftl(!clear_bits, 0, 24);

    GfxCommand::new(word, set_bits as usize)
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
