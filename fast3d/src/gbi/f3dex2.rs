use std::slice;

use glam::{Mat4, Vec2, Vec3A, Vec4};

use log::trace;

use super::defines::{Gfx, Light, Viewport, Vtx, G_FILLRECT, G_TEXRECT, G_TEXRECTFLIP};
use super::utils::{
    geometry_mode_uses_fog, get_cmd, get_cycle_type_from_other_mode_h,
    get_textfilter_from_other_mode_h,
};
use super::{
    super::{
        rdp::RDP,
        rsp::{MATRIX_STACK_SIZE, RSP},
    },
    defines::{G_LOAD, G_MW, G_SET},
};
use super::{GBIDefinition, GBIResult, GBI};
use crate::gbi::defines::G_TX;
use crate::output::RCPOutput;
use crate::{
    extensions::glam::{calculate_normal_dir, MatrixFrom},
    models::{
        color::R5G5B5A1,
        color_combiner::{CombineParams, ACMUX, CCMUX},
        texture::{ImageSize, TextFilt, TextureImageState, TextureState},
    },
    rdp::{
        OtherModeHCycleType, OtherModeH_Layout, Rect, TMEMMapEntry, MAX_BUFFERED, SCREEN_HEIGHT,
        SCREEN_WIDTH,
    },
    rsp::MAX_VERTICES,
};
use crate::rsp::RSPConstants;

pub struct RSP_GEOMETRY;

impl RSP_GEOMETRY {
    pub const G_TEXTURE_ENABLE: u32 = 0;
    pub const G_SHADING_SMOOTH: u32 = 1 << 21;
    pub const G_CULL_FRONT: u32 = 1 << 9;
    pub const G_CULL_BACK: u32 = 1 << 10;
    pub const G_CULL_BOTH: u32 = Self::G_CULL_FRONT | Self::G_CULL_BACK;
}

struct G_MTX;
impl G_MTX {
    pub const NOPUSH: u8 = 0x00;
    pub const PUSH: u8 = 0x01;
    pub const MUL: u8 = 0x00;
    pub const LOAD: u8 = 0x02;
    pub const MODELVIEW: u8 = 0x00;
    pub const PROJECTION: u8 = 0x04;
}

pub struct F3DEX2;

impl F3DEX2 {
    /*
     * MOVEWORD indices
     *
     * Each of these indexes an entry in a dmem table
     * which points to a word in dmem in dmem where
     * an immediate word will be stored.
     *
     */
    pub const G_MWO_aLIGHT_2: u8 = 0x18;
    pub const G_MWO_bLIGHT_2: u8 = 0x1c;
    pub const G_MWO_aLIGHT_3: u8 = 0x30;
    pub const G_MWO_bLIGHT_3: u8 = 0x34;
    pub const G_MWO_aLIGHT_4: u8 = 0x48;
    pub const G_MWO_bLIGHT_4: u8 = 0x4c;
    pub const G_MWO_aLIGHT_5: u8 = 0x60;
    pub const G_MWO_bLIGHT_5: u8 = 0x64;
    pub const G_MWO_aLIGHT_6: u8 = 0x78;
    pub const G_MWO_bLIGHT_6: u8 = 0x7c;
    pub const G_MWO_aLIGHT_7: u8 = 0x90;
    pub const G_MWO_bLIGHT_7: u8 = 0x94;
    pub const G_MWO_aLIGHT_8: u8 = 0xa8;
    pub const G_MWO_bLIGHT_8: u8 = 0xac;

    pub const G_NOOP: u8 = 0x00;

    // RDP
    pub const G_SETOTHERMODE_H: u8 = 0xe3;
    pub const G_SETOTHERMODE_L: u8 = 0xe2;
    pub const G_RDPHALF_1: u8 = 0xe1;
    pub const G_RDPHALF_2: u8 = 0xf1;

    pub const G_SPNOOP: u8 = 0xe0;

    // RSP
    pub const G_ENDDL: u8 = 0xdf;
    pub const G_DL: u8 = 0xde;
    pub const G_LOAD_UCODE: u8 = 0xdd;
    pub const G_MOVEMEM: u8 = 0xdc;
    pub const G_MOVEWORD: u8 = 0xdb;
    pub const G_MTX: u8 = 0xda;
    pub const G_GEOMETRYMODE: u8 = 0xd9;
    pub const G_POPMTX: u8 = 0xd8;
    pub const G_TEXTURE: u8 = 0xd7;

