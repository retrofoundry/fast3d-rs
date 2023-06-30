use crate::gbi::defines::{
    Gfx, G_FILLRECT, G_LOAD, G_NOOP, G_RDPFULLSYNC, G_RDPLOADSYNC, G_RDPPIPESYNC,
    G_RDPSETOTHERMODE, G_RDPTILESYNC, G_SET, G_TEXRECT, G_TEXRECTFLIP,
};

use crate::gbi::utils::get_cmd;
use crate::gbi::{GBICommand, GBICommandRegistry, GBIMicrocode, GBIResult};

use crate::models::color_combiner::CombineParams;
use crate::models::texture::TextureImageState;
use crate::output::RCPOutput;
use crate::rdp::{RDP, SCREEN_HEIGHT};
use crate::rsp::RSP;

pub struct Common;
impl GBIMicrocode for Common {
    fn setup(gbi: &mut GBICommandRegistry, _rsp: &mut RSP) {
        gbi.register(G_NOOP as usize, RDPNoOp);
        gbi.register(G_SET::COLORIMG as usize, RDPSetColorImage);
        gbi.register(G_SET::DEPTHIMG as usize, RDPSetDepthImage);
        gbi.register(G_SET::TEXIMG as usize, RDPSetTextureImage);
        gbi.register(G_SET::COMBINE as usize, RDPSetCombine);
        gbi.register(G_SET::TILE as usize, RDPSetTile);
        gbi.register(G_SET::TILESIZE as usize, RDPSetTileSize);
        gbi.register(G_LOAD::TILE as usize, RDPLoadTile);
        gbi.register(G_LOAD::BLOCK as usize, RDPLoadBlock);
        gbi.register(G_LOAD::TLUT as usize, RDPLoadTLUT);
        gbi.register(G_SET::ENVCOLOR as usize, RDPSetEnvColor);
        gbi.register(G_SET::PRIMCOLOR as usize, RDPSetPrimColor);
        gbi.register(G_SET::BLENDCOLOR as usize, RDPSetBlendColor);
        gbi.register(G_SET::FOGCOLOR as usize, RDPSetFogColor);
        gbi.register(G_SET::FILLCOLOR as usize, RDPSetFillColor);
        gbi.register(G_RDPSETOTHERMODE as usize, RDPSetOtherMode);
        // TODO: PRIM_DEPTH
        gbi.register(G_SET::SCISSOR as usize, RDPSetScissor);
        gbi.register(G_SET::CONVERT as usize, RDPSetConvert);
        gbi.register(G_SET::KEYR as usize, RDPSetKeyR);
        gbi.register(G_SET::KEYGB as usize, RDPSetKeyGB);
        gbi.register(G_TEXRECT as usize, RDPTextureRectangle);
        gbi.register(G_TEXRECTFLIP as usize, RDPTextureRectangle);
        gbi.register(G_FILLRECT as usize, RDPFillRectangle);
        gbi.register(G_RDPLOADSYNC as usize, RDPLoadSync);
        gbi.register(G_RDPPIPESYNC as usize, RDPPipeSync);
        gbi.register(G_RDPTILESYNC as usize, RDPTileSync);
        gbi.register(G_RDPFULLSYNC as usize, RDPFullSync);
    }
}

struct RDPNoOp;
impl GBICommand for RDPNoOp {
    fn process(
        &self,
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }
}

struct RDPLoadSync;
impl GBICommand for RDPLoadSync {
    fn process(
        &self,
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }
}

struct RDPPipeSync;
impl GBICommand for RDPPipeSync {
    fn process(
        &self,
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }
}

struct RDPTileSync;
impl GBICommand for RDPTileSync {
    fn process(
        &self,
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }
}

struct RDPFullSync;
impl GBICommand for RDPFullSync {
    fn process(
        &self,
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }
}

pub struct RDPSetColorImage;
impl GBICommand for RDPSetColorImage {
    fn process(
        &self,
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let _format = get_cmd(w0, 21, 3);
        let _size = get_cmd(w0, 19, 2);
        let _width = get_cmd(w0, 0, 12) + 1;

        rdp.color_image = rsp.from_segmented(w1);
        GBIResult::Continue
    }
}

pub struct RDPSetDepthImage;
impl GBICommand for RDPSetDepthImage {
    fn process(
        &self,
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        rdp.depth_image = rsp.from_segmented(w1);
        GBIResult::Continue
    }
}

