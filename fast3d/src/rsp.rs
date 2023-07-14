#[allow(unused_imports)]
use bitflags::Flags;
use std::slice;

use crate::extensions::glam::{calculate_normal_dir, MatrixFrom};
use crate::models::color::Color;
use crate::models::texture::TextureState;
use crate::output::RenderData;
use crate::rdp::RDP;
use fast3d_gbi::defines::{DirLight, GeometryModes, Light, Vertex};
use glam::{Mat4, Vec2, Vec3A};

pub const MATRIX_STACK_SIZE: usize = 32;
pub const MAX_VERTICES: usize = 256;
pub const MAX_LIGHTS: usize = 7;
pub const MAX_SEGMENTS: usize = 16;

#[repr(C)]
#[derive(Debug)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Position {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
}

#[repr(C)]
#[derive(Debug)]
pub struct StagingVertex {
    pub position: Position,
    pub uv: Vec2,
    pub color: Color,
    pub clip_reject: u8,
}

impl StagingVertex {
    pub const ZERO: Self = Self {
        position: Position::ZERO,
        uv: Vec2::ZERO,
        color: Color::TRANSPARENT,
        clip_reject: 0,
    };
}

pub struct RSPConstants {
    pub mtx_push_val: u8,
    pub mtx_load_val: u8,
    pub mtx_projection_val: u8,

    pub geomode_shading_smooth_val: u32,
    pub geomode_cull_front_val: u32,
    pub geomode_cull_back_val: u32,
    pub geomode_cull_both_val: u32,
}

impl RSPConstants {
    pub const EMPTY: Self = Self {
        mtx_push_val: 0,
        mtx_load_val: 0,
        mtx_projection_val: 0,

        geomode_shading_smooth_val: 0,
        geomode_cull_front_val: 0,
        geomode_cull_back_val: 0,
        geomode_cull_both_val: 0,
    };
}

#[allow(clippy::upper_case_acronyms)]
pub struct RSP {
    // constants set by each GBI
    pub constants: RSPConstants,

    pub geometry_mode: GeometryModes,
    pub projection_matrix: Mat4,

    pub matrix_stack: [Mat4; MATRIX_STACK_SIZE],
    pub matrix_stack_pointer: usize,

    pub modelview_projection_matrix: Mat4,
    pub modelview_projection_matrix_changed: bool,

    pub lights_valid: bool,
    pub num_lights: u8,
    pub lights: [Light; MAX_LIGHTS + 1],
    pub lookat: [Vec3A; 2], // lookat_x, lookat_y

    pub other_mode_l: u32,
    pub other_mode_h: u32,

    pub fog_multiplier: i16,
    pub fog_offset: i16,
    pub fog_changed: bool,

    pub vertex_table: [StagingVertex; MAX_VERTICES + 4],

    pub lights_coeffs: [Vec3A; MAX_LIGHTS],
    pub lookat_coeffs: [Vec3A; 2], // lookat_x, lookat_y

    pub segments: [usize; MAX_SEGMENTS],

    pub texture_state: TextureState,
}

impl Default for RSP {
    fn default() -> Self {
        Self::new()
    }
}

impl RSP {
    pub fn new() -> Self {
        RSP {
            constants: RSPConstants::EMPTY,

            geometry_mode: GeometryModes::empty(),
            projection_matrix: Mat4::ZERO,

            matrix_stack: [Mat4::ZERO; MATRIX_STACK_SIZE],
            matrix_stack_pointer: 0,

            modelview_projection_matrix: Mat4::ZERO,
            modelview_projection_matrix_changed: false,

            lights_valid: true,
            num_lights: 0,
            lights: [Light::ZERO; MAX_LIGHTS + 1],
            lookat: [Vec3A::ZERO; 2],

            other_mode_l: 0,
            other_mode_h: 0,

            fog_multiplier: 0,
            fog_offset: 0,
            fog_changed: false,

            vertex_table: [StagingVertex::ZERO; MAX_VERTICES + 4],

            lights_coeffs: [Vec3A::ZERO; MAX_LIGHTS],
            lookat_coeffs: [Vec3A::ZERO; 2],

            segments: [0; MAX_SEGMENTS],

            texture_state: TextureState::EMPTY,
        }
    }

