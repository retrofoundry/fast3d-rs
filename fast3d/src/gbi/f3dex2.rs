use bitflags::bitflags;
use std::cmp::max;

use super::super::rsp::RSP;
use super::defines::{self, Viewport};
use super::utils::get_cmd;
use super::{f3d, GBICommandRegistry, GBIResult};
use crate::gbi::{macros::gbi_command, GBICommand, GBICommandParams};

use crate::rsp::RSPConstants;

bitflags! {
    pub struct GeometryModes: u32 {
        const TEXTURE_ENABLE      = 0x00000002;
        const SHADING_SMOOTH      = 0x00000200;
        const CULL_FRONT          = 0x00001000;
        const CULL_BACK           = 0x00002000;
        const CULL_BOTH           = Self::CULL_FRONT.bits | Self::CULL_BACK.bits;
    }
}

#[allow(dead_code)]
pub mod g {
    pub mod mtx {
        pub const NOPUSH: u8 = 0x00;
        pub const PUSH: u8 = 0x01;
        pub const MUL: u8 = 0x00;
        pub const LOAD: u8 = 0x02;
        pub const MODELVIEW: u8 = 0x00;
        pub const PROJECTION: u8 = 0x04;
    }

    /*
     * MOVEWORD indices
     *
     * Each of these indexes an entry in a dmem table
     * which points to a word in dmem in dmem where
     * an immediate word will be stored.
     *
     */
    pub mod mwo {
        pub const aLIGHT_2: u8 = 0x18;
        pub const bLIGHT_2: u8 = 0x1c;
        pub const aLIGHT_3: u8 = 0x30;
        pub const bLIGHT_3: u8 = 0x34;
        pub const aLIGHT_4: u8 = 0x48;
        pub const bLIGHT_4: u8 = 0x4c;
        pub const aLIGHT_5: u8 = 0x60;
        pub const bLIGHT_5: u8 = 0x64;
        pub const aLIGHT_6: u8 = 0x78;
        pub const bLIGHT_6: u8 = 0x7c;
        pub const aLIGHT_7: u8 = 0x90;
        pub const bLIGHT_7: u8 = 0x94;
        pub const aLIGHT_8: u8 = 0xa8;
        pub const bLIGHT_8: u8 = 0xac;
    }
    pub const NOOP: u8 = 0x00;
    // RDP
    pub const SETOTHERMODE_H: u8 = 0xe3;
    pub const SETOTHERMODE_L: u8 = 0xe2;
    pub const RDPHALF_1: u8 = 0xe1;
    pub const RDPHALF_2: u8 = 0xf1;

    pub const SPNOOP: u8 = 0xe0;

    // RSP
    pub const ENDDL: u8 = 0xdf;
    pub const DL: u8 = 0xde;
    pub const LOAD_UCODE: u8 = 0xdd;
    pub const MOVEMEM: u8 = 0xdc;
    pub const MOVEWORD: u8 = 0xdb;
    pub const MTX: u8 = 0xda;
    pub const GEOMETRYMODE: u8 = 0xd9;
    pub const POPMTX: u8 = 0xd8;
    pub const TEXTURE: u8 = 0xd7;

    // DMA
    pub const VTX: u8 = 0x01;
    pub const MODIFYVTX: u8 = 0x02;
    pub const CULLDL: u8 = 0x03;
    pub const BRANCH_Z: u8 = 0x04;
    pub const TRI1: u8 = 0x05;
    pub const TRI2: u8 = 0x06;
    pub const QUAD: u8 = 0x07;
    pub const LINE3D: u8 = 0x08;
    pub const DMA_IO: u8 = 0xD6;

    pub const SPECIAL_1: u8 = 0xD5;

    /*
     * MOVEMEM indices
     *
     * Each of these indexes an entry in a dmem table
     * which points to a 1-4 word block of dmem in
     * which to store a 1-4 word DMA.
     *
     */
    pub mod mv {
        pub const MMTX: u8 = 2;
        pub const PMTX: u8 = 6;
        pub const VIEWPORT: u8 = 8;
        pub const LIGHT: u8 = 10;
        pub const POINT: u8 = 12;
        pub const MATRIX: u8 = 14;
    }

