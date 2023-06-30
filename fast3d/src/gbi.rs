use self::defines::Gfx;

use super::{output::RCPOutput, rdp::RDP, rsp::RSP};
use std::collections::HashMap;

mod f3d;
mod f3dex2;
mod f3dex2e;
mod f3dzex2;

mod common;
pub mod defines;
pub mod utils;

pub enum GBIResult {
    Return,
    Recurse(usize),
    Continue,
}

pub trait GBICommand {
    fn process(
        &self,
        rdp: &mut RDP,
        rsp: &mut RSP,
        output: &mut RCPOutput,
        command: &mut *mut Gfx,
    ) -> GBIResult;
}

trait GBIMicrocode {
    fn setup(gbi: &mut GBICommandRegistry, rsp: &mut RSP);
}

#[derive(Default)]
pub struct GBICommandRegistry {
    gbi_opcode_table: HashMap<usize, Box<dyn GBICommand>>,
}

impl GBICommandRegistry {
    pub fn setup(&mut self, rsp: &mut RSP) {
        common::Common::setup(self, rsp);

        #[cfg(feature = "f3d")]
        f3d::F3D::setup(self, rsp);
        #[cfg(feature = "f3dex2")]
        f3dex2::F3DEX2::setup(self, rsp);
        #[cfg(feature = "f3dex2e")]
        f3dex2e::F3DEX2E::setup(self, rsp);
        #[cfg(feature = "f3dzex2")]
        f3dzex2::F3DZEX2::setup(self, rsp);
    }

    pub fn register<G: GBICommand>(&mut self, opcode: usize, cmd: G)
    where
        G: 'static,
    {
        let cmd = Box::new(cmd);
        self.gbi_opcode_table.insert(opcode, cmd);
    }

    pub fn handler_for_opcode(&self, opcode: &usize) -> Option<&Box<dyn GBICommand>> {
        self.gbi_opcode_table.get(opcode)
    }
}
