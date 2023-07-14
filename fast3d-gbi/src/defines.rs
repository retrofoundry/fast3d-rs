use bitflags::bitflags;
use num_enum::TryFromPrimitive;
use pigment::color::Color;

pub mod color_combiner;
pub use color_combiner::*;
pub mod render_mode;
pub use render_mode::*;

pub mod f3d;
pub mod f3dex2;

pub const G_MAXZ: u32 = 0x03ff;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GfxWords {
    pub w0: usize,
    pub w1: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union GfxCommand {
    pub words: GfxWords,
    force_structure_alignment: i64,
}

impl GfxCommand {
    pub fn new(w0: usize, w1: usize) -> Self {
        Self {
            words: GfxWords { w0, w1 },
        }
    }

    /// Reads a value from the command's first word.
    pub fn w0(&self, position: u32, width: u32) -> usize {
        unsafe { (self.words.w0 >> position) & ((1 << width) - 1) }
    }

    /// Reads a value from the command's second word.
    pub fn w1(&self, position: u32, width: u32) -> usize {
        unsafe { (self.words.w1 >> position) & ((1 << width) - 1) }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union Matrix {
    pub m: [[u32; 4]; 4],
    force_structure_alignment: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union Vertex {
    pub color: ColorVertex,
    pub normal: NormalVertex,
    force_structure_alignment: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ColorVertex {
    #[cfg(feature = "gbifloats")]
    pub position: [f32; 3], // in object space
    #[cfg(not(feature = "gbifloats"))]
    pub position: [i16; 3], // in object space
    flag: u16, // unused
    pub texture_coords: [i16; 2],
    pub color: Color,
}

impl ColorVertex {
    pub const fn new(
        #[cfg(feature = "gbifloats")] position: [f32; 3],
        #[cfg(not(feature = "gbifloats"))] position: [i16; 3],
        texture_coords: [i16; 2],
        color: Color,
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
pub struct NormalVertex {
    #[cfg(feature = "gbifloats")]
    pub position: [f32; 3], // in object space
    #[cfg(not(feature = "gbifloats"))]
    pub position: [i16; 3], // in object space
    flag: u16, // unused
    pub texture_coords: [i16; 2],
    pub normal: [i8; 3],
    pub alpha: u8,
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
    pub color_copy: [u8; 3], // copy of diffuse light value (rgba)
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
    pub color_copy: [u8; 3],
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

#[allow(non_snake_case)]
pub mod OtherModeH {
    use bitflags::bitflags;

    bitflags! {
        pub struct Shift: u32 {
            /// Unsupported.
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
            /// Unsupported in HW 2.0
            const COLOR_DITHER = 0x00000016;
            const PIPELINE = 0x00000017;
        }
    }

    impl Default for Shift {
        fn default() -> Self {
            Self::empty()
        }
    }
}

#[allow(non_snake_case)]
pub mod OtherModeL {
    use bitflags::bitflags;

    bitflags! {
        pub struct Shift: u32 {
            const ALPHA_COMPARE = 0;
            const DEPTH_SOURCE = 2;
            const RENDER_MODE = 3;
            const BLENDER = 16;
        }
    }

    impl Default for Shift {
        fn default() -> Self {
            Self::empty()
        }
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum DisplayListMode {
    Display = 0,
    Branch = 1,
}

impl Default for DisplayListMode {
    fn default() -> Self {
        Self::Display
    }
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

impl TextureFilter {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_TEXTFILT: u32 = 12;
        match self {
            Self::Point => 0 << G_MDSFT_TEXTFILT,
            Self::Average => 3 << G_MDSFT_TEXTFILT,
            Self::Bilerp => 2 << G_MDSFT_TEXTFILT,
        }
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

impl TextureLUT {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_TEXTLUT: u32 = 14;
        match self {
            Self::None => 0,
            Self::Rgba16 => 2 << G_MDSFT_TEXTLUT,
            Self::Ia16 => 3 << G_MDSFT_TEXTLUT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum TextureLOD {
    Tile = 0,
    Lod = 1,
}

impl Default for TextureLOD {
    fn default() -> Self {
        Self::Tile
    }
}

impl TextureLOD {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_TEXTLOD: u32 = 16;
        match self {
            Self::Tile => 0,
            Self::Lod => 1 << G_MDSFT_TEXTLOD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum TextureDetail {
    Clamp = 0,
    Sharpen = 1,
    Detail = 2,
}

impl Default for TextureDetail {
    fn default() -> Self {
        Self::Clamp
    }
}

impl TextureDetail {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_TEXTDETAIL: u32 = 17;
        match self {
            Self::Clamp => 0,
            Self::Sharpen => 1 << G_MDSFT_TEXTDETAIL,
            Self::Detail => 2 << G_MDSFT_TEXTDETAIL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum TextureConvert {
    Conv = 0,
    FiltConv = 5,
    Filt = 6,
}

impl Default for TextureConvert {
    fn default() -> Self {
        Self::Conv
    }
}

impl TextureConvert {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_TEXTCONV: u32 = 9;
        match self {
            Self::Conv => 0 << G_MDSFT_TEXTCONV,
            Self::FiltConv => 5 << G_MDSFT_TEXTCONV,
            Self::Filt => 6 << G_MDSFT_TEXTCONV,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum ColorDither {
    MagicSq = 0,
    Bayer = 1,
    Noise = 2,
    Disable = 3,
}

impl Default for ColorDither {
    fn default() -> Self {
        Self::Disable
    }
}

impl ColorDither {
    #[cfg(not(feature = "hardware_version_1"))]
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_RGBDITHER: u32 = 19;
        match self {
            Self::MagicSq => 0 << G_MDSFT_RGBDITHER,
            Self::Bayer => 1 << G_MDSFT_RGBDITHER,
            Self::Noise => 2 << G_MDSFT_RGBDITHER,
            Self::Disable => 3 << G_MDSFT_RGBDITHER,
        }
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

impl CycleType {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_CYCLETYPE: u32 = 20;
        match self {
            CycleType::OneCycle => 0 << G_MDSFT_CYCLETYPE,
            CycleType::TwoCycle => 1 << G_MDSFT_CYCLETYPE,
            CycleType::Copy => 2 << G_MDSFT_CYCLETYPE,
            CycleType::Fill => 3 << G_MDSFT_CYCLETYPE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
pub enum PipelineMode {
    OnePrimitive = 1,
    NPrimitive = 0,
}

impl Default for PipelineMode {
    fn default() -> Self {
        Self::NPrimitive
    }
}

impl PipelineMode {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_PIPELINE: u32 = 23;
        match self {
            PipelineMode::OnePrimitive => 1 << G_MDSFT_PIPELINE,
            PipelineMode::NPrimitive => 0 << G_MDSFT_PIPELINE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromPrimitive)]
#[repr(u8)]
#[allow(clippy::enum_variant_names)]
pub enum ScissorMode {
    NonInterlace = 0,
    OddInterlace = 3,
    EvenInterlace = 2,
}

impl Default for ScissorMode {
    fn default() -> Self {
        Self::NonInterlace
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

impl AlphaCompare {
    pub const fn raw_gbi_value(&self) -> u32 {
        const G_MDSFT_ALPHACOMPARE: u32 = 0;
        match self {
            AlphaCompare::None => 0 << G_MDSFT_ALPHACOMPARE,
            AlphaCompare::Threshold => 1 << G_MDSFT_ALPHACOMPARE,
            AlphaCompare::Dither => 3 << G_MDSFT_ALPHACOMPARE,
        }
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