    pub mod mvo {
        pub const LOOKATX: u8 = 0; // (0 * 24);
        pub const LOOKATY: u8 = 24;
        pub const L0: u8 = 2 * 24;
        pub const L1: u8 = 3 * 24;
        pub const L2: u8 = 4 * 24;
        pub const L3: u8 = 5 * 24;
        pub const L4: u8 = 6 * 24;
        pub const L5: u8 = 7 * 24;
        pub const L6: u8 = 8 * 24;
        pub const L7: u8 = 9 * 24;
    }
}

pub fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
    gbi.register(g::MTX as usize, Matrix);
    gbi.register(g::POPMTX as usize, PopMatrix);
    gbi.register(g::MOVEMEM as usize, MoveMem);
    gbi.register(g::MOVEWORD as usize, MoveWord);
    gbi.register(g::TEXTURE as usize, Texture);
    gbi.register(g::VTX as usize, Vertex);
    gbi.register(g::DL as usize, f3d::SubDL);
    gbi.register(g::GEOMETRYMODE as usize, GeometryMode);
    gbi.register(g::TRI1 as usize, Tri1);
    gbi.register(g::TRI2 as usize, Tri2);
    gbi.register(g::ENDDL as usize, f3d::EndDL);
    gbi.register(g::SPNOOP as usize, f3d::SpNoOp);
    gbi.register(g::SETOTHERMODE_L as usize, SetOtherModeL);
    gbi.register(g::SETOTHERMODE_H as usize, SetOtherModeH);
    gbi.register(defines::g::RDPSETOTHERMODE as usize, SetOtherMode);

    rsp.setup_constants(RSPConstants {
        G_MTX_PUSH: g::mtx::PUSH,
        G_MTX_LOAD: g::mtx::LOAD,
        G_MTX_PROJECTION: g::mtx::PROJECTION,

        G_SHADING_SMOOTH: GeometryModes::SHADING_SMOOTH.bits,
        G_CULL_FRONT: GeometryModes::CULL_FRONT.bits,
        G_CULL_BACK: GeometryModes::CULL_BACK.bits,
        G_CULL_BOTH: GeometryModes::CULL_BOTH.bits,
    })
}

gbi_command!(Matrix, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let mtx_params = get_cmd(w0, 0, 8) as u8 ^ params.rsp.constants.G_MTX_PUSH;
    params.rsp.matrix(w1, mtx_params);

    GBIResult::Continue
});

gbi_command!(PopMatrix, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rsp.pop_matrix(w1 >> 6);

    GBIResult::Continue
});

gbi_command!(MoveMem, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let index: u8 = get_cmd(w0, 0, 8) as u8;
    let offset = get_cmd(w0, 8, 8) * 8;
    let data = params.rsp.from_segmented(w1);

    match index {
        index if index == g::mv::VIEWPORT => {
            let viewport_ptr = data as *const Viewport;
            let viewport = unsafe { &*viewport_ptr };
            params.rdp.calculate_and_set_viewport(*viewport);
        }
        index if index == g::mv::MATRIX => {
            panic!("Unimplemented move matrix");
            // unsafe { *command = (*command).add(1) };
        }
        index if index == g::mv::LIGHT => {
            let index = offset / 24;
            if index >= 2 {
                params.rsp.set_light(index - 2, w1);
            } else {
                params.rsp.set_look_at(index, w1);
            }
        }
        _ => panic!("Unimplemented move_mem command"),
    }

    GBIResult::Continue
});

gbi_command!(MoveWord, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let m_type = get_cmd(w0, 16, 8) as u8;

    match m_type {
        m_type if m_type == defines::g::mw::FORCEMTX => {
            params.rsp.modelview_projection_matrix_changed = w1 == 0
        }
        m_type if m_type == defines::g::mw::NUMLIGHT => params.rsp.set_num_lights(w1 as u8 / 24),
        m_type if m_type == defines::g::mw::CLIP => {
            params.rsp.set_clip_ratio(w1);
        }
        m_type if m_type == defines::g::mw::SEGMENT => {
            let segment = get_cmd(w0, 2, 4);
            params.rsp.set_segment(segment, w1 & 0x00FFFFFF)
        }
        m_type if m_type == defines::g::mw::FOG => {
            let multiplier = get_cmd(w1, 16, 16) as i16;
            let offset = get_cmd(w1, 0, 16) as i16;
            params.rsp.set_fog(multiplier, offset);
        }
        m_type if m_type == defines::g::mw::LIGHTCOL => {
            let index = get_cmd(w0, 0, 16) / 24;
            params.rsp.set_light_color(index, w1 as u32);
        }
        m_type if m_type == defines::g::mw::PERSPNORM => {
            params.rsp.set_persp_norm(w1);
        }
        // TODO: G_MW_MATRIX
        _ => {
            // panic!("Unknown moveword type: {}", m_type);
        }
    }

    GBIResult::Continue
});