    pub fn setup_constants(&mut self, constants: RSPConstants) {
        self.constants = constants;
    }

    pub fn reset(&mut self) {
        self.matrix_stack_pointer = 1;
        self.set_num_lights(1);
    }

    pub fn recompute_mvp_matrix(&mut self) {
        self.modelview_projection_matrix =
            self.matrix_stack[self.matrix_stack_pointer - 1] * self.projection_matrix;
    }

    pub fn set_num_lights(&mut self, num_lights: u8) {
        self.num_lights = num_lights;
        self.lights_valid = false;
    }

    pub fn set_segment(&mut self, segment: usize, address: usize) {
        assert!(segment < MAX_SEGMENTS);
        self.segments[segment] = address;
    }

    pub fn get_segment(&self, address: usize) -> usize {
        let segment = (address >> 24) & 0x0F;
        let offset = address & 0x00FFFFFF;

        if self.segments[segment] != 0 {
            self.segments[segment] + offset
        } else {
            address
        }
    }

    pub fn set_fog(&mut self, multiplier: i16, offset: i16) {
        self.fog_multiplier = multiplier;
        self.fog_offset = offset;
        self.fog_changed = true;
    }

    pub fn set_light_color(&mut self, index: usize, value: u32) {
        assert!(index <= MAX_LIGHTS);

        let light = &mut self.lights[index];
        unsafe {
            light.raw.words[0] = value;
        }
        unsafe {
            light.raw.words[1] = value;
        }
        self.lights_valid = false;
    }

    pub fn set_clip_ratio(&mut self, _ratio: usize) {
        // TODO: implement
    }

    pub fn set_persp_norm(&mut self, _norm: usize) {
        // TODO: implement
    }

    pub fn set_light(&mut self, index: usize, address: usize) {
        assert!(index <= MAX_LIGHTS);

        let data = self.get_segment(address);
        let light_ptr = data as *const Light;
        let light = unsafe { &*light_ptr };

        self.lights[index] = *light;

        self.lights_valid = false;
    }

    pub fn set_look_at(&mut self, index: usize, address: usize) {
        assert!(index < 2);
        let data = self.get_segment(address);
        let dir_light_ptr = data as *const DirLight;
        let dir_light = unsafe { &*dir_light_ptr };

        let lookat = if index == 0 {
            &mut self.lookat[0]
        } else {
            &mut self.lookat[1]
        };
        if dir_light.dir[0] != 0 || dir_light.dir[1] != 0 || dir_light.dir[2] != 0 {
            *lookat = Vec3A::new(
                dir_light.dir[0] as f32,
                dir_light.dir[1] as f32,
                dir_light.dir[2] as f32,
            )
            .normalize();
        } else {
            *lookat = Vec3A::ZERO;
        }
    }

    pub fn set_texture(
        &mut self,
        rdp: &mut RDP,
        tile: u8,
        level: u8,
        on: u8,
        scale_s: u16,
        scale_t: u16,
    ) {
        if self.texture_state.tile != tile {
            rdp.textures_changed[0] = true;
            rdp.textures_changed[1] = true;
        }

        self.texture_state = TextureState::new(on != 0, tile, level, scale_s, scale_t);
    }