    // DMA
    pub const G_VTX: u8 = 0x01;
    pub const G_MODIFYVTX: u8 = 0x02;
    pub const G_CULLDL: u8 = 0x03;
    pub const G_BRANCH_Z: u8 = 0x04;
    pub const G_TRI1: u8 = 0x05;
    pub const G_TRI2: u8 = 0x06;
    pub const G_QUAD: u8 = 0x07;
    pub const G_LINE3D: u8 = 0x08;
    pub const G_DMA_IO: u8 = 0xD6;

    pub const G_SPECIAL_1: u8 = 0xD5;

    /*
     * MOVEMEM indices
     *
     * Each of these indexes an entry in a dmem table
     * which points to a 1-4 word block of dmem in
     * which to store a 1-4 word DMA.
     *
     */
    pub const G_MV_MMTX: u8 = 2;
    pub const G_MV_PMTX: u8 = 6;
    pub const G_MV_VIEWPORT: u8 = 8;
    pub const G_MV_LIGHT: u8 = 10;
    pub const G_MV_POINT: u8 = 12;
    pub const G_MV_MATRIX: u8 = 14;
    pub const G_MVO_LOOKATX: u8 = 0; // (0 * 24);
    pub const G_MVO_LOOKATY: u8 = 24;
    pub const G_MVO_L0: u8 = (2 * 24);
    pub const G_MVO_L1: u8 = (3 * 24);
    pub const G_MVO_L2: u8 = (4 * 24);
    pub const G_MVO_L3: u8 = (5 * 24);
    pub const G_MVO_L4: u8 = (6 * 24);
    pub const G_MVO_L5: u8 = (7 * 24);
    pub const G_MVO_L6: u8 = (8 * 24);
    pub const G_MVO_L7: u8 = (9 * 24);
}

impl GBIDefinition for F3DEX2 {
    fn setup(gbi: &mut GBI, rsp: &mut RSP) {
        gbi.register(F3DEX2::G_MTX as usize, F3DEX2::gsp_matrix);
        gbi.register(F3DEX2::G_POPMTX as usize, F3DEX2::gsp_pop_matrix);
        gbi.register(F3DEX2::G_MOVEMEM as usize, F3DEX2::gsp_movemem);
        gbi.register(F3DEX2::G_MOVEWORD as usize, F3DEX2::gsp_moveword);
        gbi.register(F3DEX2::G_TEXTURE as usize, F3DEX2::gsp_texture);
        gbi.register(F3DEX2::G_VTX as usize, F3DEX2::gsp_vertex);
        gbi.register(F3DEX2::G_DL as usize, F3DEX2::sub_dl);
        gbi.register(F3DEX2::G_GEOMETRYMODE as usize, F3DEX2::gsp_geometry_mode);
        gbi.register(F3DEX2::G_TRI1 as usize, F3DEX2::gsp_tri1);
        gbi.register(F3DEX2::G_TRI2 as usize, F3DEX2::gsp_tri2);
        gbi.register(F3DEX2::G_ENDDL as usize, |_, _, _, _| GBIResult::Return);

        gbi.register(
            F3DEX2::G_SETOTHERMODE_L as usize,
            F3DEX2::gdp_set_other_mode_l,
        );
        gbi.register(
            F3DEX2::G_SETOTHERMODE_H as usize,
            F3DEX2::gdp_set_other_mode_h,
        );
        gbi.register(G_SET::TEXIMG as usize, F3DEX2::gdp_set_texture_image);
        gbi.register(G_LOAD::BLOCK as usize, F3DEX2::gdp_load_block);
        gbi.register(G_LOAD::TILE as usize, F3DEX2::gdp_load_tile);
        gbi.register(G_LOAD::TLUT as usize, F3DEX2::gdp_load_tlut);
        gbi.register(G_SET::TILE as usize, F3DEX2::gdp_set_tile);
        gbi.register(G_SET::TILESIZE as usize, F3DEX2::gdp_set_tile_size);
        gbi.register(G_SET::SCISSOR as usize, F3DEX2::gdp_set_scissor);
        gbi.register(G_SET::CONVERT as usize, F3DEX2::gdp_set_convert);
        gbi.register(G_SET::KEYR as usize, F3DEX2::gdp_set_key_r);
        gbi.register(G_SET::KEYGB as usize, F3DEX2::gdp_set_key_gb);
        gbi.register(G_SET::COMBINE as usize, F3DEX2::gdp_set_combine);
        gbi.register(G_SET::ENVCOLOR as usize, F3DEX2::gdp_set_env_color);
        gbi.register(G_SET::PRIMCOLOR as usize, F3DEX2::gdp_set_prim_color);
        gbi.register(G_SET::BLENDCOLOR as usize, F3DEX2::gdp_set_blend_color);
        gbi.register(G_SET::FOGCOLOR as usize, F3DEX2::gdp_set_fog_color);
        gbi.register(G_SET::FILLCOLOR as usize, F3DEX2::gdp_set_fill_color);
        gbi.register(G_SET::DEPTHIMG as usize, F3DEX2::gdp_set_depth_image);
        gbi.register(G_SET::COLORIMG as usize, F3DEX2::gdp_set_color_image);
        gbi.register(G_TEXRECT as usize, F3DEX2::gdp_texture_rectangle);
        gbi.register(G_TEXRECTFLIP as usize, F3DEX2::gdp_texture_rectangle);
        gbi.register(G_FILLRECT as usize, F3DEX2::gdp_fill_rectangle);

        rsp.setup_constants(RSPConstants {
            G_MTX_PUSH: G_MTX::PUSH,
            G_MTX_LOAD: G_MTX::LOAD,
            G_MTX_PROJECTION: G_MTX::PROJECTION,

            G_SHADING_SMOOTH: RSP_GEOMETRY::G_SHADING_SMOOTH,
            G_CULL_FRONT: RSP_GEOMETRY::G_CULL_FRONT,
            G_CULL_BACK: RSP_GEOMETRY::G_CULL_BACK,
            G_CULL_BOTH: RSP_GEOMETRY::G_CULL_BOTH,
        })
    }
}