gbi_command!(Texture, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let scale_s = get_cmd(w1, 16, 16) as u16;
    let scale_t = get_cmd(w1, 0, 16) as u16;
    let level = get_cmd(w0, 11, 3) as u8;
    let tile = get_cmd(w0, 8, 3) as u8;
    let on = get_cmd(w0, 1, 7) as u8;

    params
        .rsp
        .set_texture(params.rdp, tile, level, on, scale_s, scale_t);

    GBIResult::Continue
});

gbi_command!(Vertex, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let vertex_count = get_cmd(w0, 12, 8);
    let write_index = get_cmd(w0, 1, 7) - get_cmd(w0, 12, 8);
    params
        .rsp
        .set_vertex(params.rdp, params.output, w1, vertex_count, write_index);

    GBIResult::Continue
});

gbi_command!(GeometryMode, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let clear_bits = get_cmd(w0, 0, 24) as u32;
    let set_bits = w1 as u32;
    params
        .rsp
        .update_geometry_mode(params.rdp, clear_bits, set_bits);

    GBIResult::Continue
});

gbi_command!(Tri1, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };

    let vertex_id1 = get_cmd(w0, 16, 8) / 2;
    let vertex_id2 = get_cmd(w0, 8, 8) / 2;
    let vertex_id3 = get_cmd(w0, 0, 8) / 2;

    params.rdp.draw_triangles(
        params.rsp,
        params.output,
        vertex_id1,
        vertex_id2,
        vertex_id3,
        false,
    );
    GBIResult::Continue
});

gbi_command!(Tri2, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let vertex_id1 = get_cmd(w0, 16, 8) / 2;
    let vertex_id2 = get_cmd(w0, 8, 8) / 2;
    let vertex_id3 = get_cmd(w0, 0, 8) / 2;
    params.rdp.draw_triangles(
        params.rsp,
        params.output,
        vertex_id1,
        vertex_id2,
        vertex_id3,
        false,
    );

    let vertex_id1 = get_cmd(w1, 16, 8) / 2;
    let vertex_id2 = get_cmd(w1, 8, 8) / 2;
    let vertex_id3 = get_cmd(w1, 0, 8) / 2;
    params.rdp.draw_triangles(
        params.rsp,
        params.output,
        vertex_id1,
        vertex_id2,
        vertex_id3,
        false,
    );

    GBIResult::Continue
});

gbi_command!(SetOtherModeL, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let size = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - size);
    params
        .rsp
        .set_other_mode_l(params.rdp, size, offset, w1 as u32);

    GBIResult::Continue
});

gbi_command!(SetOtherModeH, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let size = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - size);
    params
        .rsp
        .set_other_mode_h(params.rdp, size, offset, w1 as u32);

    GBIResult::Continue
});

gbi_command!(SetOtherMode, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let high = get_cmd(w0, 0, 24);
    let low = w1;
    params
        .rsp
        .set_other_mode(params.rdp, high as u32, low as u32);

    GBIResult::Continue
});

#[cfg(test)]
mod tests {
    use crate::gbi::defines::{GWords, Gfx};
    use crate::gbi::f3dex2::MoveWord;
    use crate::gbi::{GBICommand, GBICommandParams};
    use crate::output::RCPOutputCollector;
    use crate::rdp::RDP;
    use crate::rsp::RSP;

    #[test]
    fn test_moveword() {
        // NUM_LIGHT
        let w0: usize = 3674341376;
        let w1: usize = 24;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RCPOutputCollector::default();

        let mut command: *mut Gfx = Box::into_raw(Box::new(Gfx {
            words: GWords { w0, w1 },
        }));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        MoveWord {}.process(&mut params);
        assert_eq!(rsp.num_lights, 1);

        // FOG
        let w0: usize = 3674734592;
        let w1: usize = 279638102;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RCPOutputCollector::default();

        let mut command: *mut Gfx = Box::into_raw(Box::new(Gfx {
            words: GWords { w0, w1 },
        }));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        MoveWord {}.process(&mut params);
        assert_eq!(rsp.fog_multiplier, 4266);
        assert_eq!(rsp.fog_offset, -4010);
    }
}
