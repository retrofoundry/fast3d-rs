use super::{output::RenderData, rdp::RDP, rsp::RSP};
use fast3d_gbi::defines::GfxCommand;
use nohash_hasher::BuildNoHashHasher;
use std::collections::HashMap;

mod f3d;
mod f3dex2;
mod f3dex2e;
mod f3dzex2;

mod common;
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
    pub output: &'a mut RenderData,
    pub command: &'a mut *mut GfxCommand,
}

pub type GBICommandFn = fn(&mut GBICommandParams) -> GBIResult;

pub struct GBICommandRegistry {
    gbi_opcode_table: HashMap<u8, GBICommandFn, BuildNoHashHasher<u8>>,
}

impl GBICommandRegistry {
    pub fn new() -> GBICommandRegistry {
        GBICommandRegistry {
            gbi_opcode_table: HashMap::with_hasher(BuildNoHashHasher::default()),
        }
    }

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

    pub fn register(&mut self, opcode: u8, cmd: GBICommandFn) {
        self.gbi_opcode_table.insert(opcode, cmd);
    }

    pub fn handler(&self, opcode: &u8) -> Option<&GBICommandFn> {
        self.gbi_opcode_table.get(opcode)
    }
}

impl Default for GBICommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}
