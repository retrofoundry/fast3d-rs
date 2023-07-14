use crate::gbi::macros::gbi_command;
use crate::gbi::{f3d, GBICommandParams, GBICommandRegistry, GBIResult};
use fast3d_gbi::defines::Viewport;
use fast3d_gbi::{
    defines::f3dex2::{
        GeometryModes, MatrixMode, MatrixOperation, MoveMemoryIndex, MoveWordIndex, OpCode,
    },
    defines::{MoveWordIndex as SharedMoveWordIndex, OpCode as SharedOpCode},
};
use std::cmp::max;

use super::utils::get_cmd;

use crate::rsp::{RSPConstants, RSP};

pub fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
    gbi.register(OpCode::MTX.bits(), Matrix);
    gbi.register(OpCode::POPMTX.bits(), PopMatrix);
    gbi.register(OpCode::MOVEMEM.bits(), MoveMem);
    gbi.register(OpCode::MOVEWORD.bits(), MoveWord);
    gbi.register(OpCode::TEXTURE.bits(), Texture);
    gbi.register(OpCode::VTX.bits(), Vertex);
    gbi.register(OpCode::DL.bits(), f3d::SubDL);
    gbi.register(OpCode::GEOMETRYMODE.bits(), GeometryMode);
    gbi.register(OpCode::TRI1.bits(), Tri1);
    gbi.register(OpCode::TRI2.bits(), Tri2);
    gbi.register(OpCode::ENDDL.bits(), f3d::EndDL);
    gbi.register(OpCode::SPNOOP.bits(), f3d::SpNoOp);
    gbi.register(OpCode::SETOTHERMODE_L.bits(), SetOtherModeL);
    gbi.register(OpCode::SETOTHERMODE_H.bits(), SetOtherModeH);
    gbi.register(SharedOpCode::RDPSETOTHERMODE.bits(), SetOtherMode);

    rsp.setup_constants(RSPConstants {
        mtx_push_val: MatrixOperation::PUSH.bits(),
        mtx_load_val: MatrixOperation::LOAD.bits(),
        mtx_projection_val: MatrixMode::PROJECTION.bits(),

        geomode_shading_smooth_val: GeometryModes::SHADING_SMOOTH.bits(),
        geomode_cull_front_val: GeometryModes::CULL_FRONT.bits(),
        geomode_cull_back_val: GeometryModes::CULL_BACK.bits(),
        geomode_cull_both_val: GeometryModes::CULL_BOTH.bits(),
    })
}

gbi_command!(Matrix, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let mtx_params = get_cmd(w0, 0, 8) as u8 ^ MatrixOperation::PUSH.bits();
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
    let data = params.rsp.get_segment(w1);

    match index {
        index if index == MoveMemoryIndex::VIEWPORT.bits() => {
            let viewport_ptr = data as *const Viewport;
            let viewport = unsafe { &*viewport_ptr };
            params.rdp.calculate_and_set_viewport(*viewport);
        }
        index if index == MoveMemoryIndex::MATRIX.bits() => {
            panic!("Unimplemented move matrix");
            // unsafe { *command = (*command).add(1) };
        }
        index if index == MoveMemoryIndex::LIGHT.bits() => {
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
        m_type if m_type == MoveWordIndex::FORCEMTX.bits() => {
            params.rsp.modelview_projection_matrix_changed = w1 == 0
        }
        m_type if m_type == SharedMoveWordIndex::NUMLIGHT.bits() => {
            params.rsp.set_num_lights(w1 as u8 / 24)
        }
        m_type if m_type == SharedMoveWordIndex::CLIP.bits() => {
            params.rsp.set_clip_ratio(w1);
        }
        m_type if m_type == SharedMoveWordIndex::SEGMENT.bits() => {
            let segment = get_cmd(w0, 2, 4);
            params.rsp.set_segment(segment, w1 & 0x00FFFFFF)
        }
        m_type if m_type == SharedMoveWordIndex::FOG.bits() => {
            let multiplier = get_cmd(w1, 16, 16) as i16;
            let offset = get_cmd(w1, 0, 16) as i16;
            params.rsp.set_fog(multiplier, offset);
        }
        m_type if m_type == SharedMoveWordIndex::LIGHTCOL.bits() => {
            let index = get_cmd(w0, 0, 16) / 24;
            params.rsp.set_light_color(index, w1 as u32);
        }
        m_type if m_type == SharedMoveWordIndex::PERSPNORM.bits() => {
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
    let write_index = get_cmd(w0, 1, 7) - vertex_count;
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

    let length = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - length);
    params
        .rsp
        .set_other_mode_l(params.rdp, length, offset, w1 as u32);

    GBIResult::Continue
});

gbi_command!(SetOtherModeH, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let length = get_cmd(w0, 0, 8) + 1;
    let offset = max(0, 32 - get_cmd(w0, 8, 8) - length);
    params
        .rsp
        .set_other_mode_h(params.rdp, length, offset, w1 as u32);

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
    use crate::gbi::f3dex2::MoveWord;
    use crate::gbi::GBICommandParams;
    use crate::output::RenderData;
    use crate::rdp::RDP;
    use crate::rsp::RSP;
    use fast3d_gbi::defines::GfxCommand;

    #[test]
    fn test_moveword() {
        // NUM_LIGHT
        let w0: usize = 3674341376;
        let w1: usize = 24;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RenderData::default();

        let mut command: *mut GfxCommand = Box::into_raw(Box::new(GfxCommand::new(w0, w1)));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        MoveWord(&mut params);
        assert_eq!(rsp.num_lights, 1);

        // FOG
        let w0: usize = 3674734592;
        let w1: usize = 279638102;

        let mut rsp = RSP::default();
        let mut rdp = RDP::default();
        let mut output = RenderData::default();

        let mut command: *mut GfxCommand = Box::into_raw(Box::new(GfxCommand::new(w0, w1)));

        let mut params = GBICommandParams {
            rdp: &mut rdp,
            rsp: &mut rsp,
            output: &mut output,
            command: &mut command,
        };

        MoveWord(&mut params);
        assert_eq!(rsp.fog_multiplier, 4266);
        assert_eq!(rsp.fog_offset, -4010);
    }
}