impl F3DEX2 {
    pub fn gsp_matrix(
        _rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let params = get_cmd(w0, 0, 8) as u8 ^ rsp.constants.G_MTX_PUSH;
        rsp.matrix(w1, params);

        GBIResult::Continue
    }

    pub fn gsp_pop_matrix(
        _rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        rsp.pop_matrix(w1 >> 6);

        GBIResult::Continue
    }

    pub fn gsp_movemem(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let index: u8 = get_cmd(w0, 0, 8) as u8;
        let offset = get_cmd(w0, 8, 8) * 8;
        let data = rsp.from_segmented(w1);

        match index {
            index if index == F3DEX2::G_MV_VIEWPORT => {
                let viewport_ptr = data as *const Viewport;
                let viewport = unsafe { &*viewport_ptr };
                rdp.calculate_and_set_viewport(*viewport);
            }
            index if index == F3DEX2::G_MV_MATRIX => {
                panic!("Unimplemented move matrix");
                // unsafe { *command = (*command).add(1) };
            }
            index if index == F3DEX2::G_MV_LIGHT => {
                let index = offset / 24;
                if index >= 2 {
                    rsp.set_light(index - 2, w1);
                } else {
                    rsp.set_look_at(index, w1);
                }
            }
            _ => panic!("Unimplemented move_mem command"),
        }

        GBIResult::Continue
    }

    pub fn gsp_moveword(
        _rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let m_type = get_cmd(w0, 16, 8) as u8;

        match m_type {
            m_type if m_type == G_MW::FORCEMTX => rsp.modelview_projection_matrix_changed = w1 == 0,
            m_type if m_type == G_MW::NUMLIGHT => rsp.set_num_lights(w1 as u8 / 24),
            m_type if m_type == G_MW::CLIP => {
                rsp.set_clip_ratio(w1);
            }
            m_type if m_type == G_MW::SEGMENT => {
                let segment = get_cmd(w0, 2, 4);
                rsp.set_segment(segment, w1 & 0x00FFFFFF)
            }
            m_type if m_type == G_MW::FOG => {
                let multiplier = get_cmd(w1, 16, 16) as i16;
                let offset = get_cmd(w1, 0, 16) as i16;
                rsp.set_fog(multiplier, offset);
            }
            m_type if m_type == G_MW::LIGHTCOL => {
                let index = get_cmd(w0, 0, 16) / 24;
                rsp.set_light_color(index, w1 as u32);
            }
            m_type if m_type == G_MW::PERSPNORM => {
                rsp.set_persp_norm(w1);
            }
            // TODO: G_MW_MATRIX
            _ => {
                // panic!("Unknown moveword type: {}", m_type);
            }
        }

        GBIResult::Continue
    }

