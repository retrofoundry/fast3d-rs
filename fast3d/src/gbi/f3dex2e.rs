use crate::gbi::macros::gbi_command;
use crate::gbi::GBICommand;
use crate::rsp::RSP;

use super::{defines::g, f3dex2, utils::get_cmd, GBICommandParams, GBICommandRegistry, GBIResult};

pub fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP) {
    f3dex2::setup(gbi, rsp);
    gbi.register(g::TEXRECT as usize, TextureRectangle);
    gbi.register(g::TEXRECTFLIP as usize, TextureRectangle);
    gbi.register(g::FILLRECT as usize, FillRectangle);
}

gbi_command!(TextureRectangle, |params: &mut GBICommandParams| {
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
        opcode == g::TEXRECTFLIP as usize,
    );

    GBIResult::Continue
});

gbi_command!(FillRectangle, |params: &mut GBICommandParams| {
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
