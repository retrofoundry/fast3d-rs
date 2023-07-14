use crate::gbi::macros::gbi_command;
use crate::gbi::utils::get_cmd;
use crate::gbi::{GBICommandParams, GBICommandRegistry, GBIResult};
use fast3d_gbi::defines::{color_combiner::CombineParams, OpCode, WrapMode};

use crate::models::texture::TextureImageState;

use crate::rdp::SCREEN_HEIGHT;
use crate::rsp::RSP;

pub struct Common;
impl Common {
    pub fn setup(gbi: &mut GBICommandRegistry, _rsp: &mut RSP) {
        gbi.register(OpCode::NOOP.bits(), RDPNoOp);
        gbi.register(OpCode::TEXRECT.bits(), RDPTextureRectangle);
        gbi.register(OpCode::TEXRECTFLIP.bits(), RDPTextureRectangle);
        gbi.register(OpCode::FILLRECT.bits(), RDPFillRectangle);
        gbi.register(OpCode::SET_COLORIMG.bits(), RDPSetColorImage);
        gbi.register(OpCode::SET_DEPTHIMG.bits(), RDPSetDepthImage);
        gbi.register(OpCode::SET_TEXIMG.bits(), RDPSetTextureImage);
        gbi.register(OpCode::SET_COMBINE.bits(), RDPSetCombine);
        gbi.register(OpCode::SET_TILE.bits(), RDPSetTile);
        gbi.register(OpCode::SET_TILESIZE.bits(), RDPSetTileSize);
        gbi.register(OpCode::SET_ENVCOLOR.bits(), RDPSetEnvColor);
        gbi.register(OpCode::SET_PRIMCOLOR.bits(), RDPSetPrimColor);
        gbi.register(OpCode::SET_BLENDCOLOR.bits(), RDPSetBlendColor);
        gbi.register(OpCode::SET_FOGCOLOR.bits(), RDPSetFogColor);
        gbi.register(OpCode::SET_FILLCOLOR.bits(), RDPSetFillColor);
        // TODO: PRIM_DEPTH
        gbi.register(OpCode::SET_SCISSOR.bits(), RDPSetScissor);
        gbi.register(OpCode::SET_CONVERT.bits(), RDPSetConvert);
        gbi.register(OpCode::SET_KEYR.bits(), RDPSetKeyR);
        gbi.register(OpCode::SET_KEYGB.bits(), RDPSetKeyGB);
        gbi.register(OpCode::LOAD_TILE.bits(), RDPLoadTile);
        gbi.register(OpCode::LOAD_BLOCK.bits(), RDPLoadBlock);
        gbi.register(OpCode::LOAD_TLUT.bits(), RDPLoadTLUT);
        gbi.register(OpCode::RDPLOADSYNC.bits(), RDPLoadSync);
        gbi.register(OpCode::RDPPIPESYNC.bits(), RDPPipeSync);
        gbi.register(OpCode::RDPTILESYNC.bits(), RDPTileSync);
        gbi.register(OpCode::RDPFULLSYNC.bits(), RDPFullSync);
        gbi.register(OpCode::RDPSETOTHERMODE.bits(), RDPSetOtherMode);
    }
}

gbi_command!(RDPNoOp, |_| { GBIResult::Continue });
gbi_command!(RDPLoadSync, |_| { GBIResult::Continue });
gbi_command!(RDPPipeSync, |_| { GBIResult::Continue });
gbi_command!(RDPTileSync, |_| { GBIResult::Continue });
gbi_command!(RDPFullSync, |_| { GBIResult::Continue });

gbi_command!(RDPSetColorImage, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let _format = get_cmd(w0, 21, 3);
    let _size = get_cmd(w0, 19, 2);
    let _width = get_cmd(w0, 0, 12) + 1;

    params.rdp.color_image = params.rsp.get_segment(w1);
    GBIResult::Continue
});

gbi_command!(RDPSetDepthImage, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };

    params.rdp.depth_image = params.rsp.get_segment(w1);
    GBIResult::Continue
});

