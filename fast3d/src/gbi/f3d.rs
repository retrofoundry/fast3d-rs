use crate::gbi::utils::get_cmd;
use crate::gbi::{
    macros::gbi_command, GBICommandParams, GBICommandRegistry, GBIResult,
};
use crate::rsp::RSP;
use bitflags::bitflags;
use fast3d_gbi::defines::GfxCommand;
use fast3d_gbi::f3d::OpCode;

bitflags! {
    pub struct MoveWordIndex: u8 {
        const POINTS = 0x0c;
    }
}

bitflags! {
    pub struct MoveWordOffset: u8 {
        const A_LIGHT_2 = 0x20;
        const B_LIGHT_2 = 0x24;
        const A_LIGHT_3 = 0x40;
        const B_LIGHT_3 = 0x44;
        const A_LIGHT_4 = 0x60;
        const B_LIGHT_4 = 0x64;
        const A_LIGHT_5 = 0x80;
        const B_LIGHT_5 = 0x84;
        const A_LIGHT_6 = 0xa0;
        const B_LIGHT_6 = 0xa4;
        const A_LIGHT_7 = 0xc0;
        const B_LIGHT_7 = 0xc4;
        const A_LIGHT_8 = 0xe0;
        const B_LIGHT_8 = 0xe4;
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

#[allow(dead_code)]
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
        let new_addr = params.rsp.get_segment(w1);
        GBIResult::Recurse(new_addr)
    } else {
        let new_addr = params.rsp.get_segment(w1);
        let cmd = new_addr as *mut GfxCommand;
        unsafe { *params.command = cmd.sub(1) };
        GBIResult::Continue
    }
});

gbi_command!(EndDL, |_| { GBIResult::Return });
