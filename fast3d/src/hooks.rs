//! Render hooks (spec §3.5): a per-frame seam that composites consumer UI (egui/imgui/overlays) onto
//! the final N64 frame, AFTER VI scan-out and BEFORE `fast3d` submits + presents. Mirrors RT64's
//! `viRenderer->render → drawHook` present sequence. No public bound adds `Send`/`Sync` (spec §3.1).

/// A consumer render hook. `init`/`deinit` are lifecycle callbacks (defaulted no-ops); `draw` runs
/// every present.
pub trait RenderHook {
    /// Once, when the hook is registered (the device already exists — `set_render_hook` fires this
    /// synchronously). Build pipelines/samplers/an egui-wgpu `Renderer`. `format` is the surface
    /// format your draw-time pipelines must target.
    fn init(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue, _format: wgpu::TextureFormat) {
    }

    /// Every present, AFTER VI scan-out has been recorded into `frame.view`, BEFORE `fast3d`
    /// submits + presents. Record commands into `frame.encoder` targeting `frame.view` with
    /// `LoadOp::Load` (Clear would erase the game). Return any standalone command buffers that
    /// MUST run BEFORE the frame encoder (egui-wgpu `update_buffers` output); else `Vec::new()`.
    fn draw(&mut self, frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer>;

    /// Once on removal / shutdown, before the device drops.
    fn deinit(&mut self) {}
}

/// The per-present handoff to a `RenderHook`. Passed BY VALUE so the `&mut encoder` borrow expires at
/// `draw`'s return and `fast3d` regains its encoder to `.finish()`.
pub struct HookFrame<'a> {
    /// egui-wgpu uploads every frame; RT64 gives only `init` the device — a deliberate, harmless
    /// divergence (owned `&`).
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// Mid-recording; record into it (LoadOp::Load) but do NOT `finish`/`submit`.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The scanned-out target view.
    pub view: &'a wgpu::TextureView,
    /// The renderer's non-sRGB render/view format your draw pipelines must target.
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

/// Wraps a stateless draw closure as a `RenderHook` (no `init`/`deinit`, never emits pre-frame
/// command buffers). Crate-internal — backs `Renderer::set_draw_hook`; not re-exported.
pub(crate) struct FnHook<F>(pub(crate) F);

impl<F> RenderHook for FnHook<F>
where
    F: for<'a> FnMut(HookFrame<'a>) + 'static,
{
    fn draw(&mut self, frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer> {
        (self.0)(frame);
        Vec::new()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::render::headless_device;

    struct NoopHook;
    impl RenderHook for NoopHook {
        fn draw(&mut self, _frame: HookFrame<'_>) -> Vec<wgpu::CommandBuffer> {
            Vec::new()
        }
    }

    #[test]
    fn hook_frame_releases_the_encoder_borrow_at_draw_return() {
        let (device, queue, _dual) = headless_device();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hf-view"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let mut hook = NoopHook;
        let extra = hook.draw(HookFrame {
            device: &device,
            queue: &queue,
            encoder: &mut encoder,
            view: &view,
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 4,
            height: 4,
        });
        assert!(extra.is_empty());
        // The `&mut encoder` borrow expired at draw's return → we regain the encoder to finish it.
        queue.submit(std::iter::once(encoder.finish()));
    }
}
