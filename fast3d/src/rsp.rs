use std::slice;
use crate::gbi::defines::{DirLight, RSP_GEOMETRY, Vtx};

use super::{gbi::defines::Light, models::color::Color};
use glam::{Mat4, Vec2, Vec3A};
use crate::extensions::glam::{calculate_normal_dir, MatrixFrom};
use crate::gbi::utils::geometry_mode_uses_fog;
use crate::models::texture::TextureState;
use crate::output::RCPOutput;
use crate::rdp::RDP;

pub const MATRIX_STACK_SIZE: usize = 32;
pub const MAX_VERTICES: usize = 256;
pub const MAX_LIGHTS: usize = 7;
pub const MAX_SEGMENTS: usize = 16;

#[repr(C)]
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
    pub G_MTX_PUSH: u8,
    pub G_MTX_LOAD: u8,
    pub G_MTX_PROJECTION: u8,

    pub G_SHADING_SMOOTH: u32,
    pub G_CULL_FRONT: u32,
    pub G_CULL_BACK: u32,
    pub G_CULL_BOTH: u32,
}

impl RSPConstants {
    pub const EMPTY: Self = Self {
        G_MTX_PUSH: 0,
        G_MTX_LOAD: 0,
        G_MTX_PROJECTION: 0,

        G_SHADING_SMOOTH: 0,
        G_CULL_FRONT: 0,
        G_CULL_BACK: 0,
        G_CULL_BOTH: 0,
    };
}

pub struct RSP {
    // constants set by each GBI
    pub constants: RSPConstants,

    pub geometry_mode: u32,
    pub projection_matrix: Mat4,

    pub matrix_stack: [Mat4; MATRIX_STACK_SIZE],
    pub matrix_stack_pointer: usize,

    pub modelview_projection_matrix: Mat4,
    pub modelview_projection_matrix_changed: bool,

    pub lights_valid: bool,
    pub num_lights: u8,
    pub lights: [Light; MAX_LIGHTS + 1],
    pub lookat: [Vec3A; 2], // lookat_x, lookat_y

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

            geometry_mode: 0,
            projection_matrix: Mat4::ZERO,

            matrix_stack: [Mat4::ZERO; MATRIX_STACK_SIZE],
            matrix_stack_pointer: 0,

            modelview_projection_matrix: Mat4::ZERO,
            modelview_projection_matrix_changed: false,

            lights_valid: true,
            num_lights: 0,
            lights: [Light::ZERO; MAX_LIGHTS + 1],
            lookat: [Vec3A::ZERO; 2],

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

    pub fn from_segmented(&self, address: usize) -> usize {
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

        let data = self.from_segmented(address);
        let light_ptr = data as *const Light;
        let light = unsafe { &*light_ptr };

        self.lights[index] = *light;

        self.lights_valid = false;
    }

