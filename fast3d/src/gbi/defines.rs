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

impl Viewport {
    pub const fn new(vscale: [i16; 4], vtrans: [i16; 4]) -> Self {
        Self {
            vscale,
            vtrans,
            _padding: [0; 8],
        }
    }
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

// TODO: Replace with pigment's color type
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
#[derive(Clone, Copy)]
pub union Mtx {
    pub m: [[u32; 4]; 4],
    force_structure_alignment: i64,
}

#[cfg(feature = "gbifloats")]
#[repr(C)]
#[derive(Clone, Copy)]
pub union Mtxf {
    pub m: [[f32; 4]; 4],
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

impl Vtx_t {
    pub const fn new(
        #[cfg(feature = "gbifloats")] position: [f32; 3],
        #[cfg(not(feature = "gbifloats"))] position: [i16; 3],
        texture_coords: [i16; 2],
        color: Color_t,
    ) -> Self {
        Self {
            position,
            flag: 0,
            texture_coords,
            color,
        }
    }
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

bitflags! {
    pub struct OpCode: u8 {
        const NOOP = 0x00;

        const SET_COLORIMG = 0xff;
        const SET_DEPTHIMG = 0xfe;
        const SET_TEXIMG = 0xfd;
        const SET_COMBINE = 0xfc;
        const SET_ENVCOLOR = 0xfb;
        const SET_PRIMCOLOR = 0xfa;
        const SET_BLENDCOLOR = 0xf9;
        const SET_FOGCOLOR = 0xf8;
        const SET_FILLCOLOR = 0xf7;
        const SET_TILE = 0xf5;
        const SET_TILESIZE = 0xf2;
        const SET_PRIMDEPTH = 0xee;
        const SET_SCISSOR = 0xed;
        const SET_CONVERT = 0xec;
        const SET_KEYR = 0xeb;
        const SET_KEYGB = 0xea;

        const LOAD_BLOCK = 0xf3;
        const LOAD_TILE = 0xf4;
        const LOAD_TLUT = 0xf0;

        const TEXRECT = 0xe4;
        const TEXRECTFLIP = 0xe5;
        const FILLRECT = 0xf6;

        const RDPFULLSYNC = 0xe9;
        const RDPTILESYNC = 0xe8;
        const RDPPIPESYNC = 0xe7;
        const RDPLOADSYNC = 0xe6;
        const RDPSETOTHERMODE = 0xef;
    }
}

bitflags! {
    pub struct MoveWordIndex: u8 {
        const MATRIX = 0x00; /* NOTE: also used by movemem */
        const NUMLIGHT = 0x02;
        const CLIP = 0x04;
        const SEGMENT = 0x06;
        const FOG = 0x08;
        const LIGHTCOL = 0x0A;
        const PERSPNORM = 0x0E;
    }
}

bitflags! {
    pub struct TextureTile: u8 {
        const LOADTILE = 0x07;
        const RENDERTILE = 0x00;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WrapMode {
    Clamp,
    Repeat,
    MirrorRepeat,
}

impl Default for WrapMode {
    fn default() -> Self {
        Self::Repeat
    }
}

impl From<u8> for WrapMode {
    fn from(v: u8) -> Self {
        let mirror = v & 0x1 != 0;
        let clamp = v & 0x2 != 0;

        if clamp {
            WrapMode::Clamp
        } else if mirror {
            WrapMode::MirrorRepeat
        } else {
            WrapMode::Repeat
        }
    }
}

bitflags! {
    pub struct TextureMask: u8 {
        const NOMASK = 0x00;
    }
}

bitflags! {
    pub struct TextureShift: u8 {
        const NOLOD = 0x00;
    }
}

bitflags! {
    pub struct GeometryModes: u32 {
        const ZBUFFER             = 0x00000001;
        const SHADE               = 0x00000004;
        const FOG                 = 0x00010000;
        const LIGHTING            = 0x00020000;
        const TEXTURE_GEN         = 0x00040000;
        const TEXTURE_GEN_LINEAR  = 0x00080000;
        const LOD                 = 0x00100000;
        const CLIPPING            = 0x00800000;
    }
}

bitflags! {
    pub struct ColorCombinerMux: u32 {
        const COMBINED = 0x00;
        const TEXEL0 = 0x01;
        const TEXEL1 = 0x02;
        const PRIMITIVE = 0x03;
        const SHADE = 0x04;
        const ENVIRONMENT = 0x05;
        const CENTER = 0x06;
        const SCALE = 0x06;
        const COMBINED_ALPHA = 0x07;
        const TEXEL0_ALPHA = 0x08;
        const TEXEL1_ALPHA = 0x09;
        const PRIMITIVE_ALPHA = 0x0A;
        const SHADE_ALPHA = 0x0B;
        const ENVIRONMENT_ALPHA = 0x0C;
        const LOD_FRACTION = 0x0D;
        const PRIM_LOD_FRAC = 0x0E;
        const NOISE = 0x07;
        const K4 = 0x07;
        const K5 = 0x0F;
        const ONE = 0x06;
        const ZERO = 0x1F;
    }
}

bitflags! {
    pub struct AlphaCombinerMux: u32 {
        const COMBINED = 0x00;
        const TEXEL0 = 0x01;
        const TEXEL1 = 0x02;
        const PRIMITIVE = 0x03;
        const SHADE = 0x04;
        const ENVIRONMENT = 0x05;
        const LOD_FRACTION = 0x00;
        const PRIM_LOD_FRAC = 0x06;
        const ONE = 0x06;
        const ZERO = 0x07;
    }
}