gbi_command!(RDPSetTextureImage, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let format = get_cmd(w0, 21, 3) as u8;
    let size = get_cmd(w0, 19, 2) as u8;
    let width = get_cmd(w0, 0, 12) as u16 + 1;
    let address = params.rsp.get_segment(w1);

    params.rdp.texture_image_state = TextureImageState {
        format,
        size,
        width,
        address,
    };

    GBIResult::Continue
});

gbi_command!(RDPSetCombine, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rdp.set_combine(CombineParams::decode(w0, w1));

    GBIResult::Continue
});

gbi_command!(RDPSetTile, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let format = get_cmd(w0, 21, 3) as u8;
    let size = get_cmd(w0, 19, 2) as u8;
    let line = get_cmd(w0, 9, 9) as u16;
    let tmem = get_cmd(w0, 0, 9) as u16;
    let tile = get_cmd(w1, 24, 3) as u8;
    let palette = get_cmd(w1, 20, 4) as u8;
    let cm_t: WrapMode = ((get_cmd(w1, 18, 2) & 0x3) as u8).into();
    let mask_t: u8 = get_cmd(w1, 14, 4) as u8;
    let shift_t: u8 = get_cmd(w1, 10, 4) as u8;
    let cm_s: WrapMode = ((get_cmd(w1, 8, 2) & 0x3) as u8).into();
    let mask_s: u8 = get_cmd(w1, 4, 4) as u8;
    let shift_s: u8 = get_cmd(w1, 0, 4) as u8;

    params.rdp.set_tile(
        tile, format, size, line, tmem, palette, cm_t, cm_s, mask_t, mask_s, shift_t, shift_s,
    );

    GBIResult::Continue
});

gbi_command!(RDPSetTileSize, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let tile = get_cmd(w1, 24, 3) as u8;
    let uls = get_cmd(w0, 12, 12) as u16;
    let ult = get_cmd(w0, 0, 12) as u16;
    let lrs = get_cmd(w1, 12, 12) as u16;
    let lrt = get_cmd(w1, 0, 12) as u16;

    params.rdp.set_tile_size(tile, ult, uls, lrt, lrs);

    GBIResult::Continue
});

gbi_command!(RDPLoadTile, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let tile = get_cmd(w1, 24, 3) as u8;
    let uls = get_cmd(w0, 12, 12) as u16;
    let ult = get_cmd(w0, 0, 12) as u16;
    let lrs = get_cmd(w1, 12, 12) as u16;
    let lrt = get_cmd(w1, 0, 12) as u16;

    params.rdp.load_tile(tile, ult, uls, lrt, lrs);

    GBIResult::Continue
});

gbi_command!(RDPLoadBlock, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let tile = get_cmd(w1, 24, 3) as u8;
    let uls = get_cmd(w0, 12, 12) as u16;
    let ult = get_cmd(w0, 0, 12) as u16;
    let texels = get_cmd(w1, 12, 12) as u16;
    let dxt = get_cmd(w1, 0, 12) as u16;

    params.rdp.load_block(tile, ult, uls, texels, dxt);

    GBIResult::Continue
});

gbi_command!(RDPLoadTLUT, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let tile = get_cmd(w1, 24, 3) as u8;
    let high_index = get_cmd(w1, 14, 10) as u16;

    params.rdp.load_tlut(tile, high_index);

    GBIResult::Continue
});

gbi_command!(RDPSetEnvColor, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rdp.set_env_color(w1);

    GBIResult::Continue
});

gbi_command!(RDPSetPrimColor, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let lod_frac = get_cmd(w0, 0, 8) as u8;
    let lod_min = get_cmd(w0, 8, 5) as u8;
    params.rdp.set_prim_color(lod_frac, lod_min, w1);

    GBIResult::Continue
});

gbi_command!(RDPSetBlendColor, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rdp.set_blend_color(w1);

    GBIResult::Continue
});

gbi_command!(RDPSetFogColor, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rdp.set_fog_color(w1);

    GBIResult::Continue
});

gbi_command!(RDPSetFillColor, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };
    params.rdp.set_fill_color(w1);

    GBIResult::Continue
});

gbi_command!(RDPSetOtherMode, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let high = get_cmd(w0, 0, 24);
    let low = w1;
    params.rdp.set_other_mode(high as u32, low as u32);

    GBIResult::Continue
});

