//! Opt-in CPU-side elapsed time and application operation accounting.
//! Detailed spans nest; exclusive time subtracts measured children. Trace runs are diagnostic.

#[cfg(any(test, feature = "profiling"))]
mod recorder;
#[cfg(any(test, feature = "profiling"))]
pub use recorder::*;
#[cfg(any(test, feature = "profiling"))]
mod requests;
#[cfg(any(test, feature = "profiling"))]
pub use requests::*;

#[cfg(not(any(test, feature = "profiling")))]
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorder;
#[cfg(not(any(test, feature = "profiling")))]
impl Recorder {
    pub fn span(&self, _: &'static str) -> Span {
        Span
    }
    pub fn count(&self, _: &'static str, _: u64) {}
    pub fn gauge(&self, _: &'static str, _: u64) {}
    pub fn target(&self, _: (u32, u32), _: (u32, u32)) {}
    pub fn buffer(&self, _: &'static str, _: u64, _: u64) {}
    pub fn texture(&self, _: &'static str, _: u64, _: u64) {}
}

#[cfg(not(any(test, feature = "profiling")))]
pub(crate) struct Span;
#[cfg(not(any(test, feature = "profiling")))]
impl Drop for Span {
    fn drop(&mut self) {}
}

#[cfg(any(test, feature = "profiling"))]
#[derive(Default)]
pub struct CpuInterpreter {
    rdp: crate::hle::rdp::Rdp,
    framebuffers: crate::hle::interp::FramebufferState,
    pub recorder: Recorder,
}

#[cfg(any(test, feature = "profiling"))]
impl CpuInterpreter {
    /// Diagnostic interpretation only; this does not submit or present GPU work.
    pub fn process(
        &mut self,
        memory: impl crate::Rdram,
        entry: u64,
        microcode: crate::Microcode,
        format: crate::DataFormat,
    ) -> (crate::DlSummary, Vec<crate::Diagnostic>) {
        let _span = self.recorder.span("interpretation");
        let result = crate::hle::interp::interpret_profiled(
            memory,
            entry,
            microcode.into(),
            format,
            self.rdp.clone(),
            self.framebuffers.clone(),
            None,
            self.recorder.clone(),
        );
        self.recorder
            .count("geometry.vertices", result.scene.raw_pos.len() as u64);
        self.recorder
            .count("geometry.indices", result.scene.indices.len() as u64);
        if result.commits_rdp() {
            self.rdp = result.rdp.clone();
            self.framebuffers = result.framebuffers.clone();
            if let Some(inputs) = crate::render::inputs::RenderInputs::with_targets(
                &result.scene,
                (320, 240),
                [0, 0],
                &self.framebuffers.targets,
            ) {
                inputs.profile_targets(&self.recorder);
                inputs.update_target_descriptors(&mut self.framebuffers.targets);
            }
        }
        (result.summary(false), result.diags)
    }
}

#[cfg(test)]
mod tests;

/// Authored GPU operation probe for native/browser measurement-driver validation.
#[cfg(feature = "profiling")]
pub fn gpu_accounting_probe(device: &wgpu::Device, queue: &wgpu::Queue) -> (Snapshot, Snapshot) {
    crate::render::gpu_accounting_probe(device, queue)
}
