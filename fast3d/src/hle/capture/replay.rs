#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
use super::{finish_recording, ReadLog, RecordingRdram};
use super::{
    invalid, CaptureError, Fixture, Frame, MemoryLayout, MemorySpan, Provenance, RecordingHardware,
    ReplayHardware, Result, SourceLayout, Task,
};
use crate::{
    ClearPolicy, DataFormat, DiagSink, Diagnostic, DlSummary, Hardware, Microcode, PresentTarget,
    Rdram, RdramImage, Renderer, RendererConfig, ViRegisters,
};
use std::collections::BTreeMap;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "state_tests.rs"]
mod state_tests;

pub struct CaptureFrame {
    pub(super) fixture: Fixture,
    error: Option<CaptureError>,
    initial_rdp: crate::hle::rdp::Rdp,
    initial_framebuffers: crate::hle::interp::FramebufferState,
    expected_generation: std::rc::Rc<()>,
    pub(super) sequence: bool,
}

impl CaptureFrame {
    /// Begins a renderer frame. The legacy `serial` argument is ignored; the recorded serial
    /// counts the renderer's `begin_frame` calls, starting at one after construction or reset.
    /// Leaves live RDP state intact. Finishing rejects render inputs or diagnostics that depend
    /// on prior RDP state, which version-one fixtures cannot store. Renderer mutations outside
    /// this wrapper (including resets) invalidate the capture; live rendering still proceeds.
    pub fn begin(
        renderer: &mut Renderer,
        _serial: u64,
        dither_seed: u32,
        provenance: Provenance,
    ) -> Self {
        renderer.begin_frame();
        renderer.inner.dither_seed = dither_seed;
        let serial = renderer.inner.frame_serial;
        let (width, height) = target_extent(renderer);
        Self {
            initial_rdp: renderer.rdp.clone(),
            initial_framebuffers: renderer.interpreter_framebuffers(),
            expected_generation: renderer.capture_generation.clone(),
            fixture: Fixture {
                frame: Frame {
                    serial,
                    dither_seed,
                    config: effective_config(renderer),
                    width,
                    height,
                    vi: None,
                    dual_source_blending: renderer
                        .device()
                        .features()
                        .contains(wgpu::Features::DUAL_SOURCE_BLENDING),
                },
                tasks: Vec::new(),
                provenance,
            },
            error: (renderer.inner.depth_reset_policy != crate::DepthResetPolicy::Never)
                .then(|| invalid("diagnostic depth resets cannot be recorded in a fixture")),
            sequence: false,
        }
    }

