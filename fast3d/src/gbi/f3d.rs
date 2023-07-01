use crate::gbi::defines::Gfx;
use crate::gbi::utils::get_cmd;
use crate::gbi::{
    macros::gbi_command, GBICommand, GBICommandParams, GBICommandRegistry, GBIResult,
};
use crate::rsp::RSP;

#[allow(dead_code)]
pub mod g {
    pub mod mw {
        pub const POINTS: u8 = 0x0c;
    }

    pub mod mwo {
        pub const aLIGHT_2: u8 = 0x20;
        pub const bLIGHT_2: u8 = 0x24;
        pub const aLIGHT_3: u8 = 0x40;
        pub const bLIGHT_3: u8 = 0x44;
        pub const aLIGHT_4: u8 = 0x60;
        pub const bLIGHT_4: u8 = 0x64;
        pub const aLIGHT_5: u8 = 0x80;
        pub const bLIGHT_5: u8 = 0x84;
        pub const aLIGHT_6: u8 = 0xa0;
        pub const bLIGHT_6: u8 = 0xa4;
        pub const aLIGHT_7: u8 = 0xc0;
        pub const bLIGHT_7: u8 = 0xc4;
        pub const aLIGHT_8: u8 = 0xe0;
        pub const bLIGHT_8: u8 = 0xe4;
    }

    pub mod mv {
        pub const VIEWPORT: u8 = 0x80;
        pub const LOOKATY: u8 = 0x82;
        pub const LOOKATX: u8 = 0x84;
        pub const L0: u8 = 0x86;
        pub const L1: u8 = 0x88;
        pub const L2: u8 = 0x8a;
        pub const L3: u8 = 0x8c;
        pub const L4: u8 = 0x8e;
        pub const L5: u8 = 0x90;
        pub const L6: u8 = 0x92;
        pub const L7: u8 = 0x94;
        pub const TXTATT: u8 = 0x96;
        pub const MATRIX_1: u8 = 0x9e;
        pub const MATRIX_2: u8 = 0x98;
        pub const MATRIX_3: u8 = 0x9a;
        pub const MATRIX_4: u8 = 0x9c;
    }

    pub const NOOP: u8 = 0xc0;
    pub const SETOTHERMODE_H: u8 = 0xBA;
    pub const SETOTHERMODE_L: u8 = 0xB9;
    pub const RDPHALF_1: u8 = 0xB4;
    pub const RDPHALF_2: u8 = 0xB3;
    pub const SPNOOP: u8 = 0x00;
    pub const ENDDL: u8 = 0xB8;
    pub const DL: u8 = 0x06;
    pub const MOVEMEM: u8 = 0x03;
    pub const MOVEWORD: u8 = 0xBC;
    pub const MTX: u8 = 0x01;
    pub const POPMTX: u8 = 0xBD;
    pub const TEXTURE: u8 = 0xBB;
    pub const VTX: u8 = 0x04;
    pub const CULLDL: u8 = 0xBE;
    pub const TRI1: u8 = 0xBF;
    pub const QUAD: u8 = 0xB5;
    pub const SPRITE2D_BASE: u8 = 0x09;
    pub const SETGEOMETRYMODE: u8 = 0xB7;
    pub const CLEARGEOMETRYMODE: u8 = 0xB6;
}

pub fn setup(gbi: &mut GBICommandRegistry, _rsp: &mut RSP) {
    gbi.register(g::SPNOOP as usize, SpNoOp);
    gbi.register(g::DL as usize, SubDL);
    gbi.register(g::ENDDL as usize, EndDL);

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
