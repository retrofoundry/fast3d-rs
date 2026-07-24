//! `fast3d` — N64 F3DEX2 HLE + wgpu renderer. The renderer is the default product. The
//! assembler is available through the opt-in `asm` cargo feature. Its supported items are
//! re-exported at `fast3d::asm::*`; `fast3d::asm::encode` and `fast3d::asm::gu` are its only
//! public submodules.

#[cfg(feature = "asm")]
pub mod asm;
#[cfg(feature = "debug-ui")]
pub mod debug;
pub mod diag;
pub mod hardware;
pub(crate) mod hle;
pub mod hooks;
pub mod microcode;
pub(crate) mod render;
pub(crate) mod scene;

use crate::render::SceneRenderer;
use crate::scene::Scene;

// ── New vNext public API (spec §3.6): structured diagnostics ──
pub use diag::{DiagKind, DiagSink, Diagnostic, DlSummary, LogSink, NopSink, Severity};
// ── New vNext public API (spec §3.6): microcode selector ──
pub use microcode::{detect_microcode, Microcode};
// ── Vertex/matrix data layout (`Fixed` N64 / `Float` GBI_FLOATS), orthogonal to the microcode ──
pub use crate::hle::mem::GbiDataFormat as DataFormat;
// ── New vNext public API (spec §3.5): render hooks ──
#[cfg(feature = "debug-ui")]
pub use debug::{DebugButton, DebugInput, DebugKey};
pub use hooks::{HookFrame, RenderHook};

// ── New vNext public API (spec §3.2/§3.3): Hardware boundary + memory readers + VI registers ──
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
pub use hardware::HostRam;
pub use hardware::{Hardware, Rdram, RdramImage, ViRegisters};

/// How internal framebuffers are cleared across frames (spec §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearPolicy {
    /// Clear each framebuffer on first touch every frame — simple/no-VI consumers (web, goldens).
    PerFrame,
    /// N64-faithful: clear only when a framebuffer texture is newly created or resized; Load
    /// thereafter (a HUD-only repaint keeps last frame's 3D underneath). Live-VI game consumers.
    Persist,
}

/// Renderer configuration. All fields are `Copy`; the `Renderer` stores it by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RendererConfig {
    /// Internal framebuffer scale. **v1: values > 1 are clamped to 1 with a `log::warn!`**.
    pub resolution_multiplier: u32,
    /// MSAA sample count (1 = off). **v1: clamped to 1**.
    pub sample_count: u32,
    pub present_mode: wgpu::PresentMode,
    /// Surface format. `None` = pick deterministically (`pick_surface_format`); NEVER
    /// blindly trusts `caps.formats[0]`.
    pub format: Option<wgpu::TextureFormat>,
    pub clear_policy: ClearPolicy,
    pub power_preference: wgpu::PowerPreference,
}

/// Where a `Renderer` scans out. `Surface` is an owned swapchain (built by `Renderer::new`, or
/// supplied to `with_device` by a host app that owns its window — wafel); `Headless` is a bare
/// format+size for render-to-texture / golden capture.
pub enum PresentTarget {
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    Headless {
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    },
}

/// Setup failure from `Renderer::new` / `with_device`. Fail-fast (spec §3.6).
#[derive(Debug)]
#[non_exhaustive]
pub enum InitError {
    Adapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
    Surface(wgpu::CreateSurfaceError),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::Adapter(e) => write!(f, "adapter request failed: {e}"),
            InitError::Device(e) => write!(f, "device request failed: {e}"),
            InitError::Surface(e) => write!(f, "surface creation failed: {e}"),
        }
    }
}

impl std::error::Error for InitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InitError::Adapter(e) => Some(e),
            InitError::Device(e) => Some(e),
            InitError::Surface(e) => Some(e),
        }
    }
}

impl From<wgpu::RequestAdapterError> for InitError {
    fn from(e: wgpu::RequestAdapterError) -> Self {
        InitError::Adapter(e)
    }
}
impl From<wgpu::RequestDeviceError> for InitError {
    fn from(e: wgpu::RequestDeviceError) -> Self {
        InitError::Device(e)
    }
}
impl From<wgpu::CreateSurfaceError> for InitError {
    fn from(e: wgpu::CreateSurfaceError) -> Self {
        InitError::Surface(e)
    }
}

/// Per-frame surface fault from `present` (synchronous, from `get_current_texture`). DL-content
/// problems are diagnostics, never this (spec §3.6).
#[derive(Debug)]
#[non_exhaustive]
pub enum PresentError {
    /// Surface was Outdated/Lost; `present` reconfigured from the stored config — retry next frame.
    SurfaceLost,
    Timeout,
    DeviceLost,
}

/// The render format for a `Surface` target — the non-sRGB `view_format` when one was configured,
/// else the surface format itself.
fn render_format_of(config: &wgpu::SurfaceConfiguration) -> wgpu::TextureFormat {
    config
        .view_formats
        .first()
        .copied()
        .unwrap_or(config.format)
}

/// v1 does not support supersampling or MSAA; warn (values are ignored, not normalized).
fn warn_unsupported(config: &RendererConfig) {
    if config.resolution_multiplier > 1 {
        log::warn!("resolution_multiplier > 1 is unsupported in v1; treated as 1");
    }
    if config.sample_count > 1 {
        log::warn!("sample_count > 1 (MSAA) is unsupported in v1; treated as 1");
    }
}

