use crate::gbi::macros::gbi_command;
use crate::gbi::GBICommand;
use crate::rsp::RSP;

use super::{
    defines::{G_FILLRECT, G_TEXRECT, G_TEXRECTFLIP},
    f3dex2::F3DEX2,
    utils::get_cmd,
    GBICommandParams, GBICommandRegistry, GBIMicrocode, GBIResult,
};

pub struct F3DEX2E;

impl GBIMicrocode for F3DEX2E {
    fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
        F3DEX2::setup(gbi, rsp);
        gbi.register(G_TEXRECT as usize, F3DEX2ETextureRectangle);
        gbi.register(G_TEXRECTFLIP as usize, F3DEX2ETextureRectangle);
        gbi.register(G_FILLRECT as usize, F3DEX2EFillRectangle);
    }
}

gbi_command!(F3DEX2ETextureRectangle, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let opcode = w0 >> 24;

    let lrx = get_cmd(w0, 0, 24) << 8 >> 8;
    let lry = get_cmd(w1, 0, 24) << 8 >> 8;
    let tile = get_cmd(w1, 24, 3);

    unsafe {
        *params.command = (*params.command).add(1);
    }
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let ulx = get_cmd(w0, 0, 24) << 8 >> 8;
    let uls = get_cmd(w1, 16, 16);
    let ult = get_cmd(w1, 0, 16);

    unsafe {
        *params.command = (*params.command).add(1);
    }
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let uly = get_cmd(w0, 0, 24) << 8 >> 8;
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
        opcode == G_TEXRECTFLIP as usize,
    );

    GBIResult::Continue
});

gbi_command!(F3DEX2EFillRectangle, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let lrx = get_cmd(w0, 0, 24) << 8 >> 8;
    let lry = get_cmd(w1, 0, 24) << 8 >> 8;

    unsafe {
        *params.command = (*params.command).add(1);
    }
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    let ulx = get_cmd(w0, 0, 24) << 8 >> 8;
    let uly = get_cmd(w1, 0, 24) << 8 >> 8;

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
