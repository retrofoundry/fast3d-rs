use crate::defines::OpCode as SharedOpCode;
use crate::defines::{GfxCommand, RenderMode};

pub mod dma;
pub mod rdp;
pub mod rsp;

// old hacky way of doing this of fixing bowtie hangs
const BOWTIE_VAL: u32 = 0;

/// Dxt is the inverse of the number of 64-bit words in a line of
/// the texture being loaded using the load_block command.  If
/// there are any 1's to the right of the 11th fractional bit,
/// dxt should be rounded up.  The following macros accomplish
/// this.  The 4b macros are a special case since 4-bit textures
/// are loaded as 8-bit textures.  Dxt is fixed point 1.11. RJM
const G_TX_DXT_FRAC: u32 = 11;

const G_TEXTURE_IMAGE_FRAC: u32 = 2;

fn shiftl(value: u32, shift: u32, width: u32) -> usize {
    ((value & ((0x01 << width) - 1)) << shift) as usize
}

#[allow(non_snake_case)]
pub const fn GPACK_RGBA5551(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (((r as u32) << 8) & 0xf800)
        | (((g as u32) << 3) & 0x7c0)
        | (((b as u32) >> 2) & 0x3e)
        | ((a as u32) >> 0x1)
}

#[allow(non_snake_case)]
pub const CALC_DXT: fn(u32, u32) -> u32 =
    |width, b_txl| ((1 << G_TX_DXT_FRAC) + TXL2WORDS(width, b_txl) - 1) / TXL2WORDS(width, b_txl);

#[allow(non_snake_case)]
pub const TXL2WORDS: fn(u32, u32) -> u32 = |txls, b_txl| std::cmp::max(1, (txls * b_txl) / 8);

// MARK: - Gfx Commands

#[allow(non_snake_case)]
pub fn gsSPEndDisplayList() -> GfxCommand {
    GfxCommand::new(0xdf000000, 0x0)
}

#[allow(non_snake_case)]
pub fn gsDPPipeSync() -> GfxCommand {
    gsDPNoParam(SharedOpCode::RDPPIPESYNC.bits() as u32)
}

#[allow(non_snake_case)]
pub fn gsDPLoadSync() -> GfxCommand {
    gsDPNoParam(SharedOpCode::RDPLOADSYNC.bits() as u32)
}

#[allow(non_snake_case)]
pub fn gsDPFullSync() -> GfxCommand {
    gsDPNoParam(SharedOpCode::RDPFULLSYNC.bits() as u32)
}

// MARK: - Other Helpers

#[allow(non_snake_case)]
fn gsDPNoParam(command: u32) -> GfxCommand {
    GfxCommand::new(shiftl(command, 24, 8), 0x0)
}

// MARK: - Render Modes

pub const G_RM_AA_OPA_SURF: u32 = RenderMode::AA_OPA_SURF(1).to_w();
pub const G_RM_AA_OPA_SURF2: u32 = RenderMode::AA_OPA_SURF(2).to_w();
pub const G_RM_AA_XLU_SURF: u32 = RenderMode::AA_XLU_SURF(1).to_w();
pub const G_RM_AA_XLU_SURF2: u32 = RenderMode::AA_XLU_SURF(2).to_w();

pub const G_RM_RA_OPA_SURF: u32 = RenderMode::RA_OPA_SURF(1).to_w();
pub const G_RM_RA_OPA_SURF2: u32 = RenderMode::RA_OPA_SURF(2).to_w();

pub const G_RM_OPA_SURF: u32 = RenderMode::OPA_SURF(1).to_w();
pub const G_RM_OPA_SURF2: u32 = RenderMode::OPA_SURF(2).to_w();

#[cfg(not(feature = "hardware_version_1"))]
pub const G_TX_LDBLK_MAX_TXL: u32 = 2047;
#[cfg(feature = "hardware_version_1")]
pub const G_TX_LDBLK_MAX_TXL: u32 = 4095;
