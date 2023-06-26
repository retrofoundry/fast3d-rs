use crate::{output::RCPOutput, rdp::RDP, rsp::RSP};

use super::{
    defines::{Gfx, G_FILLRECT, G_TEXRECT, G_TEXRECTFLIP},
    f3dex2::F3DEX2,
    utils::get_cmd,
    GBIDefinition, GBIResult, GBI,
};

pub struct F3DEX2E;

impl GBIDefinition for F3DEX2E {
    fn setup(gbi: &mut GBI) {
        F3DEX2::setup(gbi);
        gbi.register(G_TEXRECT as usize, F3DEX2E::gdp_texture_rectangle);
        gbi.register(G_TEXRECTFLIP as usize, F3DEX2E::gdp_texture_rectangle);
        gbi.register(G_FILLRECT as usize, F3DEX2E::gdp_fill_rectangle);
    }
}

impl F3DEX2E {
    pub fn gdp_texture_rectangle(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let opcode = w0 >> 24;

        let lrx = get_cmd(w0, 0, 24) << 8 >> 8;
        let lry = get_cmd(w1, 0, 24) << 8 >> 8;
        let tile = get_cmd(w1, 24, 3);

        unsafe {
            *command = (*command).add(1);
        }
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let ulx = get_cmd(w0, 0, 24) << 8 >> 8;
        let uls = get_cmd(w1, 16, 16);
        let ult = get_cmd(w1, 0, 16);

        unsafe {
            *command = (*command).add(1);
        }
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let uly = get_cmd(w0, 0, 24) << 8 >> 8;
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

    pub fn gdp_fill_rectangle(
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let lrx = get_cmd(w0, 0, 24) << 8 >> 8;
        let lry = get_cmd(w1, 0, 24) << 8 >> 8;

        unsafe {
            *command = (*command).add(1);
        }
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        let ulx = get_cmd(w0, 0, 24) << 8 >> 8;
        let uly = get_cmd(w1, 0, 24) << 8 >> 8;

        F3DEX2::gdp_fill_rectangle_raw(
            rdp, rsp, output, ulx as i32, uly as i32, lrx as i32, lry as i32,
        )
    }
}