    pub fn set_vertex(
        &mut self,
        rdp: &mut RDP,
        output: &mut RenderData,
        address: usize,
        vertex_count: usize,
        mut write_index: usize,
    ) {
        if self.modelview_projection_matrix_changed {
            rdp.flush(output);
            self.recompute_mvp_matrix();
            output.set_projection_matrix(self.modelview_projection_matrix);
            self.modelview_projection_matrix_changed = false;
        }

        let vertices = self.get_segment(address) as *const Vertex;

        for i in 0..vertex_count {
            let vertex = unsafe { &(*vertices.add(i)).color };
            let vertex_normal = unsafe { &(*vertices.add(i)).normal };
            let staging_vertex = &mut self.vertex_table[write_index];

            let mut u = (((vertex.texture_coords[0] as i32) * (self.texture_state.scale_s as i32))
                >> 16) as i16;
            let mut v = (((vertex.texture_coords[1] as i32) * (self.texture_state.scale_t as i32))
                >> 16) as i16;

            if self.geometry_mode.contains(GeometryModes::LIGHTING) {
                if !self.lights_valid {
                    for i in 0..(self.num_lights + 1) {
                        let light: &Light = &self.lights[i as usize];
                        let normalized_light_vector = Vec3A::new(
                            unsafe { light.dir.dir[0] as f32 / 127.0 },
                            unsafe { light.dir.dir[1] as f32 / 127.0 },
                            unsafe { light.dir.dir[2] as f32 / 127.0 },
                        );

                        calculate_normal_dir(
                            &normalized_light_vector,
                            &self.matrix_stack[self.matrix_stack_pointer - 1],
                            &mut self.lights_coeffs[i as usize],
                        );
                    }

                    calculate_normal_dir(
                        &self.lookat[0],
                        &self.matrix_stack[self.matrix_stack_pointer - 1],
                        &mut self.lookat_coeffs[0],
                    );

                    calculate_normal_dir(
                        &self.lookat[1],
                        &self.matrix_stack[self.matrix_stack_pointer - 1],
                        &mut self.lookat_coeffs[1],
                    );

                    self.lights_valid = true
                }

                let mut r = unsafe { self.lights[self.num_lights as usize].dir.col[0] as f32 };
                let mut g = unsafe { self.lights[self.num_lights as usize].dir.col[1] as f32 };
                let mut b = unsafe { self.lights[self.num_lights as usize].dir.col[2] as f32 };

                for i in 0..self.num_lights {
                    let mut intensity = vertex_normal.normal[0] as f32
                        * self.lights_coeffs[i as usize][0]
                        + vertex_normal.normal[1] as f32 * self.lights_coeffs[i as usize][1]
                        + vertex_normal.normal[2] as f32 * self.lights_coeffs[i as usize][2];

                    intensity /= 127.0;

                    if intensity > 0.0 {
                        unsafe {
                            r += intensity * self.lights[i as usize].dir.col[0] as f32;
                        }
                        unsafe {
                            g += intensity * self.lights[i as usize].dir.col[1] as f32;
                        }
                        unsafe {
                            b += intensity * self.lights[i as usize].dir.col[2] as f32;
                        }
                    }
                }

                staging_vertex.color.r = if r > 255.0 { 255.0 } else { r } / 255.0;
                staging_vertex.color.g = if g > 255.0 { 255.0 } else { g } / 255.0;
                staging_vertex.color.b = if b > 255.0 { 255.0 } else { b } / 255.0;

                if self.geometry_mode.contains(GeometryModes::TEXTURE_GEN) {
                    let dotx = vertex_normal.normal[0] as f32 * self.lookat_coeffs[0][0]
                        + vertex_normal.normal[1] as f32 * self.lookat_coeffs[0][1]
                        + vertex_normal.normal[2] as f32 * self.lookat_coeffs[0][2];

                    let doty = vertex_normal.normal[0] as f32 * self.lookat_coeffs[1][0]
                        + vertex_normal.normal[1] as f32 * self.lookat_coeffs[1][1]
                        + vertex_normal.normal[2] as f32 * self.lookat_coeffs[1][2];

                    u = ((dotx / 127.0 + 1.0) / 4.0) as i16 * self.texture_state.scale_s as i16;
                    v = ((doty / 127.0 + 1.0) / 4.0) as i16 * self.texture_state.scale_t as i16;
                }
            } else {
                staging_vertex.color.r = vertex.color.r as f32 / 255.0;
                staging_vertex.color.g = vertex.color.g as f32 / 255.0;
                staging_vertex.color.b = vertex.color.b as f32 / 255.0;
            }

            staging_vertex.uv[0] = u as f32;
            staging_vertex.uv[1] = v as f32;

            staging_vertex.position.x = vertex.position[0] as f32;
            staging_vertex.position.y = vertex.position[1] as f32;
            staging_vertex.position.z = vertex.position[2] as f32;
            staging_vertex.position.w = 1.0;

            if self.geometry_mode.contains(GeometryModes::FOG) && self.fog_changed {
                rdp.flush(output);
                output.set_fog(self.fog_multiplier, self.fog_offset);
            }

            staging_vertex.color.a = vertex.color.a as f32 / 255.0;

            write_index += 1;
        }
    }

