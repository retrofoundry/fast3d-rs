use bitflags::bitflags;
use num_enum::TryFromPrimitive;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderMode {
    pub flags: RenderModeFlags,
    pub cvg_dst: CvgDst,
    pub z_mode: ZMode,
    pub blend_cycle1: BlendMode,
    pub blend_cycle2: BlendMode,
}

impl RenderMode {
    pub const ZERO: Self = Self {
        flags: RenderModeFlags::empty(),
        cvg_dst: CvgDst::Clamp,
        z_mode: ZMode::Opaque,
        blend_cycle1: BlendMode::ZERO,
        blend_cycle2: BlendMode::ZERO,
    };

    #[allow(non_snake_case)]
    pub const fn AA_OPA_SURF(cycle: u8) -> RenderMode {
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
    pub const fn OPA_SURF(cycle: u32) -> RenderMode {
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
    pub const fn RA_OPA_SURF(cycle: u32) -> RenderMode {
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
    pub const fn AA_XLU_SURF(cycle: u32) -> RenderMode {
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

    pub const fn to_w(&self) -> u32 {
        let mut w1 = self.flags.bits() as u32;

        w1 |= self.cvg_dst.raw_gbi_value() << 8;
        w1 |= self.z_mode.raw_gbi_value() << 10;

        w1 |= (self.blend_cycle1.color1 as u32) << 30;
        w1 |= (self.blend_cycle1.alpha1 as u32) << 26;
        w1 |= (self.blend_cycle1.color2 as u32) << 22;
        w1 |= (self.blend_cycle1.alpha2 as u32) << 18;

        w1 |= (self.blend_cycle2.color1 as u32) << 28;
        w1 |= (self.blend_cycle2.alpha1 as u32) << 24;
        w1 |= (self.blend_cycle2.color2 as u32) << 20;
        w1 |= (self.blend_cycle2.alpha2 as u32) << 16;

        w1
    }
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::ZERO
    }
}

impl TryFrom<u32> for RenderMode {
    type Error = ();

    fn try_from(w1: u32) -> Result<Self, Self::Error> {
        Ok(Self {
            flags: RenderModeFlags::from_bits_truncate(w1 as u16),
            cvg_dst: (((w1 >> 8) & 0x3) as u8).try_into().map_err(|_| {})?,
            z_mode: (((w1 >> 10) & 0x3) as u8).try_into().map_err(|_| {})?,
            blend_cycle1: BlendMode {
                color1: (((w1 >> 30) & 0x3) as u8).try_into().map_err(|_| {})?,
                alpha1: (((w1 >> 26) & 0x3) as u8).try_into().map_err(|_| {})?,
                color2: (((w1 >> 22) & 0x3) as u8).try_into().map_err(|_| {})?,
                alpha2: (((w1 >> 18) & 0x3) as u8).try_into().map_err(|_| {})?,
            },
            blend_cycle2: BlendMode {
                color1: (((w1 >> 28) & 0x3) as u8).try_into().map_err(|_| {})?,
                alpha1: (((w1 >> 24) & 0x3) as u8).try_into().map_err(|_| {})?,
                color2: (((w1 >> 20) & 0x3) as u8).try_into().map_err(|_| {})?,
                alpha2: (((w1 >> 16) & 0x3) as u8).try_into().map_err(|_| {})?,
            },
        })
    }
}

impl From<RenderMode> for u32 {
    fn from(val: RenderMode) -> Self {
        val.to_w()
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct RenderModeFlags: u16 {
        const ANTI_ALIASING = 0x0008;
        const Z_COMPARE     = 0x0010;
        const Z_UPDATE      = 0x0020;
        const IMAGE_READ    = 0x0040;
        const CLEAR_ON_CVG  = 0x0080;
        const CVG_X_ALPHA   = 0x1000;
        const ALPHA_CVG_SEL = 0x2000;
        const FORCE_BLEND   = 0x4000;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum CvgDst {
    Clamp = 0,
    Wrap = 1,
    Full = 2,
    Save = 3,
}

impl CvgDst {
    pub const fn raw_gbi_value(&self) -> u32 {
        match self {
            Self::Clamp => 0,
            Self::Wrap => 0x100,
            Self::Full => 0x200,
            Self::Save => 0x300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum ZMode {
    Opaque = 0,
    Interpenetrating = 1,
    Translucent = 2,
    Decal = 3,
}

impl ZMode {
    pub const fn raw_gbi_value(&self) -> u32 {
        match self {
            Self::Opaque => 0,
            Self::Interpenetrating => 0x400,
            Self::Translucent => 0x800,
            Self::Decal => 0xc00,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendMode {
    pub color1: BlendColor,
    pub alpha1: BlendAlpha1,
    pub color2: BlendColor,
    pub alpha2: BlendAlpha2,
}

impl BlendMode {
    pub const ZERO: Self = Self {
        color1: BlendColor::Input,
        alpha1: BlendAlpha1::Input,
        color2: BlendColor::Input,
        alpha2: BlendAlpha2::OneMinusAlpha,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum BlendColor {
    Input = 0,
    Memory = 1,
    Blend = 2,
    Fog = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum BlendAlpha1 {
    Input = 0,
    Fog = 1,
    Shade = 2,
    Zero = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum BlendAlpha2 {
    OneMinusAlpha = 0,
    Memory = 1,
    One = 2,
}
