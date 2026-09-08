use super::{
    invalid, CaptureError, CaptureFrame, Fixture, Provenance, ReplayOutput, Result, Sequence,
    TaskCommand,
};
use crate::{ClearPolicy, DepthResetPolicy, Diagnostic, DlSummary, Hardware, Renderer};

/// Records every task and frame after resetting a renderer. Present through this wrapper.
pub struct CaptureSequence {
    frames: Vec<Fixture>,
    active: Option<CaptureFrame>,
    seed: u32,
    expected_generation: std::rc::Rc<()>,
}

impl CaptureSequence {
    pub fn begin(renderer: &mut Renderer, dither_seed: u32) -> Result<Self> {
        if renderer.config.clear_policy != ClearPolicy::Persist {
            return Err(invalid("sequence recording requires Persist clear policy"));
        }
        renderer.reset();
        renderer.set_depth_reset_policy(DepthResetPolicy::Never);
        Ok(Self {
            frames: Vec::new(),
            active: None,
            seed: dither_seed,
            expected_generation: renderer.capture_generation.clone(),
        })
    }

    pub fn begin_frame(&mut self, renderer: &mut Renderer, provenance: Provenance) -> Result<()> {
        if self.active.is_some() {
            return Err(invalid("sequence frame has not been presented"));
        }
        if !std::rc::Rc::ptr_eq(&self.expected_generation, &renderer.capture_generation) {
            return Err(invalid("renderer changed outside sequence recording"));
        }
        let mut frame = CaptureFrame::begin(renderer, 0, self.seed, provenance);
        frame.sequence = true;
        self.active = Some(frame);
        Ok(())
    }

    /// Use the frame's existing safe or native recording entry point while guest memory is valid.
    pub fn frame_mut(&mut self) -> Result<&mut CaptureFrame> {
        self.active
            .as_mut()
            .ok_or_else(|| invalid("sequence has no active frame"))
    }

    pub fn present(&mut self, renderer: &mut Renderer, hardware: &impl Hardware) -> Result<()> {
        let frame = self.take_frame()?.present(renderer, hardware)?;
        self.complete(renderer, frame);
        Ok(())
    }

    pub fn present_to(
        &mut self,
        renderer: &mut Renderer,
        hardware: &impl Hardware,
        target: &wgpu::TextureView,
    ) -> Result<()> {
        let frame = self.take_frame()?.present_to(renderer, hardware, target)?;
        self.complete(renderer, frame);
        Ok(())
    }

    pub fn present_last(&mut self, renderer: &mut Renderer) -> Result<()> {
        let frame = self.take_frame()?.present_last(renderer)?;
        self.complete(renderer, frame);
        Ok(())
    }

    pub fn present_last_to(
        &mut self,
        renderer: &mut Renderer,
        target: &wgpu::TextureView,
    ) -> Result<()> {
        let frame = self.take_frame()?.present_last_to(renderer, target)?;
        self.complete(renderer, frame);
        Ok(())
    }

    fn take_frame(&mut self) -> Result<CaptureFrame> {
        self.active
            .take()
            .ok_or_else(|| invalid("sequence has no active frame"))
    }

    fn complete(&mut self, renderer: &Renderer, frame: Fixture) {
        self.frames.push(frame);
        self.expected_generation = renderer.capture_generation.clone();
    }

    pub fn finish(self, warmup_frames: u32, presentations: Vec<u64>) -> Result<Sequence> {
        if self.active.is_some() {
            return Err(invalid("sequence frame has not been presented"));
        }
        let sequence = Sequence {
            frames: self.frames,
            warmup_frames,
            presentations,
        };
        sequence.validate()?;
        Ok(sequence)
    }
}

#[derive(Debug)]
pub struct FrameLog {
    pub serial: u64,
    pub summaries: Vec<DlSummary>,
    pub diagnostics: Vec<Vec<Diagnostic>>,
    pub commands: Vec<TaskCommand>,
}

#[derive(Debug)]
pub struct Presentation {
    pub serial: u64,
    pub output: ReplayOutput,
}

#[derive(Debug)]
pub struct SequenceOutput {
    pub presentations: Vec<Presentation>,
    pub frames: Vec<FrameLog>,
    pub adapter_info: Option<wgpu::AdapterInfo>,
}

impl Sequence {
    /// Replays the entire reset prefix through one Persist renderer, retaining selected images.
    /// Diagnostic controls discard only depth; they never change the recorded seed or task order.
    pub async fn replay(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        depth_reset: DepthResetPolicy,
    ) -> Result<SequenceOutput> {
        self.validate()?;
        for frame in &self.frames {
            frame.check_device(&device)?;
        }
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let result = self.replay_checked(device, queue, depth_reset).await;
        for scope in scopes.into_iter().rev() {
            if let Some(error) = scope.pop().await {
                return Err(CaptureError::Gpu(error.to_string()));
            }
        }
        result
    }

    async fn replay_checked(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        depth_reset: DepthResetPolicy,
    ) -> Result<SequenceOutput> {
        let mut renderer = self.frames[0].renderer(device, queue, ClearPolicy::Persist);
        renderer.set_depth_reset_policy(depth_reset);
        let mut output = SequenceOutput {
            presentations: Vec::new(),
            frames: Vec::new(),
            adapter_info: None,
        };
        for fixture in &self.frames {
            let frame = fixture.render_sequence_frame(&mut renderer).await?;
            output.frames.push(FrameLog {
                serial: fixture.frame.serial,
                summaries: frame.summaries.clone(),
                diagnostics: frame.diagnostics.clone(),
                commands: frame.commands.clone(),
            });
            if self
                .presentations
                .binary_search(&fixture.frame.serial)
                .is_ok()
            {
                output.presentations.push(Presentation {
                    serial: fixture.frame.serial,
                    output: frame,
                });
            }
        }
        Ok(output)
    }

    pub async fn replay_headless(&self, depth_reset: DepthResetPolicy) -> Result<SequenceOutput> {
        self.validate()?;
        let (device, queue, adapter_info) = self.frames[0].headless_device().await?;
        let mut output = self.replay(device, queue, depth_reset).await?;
        output.adapter_info = Some(adapter_info);
        Ok(output)
    }
}
