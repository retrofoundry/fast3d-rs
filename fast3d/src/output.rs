use crate::output::models::{
    OutputFogParams, OutputSampler, OutputStencil, OutputUniforms, OutputUniformsBlend,
    OutputUniformsCombine, OutputVBO,
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};
use texture_cache::TextureCache;

use self::gfx::{BlendState, CompareFunction, Face};

use super::{
    models::{color_combiner::CombineParams, tile_descriptor::TileDescriptor},
    rdp::NUM_TILE_DESCRIPTORS,
};

pub mod gfx;
pub mod models;
pub mod texture_cache;

const TEXTURE_CACHE_MAX_SIZE: usize = 500;

#[derive(Debug, Clone)]
pub struct IntermediateDrawCall {
    // Shader Configuration
    pub other_mode_h: u32,
    pub other_mode_l: u32,
    pub geometry_mode: u32,
    pub combine: CombineParams,
    pub tile_descriptors: [TileDescriptor; NUM_TILE_DESCRIPTORS],
    pub shader_hash: u64,

    // Textures
    pub texture_indices: [Option<u64>; 2],

    // Samplers
    pub samplers: [Option<OutputSampler>; 2],

    // Stencil
    pub stencil: Option<OutputStencil>,

    // Viewport
    pub viewport: glam::Vec4,

    // Scissor
    pub scissor: [u32; 4],

    // Blend State
    pub blend_state: Option<BlendState>,

    // Cull Mode
    pub cull_mode: Option<Face>,

    // Uniforms
    pub uniforms: OutputUniforms,

    // Triangle Data
    pub vbo: OutputVBO,

    // Projection Matrix
    pub projection_matrix: glam::Mat4,

    // Fog Params
    pub fog: OutputFogParams,
}

impl IntermediateDrawCall {
    pub const EMPTY: Self = IntermediateDrawCall {
        other_mode_h: 0,
        other_mode_l: 0,
        geometry_mode: 0,
        combine: CombineParams::ZERO,
        tile_descriptors: [TileDescriptor::EMPTY; NUM_TILE_DESCRIPTORS],
        shader_hash: 0,
        texture_indices: [None; 2],
        samplers: [None; 2],
        stencil: None,
        viewport: glam::Vec4::ZERO,
        scissor: [0; 4],
        blend_state: None,
        cull_mode: None,
        uniforms: OutputUniforms::EMPTY,
        vbo: OutputVBO::EMPTY,
        projection_matrix: glam::Mat4::ZERO,
        fog: OutputFogParams::EMPTY,
    };

    pub fn finalize(&mut self) {
        // compute the shader hash and store it
        let mut hasher = DefaultHasher::new();

        self.other_mode_h.hash(&mut hasher);
        self.other_mode_l.hash(&mut hasher);
        self.geometry_mode.hash(&mut hasher);
        self.combine.hash(&mut hasher);

        self.shader_hash = hasher.finish();
    }
}

pub struct RCPOutput {
    pub texture_cache: TextureCache,
    pub draw_calls: Vec<IntermediateDrawCall>,
}

impl Default for RCPOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl RCPOutput {
    pub fn new() -> Self {
        RCPOutput {
            texture_cache: TextureCache::new(TEXTURE_CACHE_MAX_SIZE),
            // start draw calls with a default draw call
            draw_calls: vec![IntermediateDrawCall::EMPTY],
        }
    }

    fn current_draw_call(&mut self) -> &mut IntermediateDrawCall {
        self.draw_calls.last_mut().unwrap()
    }

    fn new_draw_call(&mut self) {
        let draw_call = self.current_draw_call();
        let draw_call = draw_call.clone();
        self.draw_calls.push(draw_call);
    }

    // Public API

    pub fn clear_draw_calls(&mut self) {
        let draw_call = self.current_draw_call();
        let draw_call = draw_call.clone();
        self.draw_calls = vec![draw_call];
    }

    pub fn clear_textures(&mut self, index: usize) {
        let draw_call = self.current_draw_call();
        draw_call.texture_indices[index] = None;
    }

    pub fn set_program_params(
        &mut self,
        other_mode_h: u32,
        other_mode_l: u32,
        combine: CombineParams,
        tile_descriptors: [TileDescriptor; NUM_TILE_DESCRIPTORS],
    ) {
        let draw_call = self.current_draw_call();
        draw_call.other_mode_h = other_mode_h;
        draw_call.other_mode_l = other_mode_l;
        draw_call.combine = combine;
        draw_call.tile_descriptors = tile_descriptors;
    }

    pub fn set_texture(&mut self, tile: usize, hash: u64) {
        let draw_call = self.current_draw_call();
        draw_call.texture_indices[tile] = Some(hash);
    }

    pub fn set_sampler_parameters(
        &mut self,
        tile: usize,
        linear_filter: bool,
        clamp_s: u32,
        clamp_t: u32,
    ) {
        let draw_call = self.current_draw_call();
        draw_call.samplers[tile] = Some(OutputSampler {
            tile,
            linear_filter,
            clamp_s,
            clamp_t,
        });
    }

    pub fn set_depth_stencil_params(
        &mut self,
        _depth_test_enabled: bool,
        depth_write_enabled: bool,
        depth_compare: CompareFunction,
        polygon_offset: bool,
    ) {
        let draw_call = self.current_draw_call();
        draw_call.stencil = Some(OutputStencil {
            depth_write_enabled,
            depth_compare,
            polygon_offset,
        });
    }

    pub fn set_projection_matrix(&mut self, matrix: glam::Mat4) {
        let draw_call = self.current_draw_call();
        draw_call.projection_matrix = matrix;
    }

    pub fn set_fog(&mut self, multiplier: i16, offset: i16) {
        let draw_call = self.current_draw_call();
        draw_call.fog = OutputFogParams { multiplier, offset };
    }

    pub fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let draw_call = self.current_draw_call();
        draw_call.viewport = glam::Vec4::new(x, y, width, height);
    }

    pub fn set_scissor(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let draw_call = self.current_draw_call();
        draw_call.scissor = [x, y, width, height];
    }

    pub fn set_blend_state(&mut self, blend_state: Option<BlendState>) {
        let draw_call = self.current_draw_call();
        draw_call.blend_state = blend_state;
    }

    pub fn set_cull_mode(&mut self, cull_mode: Option<Face>) {
        let draw_call = self.current_draw_call();
        draw_call.cull_mode = cull_mode;
    }

    pub fn set_uniforms(
        &mut self,
        fog_color: glam::Vec4,
        blend_color: glam::Vec4,
        prim_color: glam::Vec4,
        env_color: glam::Vec4,
        key_center: glam::Vec3,
        key_scale: glam::Vec3,
        prim_lod: glam::Vec2,
        convert_k: [i32; 6],
    ) {
        let draw_call = self.current_draw_call();
        draw_call.uniforms = OutputUniforms {
            blend: OutputUniformsBlend {
                fog_color,
                blend_color,
            },
            combine: OutputUniformsCombine {
                prim_color,
                env_color,
                key_center,
                key_scale,
                prim_lod,
                convert_k4: convert_k[4] as f32 / 255.0,
                convert_k5: convert_k[5] as f32 / 255.0,
            },
        };
    }

    pub fn set_vbo(&mut self, vbo: Vec<u8>, num_tris: usize) {
        let draw_call = self.current_draw_call();
        draw_call.vbo = OutputVBO { vbo, num_tris };
        draw_call.finalize();

        // start a new draw call that's a copy of the current one
        // we do this cause atm we only set properties on changes
        self.new_draw_call();
    }
}
