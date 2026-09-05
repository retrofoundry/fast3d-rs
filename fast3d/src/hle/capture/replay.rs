use super::{
    invalid, CaptureError, Fixture, Frame, MemoryLayout, MemorySpan, Provenance, RecordingHardware,
    ReplayHardware, Result, SourceLayout, Task,
};
use crate::{
    ClearPolicy, DataFormat, DiagSink, Diagnostic, DlSummary, Hardware, Microcode, PresentTarget,
    Rdram, RdramImage, Renderer, RendererConfig, ViRegisters,
};
use std::collections::BTreeMap;

pub struct CaptureFrame {
    fixture: Fixture,
    error: Option<CaptureError>,
}

impl CaptureFrame {
    /// Begins a renderer frame. The legacy `serial` argument is ignored; the recorded serial
    /// counts the renderer's `begin_frame` calls, starting at one.
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
            error: None,
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

    fn check_renderer(&mut self, renderer: &Renderer) {
        let frame = &self.fixture.frame;
        if effective_config(renderer) != frame.config
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
}

impl Fixture {
    /// Replays through the public renderer and rejects dependence on prior color attachments.
    pub async fn replay(&self, device: wgpu::Device, queue: wgpu::Queue) -> Result<ReplayOutput> {
        self.validate()?;
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

    pub async fn replay_headless(&self) -> Result<ReplayOutput> {
        self.validate()?;
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
        let mut output = self.replay(device, queue).await?;
        output.adapter_info = Some(adapter_info);
        Ok(output)
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
        for scene in &renderer.frame_scenes {
            for pair in scene
                .framebuffer_pairs
                .iter()
                .filter(|pair| !pair.is_depth_clear)
            {
                let width = u32::from(pair.color_image.width).max(1);
                let height = if pair.size_extent.1 == 0 {
                    240
                } else {
                    pair.size_extent.1
                };
                targets
                    .entry(pair.color_image.addr)
                    .or_insert((width, height));
            }
        }
        drop(renderer);
        for policy in [ClearPolicy::PerFrame, ClearPolicy::Persist] {
            let mut renderer = self.renderer(device.clone(), queue.clone(), policy);
            // Fresh renderers clear both policies identically; seed prior contents before comparing.
            for color in [0xF801_F801, 0x07C1_07C1] {
                prime_framebuffers(&mut renderer, &targets, color)?;
                let candidate = self.render_frame(&mut renderer).await?;
                if candidate.rgba8 != output.rgba8 {
                    return Err(CaptureError::ClearPolicyMismatch);
                }
            }
        }
        Ok(output)
    }

    fn renderer(
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
        renderer.begin_frame();
        renderer.inner.frame_serial = self.frame.serial;
        renderer.inner.dither_seed = self.frame.dither_seed;
        let mut summaries = Vec::with_capacity(self.tasks.len());
        let mut diagnostics = Vec::with_capacity(self.tasks.len());
        for task in &self.tasks {
            let hardware = ReplayHardware::new(task, self.frame.vi)?;
            let mut task_diagnostics = Vec::new();
            renderer.set_data_format(task.data_format);
            let summary =
                renderer.process_dl(&hardware, task.entry, task.microcode, &mut task_diagnostics);
            hardware.check()?;
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
        })
    }
}

fn effective_config(renderer: &Renderer) -> RendererConfig {
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

struct PresentationHardware(Option<ViRegisters>);

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
    targets: &BTreeMap<u64, (u32, u32)>,
    color: u32,
) -> Result<()> {
    renderer.begin_frame();
    renderer.set_data_format(DataFormat::Fixed);
    for (&address, &(width, height)) in targets {
        if width > 1023 || height > 1023 {
            return Err(invalid(
                "framebuffer exceeds the fill-rectangle initialization range",
            ));
        }
        let commands = [
            [0xBA00_1402, 0x0030_0000],
            [0xED00_0000, u64::from(((width * 4) << 12) | (height * 4))],
            [0xFF10_0000 | u64::from(width - 1), address],
            [0xF700_0000, u64::from(color)],
            [
                0xF600_0000 | u64::from((((width - 1) * 4) << 12) | ((height - 1) * 4)),
                0,
            ],
            [0xB800_0000, 0],
        ];
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
        if !summary.renderable || summary.errors != 0 {
            return Err(invalid("framebuffer initialization did not render"));
        }
    }
    Ok(())
}

async fn read_rgba8(renderer: &Renderer, texture: &wgpu::Texture) -> Result<Vec<u8>> {
    let unpadded = texture.width() * 4;
    let stride =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture-readback"),
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
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
