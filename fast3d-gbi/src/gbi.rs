use crate::defines::render_mode::{BlendAlpha1, BlendAlpha2, BlendColor};
use crate::defines::render_mode::{CvgDst, RenderModeFlags, ZMode};
use crate::defines::OpCode as SharedOpCode;
use crate::defines::{BlendMode, GfxCommand, RenderMode};

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

// MARK: - Render Modes

#[allow(non_snake_case)]
const fn RM_AA_OPA_SURF(cycle: u8) -> RenderMode {
    let blend_mode = BlendMode {
        color1: BlendColor::Input,
        alpha1: BlendAlpha1::Input,
        color2: BlendColor::Memory,
        alpha2: BlendAlpha2::Memory,
    };

    RenderMode {
        flags: RenderModeFlags::ANTI_ALIASING
            .union(RenderModeFlags::IMAGE_READ)
            .union(RenderModeFlags::ALPHA_CVG_SEL),
        cvg_dst: CvgDst::Clamp,
        z_mode: ZMode::Opaque,
        blend_cycle1: if cycle == 1 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
        blend_cycle2: if cycle == 2 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
    }
}

#[allow(non_snake_case)]
const fn RM_OPA_SURF(cycle: u32) -> RenderMode {
    let blend_mode = BlendMode {
        color1: BlendColor::Input,
        alpha1: BlendAlpha1::Zero,
        color2: BlendColor::Input,
        alpha2: BlendAlpha2::One,
    };

    RenderMode {
        flags: RenderModeFlags::FORCE_BLEND,
        cvg_dst: CvgDst::Clamp,
        z_mode: ZMode::Opaque,
        blend_cycle1: if cycle == 1 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
        blend_cycle2: if cycle == 2 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
    }
}

#[allow(non_snake_case)]
const fn RM_RA_OPA_SURF(cycle: u32) -> RenderMode {
    let blend_mode = BlendMode {
        color1: BlendColor::Input,
        alpha1: BlendAlpha1::Input,
        color2: BlendColor::Memory,
        alpha2: BlendAlpha2::Memory,
    };

    RenderMode {
        flags: RenderModeFlags::ANTI_ALIASING.union(RenderModeFlags::ALPHA_CVG_SEL),
        cvg_dst: CvgDst::Clamp,
        z_mode: ZMode::Opaque,
        blend_cycle1: if cycle == 1 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
        blend_cycle2: if cycle == 2 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
    }
}

#[allow(non_snake_case)]
const fn RM_AA_XLU_SURF(cycle: u32) -> RenderMode {
    let blend_mode = BlendMode {
        color1: BlendColor::Input,
        alpha1: BlendAlpha1::Input,
        color2: BlendColor::Memory,
        alpha2: BlendAlpha2::OneMinusAlpha,
    };

    RenderMode {
        flags: RenderModeFlags::ANTI_ALIASING
            .union(RenderModeFlags::IMAGE_READ)
            .union(RenderModeFlags::CLEAR_ON_CVG)
            .union(RenderModeFlags::FORCE_BLEND),
        cvg_dst: CvgDst::Wrap,
        z_mode: ZMode::Opaque,
        blend_cycle1: if cycle == 1 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
        blend_cycle2: if cycle == 2 {
            blend_mode
        } else {
            BlendMode::ZERO
        },
    }
}

pub const G_RM_AA_OPA_SURF: u32 = RM_AA_OPA_SURF(1).to_w();
pub const G_RM_AA_OPA_SURF2: u32 = RM_AA_OPA_SURF(2).to_w();
pub const G_RM_AA_XLU_SURF: u32 = RM_AA_XLU_SURF(1).to_w();
pub const G_RM_AA_XLU_SURF2: u32 = RM_AA_XLU_SURF(2).to_w();

pub const G_RM_RA_OPA_SURF: u32 = RM_RA_OPA_SURF(1).to_w();
pub const G_RM_RA_OPA_SURF2: u32 = RM_RA_OPA_SURF(2).to_w();

pub const G_RM_OPA_SURF: u32 = RM_OPA_SURF(1).to_w();
pub const G_RM_OPA_SURF2: u32 = RM_OPA_SURF(2).to_w();
