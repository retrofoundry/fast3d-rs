use crate::models::texture::{ImageFormat, ImageSize};
use crate::output::gfx::CompareFunction;

pub struct OutputTexture {
    pub game_address: usize,
    pub format: ImageFormat,
    pub size: ImageSize,

    // properties from tile descriptor
    pub width: u32,
    pub height: u32,
    pub uls: u16,
    pub ult: u16,

    /// pixel data for the texture
    pub data: Vec<u8>,

    /// id of texture when it has been uploaded to a gfx device
    pub device_id: Option<u32>,
}

impl OutputTexture {
    pub fn new(
        game_address: usize,
        format: ImageFormat,
        size: ImageSize,
        width: u32,
        height: u32,
        uls: u16,
        ult: u16,
        data: Vec<u8>,
    ) -> Self {
        Self {
            game_address,
            format,
            size,
            width,
            height,
            uls,
            ult,
            data,
            device_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OutputSampler {
    pub tile: usize,
    pub linear_filter: bool,
    pub clamp_s: u32,
    pub clamp_t: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputStencil {
    pub depth_write_enabled: bool,
    pub depth_compare: CompareFunction,
    pub polygon_offset: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputUniformsBlend {
    pub fog_color: glam::Vec4,
    pub blend_color: glam::Vec4,
}

impl OutputUniformsBlend {
    pub const EMPTY: Self = OutputUniformsBlend {
        fog_color: glam::Vec4::ZERO,
        blend_color: glam::Vec4::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputUniformsCombine {
    pub prim_color: glam::Vec4,
    pub env_color: glam::Vec4,
    pub key_center: glam::Vec3,
    pub key_scale: glam::Vec3,
    pub prim_lod: glam::Vec2,
    pub convert_k4: f32,
    pub convert_k5: f32,
}

impl OutputUniformsCombine {
    pub const EMPTY: Self = OutputUniformsCombine {
        prim_color: glam::Vec4::ZERO,
        env_color: glam::Vec4::ZERO,
        key_center: glam::Vec3::ZERO,
        key_scale: glam::Vec3::ZERO,
        prim_lod: glam::Vec2::ZERO,
        convert_k4: 0.0,
        convert_k5: 0.0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct OutputUniforms {
    pub blend: OutputUniformsBlend,
    pub combine: OutputUniformsCombine,
}

impl OutputUniforms {
    pub const EMPTY: Self = OutputUniforms {
        blend: OutputUniformsBlend::EMPTY,
        combine: OutputUniformsCombine::EMPTY,
    };
}

#[derive(Debug, Clone)]
pub struct OutputVBO {
    pub vbo: Vec<u8>,
    pub num_tris: usize,
}

impl OutputVBO {
    pub const EMPTY: Self = OutputVBO {
        vbo: Vec::new(),
        num_tris: 0,
    };
}

#[derive(Debug, Copy, Clone)]
pub struct OutputFogParams {
    pub multiplier: i16,
    pub offset: i16,
}

impl OutputFogParams {
    pub const EMPTY: Self = OutputFogParams {
        multiplier: 0,
        offset: 0,
    };
}
