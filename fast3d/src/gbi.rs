use self::defines::Gfx;

use super::{output::RCPOutputCollector, rdp::RDP, rsp::RSP};
use std::collections::HashMap;

mod f3d;
mod f3dex2;
mod f3dex2e;
mod f3dzex2;

mod common;
pub mod defines;
pub mod macros;
pub mod utils;

pub enum GBIResult {
    Return,
    Recurse(usize),
    Continue,
}

pub struct GBICommandParams<'a> {
    pub rdp: &'a mut RDP,
    pub rsp: &'a mut RSP,
    pub output: &'a mut RCPOutputCollector,
    pub command: &'a mut *mut Gfx,
}

pub trait GBICommand {
    fn process(&self, params: &mut GBICommandParams) -> GBIResult;
}

#[derive(Default)]
pub struct GBICommandRegistry {
    gbi_opcode_table: HashMap<usize, Box<dyn GBICommand>>,
}

impl GBICommandRegistry {
    pub fn setup(&mut self, rsp: &mut RSP) {
        common::Common::setup(self, rsp);

        #[cfg(feature = "f3d")]
        f3d::setup(self, rsp);
        #[cfg(feature = "f3dex2")]
        f3dex2::setup(self, rsp);
        #[cfg(feature = "f3dex2e")]
        f3dex2e::setup(self, rsp);
        #[cfg(feature = "f3dzex2")]
        f3dzex2::setup(self, rsp);
    }

    pub fn register<G: GBICommand>(&mut self, opcode: usize, cmd: G)
    where
        G: 'static,
    {
        let cmd = Box::new(cmd);
        self.gbi_opcode_table.insert(opcode, cmd);
    }

    pub fn handler(&self, opcode: &usize) -> Option<&Box<dyn GBICommand>> {
        self.gbi_opcode_table.get(opcode)
    }
}
