use glam::{Vec2, Vec3, Vec4};
use log::trace;

use crate::output::{
    gfx::{BlendState, CompareFunction},
    RCPOutputCollector,
};

use super::models::color::Color;
use super::models::{
    texture::{
        translate_tile_ci4, translate_tile_ci8, translate_tile_i4, translate_tile_i8,
        translate_tile_ia16, translate_tile_ia4, translate_tile_ia8, translate_tile_rgba16,
        translate_tile_rgba32, translate_tlut, TextureImageState,
    },
    tile_descriptor::TileDescriptor,
};

use crate::gbi::utils::{
    get_cycle_type_from_other_mode_h, get_render_mode_from_other_mode_l,
    get_texture_filter_from_other_mode_h, get_zmode_from_other_mode_l, other_mode_l_uses_alpha,
    other_mode_l_uses_texture_edge, translate_cull_mode,
};
use crate::models::color::R5G5B5A1;
use crate::rsp::{RSPConstants, MAX_VERTICES, RSP};
use farbe::image::n64::ImageSize as FarbeImageSize;
use gbi_assembler::defines::color_combiner::{AlphaCombinerMux, ColorCombinerMux, CombineParams};
use gbi_assembler::defines::render_mode::{RenderModeFlags, ZMode};
use gbi_assembler::defines::{
    ComponentSize, CycleType, GeometryModes, GfxCommand, ImageFormat, OtherModeH, TextureFilter,
    TextureLUT, TextureTile, Viewport, WrapMode,
};

pub const SCREEN_WIDTH: f32 = 320.0;
pub const SCREEN_HEIGHT: f32 = 240.0;
const MAX_VBO_SIZE: usize = 256;
const MAX_TEXTURE_SIZE: usize = 4096;
pub const NUM_TILE_DESCRIPTORS: usize = 8;
pub const MAX_BUFFERED: usize = 256 * 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    pub fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OutputDimensions {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f32,
}

impl OutputDimensions {
    pub const ZERO: Self = Self {
        width: 0,
        height: 0,
        aspect_ratio: 0.0,
    };
}

pub struct TMEMMapEntry {
    pub address: usize,
}

impl TMEMMapEntry {
    pub fn new(address: usize) -> Self {
        Self { address }
    }
}

pub struct RDP {
    pub output_dimensions: OutputDimensions,

    pub texture_image_state: TextureImageState, // coming via GBI (texture to load)
    pub tile_descriptors: [TileDescriptor; NUM_TILE_DESCRIPTORS],
    pub tmem_map: rustc_hash::FxHashMap<u16, TMEMMapEntry>, // tmem address -> texture image state address
    pub textures_changed: [bool; 2],

    pub viewport: Rect,
    pub scissor: Rect,

    pub combine: CombineParams,
    pub other_mode_l: u32,
    pub other_mode_h: u32,
    pub shader_config_changed: bool,

    pub buf_vbo: [f32; MAX_VBO_SIZE * (26 * 3)], // 3 vertices in a triangle and 26 floats per vtx
    pub buf_vbo_len: usize,
    pub buf_vbo_num_tris: usize,

    pub env_color: Vec4,
    pub fog_color: Vec4,
    pub prim_color: Vec4,
    pub blend_color: Vec4,
    pub fill_color: Color,

    pub prim_lod: Vec2,

    pub convert_k: [i32; 6],
    pub key_center: Vec3,
    pub key_scale: Vec3,

    pub depth_image: usize,
    pub color_image: usize,
}

impl Default for RDP {
    fn default() -> Self {
        Self::new()
    }
}