    pub fn process_dl(
        &mut self,
        renderer: &mut Renderer,
        hardware: &impl Hardware,
        entry: u64,
        microcode: Microcode,
        data_format: DataFormat,
        diagnostics: &mut dyn DiagSink,
    ) -> Result<DlSummary> {
        self.check_renderer(renderer);
        renderer.set_data_format(data_format);
        if let Some(error) = &self.error {
            renderer.process_dl(hardware, entry, microcode, diagnostics);
            return Err(error.clone());
        }
        let recording = RecordingHardware::new(hardware);
        let summary = renderer.process_dl(&recording, entry, microcode, diagnostics);
        self.expected_generation = renderer.capture_generation.clone();
        let task = u32::try_from(self.fixture.tasks.len())
            .map_err(|_| invalid("too many tasks in one frame"))
            .and_then(|order| recording.finish(entry, microcode, data_format, order));
        match task {
            Ok(task) => {
                self.fixture.tasks.push(task);
                Ok(summary)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Records a native display-list walk while every reachable source span is still valid.
    ///
    /// # Safety
    /// Every command and reachable input span must be allocated, readable, initialized, in
    /// [`crate::HostRam`]'s native layout, and stable until return. Borrowed texture bytes must not
    /// be mutated concurrently. Numeric CIMG/ZIMG identities need not be readable unless used as
    /// inputs. The dispatch cap only bounds liveness; it cannot validate a native pointer.
    #[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
    pub unsafe fn process_dl_host(
        &mut self,
        renderer: &mut Renderer,
        ram: crate::HostRam<'_>,
        entry: u64,
        microcode: Microcode,
        data_format: DataFormat,
        diagnostics: &mut dyn DiagSink,
    ) -> Result<DlSummary> {
        self.check_renderer(renderer);
        renderer.set_data_format(data_format);
        if let Some(error) = &self.error {
            unsafe { renderer.process_dl_host(ram, entry, microcode, diagnostics) };
            return Err(error.clone());
        }
        let source = ram.capture_layout();
        let memory = unsafe { crate::hle::host_mem::HostMemory::new(ram) };
        let log = std::cell::RefCell::new(ReadLog {
            source: Some(source),
            ..ReadLog::default()
        });
        let recording = RecordingRdram {
            inner: memory,
            layout: source.memory,
            log: &log,
        };
        let summary = renderer.process_dl_memory(recording, entry, microcode, diagnostics);
        self.expected_generation = renderer.capture_generation.clone();
        let task = u32::try_from(self.fixture.tasks.len())
            .map_err(|_| invalid("too many tasks in one frame"))
            .and_then(|order| {
                finish_recording(log.into_inner(), entry, microcode, data_format, order)
            });
        match task {
            Ok(task) => {
                self.fixture.tasks.push(task);
                Ok(summary)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn present(mut self, renderer: &mut Renderer, hardware: &impl Hardware) -> Result<Fixture> {
        self.check_renderer(renderer);
        let vi = hardware.vi();
        let presented = renderer.present(&PresentationHardware(vi));
        let fixture = self.finish(vi)?;
        presented.map_err(|error| CaptureError::Gpu(format!("presentation failed: {error:?}")))?;
        Ok(fixture)
    }

    pub fn present_to(
        mut self,
        renderer: &mut Renderer,
        hardware: &impl Hardware,
        target: &wgpu::TextureView,
    ) -> Result<Fixture> {
        self.check_renderer(renderer);
        let expected = &self.fixture.frame;
        if (target.texture().width(), target.texture().height())
            != (expected.width, expected.height)
        {
            self.error
                .get_or_insert_with(|| invalid("presentation target differs from captured target"));
        }
        let vi = hardware.vi();
        renderer.present_to(&PresentationHardware(vi), target);
        self.finish(vi)
    }

    /// Presents the last rendered framebuffer without consulting guest memory or VI registers.
    pub fn present_last(mut self, renderer: &mut Renderer) -> Result<Fixture> {
        self.check_renderer(renderer);
        let presented = renderer.present_last();
        let fixture = self.finish(None)?;
        presented.map_err(|error| CaptureError::Gpu(format!("presentation failed: {error:?}")))?;
        Ok(fixture)
    }

    /// Presents the last rendered framebuffer into `target` without consulting guest memory.
    pub fn present_last_to(
        mut self,
        renderer: &mut Renderer,
        target: &wgpu::TextureView,
    ) -> Result<Fixture> {
        self.check_renderer(renderer);
        let expected = &self.fixture.frame;
        if (target.texture().width(), target.texture().height())
            != (expected.width, expected.height)
        {
            self.error
                .get_or_insert_with(|| invalid("presentation target differs from captured target"));
        }
        renderer.present_last_to(target);
        self.finish(None)
    }

    fn check_renderer(&mut self, renderer: &Renderer) {
        let frame = &self.fixture.frame;
        if !std::rc::Rc::ptr_eq(&self.expected_generation, &renderer.capture_generation)
            || effective_config(renderer) != frame.config
            || target_extent(renderer) != (frame.width, frame.height)
        {
            self.error
                .get_or_insert_with(|| invalid("renderer changed during capture"));
        }
    }

    fn finish(mut self, vi: Option<ViRegisters>) -> Result<Fixture> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.fixture.frame.vi = vi;
        self.fixture.validate()?;
        if self.sequence {
            return Ok(self.fixture);
        }
        let mut live = self.initial_rdp;
        let mut live_framebuffers = self.initial_framebuffers;
        let mut replay_framebuffers = crate::hle::interp::FramebufferState::default();
        let mut replay = crate::hle::rdp::Rdp::default();
        for task in &self.fixture.tasks {
            let mut actual =
                task.interpret_with_framebuffers(live.clone(), live_framebuffers.clone())?;
            let mut expected =
                task.interpret_with_framebuffers(replay.clone(), replay_framebuffers.clone())?;
            // Epochs affect only diagnostic depth-reset controls, which recording excludes.
            for scene in [&mut actual.scene, &mut expected.scene] {
                for pair in &mut scene.framebuffer_pairs {
                    pair.color_image_epoch = 0;
                }
            }
            let inputs = |scene, targets| {
                crate::render::inputs::RenderInputs::with_targets(
                    scene,
                    (self.fixture.frame.width, self.fixture.frame.height),
                    [
                        self.fixture.frame.serial as u32,
                        self.fixture.frame.dither_seed,
                    ],
                    targets,
                )
            };
            let actual_inputs = inputs(&actual.scene, &live_framebuffers.targets);
            let expected_inputs = inputs(&expected.scene, &replay_framebuffers.targets);
            if actual_inputs != expected_inputs
                || actual.diags != expected.diags
                || actual.summary(false) != expected.summary(false)
            {
                return Err(invalid(
                    "frame depends on prior RDP state or framebuffer history not stored in version-one captures",
                ));
            }
            if actual.commits_rdp() {
                if let Some(inputs) = actual_inputs {
                    inputs.update_target_descriptors(&mut live_framebuffers.targets);
                }
                if let Some(inputs) = expected_inputs {
                    inputs.update_target_descriptors(&mut replay_framebuffers.targets);
                }
                live_framebuffers.color_image_epoch = actual.framebuffers.color_image_epoch;
                replay_framebuffers.color_image_epoch = expected.framebuffers.color_image_epoch;
                live = actual.rdp;
                replay = expected.rdp;
            }
        }
        Ok(self.fixture)
    }
}

#[derive(Debug)]
pub struct ReplayOutput {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub summaries: Vec<DlSummary>,
    pub diagnostics: Vec<Vec<Diagnostic>>,
    pub adapter_info: Option<wgpu::AdapterInfo>,
    pub commands: Vec<super::TaskCommand>,
}

impl Fixture {
    /// Replays from default RDP registers and empty TMEM, preserving state between recorded tasks.
    /// Rejects dependence on prior color or depth attachments; each clear-policy probe starts from
    /// default registers while retaining its deliberately primed framebuffer contents.
    pub async fn replay(&self, device: wgpu::Device, queue: wgpu::Queue) -> Result<ReplayOutput> {
        self.validate()?;
        self.check_device(&device)?;
        let scopes = [
            wgpu::ErrorFilter::OutOfMemory,
            wgpu::ErrorFilter::Internal,
            wgpu::ErrorFilter::Validation,
        ]
        .map(|filter| device.push_error_scope(filter));
        let result = self.replay_checked(device, queue).await;
        for scope in scopes.into_iter().rev() {
            if let Some(error) = scope.pop().await {
                return Err(CaptureError::Gpu(error.to_string()));
            }
        }
        result
    }

    pub(super) fn check_device(&self, device: &wgpu::Device) -> Result<()> {
        let dual_source = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        if dual_source != self.frame.dual_source_blending {
            return Err(CaptureError::Gpu(
                "device dual-source blending differs from capture".into(),
            ));
        }
        let limits = device.limits();
        let row_bytes = (self.frame.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        if self.frame.width > limits.max_texture_dimension_2d
            || self.frame.height > limits.max_texture_dimension_2d
            || u64::from(row_bytes) * u64::from(self.frame.height) > limits.max_buffer_size
        {
            return Err(CaptureError::Gpu(
                "capture output exceeds device limits".into(),
            ));
        }
        Ok(())
    }

    pub async fn replay_headless(&self) -> Result<ReplayOutput> {
        self.validate()?;
        let (device, queue, adapter_info) = self.headless_device().await?;
        let mut output = self.replay(device, queue).await?;
        output.adapter_info = Some(adapter_info);
        Ok(output)
    }

    pub(super) async fn headless_device(
        &self,
    ) -> Result<(wgpu::Device, wgpu::Queue, wgpu::AdapterInfo)> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: self.frame.config.power_preference,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|error| CaptureError::Gpu(error.to_string()))?;
        let adapter_info = adapter.get_info();
        let features = if self.frame.dual_source_blending {
            wgpu::Features::DUAL_SOURCE_BLENDING
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("capture-replay"),
                required_features: features,
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|error| CaptureError::Gpu(error.to_string()))?;
        Ok((device, queue, adapter_info))
    }

    async fn replay_checked(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<ReplayOutput> {
        let mut renderer = self.renderer(
            device.clone(),
            queue.clone(),
            self.frame.config.clear_policy,
        );
        let output = self.render_frame(&mut renderer).await?;
        if !output.summaries.iter().any(|summary| summary.renderable) {
            return Err(invalid("frame does not render a framebuffer"));
        }
        let mut targets = BTreeMap::new();
        let mut depths = BTreeMap::new();
        for scene in &renderer.frame_scenes {
            for target in crate::render::workload::Workload::new(scene).targets {
                let (width, height) = target.logical_extent;
                if let crate::render::workload::TargetId::Guest(address) = target.id {
                    if !target.depth_clear {
                        targets
                            .entry(address)
                            .and_modify(|extent: &mut (u32, u32, u8, u8)| {
                                if (extent.0, extent.2, extent.3)
                                    == (width, target.color_image.fmt, target.color_image.siz)
                                {
                                    extent.1 = extent.1.max(height);
                                }
                            })
                            .or_insert((
                                width,
                                height,
                                target.color_image.fmt,
                                target.color_image.siz,
                            ));
                    }
                }
                if let Some(address) = target.depth_image {
                    depths
                        .entry(address)
                        .and_modify(|extent: &mut (u32, u32, u8, u8)| {
                            if extent.0 == width {
                                extent.1 = extent.1.max(height);
                            }
                        })
                        .or_insert((width, height, 0, 2));
                }
            }
        }
        drop(renderer);
        for policy in [ClearPolicy::PerFrame, ClearPolicy::Persist] {
            for color in [0xF801_F801, 0x07C1_07C1] {
                for depth in [0x0000_0000, 0xFFFC_FFFC] {
                    let mut renderer = self.renderer(device.clone(), queue.clone(), policy);
                    prime_framebuffers(&mut renderer, &targets, color, false)?;
                    prime_framebuffers(&mut renderer, &depths, depth, true)?;
                    renderer.inner.prime_legacy_attachments(
                        &device,
                        &queue,
                        if color == 0xF801_F801 {
                            wgpu::Color::RED
                        } else {
                            wgpu::Color::GREEN
                        },
                        if depth == 0 { 0.0 } else { 1.0 },
                    );
                    let candidate = self.render_frame(&mut renderer).await?;
                    if candidate.rgba8 != output.rgba8 {
                        return Err(CaptureError::ClearPolicyMismatch);
                    }
                }
            }
        }
        Ok(output)
    }

    pub(super) fn renderer(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        clear_policy: ClearPolicy,
    ) -> Renderer {
        Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: self
                    .frame
                    .config
                    .format
                    .unwrap_or(wgpu::TextureFormat::Rgba8Unorm),
                width: self.frame.width,
                height: self.frame.height,
            },
            RendererConfig {
                clear_policy,
                ..self.frame.config
            },
        )
    }

    async fn render_frame(&self, renderer: &mut Renderer) -> Result<ReplayOutput> {
        renderer.reset_rdp_state();
        self.render_sequence_frame(renderer).await
    }

    pub(super) async fn render_sequence_frame(
        &self,
        renderer: &mut Renderer,
    ) -> Result<ReplayOutput> {
        renderer.begin_frame();
        renderer.inner.frame_serial = self.frame.serial;
        renderer.inner.dither_seed = self.frame.dither_seed;
        let mut summaries = Vec::with_capacity(self.tasks.len());
        let mut diagnostics = Vec::with_capacity(self.tasks.len());
        let mut commands = Vec::new();
        for task in &self.tasks {
            let hardware = ReplayHardware::new(task, self.frame.vi)?;
            let mut task_diagnostics = Vec::new();
            renderer.set_data_format(task.data_format);
            let summary =
                renderer.process_dl(&hardware, task.entry, task.microcode, &mut task_diagnostics);
            hardware.check()?;
            commands.extend(hardware.commands.into_inner().into_iter().map(|command| {
                super::TaskCommand {
                    task: task.order,
                    command,
                }
            }));
            summaries.push(summary);
            diagnostics.push(task_diagnostics);
        }
        let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("capture-output"),
            size: wgpu::Extent3d {
                width: self.frame.width,
                height: self.frame.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        renderer.present_to(
            &PresentationHardware(self.frame.vi),
            &target.create_view(&Default::default()),
        );
        let rgba8 = read_rgba8(renderer, &target).await?;
        Ok(ReplayOutput {
            width: self.frame.width,
            height: self.frame.height,
            rgba8,
            summaries,
            diagnostics,
            adapter_info: None,
            commands,
        })
    }
}

pub(super) fn effective_config(renderer: &Renderer) -> RendererConfig {
    RendererConfig {
        format: Some(renderer.surface_format),
        ..renderer.config
    }
}

fn target_extent(renderer: &Renderer) -> (u32, u32) {
    match &renderer.target {
        PresentTarget::Surface { config, .. } => (config.width.max(1), config.height.max(1)),
        PresentTarget::Headless { width, height, .. } => ((*width).max(1), (*height).max(1)),
    }
}

pub(super) struct PresentationHardware(pub(super) Option<ViRegisters>);

impl Hardware for PresentationHardware {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&[])
    }
    fn vi(&self) -> Option<ViRegisters> {
        self.0
    }
}

fn prime_framebuffers(
    renderer: &mut Renderer,
    targets: &BTreeMap<u64, (u32, u32, u8, u8)>,
    color: u32,
    depth: bool,
) -> Result<()> {
    renderer.reset_rdp_state();
    renderer.begin_frame();
    renderer.set_data_format(DataFormat::Fixed);
    for (&address, &(width, height, fmt, siz)) in targets {
        if width > 1023 || height > 1023 {
            return Err(invalid(
                "framebuffer exceeds the fill-rectangle initialization range",
            ));
        }
        let mut commands = vec![
            [0xBA00_1402, 0x0030_0000],
            [0xED00_0000, u64::from(((width * 4) << 12) | (height * 4))],
            [
                0xFF00_0000
                    | (u64::from(fmt) << 21)
                    | (u64::from(siz) << 19)
                    | u64::from(width - 1),
                address,
            ],
            [0xF700_0000, u64::from(color)],
            [
                0xF600_0000 | u64::from((((width - 1) * 4) << 12) | ((height - 1) * 4)),
                0,
            ],
            [0xB800_0000, 0],
        ];
        if depth {
            commands.insert(0, [0xFE00_0000, address]);
        }
        let task = Task {
            entry: 0,
            microcode: Microcode::F3d,
            data_format: DataFormat::Fixed,
            order: 0,
            source: SourceLayout {
                memory: MemoryLayout::HOST64_LE,
                segments: [0; 16],
            },
            spans: vec![MemorySpan {
                address: 0,
                bytes: commands
                    .iter()
                    .flatten()
                    .flat_map(|word| word.to_le_bytes())
                    .collect(),
            }],
        };
        let hardware = ReplayHardware::new(&task, None)?;
        let mut diagnostics = Vec::new();
        let summary = renderer.process_dl(&hardware, 0, Microcode::F3d, &mut diagnostics);
        hardware.check()?;
        if (!depth && !summary.renderable) || summary.errors != 0 {
            return Err(invalid("framebuffer initialization did not render"));
        }
    }
    Ok(())
}

pub(super) async fn read_rgba8(renderer: &Renderer, texture: &wgpu::Texture) -> Result<Vec<u8>> {
    let unpadded = texture.width() * 4;
    let stride =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture-readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    renderer
        .inner
        .profiling
        .buffer("readback", buffer.size(), 0);
    renderer.inner.profiling.count(
        "copy.readback_bytes",
        u64::from(unpadded) * u64::from(texture.height()),
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    renderer.queue().submit([encoder.finish()]);
    let (sender, receiver) = futures_channel::oneshot::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    #[cfg(not(target_arch = "wasm32"))]
    renderer
        .device()
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| CaptureError::Gpu(error.to_string()))?;
    receiver
        .await
        .map_err(|error| CaptureError::Gpu(error.to_string()))?
        .map_err(|error| CaptureError::Gpu(error.to_string()))?;
    let mapped = buffer.slice(..).get_mapped_range();
    let mut rgba8: Vec<u8> = mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..unpadded as usize].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    if matches!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in rgba8.as_chunks_mut::<4>().0.iter_mut() {
            pixel.swap(0, 2);
        }
    }
    Ok(rgba8)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "sequence_tests.rs"]
mod sequence_tests;