/// Deterministically choose the swapchain config format and an optional non-sRGB view format, so we
/// render into a LINEAR target and never double-apply the sRGB EOTF (the web-wasm washout). NEVER
/// trusts `caps.formats[0]`. Returns `(config_format, view_format)`.
fn pick_surface_format(
    available: &[wgpu::TextureFormat],
    override_format: Option<wgpu::TextureFormat>,
) -> (wgpu::TextureFormat, Option<wgpu::TextureFormat>) {
    if let Some(f) = override_format {
        return (f, None); // explicit override wins verbatim
    }
    if let Some(&f) = available.iter().find(|f| !f.is_srgb()) {
        return (f, None); // prefer a non-sRGB member directly
    }
    if let Some(&f) = available.first() {
        return (f, Some(f.remove_srgb_suffix())); // only sRGB offered: flip the view
    }
    (wgpu::TextureFormat::Bgra8Unorm, None) // degenerate empty caps
}

/// The public N64 renderer. Non-generic — the `Hardware` handle is a per-call parameter (spec §3).
///
/// `!Send` + `!Sync` on every target: no public bound may add them (spec §3.1). The `_not_send`
/// marker pins this even before P4's `Box<dyn RenderHook>` field forces it structurally.
///
/// Field declaration order == drop order: `target`/`inner` release BEFORE `queue`/`device`/`instance`.
pub struct Renderer {
    target: PresentTarget,
    inner: SceneRenderer,
    pub(crate) frame_scenes: Vec<Scene>,
    pub(crate) last_scanout_addr: Option<u64>,
    pub(crate) last_backend_was_image: bool,
    pub(crate) surface_format: wgpu::TextureFormat,
    /// Guest DL vertex/matrix layout, selected via `set_data_format`. Survives `reconfigure`.
    data_format: DataFormat,
    /// Consumer render hook (P4). Declared BEFORE `queue`/`device` so it drops first (drop order ==
    /// field declaration order) — a hook's GPU resources release while the device is still alive.
    hook: Option<Box<dyn RenderHook>>,
    /// Optional built-in egui debugger (feature `debug-ui`). Declared before `queue`/`device` so its
    /// egui-wgpu GPU resources drop first. Default-disabled; a no-op under `present_to` when off.
    #[cfg(feature = "debug-ui")]
    debugger: crate::debug::Debugger,
    config: RendererConfig, // RA: clear_policy read as self.config.clear_policy
    queue: wgpu::Queue,
    device: wgpu::Device,
    #[allow(dead_code)] // permanent lifetime anchor; never read.
    instance: Option<wgpu::Instance>,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Renderer {
    /// SECONDARY constructor: adopt a device/queue the consumer already created (wafel; headless
    /// tests). Synchronous. Capability probing (`DUAL_SOURCE_BLENDING`) is internal — no
    /// `dual_source` in the public surface (spec §3.1).
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: PresentTarget,
        config: RendererConfig,
    ) -> Self {
        let (render_fmt, w, h) = match &target {
            PresentTarget::Surface { config: sc, .. } => {
                (render_format_of(sc), sc.width.max(1), sc.height.max(1))
            }
            PresentTarget::Headless {
                format,
                width,
                height,
            } => (*format, (*width).max(1), (*height).max(1)),
        };
        warn_unsupported(&config);
        let dual_source = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        let inner = SceneRenderer::new(&device, render_fmt, w, h, dual_source);
        Self {
            target,
            inner,
            frame_scenes: Vec::new(),
            last_scanout_addr: None,
            last_backend_was_image: false,
            surface_format: render_fmt,
            data_format: DataFormat::Fixed,
            hook: None,
            #[cfg(feature = "debug-ui")]
            debugger: crate::debug::Debugger::new(),
            config,
            queue,
            device,
            instance: None,
            _not_send: std::marker::PhantomData,
        }
    }

