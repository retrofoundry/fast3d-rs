use bitflags::bitflags;
use num_enum::TryFromPrimitive;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GWords {
    pub w0: usize,
    pub w1: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union Gfx {
    pub words: GWords,
    pub force_structure_alignment: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub vscale: [i16; 4], // scale, 2 bits fraction
    pub vtrans: [i16; 4], // translate, 2 bits fraction
    _padding: [u8; 8],    // padding to 64-bit boundary
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawLight {
    pub words: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union Light {
    pub raw: RawLight,
    pub pos: PosLight,
    pub dir: DirLight,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirLight {
    pub col: [u8; 3], // diffuse light value (rgba)
    pad1: i8,
    pub colc: [u8; 3], // copy of diffuse light value (rgba)
    pad2: i8,
    pub dir: [i8; 3], // direction of light (normalized)
    pad3: i8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PosLight {
    pub kc: u8,
    pub col: [u8; 3],
    pub kl: u8,
    pub colc: [u8; 3],
    pub pos: [i16; 2],
    pub reserved1: u8,
    pub kq: u8,
    pub posz: i16,
}

impl Light {
    pub const ZERO: Self = Self {
        raw: RawLight {
            words: [0, 0, 0, 0],
        },
    };
}

pub struct LookAt {
    pub x: [f32; 3],
    pub y: [f32; 3],
}

impl LookAt {
    pub const fn new(x: [f32; 3], y: [f32; 3]) -> Self {
        Self { x, y }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color_t {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union Vtx {
    pub vertex: Vtx_t,
    pub normal: Vtx_tn,
    force_structure_alignment: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vtx_t {
    #[cfg(feature = "gbifloats")]
    pub position: [f32; 3], // in object space
    #[cfg(not(feature = "gbifloats"))]
    pub position: [i16; 3], // in object space
    flag: u16, // unused
    pub texture_coords: [i16; 2],
    pub color: Color_t,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vtx_tn {
    #[cfg(feature = "gbifloats")]
    pub position: [f32; 3], // in object space
    #[cfg(not(feature = "gbifloats"))]
    pub position: [i16; 3], // in object space
    flag: u16, // unused
    pub texture_coords: [i16; 2],
    pub normal: [i8; 3],
    pub alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum TextureFilter {
    Point = 0,
    Average = 3,
    Bilerp = 2,
}

impl Default for TextureFilter {
    fn default() -> Self {
        Self::Point
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum TextureLUT {
    None = 0,
    Rgba16 = 2,
    Ia16 = 3,
}

impl Default for TextureLUT {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum ImageFormat {
    Rgba = 0,
    Yuv = 1,
    Ci = 2,
    Ia = 3,
    I = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum ComponentSize {
    Bits4 = 0,
    Bits8 = 1,
    Bits16 = 2,
    Bits32 = 3,
    DD = 5,
}

impl Default for ComponentSize {
    fn default() -> Self {
        Self::Bits4
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum CycleType {
    OneCycle = 0,
    TwoCycle = 1,
    Copy = 2,
    Fill = 3,
}

impl Default for CycleType {
    fn default() -> Self {
        Self::OneCycle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum AlphaCompare {
    None = 0,
    Threshold = 1,
    Dither = 3,
}

impl Default for AlphaCompare {
    fn default() -> Self {
        Self::None
    }
}

bitflags! {
    pub struct OtherModeHLayout: u32 {
        const BLEND_MASK = 0x00000000;
        const ALPHA_DITHER = 0x00000004;
        const RGB_DITHER = 0x00000006;
        const COMB_KEY = 0x00000008;
        const TEXT_CONV = 0x00000009;
        const TEXT_FILT = 0x0000000c;
        const TEXT_LUT = 0x0000000e;
        const TEXT_LOD = 0x00000010;
        const TEXT_DETAIL = 0x00000011;
        const TEXT_PERSP = 0x00000013;
        const CYCLE_TYPE = 0x00000014;
        const COLOR_DITHER = 0x00000016;
        const PIPELINE = 0x00000017;
    }
}

impl Default for OtherModeHLayout {
    fn default() -> Self {
        Self::empty()
    }
}

bitflags! {
    pub struct OtherModeLLayout: u32 {
        const ALPHA_COMPARE = 0x00000000;
        const DEPTH_SOURCE = 0x00000002;
        const RENDER_MODE = 0x00000003;
    }
}

impl Default for OtherModeLLayout {
    fn default() -> Self {
        Self::empty()
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum ZMode {
    Opaque = 0,
    Interpenetrating = 1,
    Translucent = 2,
    Decal = 3,
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

pub mod g {
    pub mod set {
        pub const COLORIMG: u8 = 0xff;
        pub const DEPTHIMG: u8 = 0xfe;
        pub const TEXIMG: u8 = 0xfd;
        pub const COMBINE: u8 = 0xfc;
        pub const ENVCOLOR: u8 = 0xfb;
        pub const PRIMCOLOR: u8 = 0xfa;
        pub const BLENDCOLOR: u8 = 0xf9;
        pub const FOGCOLOR: u8 = 0xf8;
        pub const FILLCOLOR: u8 = 0xf7;
        pub const TILE: u8 = 0xf5;
        pub const TILESIZE: u8 = 0xf2;
        pub const PRIMDEPTH: u8 = 0xee;
        pub const SCISSOR: u8 = 0xed;
        pub const CONVERT: u8 = 0xec;
        pub const KEYR: u8 = 0xeb;
        pub const KEYGB: u8 = 0xea;
    }

    pub mod load {
        pub const BLOCK: u8 = 0xf3;
        pub const TILE: u8 = 0xf4;
        pub const TLUT: u8 = 0xf0;
    }

    pub mod mw {
        pub const MATRIX: u8 = 0x00; /* NOTE: also used by movemem */
        pub const NUMLIGHT: u8 = 0x02;
        pub const CLIP: u8 = 0x04;
        pub const SEGMENT: u8 = 0x06;
        pub const FOG: u8 = 0x08;
        pub const LIGHTCOL: u8 = 0x0A;
        #[cfg(feature = "f3dex2")]
        pub const FORCEMTX: u8 = 0x0C;
        #[cfg(not(feature = "f3dex2"))]
        pub const POINTS: u8 = 0x0C;
        pub const PERSPNORM: u8 = 0x0E;
    }

    pub mod tx {
        pub const LOADTILE: u8 = 7;
        pub const RENDERTILE: u8 = 0;
        pub const NOMIRROR: u8 = 0;
        pub const WRAP: u8 = 0;
        pub const MIRROR: u8 = 1;
        pub const CLAMP: u8 = 2;
        pub const NOMASK: u8 = 0;
        pub const NOLOD: u8 = 0;
    }

    // lose defines

    pub const TEXRECT: u8 = 0xe4;
    pub const TEXRECTFLIP: u8 = 0xe5;
    pub const FILLRECT: u8 = 0xf6;

    pub const NOOP: u8 = 0x00;
    pub const RDPFULLSYNC: u8 = 0xe9;
    pub const RDPTILESYNC: u8 = 0xe8;
    pub const RDPPIPESYNC: u8 = 0xe7;
    pub const RDPLOADSYNC: u8 = 0xe6;

    pub const RDPSETOTHERMODE: u8 = 0xef;
}

pub mod rsp_geometry {
    pub mod g {
        pub const ZBUFFER: u32 = 1 << 0;
        pub const SHADE: u32 = 1 << 2;
        pub const FOG: u32 = 1 << 16;
        pub const LIGHTING: u32 = 1 << 17;
        pub const TEXTURE_GEN: u32 = 1 << 18;
        pub const TEXTURE_GEN_LINEAR: u32 = 1 << 19;
        pub const LOD: u32 = 1 << 20; /* NOT IMPLEMENTED */
        pub const CLIPPING: u32 = 1 << 23;
    }
}
