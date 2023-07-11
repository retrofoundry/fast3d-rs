use crate::gbi::defines::Gfx;
use crate::gbi::utils::get_cmd;
use crate::gbi::{
    macros::gbi_command, GBICommand, GBICommandParams, GBICommandRegistry, GBIResult,
};
use crate::rsp::RSP;
use bitflags::bitflags;

bitflags! {
    pub struct MoveWordIndex: u8 {
        const POINTS = 0x0c;
    }
}

bitflags! {
    pub struct MoveWordOffset: u8 {
        const aLIGHT_2 = 0x20;
        const bLIGHT_2 = 0x24;
        const aLIGHT_3 = 0x40;
        const bLIGHT_3 = 0x44;
        const aLIGHT_4 = 0x60;
        const bLIGHT_4 = 0x64;
        const aLIGHT_5 = 0x80;
        const bLIGHT_5 = 0x84;
        const aLIGHT_6 = 0xa0;
        const bLIGHT_6 = 0xa4;
        const aLIGHT_7 = 0xc0;
        const bLIGHT_7 = 0xc4;
        const aLIGHT_8 = 0xe0;
        const bLIGHT_8 = 0xe4;
    }
}

bitflags! {
    pub struct MoveMemoryIndex: u8 {
        const VIEWPORT = 0x80;
        const LOOKATY = 0x82;
        const LOOKATX = 0x84;
        const L0 = 0x86;
        const L1 = 0x88;
        const L2 = 0x8a;
        const L3 = 0x8c;
        const L4 = 0x8e;
        const L5 = 0x90;
        const L6 = 0x92;
        const L7 = 0x94;
        const TXTATT = 0x96;
        const MATRIX_1 = 0x9e;
        const MATRIX_2 = 0x98;
        const MATRIX_3 = 0x9a;
        const MATRIX_4 = 0x9c;
    }
}

bitflags! {
    pub struct OpCode: u8 {
        const NOOP = 0xc0;
        const SETOTHERMODE_H = 0xBA;
        const SETOTHERMODE_L = 0xB9;
        const RDPHALF_1 = 0xB4;
        const RDPHALF_2 = 0xB3;
        const SPNOOP = 0x00;
        const ENDDL = 0xB8;
        const DL = 0x06;
        const MOVEMEM = 0x03;
        const MOVEWORD = 0xBC;
        const MTX = 0x01;
        const POPMTX = 0xBD;
        const TEXTURE = 0xBB;
        const VTX = 0x04;
        const CULLDL = 0xBE;
        const TRI1 = 0xBF;
        const QUAD = 0xB5;
        const SPRITE2D_BASE = 0x09;
        const SETGEOMETRYMODE = 0xB7;
        const CLEARGEOMETRYMODE = 0xB6;
    }
}

pub fn setup(gbi: &mut GBICommandRegistry, _rsp: &mut RSP) {
    gbi.register(OpCode::SPNOOP.bits(), SpNoOp);
    gbi.register(OpCode::DL.bits(), SubDL);
    gbi.register(OpCode::ENDDL.bits(), EndDL);

    // TODO: Complete this
}

// MARK: - Commands

gbi_command!(SpNoOp, |_| {
    // Use rdp, rsp, output, command parameters here
    GBIResult::Continue
});

gbi_command!(SubDL, |params: &mut GBICommandParams| {
    let w0 = unsafe { (*(*params.command)).words.w0 };
    let w1 = unsafe { (*(*params.command)).words.w1 };

    if get_cmd(w0, 16, 1) == 0 {
        // Push return address
        let new_addr = params.rsp.from_segmented(w1);
        GBIResult::Recurse(new_addr)
    } else {
        let new_addr = params.rsp.from_segmented(w1);
        let cmd = new_addr as *mut Gfx;
        unsafe { *params.command = cmd.sub(1) };
        GBIResult::Continue
    }
});

gbi_command!(EndDL, |_| { GBIResult::Return });