    /// PRIMARY constructor. Create the wgpu Instance/Adapter/Device/Queue AND the surface from a
    /// window/canvas handle. Async (adapter/device requests are async).
    pub async fn new(
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        config: RendererConfig,
    ) -> Result<Self, InitError> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(surface_target)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: config.power_preference,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await?;
        let dual_source = adapter
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        let required_features = if dual_source {
            wgpu::Features::DUAL_SOURCE_BLENDING
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fast3d-device"),
                required_features,
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let (surface_fmt, view_fmt) = pick_surface_format(&caps.formats, config.format);
        let render_fmt = view_fmt.unwrap_or(surface_fmt);
        let (w, h) = (width.max(1), height.max(1));
        warn_unsupported(&config);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_fmt,
            width: w,
            height: h,
            present_mode: config.present_mode,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: view_fmt.into_iter().collect(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let inner = SceneRenderer::new(&device, render_fmt, w, h, dual_source);
        Ok(Self {
            target: PresentTarget::Surface {
                surface,
                config: surface_config,
            },
            inner,
            frame_scenes: Vec::new(),
            last_scanout_addr: None,
            last_backend_was_image: false,
            surface_format: render_fmt,
            data_format: DataFormat::Fixed,
            hook: None,
            #[cfg(feature = "debug-ui")]
            debugger: crate::debug::Debugger::new(),
            config,
            queue,
            device,
            instance: Some(instance),
            _not_send: std::marker::PhantomData,
        })
    }

    /// Live handles for consumers that upload their own GPU resources (helix HUD textures, wafel).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Select how subsequent `process_dl` calls read guest vertices and matrices (default
    /// `Fixed`). A per-consumer property — set once after construction, not per display list.
    pub fn set_data_format(&mut self, data_format: DataFormat) {
        self.data_format = data_format;
    }

    /// Window/drawable resized: reconfigure the owned surface only. Internal framebuffers are
    /// game-sized and NOT touched here (spec §3.1). No-op for `Headless`.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if let PresentTarget::Surface { surface, config } = &mut self.target {
            config.width = w;
            config.height = h;
            surface.configure(&self.device, config); // disjoint borrow: self.target vs self.device
        }
    }

    /// Apply a new config. Drains in-flight work first, then does a FULL internal-renderer rebuild
    /// (v1 trade-off; the persistent FB store is dropped and repopulated on the next `process_dl`).
    /// Re-applies `present_mode` (and an explicit `format` override) to a `Surface`. A `None` format
    /// keeps the currently picked format (no adapter retained to re-run `pick_surface_format`).
    pub fn reconfigure(&mut self, config: RendererConfig) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely()); // #[must_use]; no-op on web
        warn_unsupported(&config);

        let (render_fmt, w, h) = match &mut self.target {
            PresentTarget::Surface {
                surface,
                config: sc,
            } => {
                sc.present_mode = config.present_mode;
                if let Some(f) = config.format {
                    sc.format = f;
                    sc.view_formats.clear();
                }
                surface.configure(&self.device, sc);
                (render_format_of(sc), sc.width.max(1), sc.height.max(1))
            }
            PresentTarget::Headless {
                format,
                width,
                height,
            } => (*format, (*width).max(1), (*height).max(1)),
        };

        let dual_source = self
            .device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        self.inner = SceneRenderer::new(&self.device, render_fmt, w, h, dual_source);
        self.surface_format = render_fmt;
        self.config = config;
        // The store was just dropped with the old `inner`; drop dangling scanout state too.
        self.last_scanout_addr = None;
        self.frame_scenes.clear();
    }

    /// Deterministic teardown: drain the queue so GPU work finishes before resources release, then
    /// drop in field order. Consumes self. **Web consumers MUST call this** — on web `Drop`'s `poll`
    /// is a no-op. (P4 fires the render hook's `deinit` HERE, before the device drops.)
    pub fn shutdown(mut self) {
        // P4: fire the render hook's `deinit` before the device drops (N64 fidelity). Takes the hook
        // so `Drop` below sees `None` and does not double-fire.
        self.take_render_hook();
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// RCP: walk the display list at `entry` and rasterize every framebuffer it produces into the
    /// persistent, address-keyed store. Accumulates across calls within a frame. Touches no surface.
    /// INFALLIBLE — DL-content problems stream into `diags`; device-loss/OOM are asynchronous.
    pub fn process_dl(
        &mut self,
        hw: &impl Hardware,
        entry: u64,
        ucode: Microcode,
        diags: &mut dyn DiagSink,
    ) -> DlSummary {
        let mem = hw.rdram();
        // Contract #1/#3 (spec §3.2): record the backend kind BEFORE the reader is moved into the
        // walk — `present` gates VI-origin selection on this. RdramImage ⇒ true, HostRam ⇒ false.
        self.last_backend_was_image = mem.is_rdram_image();

        let result = crate::hle::interpret(mem, entry, ucode.into(), self.data_format);

        // Stream structured diags into the caller's sink, tallying severity for the rollup.
        let (mut warns, mut errors) = (0u32, 0u32);
        for &d in &result.diags {
            match d.kind.severity() {
                Severity::Warn => warns += 1,
                Severity::Error => errors += 1,
            }
            diags.emit(d);
        }

        let tris = (result.scene.indices.len() / 3) as u32;

        // Rasterize into the persistent store. A draw-nothing walk returns None and leaves
        // `last_scanout_addr` UNCHANGED (spec §4 step 4). RA: clear policy from self.config.
        let scanout = self.inner.render_into_store(
            &self.device,
            &self.queue,
            &result.scene,
            self.config.clear_policy,
        );
        if let Some(addr) = scanout {
            self.last_scanout_addr = Some(addr);
        }

        // Retained for the frame (P4 debugger reads all of them; cleared at begin_frame).
        self.frame_scenes.push(result.scene);

        DlSummary {
            commands: result.commands,
            tris,
            warns,
            errors,
            dropped_runs: result.dropped_runs,
            renderable: scanout.is_some(),
        }
    }

    /// Explicit frame boundary. Resets per-frame accumulation: the inner store's first-touch clear
    /// set (ClearPolicy::PerFrame) and the retained `frame_scenes`.
    pub fn begin_frame(&mut self) {
        self.inner.begin_frame();
        self.frame_scenes.clear();
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // P4: for the no-`shutdown` drop path, fire the hook's `deinit` before `self.device` drops.
        if let Some(hook) = self.hook.as_mut() {
            hook.deinit();
        }
        // Native: block until submitted work completes. On web `poll` is a no-op, so the
        // "consumer UI drops before device" guarantee holds only via `shutdown()`.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

impl Renderer {
    /// Install a render hook, firing its `init` SYNCHRONOUSLY (the device already exists). Replacing
    /// an existing hook fires the old hook's `deinit` FIRST, then the new hook's `init`.
    pub fn set_render_hook(&mut self, mut hook: Box<dyn RenderHook>) {
        if let Some(mut old) = self.hook.take() {
            old.deinit();
        }
        hook.init(&self.device, &self.queue, self.surface_format);
        self.hook = Some(hook);
    }

    /// Remove the render hook (if any), firing its `deinit` before returning it to the caller.
    pub fn take_render_hook(&mut self) -> Option<Box<dyn RenderHook>> {
        let mut hook = self.hook.take()?;
        hook.deinit();
        Some(hook)
    }

    /// Convenience for stateless overlays: wrap a draw closure as a `RenderHook`. Note the HRTB —
    /// `HookFrame` is borrowed; the closure may be `!Send` (it captures `Rc`/`RefCell` freely).
    pub fn set_draw_hook(&mut self, draw: impl for<'a> FnMut(HookFrame<'a>) + 'static) {
        self.set_render_hook(Box::new(crate::hooks::FnHook(draw)));
    }

    /// Enable/disable the built-in egui debugger (feature `debug-ui`). Default off. While off it
    /// never renders and `present_to` stays byte-identical (headless goldens unaffected).
    #[cfg(feature = "debug-ui")]
    pub fn set_debugger_enabled(&mut self, enabled: bool) {
        self.debugger.enabled = enabled;
    }

    /// Whether the built-in debugger is currently enabled.
    #[cfg(feature = "debug-ui")]
    pub fn debugger_enabled(&self) -> bool {
        self.debugger.enabled
    }

    /// Feed a UI event to the built-in debugger. Returns whether egui consumed it (so the host can
    /// withhold it from the game). No-op returning `false` while the debugger is disabled.
    #[cfg(feature = "debug-ui")]
    pub fn debugger_input(&mut self, input: &crate::debug::DebugInput) -> bool {
        self.debugger.input(input)
    }
}

/// A `VI_ORIGIN` register is an RDRAM framebuffer offset masked to 24 bits (spec §3.3). Store keys
/// are the recorded `color_image.addr` (a physical offset for `RdramImage`).
#[allow(dead_code)] // RJ bridge: consumed by `present_to`/`present` (P3.9b/P3.9c); test-only until then.
pub(crate) fn fb_address(origin: u32) -> u64 {
    (origin & 0x00FF_FFFF) as u64
}

/// Pick the framebuffer to scan out. With a live VI over a unified-RDRAM (`RdramImage`) frame,
/// `VI_ORIGIN` is authoritative IF that framebuffer exists in the store; otherwise, and for every
/// host-pointer frame (`last_backend_was_image == false`, contract #3) and every no-VI consumer,
/// fall back to the last-rendered color image (spec §4).
#[allow(dead_code)] // RJ bridge: consumed by `present_to`/`present` (P3.9b/P3.9c); test-only until then.
pub(crate) fn select_scanout_source(
    vi: Option<ViRegisters>,
    last_backend_was_image: bool,
    last_scanout_addr: Option<u64>,
    fb_present: impl Fn(u64) -> bool,
) -> Option<u64> {
    vi.filter(|_| last_backend_was_image)
        .map(|v| fb_address(v.origin))
        .filter(|addr| fb_present(*addr))
        .or(last_scanout_addr)
}

impl Renderer {
    #[allow(dead_code)] // RJ bridge: consumed by `present_to`/`present` (P3.9b/P3.9c).
    fn scanout_source(&self, vi: Option<ViRegisters>) -> Option<u64> {
        select_scanout_source(
            vi,
            self.last_backend_was_image,
            self.last_scanout_addr,
            |a| self.inner.has_fb(a),
        )
    }

    /// Shared present tail (P4). Records the render hook over the just-scanned-out `view`
    /// (LoadOp::Load), then submits the hook's returned pre-frame command buffers STRICTLY BEFORE the
    /// frame `encoder`. `HookFrame.format` is `self.surface_format` (the non-sRGB render/view format),
    /// NOT `view.texture().format()` (the swapchain texture's possibly-sRGB format). Width/height come
    /// from the view's full-texture dimensions. Shared by `present` and `present_to`.
    fn record_and_submit(&mut self, mut encoder: wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let width = view.texture().width();
        let height = view.texture().height();
        let mut extra = Vec::new();
        if let Some(hook) = self.hook.as_mut() {
            extra = hook.draw(HookFrame {
                device: &self.device,
                queue: &self.queue,
                encoder: &mut encoder,
                view,
                format: self.surface_format,
                width,
                height,
            });
        }
        #[cfg(feature = "debug-ui")]
        if self.debugger.enabled {
            self.debugger.draw(
                &self.device,
                &self.queue,
                &mut encoder,
                view,
                self.surface_format,
                &self.frame_scenes,
            );
        }
        self.queue
            .submit(extra.into_iter().chain(std::iter::once(encoder.finish())));
    }

    /// VI scanout into a CALLER-OWNED view (wafel embeds the N64 image in its own UI; headless
    /// golden capture). The caller owns acquisition + present. No-op if nothing has been rendered
    /// yet (leaves `target` as-is).
    pub fn present_to(&mut self, hw: &impl Hardware, target: &wgpu::TextureView) {
        // Always create an encoder + run the render hook, even when nothing has been scanned out yet
        // (a UI overlay should still draw — RN). Scanout is recorded only when a source FB exists;
        // with no hook and no scanout the submit is an empty no-op, so `target` is left as-is.
        let src = self.scanout_source(hw.vi());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present_to"),
            });
        if let Some(addr) = src {
            self.inner.scanout(&mut encoder, target, addr);
        }
        self.record_and_submit(encoder, target);
    }

    /// VI: scan the selected framebuffer out to the OWNED surface, then present. `fast3d` pulls the
    /// VI snapshot via `hw.vi()` itself (N64 fidelity). Auto-reconfigures the surface from the
    /// stored config on Outdated/Lost and returns `SurfaceLost` so the caller retries next frame.
    pub fn present(&mut self, hw: &impl Hardware) -> Result<(), PresentError> {
        let src = self.scanout_source(hw.vi());

        // RO: acquire in a scope so the `&self.target` borrow (surface/config) ENDS before
        // `record_and_submit` needs `&mut self` (it borrows `self.hook`). `frame` is owned.
        let frame = {
            let PresentTarget::Surface { surface, config } = &self.target else {
                // RD: Headless targets have no swapchain — use present_to(view). No debug_assert.
                log::warn!("Renderer::present called on a Headless target; use present_to instead");
                return Ok(());
            };
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    surface.configure(&self.device, config); // reconfigure from the stored config
                    return Err(PresentError::SurfaceLost); // caller retries next frame
                }
                wgpu::CurrentSurfaceTexture::Timeout => return Err(PresentError::Timeout),
                wgpu::CurrentSurfaceTexture::Occluded => return Ok(()), // nothing to show
                wgpu::CurrentSurfaceTexture::Validation => return Err(PresentError::DeviceLost),
            }
        };

        // RE: non-sRGB render/view format, NOT Default (which would reintroduce washout).
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.surface_format),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present"),
            });
        if let Some(addr) = src {
            self.inner.scanout(&mut encoder, &view, addr);
        }
        // ── SEAM (P4): hook draw() records over `view` (LoadOp::Load); its pre-frame CommandBuffers
        //    submit STRICTLY BEFORE this encoder.
        self.record_and_submit(encoder, &view);
        frame.present();
        Ok(())
    }
}

