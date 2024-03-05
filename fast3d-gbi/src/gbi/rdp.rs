use crate::defines::color_combiner::{AlphaCombinerMux, ColorCombinerMux, CombineParams};
use crate::gbi::{shiftl, G_TX_LDBLK_MAX_TXL};

use crate::defines::{
    ComponentSize, GfxCommand, ImageFormat, OpCode as SharedOpCode, TextureShift, TextureTile,
    WrapMode,
};

#[cfg(not(feature = "f3dex2"))]
use crate::f3d::OpCode;

#[allow(non_snake_case)]
pub fn gsDPSetColorImage(
    format: ImageFormat,
    size: ComponentSize,
    width: u32,
    address: usize,
) -> GfxCommand {
    gsSetImage(
        SharedOpCode::SET_COLORIMG.bits() as u32,
        format,
        size,
        width,
        address,
    )
}

#[allow(non_snake_case)]
pub fn gsDPSetFillColor(color: u32) -> GfxCommand {
    gsDPSetColor(SharedOpCode::SET_FILLCOLOR.bits() as u32, color)
}

#[cfg(not(feature = "f3dex2e"))]
#[allow(non_snake_case)]
pub fn gsDPFillRectangle(ulx: u32, uly: u32, lrx: u32, lry: u32) -> GfxCommand {
    let mut w0: usize = 0x0;
    let mut w1: usize = 0x0;

    w0 |= shiftl(SharedOpCode::FILLRECT.bits() as u32, 24, 8);
    w0 |= shiftl(lrx, 14, 10);
    w0 |= shiftl(lry, 2, 10);

    w1 |= shiftl(ulx, 14, 10);
    w1 |= shiftl(uly, 2, 10);

    GfxCommand::new(w0, w1)
}

#[allow(non_snake_case)]
pub fn gsDPSetScissor(mode: u32, ulx: u32, uly: u32, lrx: u32, lry: u32) -> GfxCommand {
    let mut w0: usize = 0x0;
    w0 |= shiftl(SharedOpCode::SET_SCISSOR.bits() as u32, 24, 8);
    w0 |= shiftl((ulx as f32 * 4.0) as u32, 12, 12);
    w0 |= shiftl((uly as f32 * 4.0) as u32, 0, 12);

    let mut w1: usize = 0x0;
    w1 |= shiftl(mode, 24, 2);
    w1 |= shiftl((lrx as f32 * 4.0) as u32, 12, 12);
    w1 |= shiftl((lry as f32 * 4.0) as u32, 0, 12);

    GfxCommand::new(w0, w1)
}

#[allow(non_snake_case)]
pub fn gsDPSetCombineLERP(combine: CombineParams) -> GfxCommand {
    let mut w0: usize = 0x0;
    w0 |= shiftl(SharedOpCode::SET_COMBINE.bits() as u32, 24, 8);
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

    GfxCommand::new(w0, w1)
}

#[allow(non_snake_case)]
pub fn gsDPSetTextureImage(
    format: ImageFormat,
    size: ComponentSize,
    width: u32,
    address: usize,
) -> GfxCommand {
    gsSetImage(
        SharedOpCode::SET_TEXIMG.bits() as u32,
        format,
        size,
        width,
        address,
    )
}

#[allow(clippy::too_many_arguments)]
#[allow(non_snake_case)]
pub fn gsDPSetTile(
    format: ImageFormat,
    size: ComponentSize,
    line: u32,
    tmem: u32,
    tile: TextureTile,
    palette: u32,
    cm_t: WrapMode,
    mask_t: u32,
    shift_t: TextureShift,
    cm_s: WrapMode,
    mask_s: u32,
    shift_s: TextureShift,
) -> GfxCommand {
    let mut w0: usize = 0x0;
    w0 |= shiftl(SharedOpCode::SET_TILE.bits() as u32, 24, 8);
    w0 |= shiftl(format as u32, 21, 3);
    w0 |= shiftl(size as u32, 19, 2);
    w0 |= shiftl(line, 9, 9);
    w0 |= shiftl(tmem, 0, 9);

    let mut w1: usize = 0x0;
    w1 |= shiftl(tile.bits() as u32, 24, 3);
    w1 |= shiftl(palette, 20, 4);
    w1 |= shiftl(cm_t.raw_gbi_value(), 18, 2);
    w1 |= shiftl(mask_t, 14, 4);
    w1 |= shiftl(shift_t.bits() as u32, 10, 4);
    w1 |= shiftl(cm_s.raw_gbi_value(), 8, 2);
    w1 |= shiftl(mask_s, 4, 4);
    w1 |= shiftl(shift_s.bits() as u32, 0, 4);

    GfxCommand::new(w0, w1)
}

#[allow(non_snake_case)]
pub fn gsDPSetTileSize(
    tile: TextureTile,
    ul_s: u32,
    ul_t: u32,
    lr_s: u32,
    lr_t: u32,
) -> GfxCommand {
    gsDPLoadTileGeneric(
        SharedOpCode::SET_TILESIZE.bits() as u32,
        tile,
        ul_s,
        ul_t,
        lr_s,
        lr_t,
    )
}

#[allow(non_snake_case)]
pub fn gsDPLoadBlock(tile: TextureTile, ul_s: u32, ul_t: u32, lr_s: u32, dxt: u32) -> GfxCommand {
    let mut w0: usize = 0x0;
    w0 |= shiftl(SharedOpCode::LOAD_BLOCK.bits() as u32, 24, 8);
    w0 |= shiftl(ul_s, 12, 12);
    w0 |= shiftl(ul_t, 0, 12);

    let mut w1: usize = 0x0;
    w1 |= shiftl(tile.bits() as u32, 24, 3);
    w1 |= shiftl(std::cmp::min(lr_s, G_TX_LDBLK_MAX_TXL), 12, 12);
    w1 |= shiftl(dxt, 0, 12);

    GfxCommand::new(w0, w1)
}

// MARK: - Private Helpers

#[allow(non_snake_case)]
fn gsDPSetColor(command: u32, color: u32) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);

    GfxCommand::new(word, color as usize)
}

#[allow(non_snake_case)]
fn gsSetImage(
    command: u32,
    format: ImageFormat,
    size: ComponentSize,
    width: u32,
    address: usize,
) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(format as u32, 21, 3);
    word |= shiftl(size as u32, 19, 2);
    word |= shiftl(width - 1, 0, 12);

    GfxCommand::new(word, address)
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

#[allow(non_snake_case)]
fn gsDPLoadTileGeneric(
    command: u32,
    tile: TextureTile,
    ul_s: u32,
    ul_t: u32,
    lr_s: u32,
    lr_t: u32,
) -> GfxCommand {
    let mut w0: usize = 0x0;
    w0 |= shiftl(command, 24, 8);
    w0 |= shiftl(ul_s, 12, 12);
    w0 |= shiftl(ul_t, 0, 12);

    let mut w1: usize = 0x0;
    w1 |= shiftl(tile.bits() as u32, 24, 3);
    w1 |= shiftl(lr_s, 12, 12);
    w1 |= shiftl(lr_t, 0, 12);

    GfxCommand::new(w0, w1)
}
