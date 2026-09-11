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

#[cfg(feature = "profiling")]
#[derive(Clone, Copy, Debug)]
pub struct MeasurementOptions {
    pub mode: crate::profiling::Mode,
    pub readback: bool,
    pub command_trace: bool,
    pub frames_in_flight: usize,
}

#[cfg(feature = "profiling")]
impl Default for MeasurementOptions {
    fn default() -> Self {
        Self {
            mode: crate::profiling::Mode::Coarse,
            readback: false,
            command_trace: false,
            frames_in_flight: 2,
        }
    }
}

#[cfg(feature = "profiling")]
pub struct MeasuredFrame {
    pub serial: u64,
    pub observed: bool,
    pub output: ReplayOutput,
    pub profile: crate::profiling::Snapshot,
    pub adapter_ms: f64,
    pub wait_ms: f64,
    pub readback_ms: f64,
    pub readback_profile: crate::profiling::Snapshot,
}

#[cfg(feature = "profiling")]
#[derive(Debug, serde::Serialize)]
pub struct MeasurementSetup {
    pub validation_ms: f64,
    pub renderer_ms: f64,
    pub resources: crate::profiling::Snapshot,
    pub final_wait_ms: f64,
}

#[cfg(feature = "profiling")]
impl Sequence {
    /// Streams every frame from reset. Readback is a separate correctness configuration.
    pub async fn measure(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        options: MeasurementOptions,
        mut frame_ready: impl FnMut(MeasuredFrame),
    ) -> Result<MeasurementSetup> {
        use crate::profiling::{now_ms, Recorder};
        let start = now_ms();
        self.validate()?;
        if !(1..=2).contains(&options.frames_in_flight) {
            return Err(invalid("frames_in_flight must be 1 or 2"));
        }
        for fixture in &self.frames {
            fixture.check_device(&device)?;
        }
        let validation_ms = now_ms() - start;
        let first = &self.frames[0].frame;
        let profiling = Recorder::new(options.mode);
        let start = now_ms();
        let mut renderer = Renderer::with_device_profiled(
            device,
            queue,
            crate::PresentTarget::Headless {
                format: first
                    .config
                    .format
                    .unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
                width: first.width,
                height: first.height,
            },
            crate::RendererConfig {
                clear_policy: ClearPolicy::Persist,
                ..first.config
            },
            profiling.clone(),
        );
        let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("measurement-output"),
            size: wgpu::Extent3d {
                width: first.width,
                height: first.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        profiling.texture(
            "output",
            u64::from(first.width) * u64::from(first.height) * 4,
            0,
        );
        let view = target.create_view(&Default::default());
        let mut setup = MeasurementSetup {
            validation_ms,
            renderer_ms: now_ms() - start,
            resources: profiling.drain(),
            final_wait_ms: 0.0,
        };
        let mut completions = std::collections::VecDeque::new();
        for fixture in &self.frames {
            let mut wait_ms = 0.0;
            if completions.len() >= options.frames_in_flight {
                let start = now_ms();
                wait_completion(renderer.device(), completions.pop_front().unwrap()).await?;
                wait_ms = now_ms() - start;
            }
            renderer.begin_frame();
            renderer.inner.frame_serial = fixture.frame.serial;
            renderer.inner.dither_seed = fixture.frame.dither_seed;
            let mut summaries = Vec::with_capacity(fixture.tasks.len());
            let mut diagnostics = Vec::with_capacity(fixture.tasks.len());
            let mut commands = Vec::new();
            let mut adapter_ms = 0.0;
            for task in &fixture.tasks {
                let start = now_ms();
                let mut hardware = super::ReplayHardware::new(task, fixture.frame.vi)?;
                hardware.set_command_tracing(options.command_trace);
                let mut task_diags = Vec::new();
                renderer.set_data_format(task.data_format);
                adapter_ms += now_ms() - start;
                let summary =
                    renderer.process_dl(&hardware, task.entry, task.microcode, &mut task_diags);
                let start = now_ms();
                hardware.check()?;
                commands.extend(hardware.commands.into_inner().into_iter().map(|command| {
                    TaskCommand {
                        task: task.order,
                        command,
                    }
                }));
                summaries.push(summary);
                diagnostics.push(task_diags);
                adapter_ms += now_ms() - start;
            }
            renderer.present_to(
                &super::replay::PresentationHardware(fixture.frame.vi),
                &view,
            );
            let (send, receive) = futures_channel::oneshot::channel();
            renderer.queue().on_submitted_work_done(move || {
                let _ = send.send(());
            });
            completions.push_back(receive);
            let profile = profiling.drain();
            let start = now_ms();
            let rgba8 = if options.readback {
                super::replay::read_rgba8(&renderer, &target).await?
            } else {
                Vec::new()
            };
            let readback_ms = if options.readback {
                now_ms() - start
            } else {
                0.0
            };
            frame_ready(MeasuredFrame {
                serial: fixture.frame.serial,
                observed: fixture.frame.serial > u64::from(self.warmup_frames),
                output: ReplayOutput {
                    width: first.width,
                    height: first.height,
                    rgba8,
                    summaries,
                    diagnostics,
                    adapter_info: None,
                    commands,
                },
                profile,
                adapter_ms,
                wait_ms,
                readback_ms,
                readback_profile: profiling.drain(),
            });
        }
        let start = now_ms();
        for completion in completions {
            wait_completion(renderer.device(), completion).await?;
        }
        setup.final_wait_ms = now_ms() - start;
        Ok(setup)
    }

    pub async fn measurement_device(
        &self,
    ) -> Result<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
        self.validate()?;
        self.frames[0].headless_device().await
    }
}

#[cfg(feature = "profiling")]
async fn wait_completion(
    device: &wgpu::Device,
    completion: futures_channel::oneshot::Receiver<()>,
) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| CaptureError::Gpu(e.to_string()))?;
    #[cfg(target_arch = "wasm32")]
    let _ = device;
    completion
        .await
        .map_err(|e| CaptureError::Gpu(e.to_string()))
}

#[cfg(feature = "profiling")]
impl Sequence {
    pub async fn replay_streamed(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        mut frame_ready: impl FnMut(u64, ReplayOutput),
    ) -> Result<()> {
        self.validate()?;
        for frame in &self.frames {
            frame.check_device(&device)?;
        }
        let mut renderer = self.frames[0].renderer(device, queue, ClearPolicy::Persist);
        for fixture in &self.frames {
            frame_ready(
                fixture.frame.serial,
                fixture.render_sequence_frame(&mut renderer).await?,
            );
        }
        Ok(())
    }
}