#[cfg(test)]
mod present_headless_tests {
    use super::*;

    struct ImgHw {
        rdram: Vec<u8>,
    }
    impl crate::hardware::Hardware for ImgHw {
        fn rdram(&self) -> impl crate::hardware::Rdram + '_ {
            crate::hardware::RdramImage::new(&self.rdram)
        }
    }

    #[test]
    fn present_on_a_headless_target_is_a_noop_ok() {
        // No swapchain exists for a Headless target: present() must not acquire; it returns Ok(()).
        // (The surface acquire path + 7-variant handling needs a real window — P5.3 in-browser/in-game.)
        let (device, queue, _dual) = crate::render::headless_device();
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                clear_policy: ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::LowPower,
            },
        );
        let hw = ImgHw { rdram: Vec::new() };
        assert!(r.present(&hw).is_ok());
    }
}

#[cfg(test)]
mod source_select_tests {
    use super::{fb_address, select_scanout_source};
    use crate::hardware::ViRegisters;

    fn vi(origin: u32) -> ViRegisters {
        ViRegisters {
            origin,
            ..Default::default()
        }
    }

    #[test]
    fn rdram_image_with_vi_in_store_picks_vi_origin() {
        let origin = 0x0020_0000u32;
        let got = select_scanout_source(Some(vi(origin)), true, Some(0x10_0000), |a| {
            a == fb_address(origin)
        });
        assert_eq!(got, Some(fb_address(origin)));
    }

