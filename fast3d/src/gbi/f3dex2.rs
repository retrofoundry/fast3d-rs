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

bitflags! {
    pub struct MatrixMode: u8 {
        const MODELVIEW = 0x00000000;
        const PROJECTION = 0x00000004;
    }
}

bitflags! {
    pub struct MatrixOperation: u8 {
        const NOPUSH = 0x00000000;
        const PUSH = 0x00000001;
        const MUL = 0x00000000;
        const LOAD = 0x00000002;
    }
}

bitflags! {
    pub struct MoveWordIndex: u8 {
        const FORCEMTX = 0x0C;
    }
}

bitflags! {
    pub struct MoveWordOffset: u8 {
        const A_LIGHT_2 = 0x18;
        const B_LIGHT_2 = 0x1c;
        const A_LIGHT_3 = 0x30;
        const B_LIGHT_3 = 0x34;
        const A_LIGHT_4 = 0x48;
        const B_LIGHT_4 = 0x4c;
        const A_LIGHT_5 = 0x60;
        const B_LIGHT_5 = 0x64;
        const A_LIGHT_6 = 0x78;
        const B_LIGHT_6 = 0x7c;
        const A_LIGHT_7 = 0x90;
        const B_LIGHT_7 = 0x94;
        const A_LIGHT_8 = 0xa8;
        const B_LIGHT_8 = 0xac;
    }
}

bitflags! {
    pub struct MoveMemoryIndex: u8 {
        const MMTX = 2;
        const PMTX = 6;
        const VIEWPORT = 8;
        const LIGHT = 10;
        const POINT = 12;
        const MATRIX = 14;
    }
}

bitflags! {
    pub struct MoveMemoryOffset: u8 {
        const LOOKATX = 0; // (0 * 24);
        const LOOKATY = 24;
        const L0 = 2 * 24;
        const L1 = 3 * 24;
        const L2 = 4 * 24;
        const L3 = 5 * 24;
        const L4 = 6 * 24;
        const L5 = 7 * 24;
        const L6 = 8 * 24;
        const L7 = 9 * 24;
    }
}

bitflags! {
    pub struct OpCode: u8 {
        const NOOP = 0x00;
        // RDP
        const SETOTHERMODE_H = 0xe3;
        const SETOTHERMODE_L = 0xe2;
        const RDPHALF_1 = 0xe1;
        const RDPHALF_2 = 0xf1;

        const SPNOOP = 0xe0;

        // RSP
        const ENDDL = 0xdf;
        const DL = 0xde;
        const LOAD_UCODE = 0xdd;
        const MOVEMEM = 0xdc;
        const MOVEWORD = 0xdb;
        const MTX = 0xda;
        const GEOMETRYMODE = 0xd9;
        const POPMTX = 0xd8;
        const TEXTURE = 0xd7;

        // DMA
        const VTX = 0x01;
        const MODIFYVTX = 0x02;
        const CULLDL = 0x03;
        const BRANCH_Z = 0x04;
        const TRI1 = 0x05;
        const TRI2 = 0x06;
        const QUAD = 0x07;
        const LINE3D = 0x08;
        const DMA_IO = 0xD6;

        const SPECIAL_1 = 0xD5;
    }
}

pub fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
    gbi.register(OpCode::MTX.bits, Matrix);
    gbi.register(OpCode::POPMTX.bits, PopMatrix);
    gbi.register(OpCode::MOVEMEM.bits, MoveMem);
    gbi.register(OpCode::MOVEWORD.bits, MoveWord);
    gbi.register(OpCode::TEXTURE.bits, Texture);
    gbi.register(OpCode::VTX.bits, Vertex);
    gbi.register(OpCode::DL.bits, f3d::SubDL);
    gbi.register(OpCode::GEOMETRYMODE.bits, GeometryMode);
    gbi.register(OpCode::TRI1.bits, Tri1);
    gbi.register(OpCode::TRI2.bits, Tri2);
    gbi.register(OpCode::ENDDL.bits, f3d::EndDL);
    gbi.register(OpCode::SPNOOP.bits, f3d::SpNoOp);
    gbi.register(OpCode::SETOTHERMODE_L.bits, SetOtherModeL);
    gbi.register(OpCode::SETOTHERMODE_H.bits, SetOtherModeH);
    gbi.register(defines::OpCode::RDPSETOTHERMODE.bits(), SetOtherMode);

    rsp.setup_constants(RSPConstants {
        G_MTX_PUSH: MatrixOperation::PUSH.bits,
        G_MTX_LOAD: MatrixOperation::LOAD.bits,
        G_MTX_PROJECTION: MatrixMode::PROJECTION.bits,

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
        index if index == MoveMemoryIndex::VIEWPORT.bits => {
            let viewport_ptr = data as *const Viewport;
            let viewport = unsafe { &*viewport_ptr };
            params.rdp.calculate_and_set_viewport(*viewport);
        }
        index if index == MoveMemoryIndex::MATRIX.bits => {
            panic!("Unimplemented move matrix");
            // unsafe { *command = (*command).add(1) };
        }
        index if index == MoveMemoryIndex::LIGHT.bits => {
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
        m_type if m_type == MoveWordIndex::FORCEMTX.bits => {
            params.rsp.modelview_projection_matrix_changed = w1 == 0
        }
        m_type if m_type == defines::MoveWordIndex::NUMLIGHT.bits() => {
            params.rsp.set_num_lights(w1 as u8 / 24)
        }
        m_type if m_type == defines::MoveWordIndex::CLIP.bits() => {
            params.rsp.set_clip_ratio(w1);
        }
        m_type if m_type == defines::MoveWordIndex::SEGMENT.bits() => {
            let segment = get_cmd(w0, 2, 4);
            params.rsp.set_segment(segment, w1 & 0x00FFFFFF)
        }
        m_type if m_type == defines::MoveWordIndex::FOG.bits() => {
            let multiplier = get_cmd(w1, 16, 16) as i16;
            let offset = get_cmd(w1, 0, 16) as i16;
            params.rsp.set_fog(multiplier, offset);
        }
        m_type if m_type == defines::MoveWordIndex::LIGHTCOL.bits() => {
            let index = get_cmd(w0, 0, 16) / 24;
            params.rsp.set_light_color(index, w1 as u32);
        }
        m_type if m_type == defines::MoveWordIndex::PERSPNORM.bits() => {
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