pub struct RDPSetTextureImage;
impl GBICommand for RDPSetTextureImage {
    fn process(
        &self,
        rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let format = get_cmd(w0, 21, 3) as u8;
        let size = get_cmd(w0, 19, 2) as u8;
        let width = get_cmd(w0, 0, 12) as u16 + 1;
        let address = rsp.from_segmented(w1);

        rdp.texture_image_state = TextureImageState {
            format,
            size,
            width,
            address,
        };

        GBIResult::Continue
    }
}

pub struct RDPSetCombine;
impl GBICommand for RDPSetCombine {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };
        rdp.set_combine(CombineParams::decode(w0, w1));

        GBIResult::Continue
    }
}

pub struct RDPSetTile;
impl GBICommand for RDPSetTile {
    fn process(
        &self,
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
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

        rdp.set_tile(
            tile, format, size, line, tmem, palette, cm_t, cm_s, mask_t, mask_s, shift_t, shift_s,
        );

        GBIResult::Continue
    }
}

pub struct RDPSetTileSize;
impl GBICommand for RDPSetTileSize {
    fn process(
        &self,
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

        rdp.set_tile_size(tile, ult, uls, lrt, lrs);

        GBIResult::Continue
    }
}

pub struct RDPLoadTile;
impl GBICommand for RDPLoadTile {
    fn process(
        &self,
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

        rdp.load_tile(tile, ult, uls, lrt, lrs);

        GBIResult::Continue
    }
}

pub struct RDPLoadBlock;
impl GBICommand for RDPLoadBlock {
    fn process(
        &self,
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
        let texels = get_cmd(w1, 12, 12) as u16;
        let dxt = get_cmd(w1, 0, 12) as u16;

        rdp.load_block(tile, ult, uls, texels, dxt);

        GBIResult::Continue
    }
}

pub struct RDPLoadTLUT;
impl GBICommand for RDPLoadTLUT {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };

        let tile = get_cmd(w1, 24, 3) as u8;
        let high_index = get_cmd(w1, 14, 10) as u16;

        rdp.load_tlut(tile, high_index);

        GBIResult::Continue
    }
}

pub struct RDPSetEnvColor;
impl GBICommand for RDPSetEnvColor {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        rdp.set_env_color(w1);

        GBIResult::Continue
    }
}

pub struct RDPSetPrimColor;
impl GBICommand for RDPSetPrimColor {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let lod_frac = get_cmd(w0, 0, 8) as u8;
        let lod_min = get_cmd(w0, 8, 5) as u8;
        rdp.set_prim_color(lod_frac, lod_min, w1);

        GBIResult::Continue
    }
}

pub struct RDPSetBlendColor;
impl GBICommand for RDPSetBlendColor {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        rdp.set_blend_color(w1);

        GBIResult::Continue
    }
}

pub struct RDPSetFogColor;
impl GBICommand for RDPSetFogColor {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        rdp.set_fog_color(w1);

        GBIResult::Continue
    }
}

pub struct RDPSetFillColor;
impl GBICommand for RDPSetFillColor {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w1 = unsafe { (*(*command)).words.w1 };
        rdp.set_fill_color(w1);

        GBIResult::Continue
    }
}

pub struct RDPSetOtherMode;
impl GBICommand for RDPSetOtherMode {
    fn process(
        &self,
        rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let high = get_cmd(w0, 0, 24);
        let low = w1;
        rdp.set_other_mode(high as u32, low as u32);

        GBIResult::Continue
    }
}

pub struct RDPSetScissor;
impl GBICommand for RDPSetScissor {
    fn process(
        &self,
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
}

pub struct RDPSetConvert;
impl GBICommand for RDPSetConvert {
    fn process(
        &self,
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
}

pub struct RDPSetKeyR;
impl GBICommand for RDPSetKeyR {
    fn process(
        &self,
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
}

pub struct RDPSetKeyGB;
impl GBICommand for RDPSetKeyGB {
    fn process(
        &self,
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
}

pub struct RDPTextureRectangle;
impl GBICommand for RDPTextureRectangle {
    fn process(
        &self,
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

        rdp.draw_texture_rectangle(
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
        );

        GBIResult::Continue
    }
}

pub struct RDPFillRectangle;
impl GBICommand for RDPFillRectangle {
    fn process(
        &self,
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

        rdp.fill_rect(rsp, output, ulx as i32, uly as i32, lrx as i32, lry as i32);

        GBIResult::Continue
    }
}
