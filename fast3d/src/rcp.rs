use super::{output::RenderData, rdp::RDP, rsp::RSP};
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

    /// Processes a render list. Takes in a pointer to the start of the
    /// render list and process until it hits a final `G_ENDDL`.
    /// Returns a `RenderData` struct containing graphics data that
    /// can be used to render.
    pub fn process_dl(&mut self, commands: usize) -> RenderData {
        self.reset();

        let mut output = RenderData::new();
        self.run_dl(&mut output, commands);
        self.rdp.flush(&mut output);

        output
    }

    fn run_dl(&mut self, output: &mut RenderData, commands: usize) {
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