impl RDP {
    pub fn new() -> Self {
        RDP {
            output_dimensions: OutputDimensions::ZERO,

            texture_image_state: TextureImageState::EMPTY,
            tile_descriptors: [TileDescriptor::EMPTY; 8],
            tmem_map: rustc_hash::FxHashMap::default(),
            textures_changed: [false; 2],

            viewport: Rect::ZERO,
            scissor: Rect::ZERO,

            combine: CombineParams::ZERO,
            other_mode_l: 0,
            other_mode_h: 0,
            shader_config_changed: false,

            buf_vbo: [0.0; MAX_VBO_SIZE * (26 * 3)],
            buf_vbo_len: 0,
            buf_vbo_num_tris: 0,

            env_color: Vec4::ZERO,
            fog_color: Vec4::ZERO,
            prim_color: Vec4::ZERO,
            blend_color: Vec4::ZERO,
            fill_color: Color::TRANSPARENT,

            prim_lod: Vec2::ZERO,

            convert_k: [0; 6],
            key_center: Vec3::ZERO,
            key_scale: Vec3::ZERO,

            depth_image: 0,
            color_image: 0,
        }
    }

    pub fn reset(&mut self) {
        self.combine = CombineParams::ZERO;
        self.other_mode_l = 0;
        self.other_mode_h = 0;
        self.env_color = Vec4::ZERO;
        self.fog_color = Vec4::ZERO;
        self.prim_color = Vec4::ZERO;
        self.blend_color = Vec4::ZERO;
        self.fill_color = Color::TRANSPARENT;
        self.prim_lod = Vec2::ZERO;
        self.key_center = Vec3::ZERO;
        self.key_scale = Vec3::ZERO;
        self.convert_k = [0; 6];
    }

    // Viewport

    pub fn calculate_and_set_viewport(&mut self, viewport: Viewport) {
        let mut width = 2.0 * viewport.vscale[0] as f32 / 4.0;
        let mut height = 2.0 * viewport.vscale[1] as f32 / 4.0;
        let mut x = viewport.vtrans[0] as f32 / 4.0 - width / 2.0;
        let mut y = SCREEN_HEIGHT - ((viewport.vtrans[1] as f32 / 4.0) + height / 2.0);

        width *= self.scaled_x();
        height *= self.scaled_y();
        x *= self.scaled_x();
        y *= self.scaled_y();

        self.viewport.x = x as u16;
        self.viewport.y = y as u16;
        self.viewport.width = width as u16;
        self.viewport.height = height as u16;

        self.shader_config_changed = true;
    }

    pub fn adjust_x_for_viewport(&self, x: f32) -> f32 {
        x * (4.0 / 3.0)
            / (self.output_dimensions.width as f32 / self.output_dimensions.height as f32)
    }

    // Textures

    pub fn load_tile(&mut self, tile: u8, ult: u16, uls: u16, lrt: u16, lrs: u16) {
        // First, verify that we're loading the whole texture.
        assert!(uls == 0 && ult == 0);
        // Verify that we're loading into LOADTILE.
        assert_eq!(tile, TextureTile::LOADTILE.bits());

        let tile = &mut self.tile_descriptors[tile as usize];
        self.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(self.texture_image_state.address),
        );

        tile.uls = uls;
        tile.ult = ult;
        tile.lrs = lrs;
        tile.lrt = lrt;