    #[test]
    fn rdram_image_with_vi_not_in_store_falls_back() {
        let got = select_scanout_source(Some(vi(0x0090_0000)), true, Some(0x10_0000), |_| false);
        assert_eq!(
            got,
            Some(0x10_0000),
            "VI origin absent from store → last_scanout_addr"
        );
    }

    #[test]
    fn host_ram_ignores_vi_and_uses_last_scanout() {
        let got = select_scanout_source(Some(vi(0x0020_0000)), false, Some(0x10_0000), |_| true);
        assert_eq!(got, Some(0x10_0000));
    }

    #[test]
    fn no_vi_uses_last_scanout() {
        let got = select_scanout_source(None, true, Some(0x10_0000), |_| true);
        assert_eq!(got, Some(0x10_0000));
    }

    #[test]
    fn nothing_rendered_yet_is_none() {
        assert_eq!(select_scanout_source(None, true, None, |_| true), None);
    }
}

#[cfg(all(test, feature = "asm"))]
mod begin_frame_tests {
    use super::*;

    struct ImgHw {
        rdram: Vec<u8>,
    }
    impl crate::hardware::Hardware for ImgHw {
        fn rdram(&self) -> impl crate::hardware::Rdram + '_ {
            crate::hardware::RdramImage::new(&self.rdram)
        }
    }

    fn cfg() -> RendererConfig {
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        }
    }

    #[test]
    fn begin_frame_clears_retained_frame_scenes() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/scenes/flat-color.n64"),
        )
        .unwrap();
        let img = crate::asm::assemble_with_texture(&src, &[255u8; 4], 1, 1).unwrap();
        let hw = ImgHw { rdram: img.rdram };

        let (device, queue, _dual) = crate::render::headless_device();
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            cfg(),
        );

        r.begin_frame();
        r.process_dl(
            &hw,
            img.entry_addr as u64,
            Microcode::F3dex2,
            &mut crate::diag::NopSink,
        );
        r.process_dl(
            &hw,
            img.entry_addr as u64,
            Microcode::F3dex2,
            &mut crate::diag::NopSink,
        );
        assert_eq!(
            r.frame_scenes.len(),
            2,
            "two walks accumulate into frame_scenes"
        );

        r.begin_frame();
        assert_eq!(
            r.frame_scenes.len(),
            0,
            "begin_frame resets per-frame accumulation"
        );
    }

    /// spec §4 step 4: a transient empty/failed DL must NOT blank the screen — `process_dl` leaves
    /// `last_scanout_addr` pointing at the last good frame (the `if let Some(addr) = scanout` guard).
    /// A regression to an unconditional `self.last_scanout_addr = scanout;` would drop the last good
    /// frame to black on any frame carrying an empty DL; this test is what catches that.
    #[test]
    fn process_dl_draw_nothing_keeps_last_good_scanout_addr() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/scenes/flat-color.n64"),
        )
        .unwrap();
        let img = crate::asm::assemble_with_texture(&src, &[255u8; 4], 1, 1).unwrap();
        let good = ImgHw { rdram: img.rdram };
        let empty = ImgHw { rdram: Vec::new() }; // out-of-bounds entry → draw-nothing walk

        let (device, queue, _dual) = crate::render::headless_device();
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            cfg(),
        );

        r.begin_frame();
        let good_summary = r.process_dl(
            &good,
            img.entry_addr as u64,
            Microcode::F3dex2,
            &mut crate::diag::NopSink,
        );
        assert!(good_summary.renderable, "a valid DL produces a framebuffer");
        let good_addr = r.last_scanout_addr;
        assert!(good_addr.is_some(), "a valid DL sets last_scanout_addr");

        // A draw-nothing DL in the same frame must leave last_scanout_addr UNCHANGED.
        let s = r.process_dl(&empty, 0, Microcode::F3dex2, &mut crate::diag::NopSink);
        assert!(!s.renderable, "an out-of-bounds/empty DL renders nothing");
        assert_eq!(
            r.last_scanout_addr, good_addr,
            "draw-nothing must keep the last good frame (spec §4 step 4)"
        );
    }
}