    pub fn set_look_at(&mut self, index: usize, address: usize) {
        assert!(index < 2);
        let data = self.from_segmented(address);
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

    pub fn set_texture(&mut self, rdp: &mut RDP, tile: u8, level: u8, on: u8, scale_s: u16, scale_t: u16) {
        if self.texture_state.tile != tile {
            rdp.textures_changed[0] = true;
            rdp.textures_changed[1] = true;
        }

        self.texture_state = TextureState::new(on != 0, tile, level, scale_s, scale_t);
    }

    pub fn set_vertex(&mut self, rdp: &mut RDP, output: &mut RCPOutput, address: usize, vertex_count: usize, mut write_index: usize) {
        if self.modelview_projection_matrix_changed {
            rdp.flush(output);
            self.recompute_mvp_matrix();
            output.set_projection_matrix(self.modelview_projection_matrix);
            self.modelview_projection_matrix_changed = false;
        }

        let vertices = self.from_segmented(address) as *const Vtx;

        for i in 0..vertex_count {
            let vertex = unsafe { &(*vertices.offset(i as isize)).vertex };
            let vertex_normal = unsafe { &(*vertices.offset(i as isize)).normal };
            let staging_vertex = &mut self.vertex_table[write_index as usize];

            let mut U = (((vertex.texture_coords[0] as i32) * (self.texture_state.scale_s as i32))
                >> 16) as i16;
            let mut V = (((vertex.texture_coords[1] as i32) * (self.texture_state.scale_t as i32))
                >> 16) as i16;

            if self.geometry_mode & RSP_GEOMETRY::G_LIGHTING as u32 > 0 {
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

                if self.geometry_mode & RSP_GEOMETRY::G_TEXTURE_GEN as u32 > 0 {
                    let dotx = vertex_normal.normal[0] as f32 * self.lookat_coeffs[0][0]
                        + vertex_normal.normal[1] as f32 * self.lookat_coeffs[0][1]
                        + vertex_normal.normal[2] as f32 * self.lookat_coeffs[0][2];

                    let doty = vertex_normal.normal[0] as f32 * self.lookat_coeffs[1][0]
                        + vertex_normal.normal[1] as f32 * self.lookat_coeffs[1][1]
                        + vertex_normal.normal[2] as f32 * self.lookat_coeffs[1][2];

                    U = ((dotx / 127.0 + 1.0) / 4.0) as i16 * self.texture_state.scale_s as i16;
                    V = ((doty / 127.0 + 1.0) / 4.0) as i16 * self.texture_state.scale_t as i16;
                }
            } else {
                staging_vertex.color.r = vertex.color.r as f32 / 255.0;
                staging_vertex.color.g = vertex.color.g as f32 / 255.0;
                staging_vertex.color.b = vertex.color.b as f32 / 255.0;
            }

            staging_vertex.uv[0] = U as f32;
            staging_vertex.uv[1] = V as f32;

            staging_vertex.position.x = vertex.position[0] as f32;
            staging_vertex.position.y = vertex.position[1] as f32;
            staging_vertex.position.z = vertex.position[2] as f32;
            staging_vertex.position.w = 1.0;

            if geometry_mode_uses_fog(self.geometry_mode) && self.fog_changed {
                rdp.flush(output);
                output.set_fog(self.fog_multiplier, self.fog_offset);
            }

            staging_vertex.color.a = vertex.color.a as f32 / 255.0;

            write_index += 1;
        }
    }

    pub fn matrix(&mut self, address: usize, params: u8) {
        let matrix = if cfg!(feature = "gbifloats") {
            let addr = self.from_segmented(address) as *const f32;
            let slice = unsafe { slice::from_raw_parts(addr, 16) };
            Mat4::from_floats(slice)
        } else {
            let addr = self.from_segmented(address) as *const i32;
            let slice = unsafe { slice::from_raw_parts(addr, 16) };
            Mat4::from_fixed_point(slice)
        };

        if params & self.constants.G_MTX_PROJECTION != 0 {
            if (params & self.constants.G_MTX_LOAD) != 0 {
                // Load the input matrix into the projection matrix
                // rsp.projection_matrix.copy_from_slice(&matrix);
                self.projection_matrix = matrix;
            } else {
                // Multiply the current projection matrix with the input matrix
                self.projection_matrix = matrix * self.projection_matrix;
            }
        } else {
            // Modelview matrix
            if params & self.constants.G_MTX_PUSH != 0 && self.matrix_stack_pointer < MATRIX_STACK_SIZE {
                // Push a copy of the current matrix onto the stack
                self.matrix_stack_pointer += 1;

                let src_index = self.matrix_stack_pointer - 2;
                let dst_index = self.matrix_stack_pointer - 1;
                let (left, right) = self.matrix_stack.split_at_mut(dst_index);
                right[0] = left[src_index];
            }

            if params & self.constants.G_MTX_LOAD != 0 {
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
        self.geometry_mode &= !clear_bits;
        self.geometry_mode |= set_bits;

        rdp.shader_config_changed = true;
    }
}
