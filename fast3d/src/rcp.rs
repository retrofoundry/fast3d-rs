use super::{output::RCPOutputCollector, rdp::RDP, rsp::RSP};
use crate::gbi::{GBICommandParams, GBICommandRegistry, GBIResult};
use fast3d_gbi::defines::GfxCommand;

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
        let mut gbi = GBICommandRegistry::new();
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
    pub fn run(&mut self, output: &mut RCPOutputCollector, commands: usize) {
        self.reset();
        self.run_dl(output, commands);
        self.rdp.flush(output);
    }

    fn run_dl(&mut self, output: &mut RCPOutputCollector, commands: usize) {
        let mut command = commands as *mut GfxCommand;

        loop {
            let opcode = (unsafe { (*command).words.w0 } >> 24) as u8;
            if let Some(handler) = self.gbi.handler(&opcode) {
                let handler_input = &mut GBICommandParams {
                    rdp: &mut self.rdp,
                    rsp: &mut self.rsp,
                    output,
                    command: &mut command,
                };
                match handler(handler_input) {
                    GBIResult::Recurse(new_command) => self.run_dl(output, new_command),
                    GBIResult::Return => return,
                    GBIResult::Continue => {}
                }
            } else {
                log::trace!("Unknown GBI command: {:#x}", opcode);
            }

            unsafe { command = command.add(1) };
        }
    }
}