gbi_command!(RDPSetScissor, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let _mode = get_cmd(w1, 24, 2);
    let ulx = get_cmd(w0, 12, 12);
    let uly = get_cmd(w0, 0, 12);
    let lrx = get_cmd(w1, 12, 12);
    let lry = get_cmd(w1, 0, 12);

    let x = ulx as f32 / 4.0 * params.rdp.scaled_x();
    let y = (SCREEN_HEIGHT - lry as f32 / 4.0) * params.rdp.scaled_y();
    let width = (lrx as f32 - ulx as f32) / 4.0 * params.rdp.scaled_x();
    let height = (lry as f32 - uly as f32) / 4.0 * params.rdp.scaled_y();

    params.rdp.scissor.x = x as u16;
    params.rdp.scissor.y = y as u16;
    params.rdp.scissor.width = width as u16;
    params.rdp.scissor.height = height as u16;

    params.rdp.shader_config_changed = true;
    GBIResult::Continue
});

gbi_command!(RDPSetConvert, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let k0 = get_cmd(w0, 13, 9);
    let k1 = get_cmd(w0, 4, 9);
    let k2 = (get_cmd(w0, 0, 4) << 5) | get_cmd(w1, 27, 5);
    let k3 = get_cmd(w1, 18, 9);
    let k4 = get_cmd(w1, 9, 9);
    let k5 = get_cmd(w1, 0, 9);

    params.rdp.set_convert(
        k0 as i32, k1 as i32, k2 as i32, k3 as i32, k4 as i32, k5 as i32,
    );

    GBIResult::Continue
});

gbi_command!(RDPSetKeyR, |params: &mut GBICommandParams| {
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let cr = get_cmd(w1, 8, 8);
    let sr = get_cmd(w1, 0, 8);
    let wr = get_cmd(w1, 16, 2);

    params.rdp.set_key_r(cr as u32, sr as u32, wr as u32);

    GBIResult::Continue
});

gbi_command!(RDPSetKeyGB, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let cg = get_cmd(w1, 24, 8);
    let sg = get_cmd(w1, 16, 8);
    let wg = get_cmd(w0, 12, 12);
    let cb = get_cmd(w1, 8, 8);
    let sb = get_cmd(w1, 0, 8);
    let wb = get_cmd(w0, 0, 12);

    params.rdp.set_key_gb(
        cg as u32, sg as u32, wg as u32, cb as u32, sb as u32, wb as u32,
    );

    GBIResult::Continue
});

gbi_command!(RDPTextureRectangle, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let opcode = (w0 >> 24) as u8;

    let lrx = get_cmd(w0, 12, 12);
    let lry = get_cmd(w0, 0, 12);
    let tile = get_cmd(w1, 24, 3);
    let ulx = get_cmd(w1, 12, 12);
    let uly = get_cmd(w1, 0, 12);

    unsafe {
        *params.command = (*params.command).add(1);
    }
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let uls = get_cmd(w1, 16, 16);
    let ult = get_cmd(w1, 0, 16);

    unsafe {
        *params.command = (*params.command).add(1);
    }
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let dsdx = get_cmd(w1, 16, 16);
    let dtdy = get_cmd(w1, 0, 16);

    params.rdp.draw_texture_rectangle(
        params.rsp,
        params.output,
        ulx as i32,
        uly as i32,
        lrx as i32,
        lry as i32,
        tile as u8,
        uls as i16,
        ult as i16,
        dsdx as i16,
        dtdy as i16,
        opcode == OpCode::TEXRECTFLIP.bits(),
    );

    GBIResult::Continue
});

gbi_command!(RDPFillRectangle, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let ulx = get_cmd(w1, 12, 12);
    let uly = get_cmd(w1, 0, 12);
    let lrx = get_cmd(w0, 12, 12);
    let lry = get_cmd(w0, 0, 12);

    params.rdp.fill_rect(
        params.rsp,
        params.output,
        ulx as i32,
        uly as i32,
        lrx as i32,
        lry as i32,
    );

    GBIResult::Continue
});