#[cfg(test)]
mod config_construction_tests {
    use super::*;

    #[test]
    fn renderer_config_and_clear_policy_construct() {
        let cfg = RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::HighPerformance,
        };
        assert_eq!(cfg.clear_policy, ClearPolicy::Persist);
        assert_eq!(cfg.resolution_multiplier, 1);
        let cfg2 = cfg; // Copy
        assert_eq!(cfg, cfg2); // Eq (reconfigure diffs config)
        assert_ne!(ClearPolicy::PerFrame, ClearPolicy::Persist);
    }

    #[test]
    fn present_target_headless_variant_fields() {
        let t = PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 320,
            height: 240,
        };
        match t {
            PresentTarget::Headless {
                format,
                width,
                height,
            } => {
                assert_eq!(format, wgpu::TextureFormat::Rgba8Unorm);
                assert_eq!((width, height), (320, 240));
            }
            PresentTarget::Surface { .. } => panic!("expected Headless"),
        }
    }

    #[test]
    fn init_error_is_std_error_with_source_and_display() {
        fn assert_error<E: std::error::Error>() {}
        fn assert_display<T: std::fmt::Display>() {}
        fn assert_from<S, D: From<S>>() {}
        assert_error::<InitError>();
        assert_display::<InitError>();
        assert_from::<wgpu::RequestAdapterError, InitError>();
        assert_from::<wgpu::RequestDeviceError, InitError>();
        assert_from::<wgpu::CreateSurfaceError, InitError>();
    }

    #[test]
    fn present_error_variants_are_debug() {
        let e = PresentError::SurfaceLost;
        assert_eq!(format!("{e:?}"), "SurfaceLost");
        let _ = PresentError::Timeout;
        let _ = PresentError::DeviceLost;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn with_device_headless_builds_and_exposes_handles() {
        let (device, queue, _dual) = crate::render::headless_device();
        let cfg = RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::HighPerformance,
        };
        let r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            cfg,
        );
        let _ = r.device().limits();
        let _ = r.queue();
        assert_eq!(r.surface_format, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(r.config.clear_policy, ClearPolicy::PerFrame);
        assert!(r.last_scanout_addr.is_none());
        assert!(!r.last_backend_was_image);
        assert!(r.frame_scenes.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn shutdown_headless_drains_without_panic() {
        let (device, queue, _dual) = crate::render::headless_device();
        let cfg = RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::HighPerformance,
        };
        let r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 32,
                height: 32,
            },
            cfg,
        );
        r.shutdown(); // drains via device.poll(wait_indefinitely) and consumes self; Drop then runs.
    }

    #[test]
    fn pick_surface_format_never_trusts_caps_zero() {
        use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm};
        // Only sRGB offered: keep the sRGB SURFACE format, render through a non-sRGB VIEW.
        assert_eq!(
            pick_surface_format(&[Bgra8UnormSrgb], None),
            (Bgra8UnormSrgb, Some(Bgra8Unorm))
        );
        // A non-sRGB member exists → use it directly, no view flip.
        assert_eq!(
            pick_surface_format(&[Bgra8UnormSrgb, Bgra8Unorm], None),
            (Bgra8Unorm, None)
        );
        assert_eq!(pick_surface_format(&[Rgba8Unorm], None), (Rgba8Unorm, None));
        // An explicit override is honored verbatim (helix passes Some(Bgra8Unorm)).
        assert_eq!(
            pick_surface_format(&[Bgra8UnormSrgb], Some(Bgra8Unorm)),
            (Bgra8Unorm, None)
        );
        // Degenerate empty caps → safe non-sRGB default.
        assert_eq!(pick_surface_format(&[], None), (Bgra8Unorm, None));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn resize_headless_is_noop_and_keeps_surface_format() {
        let (device, queue, _dual) = crate::render::headless_device();
        let cfg = RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::HighPerformance,
        };
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            cfg,
        );
        r.resize(128, 96); // Headless: no owned surface — must not panic.
        assert_eq!(r.surface_format, wgpu::TextureFormat::Rgba8Unorm);
        let _ = r.device().limits();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reconfigure_headless_rebuilds_and_stores_config() {
        let (device, queue, _dual) = crate::render::headless_device();
        let base = RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: None,
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::HighPerformance,
        };
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 64,
                height: 64,
            },
            base,
        );
        let next = RendererConfig {
            present_mode: wgpu::PresentMode::Immediate,
            ..base
        };
        r.reconfigure(next); // drains + rebuilds inner; must not panic.
        assert_eq!(r.config.present_mode, wgpu::PresentMode::Immediate);
        assert_eq!(r.surface_format, wgpu::TextureFormat::Rgba8Unorm); // Headless format unchanged
        let _ = r.device().limits();
    }

    // `new` needs a real window/canvas surface (unavailable headless). This never-called fn only
    // TYPE-CHECKS the exact async signature + the `?`→InitError wiring; runtime coverage is P5.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(dead_code)]
    async fn _new_signature_typechecks(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<Renderer, InitError> {
        Renderer::new(
            target,
            320,
            240,
            RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::AutoVsync,
                format: None,
                clear_policy: ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::HighPerformance,
            },
        )
        .await
    }
}