    pub fn set_other_mode_h(&mut self, rdp: &mut RDP, length: usize, offset: usize, data: u32) {
        let mask = ((1 << length) - 1) << offset;
        self.other_mode_h = (self.other_mode_h & !mask) | data;
        rdp.set_other_mode(self.other_mode_h, self.other_mode_l);
    }

    pub fn set_other_mode_l(&mut self, rdp: &mut RDP, length: usize, offset: usize, data: u32) {
        let mask = ((1 << length) - 1) << offset;
        self.other_mode_l = (self.other_mode_l & !mask) | data;
        rdp.set_other_mode(self.other_mode_h, self.other_mode_l);
    }

    pub fn set_other_mode(&mut self, rdp: &mut RDP, high: u32, low: u32) {
        self.other_mode_h = high;
        self.other_mode_l = low;
        rdp.set_other_mode(self.other_mode_h, self.other_mode_l);
    }

    pub fn matrix(&mut self, address: usize, params: u8) {
        let matrix = if cfg!(feature = "gbifloats") {
            let addr = self.get_segment(address) as *const f32;
            let slice = unsafe { slice::from_raw_parts(addr, 16) };
            Mat4::from_floats(slice)
        } else {
            let addr = self.get_segment(address) as *const i32;
            let slice = unsafe { slice::from_raw_parts(addr, 16) };
            Mat4::from_fixed_point(slice)
        };

        if params & self.constants.mtx_projection_val != 0 {
            if (params & self.constants.mtx_load_val) != 0 {
                // Load the input matrix into the projection matrix
                // rsp.projection_matrix.copy_from_slice(&matrix);
                self.projection_matrix = matrix;
            } else {
                // Multiply the current projection matrix with the input matrix
                self.projection_matrix = matrix * self.projection_matrix;
            }
        } else {
            // Modelview matrix
            if params & self.constants.mtx_push_val != 0
                && self.matrix_stack_pointer < MATRIX_STACK_SIZE
            {
                // Push a copy of the current matrix onto the stack
                self.matrix_stack_pointer += 1;

                let src_index = self.matrix_stack_pointer - 2;
                let dst_index = self.matrix_stack_pointer - 1;
                let (left, right) = self.matrix_stack.split_at_mut(dst_index);
                right[0] = left[src_index];
            }

            if params & self.constants.mtx_load_val != 0 {
                // Load the input matrix into the current matrix
                self.matrix_stack[self.matrix_stack_pointer - 1] = matrix;
            } else {
                // Multiply the current matrix with the input matrix
                let result = matrix * self.matrix_stack[self.matrix_stack_pointer - 1];
                self.matrix_stack[self.matrix_stack_pointer - 1] = result;
            }

            // Clear the lights_valid flag
            self.lights_valid = false;
        }

        self.modelview_projection_matrix_changed = true;
    }

    pub fn pop_matrix(&mut self, mut count: usize) {
        while count > 0 {
            if self.matrix_stack_pointer > 0 {
                self.matrix_stack_pointer -= 1;
                self.modelview_projection_matrix_changed = true;
            }

            count -= 1;
        }
    }

    pub fn update_geometry_mode(&mut self, rdp: &mut RDP, clear_bits: u32, set_bits: u32) {
        let casted_clear_bits = GeometryModes::from_bits_retain(clear_bits);
        let casted_set_bits = GeometryModes::from_bits_retain(set_bits);

        self.geometry_mode &= casted_clear_bits;
        self.geometry_mode |= casted_set_bits;

        rdp.shader_config_changed = true;
    }
}
