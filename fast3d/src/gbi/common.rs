use log::trace;
use crate::gbi::{GBI, GBIDefinition, GBIResult};
use crate::gbi::defines::{G_LOAD, G_NOOP, G_RDPFULLSYNC, G_RDPLOADSYNC, G_RDPPIPESYNC, G_RDPTILESYNC, G_SET, G_TX, Gfx};
use crate::gbi::utils::get_cmd;
use crate::models::color_combiner::CombineParams;
use crate::models::texture::{ImageSize, TextureImageState};
use crate::output::RCPOutput;
use crate::rdp::{RDP, TMEMMapEntry};
use crate::rsp::RSP;

pub struct Common;
impl GBIDefinition for Common {
    fn setup(gbi: &mut GBI, rsp: &mut RSP) {
        gbi.register(G_NOOP as usize, |_, _, _, _| GBIResult::Continue);
        gbi.register(G_SET::COLORIMG as usize, Self::gdp_set_color_image);
        gbi.register(G_SET::DEPTHIMG as usize, Self::gdp_set_depth_image);
        gbi.register(G_SET::TEXIMG as usize, Self::gdp_set_texture_image);
        gbi.register(G_SET::COMBINE as usize, Self::gdp_set_combine);
        gbi.register(G_SET::TILE as usize, Self::gdp_set_tile);
        gbi.register(G_SET::TILESIZE as usize, Self::gdp_set_tile_size);
        gbi.register(G_LOAD::TILE as usize, Self::gdp_load_tile);
        gbi.register(G_LOAD::BLOCK as usize, Self::gdp_load_block);
        gbi.register(G_LOAD::TLUT as usize, Self::gdp_load_tlut);

        gbi.register(G_RDPLOADSYNC as usize, |_, _, _, _| GBIResult::Continue);
        gbi.register(G_RDPPIPESYNC as usize, |_, _, _, _| GBIResult::Continue);
        gbi.register(G_RDPTILESYNC as usize, |_, _, _, _| GBIResult::Continue);
        gbi.register(G_RDPFULLSYNC as usize, |_, _, _, _| GBIResult::Continue);
    }
}

impl Common {
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
        let _width = get_cmd(w0, 0, 12) + 1;

        rdp.color_image = rsp.from_segmented(w1);
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

    pub fn gdp_set_combine(
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

        rdp.set_tile(tile, format, size, line, tmem, palette, cm_t, cm_s, mask_t, mask_s, shift_t, shift_s);

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

        rdp.set_tile_size(tile, ult, uls, lrt, lrs);

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

        rdp.load_tile(tile, ult, uls, lrt, lrs);

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
        let uls = get_cmd(w0, 12, 12) as u16;
        let ult = get_cmd(w0, 0, 12) as u16;
        let texels = get_cmd(w1, 12, 12) as u16;
        let dxt = get_cmd(w1, 0, 12) as u16;

        rdp.load_block(tile, ult, uls, texels, dxt);

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

        rdp.load_tlut(tile, high_index);

        GBIResult::Continue
    }
}