        trace!("texture {} is being marked as has changed", tile.tmem / 256);
        let tmem_index = if tile.tmem != 0 { 1 } else { 0 };
        self.textures_changed[tmem_index as usize] = true;
    }

    pub fn load_block(&mut self, tile: u8, ult: u16, uls: u16, dxt: u16, texels: u16) {
        // First, verify that we're loading the whole texture.
        assert!(uls == 0 && ult == 0);
        // Verify that we're loading into LOADTILE.
        assert_eq!(tile, TextureTile::LOADTILE.bits());

        let tile = &mut self.tile_descriptors[tile as usize];
        self.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(self.texture_image_state.address),
        );

        tile.uls = uls;
        tile.ult = ult;
        tile.lrs = texels;
        tile.lrt = dxt;

        let tmem_index = if tile.tmem != 0 { 1 } else { 0 };
        self.textures_changed[tmem_index as usize] = true;
    }

    // TODO: Verify this method against a game that uses TLUTs
    pub fn load_tlut(&mut self, tile: u8, high_index: u16) {
        // Verify that we're loading into LOADTILE.
        assert_eq!(tile, TextureTile::LOADTILE.bits());
        assert_eq!(self.texture_image_state.size, ComponentSize::Bits16 as u8); // TLUTs are always 16-bit (so far)

        assert!(
            self.tile_descriptors[tile as usize].tmem == 256
                && (high_index <= 127 || high_index == 255)
                || self.tile_descriptors[tile as usize].tmem == 384 && high_index == 127
        );

        trace!("gdp_load_tlut(tile: {}, high_index: {})", tile, high_index);

        let tile = &mut self.tile_descriptors[tile as usize];
        self.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(self.texture_image_state.address),
        );
    }

    pub fn import_tile_texture(
        &mut self,
        rsp: &RSP,
        output: &mut RCPOutputCollector,
        tmem_index: usize,
    ) {
        let tile = self.tile_descriptors[rsp.texture_state.tile as usize + tmem_index];
        let format = tile.format as u32;
        let size = tile.size as u32;
        let width = tile.get_width() as u32;
        let height = tile.get_height() as u32;

        let tmap_entry = self.tmem_map.get(&tile.tmem).unwrap();
        let texture_address = tmap_entry.address;

        if let Some(hash) = output
            .texture_cache
            .contains(texture_address, tile.format, tile.size)
        {
            output.set_texture(tmem_index, hash);
            return;
        }

        // TODO: figure out how to find the size of bytes in the texture
        let texture_data = unsafe {
            std::slice::from_raw_parts(texture_address as *const u8, MAX_TEXTURE_SIZE * 4)
        };

        let texture = match (format << 4) | size {
            x if x == ((ImageFormat::Rgba as u32) << 4 | ComponentSize::Bits16 as u32) => {
                translate_tile_rgba16(texture_data, width, height)
            }
            x if x == ((ImageFormat::Rgba as u32) << 4 | ComponentSize::Bits32 as u32) => {
                translate_tile_rgba32(texture_data, width, height)
            }
            x if x == ((ImageFormat::Ia as u32) << 4 | ComponentSize::Bits4 as u32) => {
                translate_tile_ia4(texture_data, width, height)
            }
            x if x == ((ImageFormat::Ia as u32) << 4 | ComponentSize::Bits8 as u32) => {
                translate_tile_ia8(texture_data, width, height)
            }
            x if x == ((ImageFormat::Ia as u32) << 4 | ComponentSize::Bits16 as u32) => {
                translate_tile_ia16(texture_data, width, height)
            }
            x if x == ((ImageFormat::I as u32) << 4 | ComponentSize::Bits4 as u32) => {
                translate_tile_i4(texture_data, width, height)
            }
            x if x == ((ImageFormat::I as u32) << 4 | ComponentSize::Bits8 as u32) => {
                translate_tile_i8(texture_data, width, height)
            }
            x if x == ((ImageFormat::Ci as u32) << 4 | ComponentSize::Bits4 as u32) => {
                let pal_addr = self
                    .tmem_map
                    .get(&(u16::MAX - tmem_index as u16))
                    .unwrap()
                    .address;
                let texlut: TextureLUT = (((self.other_mode_h >> 14) & 0x3) as u8)
                    .try_into()
                    .unwrap();
                let palette = translate_tlut(pal_addr, FarbeImageSize::S4B, &texlut);
                translate_tile_ci4(texture_data, &palette, width, height)
            }
            x if x == ((ImageFormat::Ci as u32) << 4 | ComponentSize::Bits8 as u32) => {
                let pal_addr = self
                    .tmem_map
                    .get(&(u16::MAX - tmem_index as u16))
                    .unwrap()
                    .address;
                let texlut: TextureLUT = (((self.other_mode_h >> 14) & 0x3) as u8)
                    .try_into()
                    .unwrap();
                let palette = translate_tlut(pal_addr, FarbeImageSize::S8B, &texlut);
                translate_tile_ci8(texture_data, &palette, width, height)
            }
            _ => {
                // TODO: Create an empty texture?
                panic!("Unsupported texture format: {:?} {:?}", format, size);
            }
        };

        let hash = output.texture_cache.insert(
            texture_address,
            tile.format,
            tile.size,
            width,
            height,
            tile.uls,
            tile.ult,
            texture,
        );
        output.set_texture(tmem_index, hash);
    }

    pub fn uses_texture1(&self) -> bool {
        get_cycle_type_from_other_mode_h(self.other_mode_h) == CycleType::TwoCycle
            && self.combine.uses_texture1()
    }

    pub fn flush_textures(&mut self, rsp: &RSP, output: &mut RCPOutputCollector) {
        // if textures are not on, then we have no textures to flush
        // if !self.texture_state.on {
        //     return;
        // }

        // let lod_en = (self.other_mode_h >> 16 & 0x1) != 0;
        // if lod_en {
        //     // TODO: Support mip-mapping
        //     trace!("Mip-mapping is enabled, but not supported yet");
        //     assert!(false);
        // } else {
        // we're in TILE mode. Let's check if we're in two-cycle mode.
        // let cycle_type = RDP::get_cycle_type_from_other_mode_h(self.other_mode_h);
        // assert!(
        //     cycle_type == OtherModeHCycleType::G_CYC_1CYCLE
        //         || cycle_type == OtherModeHCycleType::G_CYC_2CYCLE
        // );

        for i in 0..2 {
            if (i == 0 && self.combine.uses_texture0()) || self.uses_texture1() {
                if self.textures_changed[i as usize] {
                    self.flush(output);
                    output.clear_textures(i as usize);

                    self.import_tile_texture(rsp, output, i as usize);
                    self.textures_changed[i as usize] = false;
                }

                let tile_descriptor = self.tile_descriptors[(rsp.texture_state.tile + i) as usize];
                let linear_filter =
                    get_texture_filter_from_other_mode_h(self.other_mode_h) != TextureFilter::Point;
                output.set_sampler_parameters(
                    i as usize,
                    linear_filter,
                    tile_descriptor.clamp_s,
                    tile_descriptor.clamp_t,
                );
            }
        }
        // }
    }

    pub fn flush(&mut self, output: &mut RCPOutputCollector) {
        if self.buf_vbo_len > 0 {
            let vbo = bytemuck::cast_slice(&self.buf_vbo[..self.buf_vbo_len]);
            output.set_vbo(vbo.to_vec(), self.buf_vbo_num_tris);
            self.buf_vbo_len = 0;
            self.buf_vbo_num_tris = 0;
        }
    }

    // MARK: - Blend

    fn process_depth_params(
        &mut self,
        output: &mut RCPOutputCollector,
        geometry_mode: GeometryModes,
    ) {
        let depth_test = geometry_mode.contains(GeometryModes::ZBUFFER);

        let zmode = get_zmode_from_other_mode_l(self.other_mode_l);

        // handle depth compare
        let depth_compare = if get_render_mode_from_other_mode_l(self.other_mode_l)
            .flags
            .contains(RenderModeFlags::Z_COMPARE)
        {
            match zmode {
                ZMode::Opaque => CompareFunction::Less,
                ZMode::Interpenetrating => CompareFunction::Less, // TODO: Understand this
                ZMode::Translucent => CompareFunction::Less,
                ZMode::Decal => CompareFunction::LessEqual,
            }
        } else {
            CompareFunction::Always
        };

        // handle depth write
        let depth_write = get_render_mode_from_other_mode_l(self.other_mode_l)
            .flags
            .contains(RenderModeFlags::Z_UPDATE);

        // handle polygon offset (slope scale depth bias)
        let polygon_offset = zmode == ZMode::Decal;

        output.set_depth_stencil_params(depth_test, depth_write, depth_compare, polygon_offset);
    }

    pub fn update_render_state(
        &mut self,
        output: &mut RCPOutputCollector,
        geometry_mode: GeometryModes,
        rsp_constants: &RSPConstants,
    ) {
        let cull_mode = translate_cull_mode(geometry_mode, rsp_constants);
        output.set_cull_mode(cull_mode);

        self.process_depth_params(output, geometry_mode);

        // handle alpha blending
        let do_blend = other_mode_l_uses_alpha(self.other_mode_l)
            || other_mode_l_uses_texture_edge(self.other_mode_l);

        let blend_state = if do_blend {
            Some(BlendState::ALPHA_BLENDING)
        } else {
            None
        };

        output.set_blend_state(blend_state);

        // handle viewport and scissor
        let viewport = self.viewport;
        output.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.width as f32,
            viewport.height as f32,
        );

        let scissor = self.scissor;
        output.set_scissor(
            scissor.x as u32,
            scissor.y as u32,
            scissor.width as u32,
            scissor.height as u32,
        );
    }

    // MARK: - Setters

    pub fn set_convert(&mut self, k0: i32, k1: i32, k2: i32, k3: i32, k4: i32, k5: i32) {
        self.convert_k[0] = k0;
        self.convert_k[1] = k1;
        self.convert_k[2] = k2;
        self.convert_k[3] = k3;
        self.convert_k[4] = k4;
        self.convert_k[5] = k5;
    }

    pub fn set_key_r(&mut self, cr: u32, sr: u32, _wr: u32) {
        // TODO: Figure out how to use width
        self.key_center.x = cr as f32 / 255.0;
        self.key_scale.x = sr as f32 / 255.0;
    }

    pub fn set_key_gb(&mut self, cg: u32, sg: u32, _wg: u32, cb: u32, sb: u32, _wb: u32) {
        // TODO: Figure out how to use width
        self.key_center.y = cg as f32 / 255.0;
        self.key_center.z = cb as f32 / 255.0;
        self.key_scale.y = sg as f32 / 255.0;
        self.key_scale.z = sb as f32 / 255.0;
    }

    pub fn set_other_mode(&mut self, other_mode_h: u32, other_mode_l: u32) {
        self.other_mode_h = other_mode_h;
        self.other_mode_l = other_mode_l;
        self.shader_config_changed = true;
    }

    pub fn set_combine(&mut self, combine: CombineParams) {
        self.combine = combine;
        self.shader_config_changed = true;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_tile(
        &mut self,
        tile: u8,
        format: u8,
        size: u8,
        line: u16,
        tmem: u16,
        palette: u8,
        clamp_t: WrapMode,
        clamp_s: WrapMode,
        mask_t: u8,
        mask_s: u8,
        shift_t: u8,
        shift_s: u8,
    ) {
        assert!(tile < NUM_TILE_DESCRIPTORS as u8);
        let tile = &mut self.tile_descriptors[tile as usize];
        tile.set_format(format);
        tile.set_size(size);
        tile.line = line;
        tile.tmem = tmem;
        tile.palette = palette;
        tile.clamp_t = clamp_t;
        tile.mask_t = mask_t;
        tile.shift_t = shift_t;
        tile.clamp_s = clamp_s;
        tile.mask_s = mask_s;
        tile.shift_s = shift_s;

        self.textures_changed[0] = true;
        self.textures_changed[1] = true;
    }

    pub fn set_tile_size(&mut self, tile: u8, ult: u16, uls: u16, lrt: u16, lrs: u16) {
        assert!(tile < NUM_TILE_DESCRIPTORS as u8);
        let tile = &mut self.tile_descriptors[tile as usize];
        tile.uls = uls;
        tile.ult = ult;
        tile.lrs = lrs;
        tile.lrt = lrt;

        self.textures_changed[0] = true;
        self.textures_changed[1] = true;
    }

    pub fn set_env_color(&mut self, color: usize) {
        self.env_color = Vec4::new(
            ((color >> 24) & 0xFF) as f32 / 255.0,
            ((color >> 16) & 0xFF) as f32 / 255.0,
            ((color >> 8) & 0xFF) as f32 / 255.0,
            (color & 0xFF) as f32 / 255.0,
        );
    }

    pub fn set_prim_color(&mut self, lod_frac: u8, lod_min: u8, color: usize) {
        self.prim_lod = Vec2::new(lod_frac as f32 / 256.0, lod_min as f32 / 32.0);
        self.prim_color = Vec4::new(
            ((color >> 24) & 0xFF) as f32 / 255.0,
            ((color >> 16) & 0xFF) as f32 / 255.0,
            ((color >> 8) & 0xFF) as f32 / 255.0,
            (color & 0xFF) as f32 / 255.0,
        );
    }

    pub fn set_blend_color(&mut self, color: usize) {
        self.blend_color = Vec4::new(
            ((color >> 24) & 0xFF) as f32 / 255.0,
            ((color >> 16) & 0xFF) as f32 / 255.0,
            ((color >> 8) & 0xFF) as f32 / 255.0,
            (color & 0xFF) as f32 / 255.0,
        );
    }

    pub fn set_fog_color(&mut self, color: usize) {
        self.fog_color = Vec4::new(
            ((color >> 24) & 0xFF) as f32 / 255.0,
            ((color >> 16) & 0xFF) as f32 / 255.0,
            ((color >> 8) & 0xFF) as f32 / 255.0,
            (color & 0xFF) as f32 / 255.0,
        );
    }

    pub fn set_fill_color(&mut self, color: usize) {
        let packed_color = color as u16;
        self.fill_color = R5G5B5A1::to_rgba(packed_color);
    }

    // MARK: - Drawing

    pub fn draw_triangles(
        &mut self,
        rsp: &mut RSP,
        output: &mut RCPOutputCollector,
        vertex_id1: usize,
        vertex_id2: usize,
        vertex_id3: usize,
        is_drawing_rect: bool,
    ) {
        if self.shader_config_changed {
            self.flush(output);
            self.shader_config_changed = false;
        }

        let vertex1 = &rsp.vertex_table[vertex_id1];
        let vertex2 = &rsp.vertex_table[vertex_id2];
        let vertex3 = &rsp.vertex_table[vertex_id3];
        let vertex_array = [vertex1, vertex2, vertex3];

        // Don't draw anything if both tris are being culled.
        unsafe {
            // We do unchecked comparisons because the values set in rsp.constants are per GBI
            // and do not appear in the general GeometryModes enum
            if rsp
                .geometry_mode
                .contains(GeometryModes::from_bits_unchecked(
                    rsp.constants.geomode_cull_both_val,
                ))
            {
                return;
            }
        }

        self.update_render_state(output, rsp.geometry_mode, &rsp.constants);

        output.set_program_params(
            self.other_mode_h,
            self.other_mode_l,
            rsp.geometry_mode,
            self.combine,
        );

        self.flush_textures(rsp, output);

        output.set_uniforms(
            self.fog_color,
            self.blend_color,
            self.prim_color,
            self.env_color,
            self.key_center,
            self.key_scale,
            self.prim_lod,
            self.convert_k,
        );

        let current_tile = self.tile_descriptors[rsp.texture_state.tile as usize];
        let tex_width = current_tile.get_width();
        let tex_height = current_tile.get_height();
        let use_texture = self.combine.uses_texture0() || self.combine.uses_texture1();

        for vertex in &vertex_array {
            self.add_to_buf_vbo(vertex.position.x);
            self.add_to_buf_vbo(vertex.position.y);
            self.add_to_buf_vbo(vertex.position.z);
            self.add_to_buf_vbo(if is_drawing_rect {
                0.0
            } else {
                vertex.position.w
            });

            self.add_to_buf_vbo(vertex.color.r);
            self.add_to_buf_vbo(vertex.color.g);
            self.add_to_buf_vbo(vertex.color.b);
            self.add_to_buf_vbo(vertex.color.a);

            if use_texture {
                let mut u = (vertex.uv[0] - (current_tile.uls as f32) * 8.0) / 32.0;
                let mut v = (vertex.uv[1] - (current_tile.ult as f32) * 8.0) / 32.0;

                if get_texture_filter_from_other_mode_h(self.other_mode_h) != TextureFilter::Point {
                    u += 0.5;
                    v += 0.5;
                }

                self.add_to_buf_vbo(u / tex_width as f32);
                self.add_to_buf_vbo(v / tex_height as f32);
            }
        }

        self.buf_vbo_num_tris += 1;
        if self.buf_vbo_num_tris == MAX_BUFFERED {
            self.flush(output);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_texture_rectangle(
        &mut self,
        rsp: &mut RSP,
        output: &mut RCPOutputCollector,
        ulx: i32,
        uly: i32,
        mut lrx: i32,
        mut lry: i32,
        _tile: u8,
        uls: i16,
        ult: i16,
        mut dsdx: i16,
        mut dtdy: i16,
        flipped: bool,
    ) {
        let saved_combine_mode = self.combine;
        if get_cycle_type_from_other_mode_h(self.other_mode_h) == CycleType::Copy {
            // Per RDP Command Summary Set Tile's shift s and this dsdx should be set to 4 texels
            // Divide by 4 to get 1 instead
            dsdx >>= 2;

            // Color combiner is turned off in copy mode
            let rhs = (ColorCombinerMux::TEXEL0.bits() & 0b111) << 15
                | (AlphaCombinerMux::TEXEL0.bits() & 0b111) << 9;
            self.combine = CombineParams::decode(0, rhs as usize);
            self.shader_config_changed = true;

            // Per documentation one extra pixel is added in this modes to each edge
            lrx += 1 << 2;
            lry += 1 << 2;
        }

        // uls and ult are S10.5
        // dsdx and dtdy are S5.10
        // lrx, lry, ulx, uly are U10.2
        // lrs, lrt are S10.5
        if flipped {
            dsdx = -dsdx;
            dtdy = -dtdy;
        }

        let width = if !flipped { lrx - ulx } else { lry - uly } as i64;
        let height = if !flipped { lry - uly } else { lrx - ulx } as i64;
        let lrs: i64 = ((uls << 7) as i64 + (dsdx as i64) * width) >> 7;
        let lrt: i64 = ((ult << 7) as i64 + (dtdy as i64) * height) >> 7;

        let ul = &mut rsp.vertex_table[MAX_VERTICES];
        ul.uv[0] = uls as f32;
        ul.uv[1] = ult as f32;

        let lr = &mut rsp.vertex_table[MAX_VERTICES + 2];
        lr.uv[0] = lrs as f32;
        lr.uv[1] = lrt as f32;

        let ll = &mut rsp.vertex_table[MAX_VERTICES + 1];
        ll.uv[0] = if !flipped { uls as f32 } else { lrs as f32 };
        ll.uv[1] = if !flipped { lrt as f32 } else { ult as f32 };

        let ur = &mut rsp.vertex_table[MAX_VERTICES + 3];
        ur.uv[0] = if !flipped { lrs as f32 } else { uls as f32 };
        ur.uv[1] = if !flipped { ult as f32 } else { lrt as f32 };

        self.draw_rectangle(rsp, output, ulx, uly, lrx, lry);
        self.combine = saved_combine_mode;
        self.shader_config_changed = true;
    }

    pub fn fill_rect(
        &mut self,
        rsp: &mut RSP,
        output: &mut RCPOutputCollector,
        ulx: i32,
        uly: i32,
        mut lrx: i32,
        mut lry: i32,
    ) {
        if self.color_image == self.depth_image {
            // used to clear depth buffer, not necessary in modern pipelines
            return;
        }

        let cycle_type = get_cycle_type_from_other_mode_h(self.other_mode_h);
        if cycle_type == CycleType::Copy || cycle_type == CycleType::Fill {
            // Per documentation one extra pixel is added in this modes to each edge
            lrx += 1 << 2;
            lry += 1 << 2;
        }

        for i in MAX_VERTICES..MAX_VERTICES + 4 {
            let v = &mut rsp.vertex_table[i];
            v.color = self.fill_color;
        }

        let saved_combine_mode = self.combine;
        let rhs = (ColorCombinerMux::SHADE.bits() & 0b111) << 15
            | (AlphaCombinerMux::SHADE.bits() & 0b111) << 9;
        self.combine = CombineParams::decode(0, rhs as usize);
        self.shader_config_changed = true;
        self.draw_rectangle(rsp, output, ulx, uly, lrx, lry);
        self.combine = saved_combine_mode;
        self.shader_config_changed = true;
    }

    // MARK: - Helpers

    fn draw_rectangle(
        &mut self,
        rsp: &mut RSP,
        output: &mut RCPOutputCollector,
        ulx: i32,
        uly: i32,
        lrx: i32,
        lry: i32,
    ) {
        let saved_other_mode_h = self.other_mode_h;
        let cycle_type = get_cycle_type_from_other_mode_h(self.other_mode_h);

        if cycle_type == CycleType::Copy {
            self.other_mode_h = (self.other_mode_h & !(3 << OtherModeH::Shift::TEXT_FILT.bits()))
                | (TextureFilter::Point as u32);
            self.shader_config_changed = true;
        }

        // U10.2 coordinates
        let mut ulxf = ulx as f32 / (4.0 * (SCREEN_WIDTH / 2.0)) - 1.0;
        let ulyf = -(uly as f32 / (4.0 * (SCREEN_HEIGHT / 2.0))) + 1.0;
        let mut lrxf = lrx as f32 / (4.0 * (SCREEN_WIDTH / 2.0)) - 1.0;
        let lryf = -(lry as f32 / (4.0 * (SCREEN_HEIGHT / 2.0))) + 1.0;

        ulxf = self.adjust_x_for_viewport(ulxf);
        lrxf = self.adjust_x_for_viewport(lrxf);

        {
            let ul = &mut rsp.vertex_table[MAX_VERTICES];
            ul.position.x = ulxf;
            ul.position.y = ulyf;
            ul.position.z = -1.0;
            ul.position.w = 1.0;
        }

        {
            let ll = &mut rsp.vertex_table[MAX_VERTICES + 1];
            ll.position.x = ulxf;
            ll.position.y = lryf;
            ll.position.z = -1.0;
            ll.position.w = 1.0;
        }

        {
            let lr = &mut rsp.vertex_table[MAX_VERTICES + 2];
            lr.position.x = lrxf;
            lr.position.y = lryf;
            lr.position.z = -1.0;
            lr.position.w = 1.0;
        }

        {
            let ur = &mut rsp.vertex_table[MAX_VERTICES + 3];
            ur.position.x = lrxf;
            ur.position.y = ulyf;
            ur.position.z = -1.0;
            ur.position.w = 1.0;
        }

        // The coordinates for texture rectangle shall bypass the viewport setting
        let default_viewport = Rect::new(
            0,
            0,
            self.output_dimensions.width as u16,
            self.output_dimensions.height as u16,
        );
        let viewport_saved = self.viewport;
        let geometry_mode_saved = rsp.geometry_mode;

        self.viewport = default_viewport;
        rsp.geometry_mode = GeometryModes::empty();
        self.shader_config_changed = true;

        self.draw_triangles(
            rsp,
            output,
            MAX_VERTICES,
            MAX_VERTICES + 1,
            MAX_VERTICES + 3,
            true,
        );
        self.draw_triangles(
            rsp,
            output,
            MAX_VERTICES + 1,
            MAX_VERTICES + 2,
            MAX_VERTICES + 3,
            true,
        );

        rsp.geometry_mode = geometry_mode_saved;
        self.viewport = viewport_saved;
        self.shader_config_changed = true;

        if cycle_type == CycleType::Copy {
            self.other_mode_h = saved_other_mode_h;
            self.shader_config_changed = true;
        }
    }

    pub fn scaled_x(&self) -> f32 {
        self.output_dimensions.width as f32 / SCREEN_WIDTH
    }

    pub fn scaled_y(&self) -> f32 {
        self.output_dimensions.height as f32 / SCREEN_HEIGHT
    }

    pub fn add_to_buf_vbo(&mut self, data: f32) {
        self.buf_vbo[self.buf_vbo_len] = data;
        self.buf_vbo_len += 1;
    }
}
