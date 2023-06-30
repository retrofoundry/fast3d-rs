use crate::gbi::defines::Gfx;
use crate::gbi::utils::get_cmd;
use crate::gbi::{GBIDefinition, GBIResult, GBI};

use crate::output::RCPOutput;
use crate::rdp::RDP;
use crate::rsp::RSP;

pub struct F3D;

impl F3D {
    pub const G_MW_POINTS: u8 = 0x0c;
    pub const G_MWO_aLIGHT_2: u8 = 0x20;
    pub const G_MWO_bLIGHT_2: u8 = 0x24;
    pub const G_MWO_aLIGHT_3: u8 = 0x40;
    pub const G_MWO_bLIGHT_3: u8 = 0x44;
    pub const G_MWO_aLIGHT_4: u8 = 0x60;
    pub const G_MWO_bLIGHT_4: u8 = 0x64;
    pub const G_MWO_aLIGHT_5: u8 = 0x80;
    pub const G_MWO_bLIGHT_5: u8 = 0x84;
    pub const G_MWO_aLIGHT_6: u8 = 0xa0;
    pub const G_MWO_bLIGHT_6: u8 = 0xa4;
    pub const G_MWO_aLIGHT_7: u8 = 0xc0;
    pub const G_MWO_bLIGHT_7: u8 = 0xc4;
    pub const G_MWO_aLIGHT_8: u8 = 0xe0;
    pub const G_MWO_bLIGHT_8: u8 = 0xe4;
    pub const G_NOOP: u8 = 0xc0;
    pub const G_SETOTHERMODE_H: u8 = 0xBA;
    pub const G_SETOTHERMODE_L: u8 = 0xB9;
    pub const G_RDPHALF_1: u8 = 0xB4;
    pub const G_RDPHALF_2: u8 = 0xB3;
    pub const G_SPNOOP: u8 = 0x00;
    pub const G_ENDDL: u8 = 0xB8;
    pub const G_DL: u8 = 0x06;
    pub const G_MOVEMEM: u8 = 0x03;
    pub const G_MOVEWORD: u8 = 0xBC;
    pub const G_MTX: u8 = 0x01;
    pub const G_POPMTX: u8 = 0xBD;
    pub const G_TEXTURE: u8 = 0xBB;
    pub const G_VTX: u8 = 0x04;
    pub const G_CULLDL: u8 = 0xBE;
    pub const G_TRI1: u8 = 0xBF;
    pub const G_QUAD: u8 = 0xB5;
    pub const G_SPRITE2D_BASE: u8 = 0x09;
    pub const G_SETGEOMETRYMODE: u8 = 0xB7;
    pub const G_CLEARGEOMETRYMODE: u8 = 0xB6;
    pub const G_MV_VIEWPORT: u8 = 0x80;
    pub const G_MV_LOOKATY: u8 = 0x82;
    pub const G_MV_LOOKATX: u8 = 0x84;
    pub const G_MV_L0: u8 = 0x86;
    pub const G_MV_L1: u8 = 0x88;
    pub const G_MV_L2: u8 = 0x8a;
    pub const G_MV_L3: u8 = 0x8c;
    pub const G_MV_L4: u8 = 0x8e;
    pub const G_MV_L5: u8 = 0x90;
    pub const G_MV_L6: u8 = 0x92;
    pub const G_MV_L7: u8 = 0x94;
    pub const G_MV_TXTATT: u8 = 0x96;
    pub const G_MV_MATRIX_1: u8 = 0x9e;
    pub const G_MV_MATRIX_2: u8 = 0x98;
    pub const G_MV_MATRIX_3: u8 = 0x9a;
    pub const G_MV_MATRIX_4: u8 = 0x9c;
}

impl GBIDefinition for F3D {
    fn setup(gbi: &mut GBI, _rsp: &mut RSP) {
        gbi.register(F3D::G_SPNOOP as usize, Self::gsp_no_op);
        gbi.register(F3D::G_DL as usize, Self::sub_dl);
        gbi.register(F3D::G_ENDDL as usize, Self::end_dl);

        // TODO: Complete this
    }
}

impl F3D {
    pub fn gsp_no_op(
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Continue
    }

    pub fn sub_dl(
        _rdp: &mut RDP,
        rsp: &mut RSP,
        _output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult {
        let w0 = unsafe { (*(*command)).words.w0 };
        let w1 = unsafe { (*(*command)).words.w1 };

        if get_cmd(w0, 16, 1) == 0 {
            // Push return address
            let new_addr = rsp.from_segmented(w1);
            GBIResult::Recurse(new_addr)
        } else {
            let new_addr = rsp.from_segmented(w1);
            let cmd = new_addr as *mut Gfx;
            unsafe {
                *command = cmd.sub(1);
            }
            GBIResult::Continue
        }
    }

    pub fn end_dl(
        _rdp: &mut RDP,
        _rsp: &mut RSP,
        _output: &mut RCPOutput,
        _command: &mut *mut Gfx,
    ) -> GBIResult {
        GBIResult::Return
    }
}
