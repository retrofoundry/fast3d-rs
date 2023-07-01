use std::cmp::max;

use super::defines::{Gfx, Viewport};
use super::utils::get_cmd;
use super::{
    super::{rdp::RDP, rsp::RSP},
    defines::G_MW,
};
use super::{GBICommandRegistry, GBIMicrocode, GBIResult};
use crate::gbi::{
    defines::G_RDPSETOTHERMODE,
    f3d::{F3DEndDL, F3DSpNoOp, F3DSubDL},
    macros::gbi_command,
    GBICommand, GBICommandParams,
};
use crate::output::RCPOutput;
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

impl GBIMicrocode for F3DEX2 {
    fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
        gbi.register(Self::G_MTX as usize, F3DEX2Matrix);
        gbi.register(Self::G_POPMTX as usize, F3DEX2PopMatrix);
        gbi.register(Self::G_MOVEMEM as usize, F3DEX2MoveMem);
        gbi.register(Self::G_MOVEWORD as usize, F3DEX2MoveWord);
        gbi.register(Self::G_TEXTURE as usize, F3DEX2Texture);
        gbi.register(Self::G_VTX as usize, F3DEX2Vertex);
        gbi.register(Self::G_DL as usize, F3DSubDL);
        gbi.register(Self::G_GEOMETRYMODE as usize, F3DEX2GeometryMode);
        gbi.register(Self::G_TRI1 as usize, F3DEX2Tri1);
        gbi.register(Self::G_TRI2 as usize, F3DEX2Tri2);
        gbi.register(Self::G_ENDDL as usize, F3DEndDL);
        gbi.register(Self::G_SPNOOP as usize, F3DSpNoOp);
        gbi.register(Self::G_SETOTHERMODE_L as usize, F3DEX2SetOtherModeL);
        gbi.register(Self::G_SETOTHERMODE_H as usize, F3DEX2SetOtherModeH);
        gbi.register(G_RDPSETOTHERMODE as usize, F3DEX2SetOtherMode);

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

gbi_command!(F3DEX2Matrix, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let mtx_params = get_cmd(w0, 0, 8) as u8 ^ params.rsp.constants.G_MTX_PUSH;
    params.rsp.matrix(w1, mtx_params);

    GBIResult::Continue
});

gbi_command!(F3DEX2PopMatrix, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rsp.pop_matrix(w1 >> 6);

    GBIResult::Continue
});

gbi_command!(F3DEX2MoveMem, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let index: u8 = get_cmd(w0, 0, 8) as u8;
    let offset = get_cmd(w0, 8, 8) * 8;
    let data = params.rsp.from_segmented(w1);

    match index {
        index if index == F3DEX2::G_MV_VIEWPORT => {
            let viewport_ptr = data as *const Viewport;
            let viewport = unsafe { &*viewport_ptr };
            params.rdp.calculate_and_set_viewport(*viewport);
        }
        index if index == F3DEX2::G_MV_MATRIX => {
            panic!("Unimplemented move matrix");
            // unsafe { *command = (*command).add(1) };
        }
        index if index == F3DEX2::G_MV_LIGHT => {
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

gbi_command!(F3DEX2MoveWord, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let m_type = get_cmd(w0, 16, 8) as u8;

    match m_type {
        m_type if m_type == G_MW::FORCEMTX => {
            params.rsp.modelview_projection_matrix_changed = w1 == 0
        }
        m_type if m_type == G_MW::NUMLIGHT => params.rsp.set_num_lights(w1 as u8 / 24),
        m_type if m_type == G_MW::CLIP => {
            params.rsp.set_clip_ratio(w1);
        }
        m_type if m_type == G_MW::SEGMENT => {
            let segment = get_cmd(w0, 2, 4);
            params.rsp.set_segment(segment, w1 & 0x00FFFFFF)
        }
        m_type if m_type == G_MW::FOG => {
            let multiplier = get_cmd(w1, 16, 16) as i16;
            let offset = get_cmd(w1, 0, 16) as i16;
            params.rsp.set_fog(multiplier, offset);
        }
        m_type if m_type == G_MW::LIGHTCOL => {
            let index = get_cmd(w0, 0, 16) / 24;
            params.rsp.set_light_color(index, w1 as u32);
        }
        m_type if m_type == G_MW::PERSPNORM => {
            params.rsp.set_persp_norm(w1);
        }
        // TODO: G_MW_MATRIX
        _ => {
            // panic!("Unknown moveword type: {}", m_type);
        }
    }

    GBIResult::Continue
});

gbi_command!(F3DEX2Texture, |params: &mut GBICommandParams| {
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

gbi_command!(F3DEX2Vertex, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let vertex_count = get_cmd(w0, 12, 8);
    let write_index = get_cmd(w0, 1, 7) - get_cmd(w0, 12, 8);
    params
        .rsp
        .set_vertex(params.rdp, params.output, w1, vertex_count, write_index);

    GBIResult::Continue
});

gbi_command!(F3DEX2GeometryMode, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let clear_bits = get_cmd(w0, 0, 24) as u32;
    let set_bits = w1 as u32;
    params
        .rsp
        .update_geometry_mode(params.rdp, clear_bits, set_bits);

    GBIResult::Continue
});

gbi_command!(F3DEX2Tri1, |params: &mut GBICommandParams| {
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

gbi_command!(F3DEX2Tri2, |params: &mut GBICommandParams| {
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

gbi_command!(F3DEX2SetOtherModeL, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let size = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - size);
    params
        .rsp
        .set_other_mode_l(params.rdp, size, offset, w1 as u32);

    GBIResult::Continue
});

gbi_command!(F3DEX2SetOtherModeH, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let size = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - size);
    params
        .rsp
        .set_other_mode_h(params.rdp, size, offset, w1 as u32);

    GBIResult::Continue
});

gbi_command!(F3DEX2SetOtherMode, |params: &mut GBICommandParams| {
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
    use crate::gbi::f3dex2::F3DEX2MoveWord;
    use crate::gbi::{GBICommand, GBICommandParams};
    use crate::output::RCPOutput;
    use crate::rdp::RDP;
    use crate::rsp::RSP;

    #[test]
    fn test_moveword() {
        // NUM_LIGHT
        let w0: usize = 3674341376;
        let w1: usize = 24;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RCPOutput::default();

        let mut command: *mut Gfx = Box::into_raw(Box::new(Gfx {
            words: GWords { w0, w1 },
        }));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        F3DEX2MoveWord {}.process(&mut params);
        assert_eq!(rsp.num_lights, 1);

        // FOG
        let w0: usize = 3674734592;
        let w1: usize = 279638102;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RCPOutput::default();

        let mut command: *mut Gfx = Box::into_raw(Box::new(Gfx {
            words: GWords { w0, w1 },
        }));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        F3DEX2MoveWord {}.process(&mut params);
        assert_eq!(rsp.fog_multiplier, 4266);
        assert_eq!(rsp.fog_offset, -4010);
    }
}
