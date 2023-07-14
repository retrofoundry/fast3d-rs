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
        blend_cycle1: BlendMode {
            color1: BlendColor::Input,
            alpha1: BlendAlpha1::Input,
            color2: BlendColor::Input,
            alpha2: BlendAlpha2::OneMinusAlpha,
        },
        blend_cycle2: BlendMode {
            color1: BlendColor::Input,
            alpha1: BlendAlpha1::Input,
            color2: BlendColor::Input,
            alpha2: BlendAlpha2::OneMinusAlpha,
        },
    };
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

bitflags! {
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