#[cfg(all(test, feature = "debug-ui", not(target_arch = "wasm32")))]
mod debugger_toggle_tests {
    use super::*;
    // In-crate test module: the current crate is reached via `crate::`, NOT its package name
    // `fast3d::` (lib.rs has no `extern crate self as fast3d`) — using `fast3d::` here would fail with
    // E0433 `use of undeclared crate or module 'fast3d'`.
    use crate::debug::{DebugButton, DebugInput};
    use crate::render::headless_device;

    fn cfg() -> RendererConfig {
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        }
    }

    #[test]
    fn debugger_is_off_by_default_and_toggles() {
        let (device, queue, _dual) = headless_device();
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 8,
                height: 8,
            },
            cfg(),
        );
        assert!(
            !r.debugger_enabled(),
            "the built-in debugger is default-off"
        );
        // Disabled: input is a no-op that reports no consumption.
        assert!(!r.debugger_input(&DebugInput::PointerMoved { x: 1.0, y: 2.0 }));
        r.set_debugger_enabled(true);
        assert!(r.debugger_enabled());
        // Enabled: the event is buffered; egui reports no consumption yet (no prior frame) => false.
        assert!(!r.debugger_input(&DebugInput::PointerButton {
            button: DebugButton::Left,
            pressed: true,
        }));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod hook_lifecycle_tests {
    use super::*;
    use crate::render::headless_device;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<&'static str>>>;

    struct Spy {
        init_tag: &'static str,
        deinit_tag: &'static str,
        log: Log,
    }
    impl RenderHook for Spy {
        fn init(&mut self, _d: &wgpu::Device, _q: &wgpu::Queue, _f: wgpu::TextureFormat) {
            self.log.borrow_mut().push(self.init_tag);
        }
        fn draw(&mut self, _frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer> {
            Vec::new()
        }
        fn deinit(&mut self) {
            self.log.borrow_mut().push(self.deinit_tag);
        }
    }

    fn cfg() -> RendererConfig {
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        }
    }

    fn hw_renderer() -> Renderer {
        let (device, queue, _dual) = headless_device();
        Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 8,
                height: 8,
            },
            cfg(),
        )
    }

    fn spy(init_tag: &'static str, deinit_tag: &'static str, log: &Log) -> Box<Spy> {
        Box::new(Spy {
            init_tag,
            deinit_tag,
            log: log.clone(),
        })
    }

    #[test]
    fn set_render_hook_fires_init_once_and_replace_orders_deinit_before_init() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut r = hw_renderer();
        r.set_render_hook(spy("A.init", "A.deinit", &log));
        assert_eq!(
            *log.borrow(),
            ["A.init"],
            "init fires synchronously, exactly once"
        );
        r.set_render_hook(spy("B.init", "B.deinit", &log));
        assert_eq!(
            *log.borrow(),
            ["A.init", "A.deinit", "B.init"],
            "replacing A with B = A.deinit() THEN B.init(), in that order"
        );
    }

    #[test]
    fn take_render_hook_fires_deinit_and_removes() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut r = hw_renderer();
        r.set_render_hook(spy("A.init", "A.deinit", &log));
        assert!(r.take_render_hook().is_some());
        assert_eq!(*log.borrow(), ["A.init", "A.deinit"]);
        assert!(r.take_render_hook().is_none(), "a second take is None");
    }

    #[test]
    fn shutdown_fires_deinit_before_device_drops() {
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        let mut r = hw_renderer();
        r.set_render_hook(spy("A.init", "A.deinit", &log));
        r.shutdown();
        assert_eq!(*log.borrow(), ["A.init", "A.deinit"]);
    }

    #[test]
    fn drop_without_shutdown_fires_deinit() {
        // The `impl Drop for Renderer` fires the hook's `deinit` on the no-`shutdown` path (a
        // load-bearing lifecycle trigger). Install a hook, then let the Renderer go out of scope
        // WITHOUT take_render_hook/shutdown, so Drop sees `Some(hook)` and fires it. The
        // `Rc<RefCell<_>>` log is cloned into the hook, so it outlives the Renderer and stays readable
        // after the drop. If Drop's `hook.deinit()` were removed, "A.deinit" would never be logged and
        // this assertion would fail.
        let log: Log = Rc::new(RefCell::new(Vec::new()));
        {
            let mut r = hw_renderer();
            r.set_render_hook(spy("A.init", "A.deinit", &log));
            assert_eq!(*log.borrow(), ["A.init"], "init fired on install");
            // No take_render_hook / shutdown here: `r` drops at the end of this scope, exercising
            // `impl Drop for Renderer` with a hook still installed.
        }
        assert_eq!(
            *log.borrow(),
            ["A.init", "A.deinit"],
            "Drop fired the hook's deinit on the no-shutdown path"
        );
    }

    #[test]
    fn set_draw_hook_accepts_a_non_send_closure() {
        // Rc is !Send — this compiling proves the pragmatic !Send guard + the HRTB bound.
        let counter = Rc::new(RefCell::new(0u32));
        let c = counter.clone();
        let mut r = hw_renderer();
        r.set_draw_hook(move |_frame: HookFrame<'_>| {
            *c.borrow_mut() += 1;
        });
        assert!(
            r.take_render_hook().is_some(),
            "the closure was installed as a RenderHook"
        );
    }
}