    pub fn gsp_texture(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let scale_s = get_cmd(w1, 16, 16) as u16;
        let scale_t = get_cmd(w1, 0, 16) as u16;
        let level = get_cmd(w0, 11, 3) as u8;
        let tile = get_cmd(w0, 8, 3) as u8;
        let on = get_cmd(w0, 1, 7) as u8;

        rsp.set_texture(rdp, tile, level, on, scale_s, scale_t);

        GBIResult::Continue
    }

    pub fn gsp_vertex(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let vertex_count = get_cmd(w0, 12, 8);
        let write_index = get_cmd(w0, 1, 7) - get_cmd(w0, 12, 8);
        rsp.set_vertex(rdp, output, w1, vertex_count, write_index);

        GBIResult::Continue
    }

    pub fn gsp_geometry_mode(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let clear_bits = get_cmd(w0, 0, 24) as u32;
        let set_bits = w1 as u32;
        rsp.update_geometry_mode(rdp, clear_bits, set_bits);

        GBIResult::Continue
    }

    pub fn gsp_tri1(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };

        let vertex_id1 = get_cmd(w0, 16, 8) / 2;
        let vertex_id2 = get_cmd(w0, 8, 8) / 2;
        let vertex_id3 = get_cmd(w0, 0, 8) / 2;

