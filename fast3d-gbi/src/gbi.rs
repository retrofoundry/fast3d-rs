use crate::defines::render_mode::{BlendAlpha1, BlendAlpha2, BlendColor};
use crate::defines::render_mode::{CvgDst, RenderModeFlags, ZMode};
use crate::defines::GfxCommand;
use crate::defines::OpCode as SharedOpCode;

pub mod dma;
pub mod rdp;
pub mod rsp;

// old hacky way of doing this of fixing bowtie hangs
const BOWTIE_VAL: u32 = 0;

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
pub fn gsDPFullSync() -> GfxCommand {
    gsDPNoParam(SharedOpCode::RDPFULLSYNC.bits() as u32)
}

// MARK: - Other Helpers

#[allow(non_snake_case)]
fn gsDPNoParam(command: u32) -> GfxCommand {
    GfxCommand::new(shiftl(command, 24, 8), 0x0)
}

// MARK: - OtherMode L Helpers

#[allow(non_snake_case)]
const fn GBL_c1(m1a: u32, m1b: u32, m2a: u32, m2b: u32) -> u32 {
    (m1a) << 30 | (m1b) << 26 | (m2a) << 22 | (m2b) << 18
}

#[allow(non_snake_case)]
const fn GBL_c2(m1a: u32, m1b: u32, m2a: u32, m2b: u32) -> u32 {
    (m1a) << 28 | (m1b) << 24 | (m2a) << 20 | (m2b) << 16
}

#[allow(non_snake_case)]
const fn RM_AA_OPA_SURF(clk: u8) -> u32 {
    // TODO: better way to do something like this?
    let cvg_dst = CvgDst::Clamp;
    let zmode = ZMode::Opaque;

    RenderModeFlags::ANTI_ALIASING.bits() as u32
        | RenderModeFlags::IMAGE_READ.bits() as u32
        | cvg_dst.raw_gbi_value()
        | zmode.raw_gbi_value()
        | RenderModeFlags::ALPHA_CVG_SEL.bits() as u32
        | match clk {
            1 => GBL_c1(
                BlendColor::Input as u32,
                BlendAlpha1::Input as u32,
                BlendColor::Memory as u32,
                BlendAlpha2::Memory as u32,
            ),
            2 => GBL_c2(
                BlendColor::Input as u32,
                BlendAlpha1::Input as u32,
                BlendColor::Memory as u32,
                BlendAlpha2::Memory as u32,
            ),
            _ => 0, // This should really panic.. but in a const we can't do that.
        }
}

#[allow(non_snake_case)]
const fn RM_OPA_SURF(clk: u8) -> u32 {
    // TODO: better way to do something like this?
    let cvg_dst = CvgDst::Clamp;
    let zmode = ZMode::Opaque;

    cvg_dst.raw_gbi_value()
        | RenderModeFlags::FORCE_BLEND.bits() as u32
        | zmode.raw_gbi_value()
        | match clk {
            1 => GBL_c1(
                BlendColor::Input as u32,
                BlendAlpha1::Zero as u32,
                BlendColor::Input as u32,
                BlendAlpha2::One as u32,
            ),
            2 => GBL_c2(
                BlendColor::Input as u32,
                BlendAlpha1::Zero as u32,
                BlendColor::Input as u32,
                BlendAlpha2::One as u32,
            ),
            _ => 0, // This should really panic.. but in a const we can't do that.
        }
}

pub const G_RM_AA_OPA_SURF: u32 = RM_AA_OPA_SURF(1);
pub const G_RM_AA_OPA_SURF2: u32 = RM_AA_OPA_SURF(2);

pub const G_RM_OPA_SURF: u32 = RM_OPA_SURF(1);
pub const G_RM_OPA_SURF2: u32 = RM_OPA_SURF(2);
