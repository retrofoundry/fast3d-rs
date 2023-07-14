use crate::defines::{DisplayListMode, GfxCommand, Matrix, Viewport};
use crate::gbi::shiftl;

#[cfg(feature = "f3dex2")]
use crate::defines::f3dex2::MatrixOperation;
#[cfg(feature = "f3dex2")]
use crate::defines::f3dex2::MoveMemoryIndex;
#[cfg(feature = "f3dex2")]
use crate::defines::f3dex2::OpCode;

#[cfg(not(feature = "f3dex2"))]
use crate::defines::f3d::MatrixOperation;
#[cfg(not(feature = "f3dex2"))]
use crate::defines::f3d::MoveMemoryIndex;
#[cfg(not(feature = "f3dex2"))]
use crate::defines::f3d::OpCode;

#[allow(non_snake_case)]
pub fn gsSPMatrix(matrix: *mut Matrix, params: u32) -> GfxCommand {
    #[cfg(feature = "f3dex2")]
    return gsDma2p(
        OpCode::MTX.bits() as u32,
        matrix as usize,
        std::mem::size_of::<[u32; 16]>() as u32,
        params ^ MatrixOperation::PUSH.bits() as u32,
        0,
    );
    #[cfg(not(feature = "f3dex2"))]
    return gsDma1p(
        OpCode::MTX.bits() as u32,
        matrix as usize,
        std::mem::size_of::<[u32; 16]>() as u32,
        params,
    );
}

#[allow(non_snake_case)]
pub fn gsSPDisplayList(display_list: &[GfxCommand]) -> GfxCommand {
    let address = display_list.as_ptr() as usize;
    gsDma1p(
        OpCode::DL.bits() as u32,
        address,
        0,
        DisplayListMode::Push as u32,
    )
}

#[allow(non_snake_case)]
pub fn gsSPViewport(viewport: &Viewport) -> GfxCommand {
    gsDma2p(
        OpCode::MOVEMEM.bits() as u32,
        viewport as *const Viewport as usize,
        std::mem::size_of::<Viewport>() as u32,
        MoveMemoryIndex::VIEWPORT.bits() as u32,
        0,
    )
}

// MARK: - Helpers

#[allow(non_snake_case)]
fn gsDma1p(command: u32, segment: usize, length: u32, params: u32) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl(params, 16, 8);
    word |= shiftl(length, 0, 16);

    GfxCommand::new(word, segment)
}

#[allow(non_snake_case)]
fn gsDma2p(command: u32, address: usize, length: u32, index: u32, offset: u32) -> GfxCommand {
    let mut word: usize = 0x0;
    word |= shiftl(command, 24, 8);
    word |= shiftl((length - 1) / 8, 19, 5);
    word |= shiftl(offset / 8, 8, 8);
    word |= shiftl(index, 0, 8);

    GfxCommand::new(word, address)
}