#[cfg(all(
    test,
    feature = "asm",
    feature = "debug-ui",
    not(target_arch = "wasm32")
))]
mod debugger_present_tests {
    use super::*;
    use crate::render::headless_device;

    fn cfg() -> RendererConfig {
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        }
    }

    struct ImgHw {
        rdram: Vec<u8>,
    }
    impl crate::hardware::Hardware for ImgHw {
        fn rdram(&self) -> impl crate::hardware::Rdram + '_ {
            crate::hardware::RdramImage::new(&self.rdram)
        }
    }

    #[test]
    fn debugger_composites_over_scanout_without_erasing_the_game() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/scenes/flat-color.n64"),
        )
        .unwrap();
        let img = crate::asm::assemble_with_texture(&src, &[255u8; 4], 1, 1).unwrap();
        let hw = ImgHw { rdram: img.rdram };

        let (device, queue, _dual) = headless_device();
        let mut r = Renderer::with_device(
            device,
            queue,
            PresentTarget::Headless {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: 256,
                height: 256,
            },
            cfg(),
        );
        r.set_debugger_enabled(true);
        r.begin_frame();
        // TWO walks in ONE frame accumulate TWO scenes in `frame_scenes`. present_to must forward the
        // FULL list to the debugger (a `&frame_scenes[len-1..]` last-only wiring regression would leave
        // last_scene_count == 1). This is the end-to-end guard the direct-`draw` render_test cannot give.
        r.process_dl(&hw, img.entry_addr as u64, Microcode::F3dex2, &mut NopSink);
        r.process_dl(&hw, img.entry_addr as u64, Microcode::F3dex2, &mut NopSink);

        let target = r.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("dbg-present"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        r.present_to(&hw, &view); // must not panic; the debugger paints its small top-left window

        // END-TO-END last-only guard: the two `process_dl` walks above accumulated TWO scenes in
        // `frame_scenes`, and `record_and_submit` must have forwarded the FULL list to the debugger.
        // A `&self.frame_scenes[len-1..]` wiring regression would leave this at 1. The direct-`draw`
        // `render_tests` case cannot catch that (it hands `draw` the slice itself); this drives the
        // real `present_to`/`record_and_submit` caller. (`debugger` is a private field of the
        // crate-root `Renderer`; this descendant test module may read it, and `last_scene_count` is
        // `pub(crate)`.)
        assert_eq!(
            r.debugger.last_scene_count, 2,
            "record_and_submit forwarded ALL frame_scenes to the debugger (not last-only)"
        );

        let buf = r.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("rb"),
            size: 256 * 256 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = r
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256 * 4),
                    rows_per_image: Some(256),
                },
            },
            wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
        );
        r.queue().submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| tx.send(res).unwrap());
        r.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = slice.get_mapped_range().to_vec();

        // Center (128,128) is far from the small top-left debugger window → still the scanned-out game.
        let off = (128 * 256 + 128) * 4;
        let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
        let near = |a: u8, b: u8| (a as i16 - b as i16).abs() <= 2;
        assert!(
            near(px[0], 64) && near(px[1], 200) && near(px[2], 255) && near(px[3], 255),
            "the debugger Load-composited over the scan-out; center pixel is still the game PRIM: {px:?}"
        );
    }
}

#[cfg(all(test, feature = "asm"))]
mod tests;
