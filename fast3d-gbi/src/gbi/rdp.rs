use crate::defines::color_combiner::{AlphaCombinerMux, ColorCombinerMux, CombineParams};
use crate::gbi::shiftl;

use crate::defines::{GfxCommand, OpCode as SharedOpCode};

#[cfg(not(feature = "f3dex2"))]
use crate::f3d::OpCode;

#[allow(non_snake_case)]
pub fn gsDPSetColorImage(format: u32, size: u32, width: u32, address: usize) -> GfxCommand {
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

// MARK: - Private Helpers

#[allow(non_snake_case)]
fn gsDPSetColor(command: u32, color: u32) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);

    GfxCommand::new(word, color as usize)
}

#[allow(non_snake_case)]
fn gsSetImage(command: u32, format: u32, size: u32, width: u32, address: usize) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(format, 21, 3);
    word |= shiftl(size, 19, 2);
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