        rdp.draw_triangles(rsp, output, vertex_id1, vertex_id2, vertex_id3, false);
        GBIResult::Continue
    }

    pub fn gsp_tri2(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let vertex_id1 = get_cmd(w0, 16, 8) / 2;
        let vertex_id2 = get_cmd(w0, 8, 8) / 2;
        let vertex_id3 = get_cmd(w0, 0, 8) / 2;
        rdp.draw_triangles(rsp, output, vertex_id1, vertex_id2, vertex_id3, false);

        let vertex_id1 = get_cmd(w1, 16, 8) / 2;
        let vertex_id2 = get_cmd(w1, 8, 8) / 2;
        let vertex_id3 = get_cmd(w1, 0, 8) / 2;
        rdp.draw_triangles(rsp, output, vertex_id1, vertex_id2, vertex_id3, false);

        GBIResult::Continue
    }

    pub fn sub_dl(
        _rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        if get_cmd(w0, 16, 1) == 0 {
            // Push return address
            let new_addr = rsp.from_segmented(w1);
            GBIResult::Recurse(new_addr)
        } else {
            let new_addr = rsp.from_segmented(w1);
            let cmd = new_addr as *mut Gfx;
            unsafe {
                *command = cmd.sub(1);
            }
            GBIResult::Continue
        }
    }

    pub fn gdp_set_other_mode_l(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let shift = 31 - get_cmd(w0, 8, 8) - get_cmd(w0, 0, 8);
        let mask = get_cmd(w0, 0, 8) + 1;
        let mode = w1;

        F3DEX2::gdp_other_mode(rdp, shift, mask, mode as u64)
    }

    pub fn gdp_set_other_mode_h(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let shift = 63 - get_cmd(w0, 8, 8) - get_cmd(w0, 0, 8);
        let mask = get_cmd(w0, 0, 8) + 1;
        let mode = (w1 as u64) << 32;

        F3DEX2::gdp_other_mode(rdp, shift, mask, mode)
    }

    pub fn gdp_other_mode(rdp: &mut RDP, shift: usize, mask: usize, mode: u64) -> GBIResult {
        let mask = ((1_u64 << (mask as u64)) - 1) << shift as u64;
        let mut om = rdp.other_mode_l as u64 | ((rdp.other_mode_h as u64) << 32);
        om = (om & !mask) | mode;

        rdp.other_mode_l = om as u32;
        rdp.other_mode_h = (om >> 32) as u32;
        rdp.shader_config_changed = true;

        GBIResult::Continue
    }

    pub fn gdp_set_scissor(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let _mode = get_cmd(w1, 24, 2);
        let ulx = get_cmd(w0, 12, 12);
        let uly = get_cmd(w0, 0, 12);
        let lrx = get_cmd(w1, 12, 12);
        let lry = get_cmd(w1, 0, 12);

        let x = ulx as f32 / 4.0 * rdp.scaled_x();
        let y = (SCREEN_HEIGHT - lry as f32 / 4.0) * rdp.scaled_y();
        let width = (lrx as f32 - ulx as f32) / 4.0 * rdp.scaled_x();
        let height = (lry as f32 - uly as f32) / 4.0 * rdp.scaled_y();

        rdp.scissor.x = x as u16;
        rdp.scissor.y = y as u16;
        rdp.scissor.width = width as u16;
        rdp.scissor.height = height as u16;

        rdp.shader_config_changed = true;
        GBIResult::Continue
    }

    pub fn gdp_set_convert(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let k0 = get_cmd(w0, 13, 9);
        let k1 = get_cmd(w0, 4, 9);
        let k2 = (get_cmd(w0, 0, 4) << 5) | get_cmd(w1, 27, 5);
        let k3 = get_cmd(w1, 18, 9);
        let k4 = get_cmd(w1, 9, 9);
        let k5 = get_cmd(w1, 0, 9);

        rdp.set_convert(
            k0 as i32, k1 as i32, k2 as i32, k3 as i32, k4 as i32, k5 as i32,
        );

        GBIResult::Continue
    }

    pub fn gdp_set_key_r(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let cr = get_cmd(w1, 8, 8);
        let sr = get_cmd(w1, 0, 8);
        let wr = get_cmd(w1, 16, 2);

        rdp.set_key_r(cr as u32, sr as u32, wr as u32);

        GBIResult::Continue
    }

    pub fn gdp_set_key_gb(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let cg = get_cmd(w1, 24, 8);
        let sg = get_cmd(w1, 16, 8);
        let wg = get_cmd(w0, 12, 12);
        let cb = get_cmd(w1, 8, 8);
        let sb = get_cmd(w1, 0, 8);
        let wb = get_cmd(w0, 0, 12);

        rdp.set_key_gb(
            cg as u32, sg as u32, wg as u32, cb as u32, sb as u32, wb as u32,
        );

        GBIResult::Continue
    }

    pub fn gdp_set_combine(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        rdp.combine = CombineParams::decode(w0, w1);
        rdp.shader_config_changed = true;

        GBIResult::Continue
    }

    pub fn gdp_set_tile(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let format = get_cmd(w0, 21, 3) as u8;
        let size = get_cmd(w0, 19, 2) as u8;
        let line = get_cmd(w0, 9, 9) as u16;
        let tmem = get_cmd(w0, 0, 9) as u16;
        let tile = get_cmd(w1, 24, 3) as u8;
        let palette = get_cmd(w1, 20, 4) as u8;
        let cm_t: u8 = get_cmd(w1, 18, 2) as u8;
        let mask_t: u8 = get_cmd(w1, 14, 4) as u8;
        let shift_t: u8 = get_cmd(w1, 10, 4) as u8;
        let cm_s: u8 = get_cmd(w1, 8, 2) as u8;
        let mask_s: u8 = get_cmd(w1, 4, 4) as u8;
        let shift_s: u8 = get_cmd(w1, 0, 4) as u8;

        let tile = &mut rdp.tile_descriptors[tile as usize];
        tile.set_format(format);
        tile.set_size(size);
        tile.line = line;
        tile.tmem = tmem;
        tile.palette = palette;
        tile.cm_t = cm_t;
        tile.mask_t = mask_t;
        tile.shift_t = shift_t;
        tile.cm_s = cm_s;
        tile.mask_s = mask_s;
        tile.shift_s = shift_s;

        rdp.textures_changed[0] = true;
        rdp.textures_changed[1] = true;

        GBIResult::Continue
    }

    pub fn gdp_set_tile_size(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let tile = get_cmd(w1, 24, 3) as u8;
        let uls = get_cmd(w0, 12, 12) as u16;
        let ult = get_cmd(w0, 0, 12) as u16;
        let lrs = get_cmd(w1, 12, 12) as u16;
        let lrt = get_cmd(w1, 0, 12) as u16;

        let tile = &mut rdp.tile_descriptors[tile as usize];
        tile.uls = uls;
        tile.ult = ult;
        tile.lrs = lrs;
        tile.lrt = lrt;

        rdp.textures_changed[0] = true;
        rdp.textures_changed[1] = true;

        GBIResult::Continue
    }

    pub fn gdp_set_texture_image(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let format = get_cmd(w0, 21, 3) as u8;
        let size = get_cmd(w0, 19, 2) as u8;
        let width = get_cmd(w0, 0, 10) as u16;
        let address = rsp.from_segmented(w1);

        rdp.texture_image_state = TextureImageState {
            format,
            size,
            width,
            address,
        };

        GBIResult::Continue
    }

    pub fn gdp_load_tlut(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let tile = get_cmd(w1, 24, 3) as u8;
        let high_index = get_cmd(w1, 14, 10) as u16;

        assert!(tile == G_TX::LOADTILE);
        assert!(rdp.texture_image_state.size == ImageSize::G_IM_SIZ_16b as u8); // TLUTs are always 16-bit (so far)
        assert!(
            rdp.tile_descriptors[tile as usize].tmem == 256
                && (high_index <= 127 || high_index == 255)
                || rdp.tile_descriptors[tile as usize].tmem == 384 && high_index == 127
        );

        trace!("gdp_load_tlut(tile: {}, high_index: {})", tile, high_index);

        let tile = &mut rdp.tile_descriptors[tile as usize];
        rdp.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(rdp.texture_image_state.address),
        );

        GBIResult::Continue
    }

    pub fn gdp_load_block(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let tile = get_cmd(w1, 24, 3) as u8;
        let uls = get_cmd(w0, 12, 12);
        let ult = get_cmd(w0, 0, 12);
        let _texels = get_cmd(w1, 12, 12) as u16;
        let _dxt = get_cmd(w1, 0, 12);

        // First, verify that we're loading the whole texture.
        assert!(uls == 0 && ult == 0);
        // Verify that we're loading into LOADTILE.
        assert!(tile == G_TX::LOADTILE);

        let tile = &mut rdp.tile_descriptors[tile as usize];
        rdp.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(rdp.texture_image_state.address),
        );

        let tmem_index = if tile.tmem != 0 { 1 } else { 0 };
        rdp.textures_changed[tmem_index as usize] = true;

        GBIResult::Continue
    }

    pub fn gdp_load_tile(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let tile = get_cmd(w1, 24, 3) as u8;
        let uls = get_cmd(w0, 12, 12) as u16;
        let ult = get_cmd(w0, 0, 12) as u16;
        let lrs = get_cmd(w1, 12, 12) as u16;
        let lrt = get_cmd(w1, 0, 12) as u16;

        // First, verify that we're loading the whole texture.
        assert!(uls == 0 && ult == 0);
        // Verify that we're loading into LOADTILE.
        assert!(tile == G_TX::LOADTILE);

        let tile = &mut rdp.tile_descriptors[tile as usize];
        rdp.tmem_map.insert(
            tile.tmem,
            TMEMMapEntry::new(rdp.texture_image_state.address),
        );

        // TODO: Really necessary?
        tile.uls = uls;
        tile.ult = ult;
        tile.lrs = lrs;
        tile.lrt = lrt;

        trace!("texture {} is being marked as has changed", tile.tmem / 256);
        let tmem_index = if tile.tmem != 0 { 1 } else { 0 };
        rdp.textures_changed[tmem_index as usize] = true;

        GBIResult::Continue
    }

    pub fn gdp_set_env_color(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let r = get_cmd(w1, 24, 8) as u8;
        let g = get_cmd(w1, 16, 8) as u8;
        let b = get_cmd(w1, 8, 8) as u8;
        let a = get_cmd(w1, 0, 8) as u8;

        rdp.env_color = Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );
        GBIResult::Continue
    }

    pub fn gdp_set_prim_color(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let lod_frac = get_cmd(w0, 0, 8) as u8;
        let lod_min = get_cmd(w0, 8, 5) as u8;

        let r = get_cmd(w1, 24, 8) as u8;
        let g = get_cmd(w1, 16, 8) as u8;
        let b = get_cmd(w1, 8, 8) as u8;
        let a = get_cmd(w1, 0, 8) as u8;

        rdp.prim_lod = Vec2::new(lod_frac as f32 / 256.0, lod_min as f32 / 32.0);
        rdp.prim_color = Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );

        GBIResult::Continue
    }

    pub fn gdp_set_blend_color(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let r = get_cmd(w1, 24, 8) as u8;
        let g = get_cmd(w1, 16, 8) as u8;
        let b = get_cmd(w1, 8, 8) as u8;
        let a = get_cmd(w1, 0, 8) as u8;

        rdp.blend_color = Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );

        GBIResult::Continue
    }

    pub fn gdp_set_fog_color(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let r = get_cmd(w1, 24, 8) as u8;
        let g = get_cmd(w1, 16, 8) as u8;
        let b = get_cmd(w1, 8, 8) as u8;
        let a = get_cmd(w1, 0, 8) as u8;

        rdp.fog_color = Vec4::new(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );

        GBIResult::Continue
    }

    pub fn gdp_set_fill_color(
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        let packed_color = w1 as u16;
        rdp.fill_color = R5G5B5A1::to_rgba(packed_color);

        GBIResult::Continue
    }

    pub fn gdp_set_depth_image(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        rdp.depth_image = rsp.from_segmented(w1);
        GBIResult::Continue
    }

    pub fn gdp_set_color_image(
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let _format = get_cmd(w0, 21, 3);
        let _size = get_cmd(w0, 19, 2);
        let _width = get_cmd(w0, 0, 11);

        rdp.color_image = rsp.from_segmented(w1);
        GBIResult::Continue
    }

    pub fn draw_rectangle(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        ulx: i32,
        uly: i32,
        lrx: i32,
        lry: i32,
    ) {
        let saved_other_mode_h = rdp.other_mode_h;
        let cycle_type = get_cycle_type_from_other_mode_h(rdp.other_mode_h);

        if cycle_type == OtherModeHCycleType::G_CYC_COPY {
            rdp.other_mode_h = (rdp.other_mode_h
                & !(3 << OtherModeH_Layout::G_MDSFT_TEXTFILT as u32))
                | (TextFilt::G_TF_POINT as u32);
            rdp.shader_config_changed = true;
        }

        // U10.2 coordinates
        let mut ulxf = ulx as f32 / (4.0 * (SCREEN_WIDTH / 2.0)) - 1.0;
        let ulyf = -(uly as f32 / (4.0 * (SCREEN_HEIGHT / 2.0))) + 1.0;
        let mut lrxf = lrx as f32 / (4.0 * (SCREEN_WIDTH / 2.0)) - 1.0;
        let lryf = -(lry as f32 / (4.0 * (SCREEN_HEIGHT / 2.0))) + 1.0;

        ulxf = rdp.adjust_x_for_viewport(ulxf);
        lrxf = rdp.adjust_x_for_viewport(lrxf);

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
            rdp.output_dimensions.width as u16,
            rdp.output_dimensions.height as u16,
        );
        let viewport_saved = rdp.viewport;
        let geometry_mode_saved = rsp.geometry_mode;

        rdp.viewport = default_viewport;
        rsp.geometry_mode = 0;
        rdp.shader_config_changed = true;

        rdp.draw_triangles(rsp, output, MAX_VERTICES, MAX_VERTICES + 1, MAX_VERTICES + 3, true);
        rdp.draw_triangles(rsp, output, MAX_VERTICES + 1, MAX_VERTICES + 2, MAX_VERTICES + 3, true);

        rsp.geometry_mode = geometry_mode_saved;
        rdp.viewport = viewport_saved;
        rdp.shader_config_changed = true;

        if cycle_type == OtherModeHCycleType::G_CYC_COPY {
            rdp.other_mode_h = saved_other_mode_h;
            rdp.shader_config_changed = true;
        }
    }

    pub fn gdp_texture_rectangle_raw(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
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
    ) -> GBIResult {
        let saved_combine_mode = rdp.combine;
        if (rdp.other_mode_h >> OtherModeH_Layout::G_MDSFT_CYCLETYPE as u32) & 0x03
            == OtherModeHCycleType::G_CYC_COPY as u32
        {
            // Per RDP Command Summary Set Tile's shift s and this dsdx should be set to 4 texels
            // Divide by 4 to get 1 instead
            dsdx >>= 2;

            // Color combiner is turned off in copy mode
            let rhs =
                (CCMUX::TEXEL0 as usize & 0b111) << 15 | (ACMUX::TEXEL0 as usize & 0b111) << 9;
            rdp.combine = CombineParams::decode(0, rhs);
            rdp.shader_config_changed = true;

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

        F3DEX2::draw_rectangle(rdp, rsp, output, ulx, uly, lrx, lry);
        rdp.combine = saved_combine_mode;
        rdp.shader_config_changed = true;

        GBIResult::Continue
    }

    pub fn gdp_texture_rectangle(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let opcode = w0 >> 24;

        let lrx = get_cmd(w0, 12, 12);
        let lry = get_cmd(w0, 0, 12);
        let tile = get_cmd(w1, 24, 3);
        let ulx = get_cmd(w1, 12, 12);
        let uly = get_cmd(w1, 0, 12);

        unsafe {
            *command = (*command).add(1);
        }
        let w1 = unsafe { (*(*command)).words.w1 };

        let uls = get_cmd(w1, 16, 16);
        let ult = get_cmd(w1, 0, 16);

        unsafe {
            *command = (*command).add(1);
        }
        let w1 = unsafe { (*(*command)).words.w1 };

        let dsdx = get_cmd(w1, 16, 16);
        let dtdy = get_cmd(w1, 0, 16);

        F3DEX2::gdp_texture_rectangle_raw(
            rdp,
            rsp,
            output,
            ulx as i32,
            uly as i32,
            lrx as i32,
            lry as i32,
            tile as u8,
            uls as i16,
            ult as i16,
            dsdx as i16,
            dtdy as i16,
            opcode == G_TEXRECTFLIP as usize,
        )
    }

    pub fn gdp_fill_rectangle_raw(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        ulx: i32,
        uly: i32,
        mut lrx: i32,
        mut lry: i32,
    ) -> GBIResult {
        if rdp.color_image == rdp.depth_image {
            // used to clear depth buffer, not necessary in modern pipelines
            return GBIResult::Continue;
        }

        let cycle_type = get_cycle_type_from_other_mode_h(rdp.other_mode_h);
        if cycle_type == OtherModeHCycleType::G_CYC_COPY
            || cycle_type == OtherModeHCycleType::G_CYC_FILL
        {
            // Per documentation one extra pixel is added in this modes to each edge
            lrx += 1 << 2;
            lry += 1 << 2;
        }

        for i in MAX_VERTICES..MAX_VERTICES + 4 {
            let v = &mut rsp.vertex_table[i];
            v.color = rdp.fill_color;
        }

        let saved_combine_mode = rdp.combine;
        let rhs = (CCMUX::SHADE as usize & 0b111) << 15 | (ACMUX::SHADE as usize & 0b111) << 9;
        rdp.combine = CombineParams::decode(0, rhs);
        rdp.shader_config_changed = true;
        F3DEX2::draw_rectangle(rdp, rsp, output, ulx, uly, lrx, lry);
        rdp.combine = saved_combine_mode;
        rdp.shader_config_changed = true;

        GBIResult::Continue
    }

    pub fn gdp_fill_rectangle(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let ulx = get_cmd(w1, 12, 12);
        let uly = get_cmd(w1, 0, 12);
        let lrx = get_cmd(w0, 12, 12);
        let lry = get_cmd(w0, 0, 12);

        F3DEX2::gdp_fill_rectangle_raw(
            rdp, rsp, output, ulx as i32, uly as i32, lrx as i32, lry as i32,
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_moveword() {
        // // NUM_LIGHT
        // let w0: usize = 3674341376;
        // let w1: usize = 24;

        // let mut rsp = RSP::new();
        // let mut rdp = RDP::new();

        // let dummy_gfx_context = GraphicsContext::new(Box::new(DummyGraphicsDevice {}));
        // F3DEX2::gsp_moveword(&mut rdp, &mut rsp, &dummy_gfx_context, w0, w1);
        // assert!(rsp.num_lights == 2);

        // // FOG
        // let w0: usize = 3674734592;
        // let w1: usize = 279638102;

        // let mut rsp = RSP::new();
        // let mut rdp = RDP::new();

        // let dummy_gfx_context = GraphicsContext::new(Box::new(DummyGraphicsDevice {}));
        // F3DEX2::gsp_moveword(&mut rdp, &mut rsp, &dummy_gfx_context, w0, w1);
        // assert!(rsp.fog_multiplier == 4266);
        // assert!(rsp.fog_offset == -4010);
    }
}
