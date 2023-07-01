use crate::gbi::GBICommandParams;
use log::trace;

use super::{
    gbi::{defines::Gfx, GBICommandRegistry, GBIResult},
    output::RCPOutput,
    rdp::RDP,
    rsp::RSP,
};

pub struct RCP {
    gbi: GBICommandRegistry,
    pub rdp: RDP,
    pub rsp: RSP,
}

impl Default for RCP {
    fn default() -> Self {
        Self::new()
    }
}

impl RCP {
    pub fn new() -> Self {
        let mut gbi = GBICommandRegistry::default();
        let mut rsp = RSP::default();
        gbi.setup(&mut rsp);

        RCP {
            gbi,
            rdp: RDP::default(),
            rsp,
        }
    }

    pub fn reset(&mut self) {
        self.rdp.reset();
        self.rsp.reset();
    }

    /// This function is called to process a work buffer.
    /// It takes in a pointer to the start of the work buffer and will
    /// process until it hits a `G_ENDDL` indicating the end.
    pub fn run(&mut self, output: &mut RCPOutput, commands: usize) {
        self.reset();
        self.run_dl(output, commands);
        self.rdp.flush(output);
    }

    fn run_dl(&mut self, output: &mut RCPOutput, commands: usize) {
        let mut command = commands as *mut Gfx;

        loop {
            let opcode = unsafe { (*command).words.w0 } >> 24;
            if let Some(handler) = self.gbi.handler(&opcode) {
                match handler.process(&mut self.rdp, &mut self.rsp, output, &mut command) {
                    GBIResult::Recurse(new_command) => self.run_dl(output, new_command),
                    GBIResult::Return => return,
                    GBIResult::Continue => {}
                }
            } else {
                trace!("Unknown GBI command: {:#x}", opcode);
            }

            unsafe { command = command.add(1) };
        }
    }
}
