//! P3.8: `Renderer::process_dl` walks a DL into the persistent store and returns a `DlSummary`.

use crate::{
    ClearPolicy, Diagnostic, DlSummary, Hardware, Microcode, PresentTarget, Rdram, RdramImage,
    Renderer, RendererConfig,
};

/// A byte-image N64 (web/wafel class): safe `RdramImage`, no live VI.
struct ImgHw {
    rdram: Vec<u8>,
}
impl Hardware for ImgHw {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.rdram)
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

fn headless_renderer() -> Renderer {
    let (device, queue, _dual) = crate::render::headless_device();
    Renderer::with_device(
        device,
        queue,
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        cfg(),
    )
}

fn flat_color_hw() -> (ImgHw, u64) {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("flat-color--white1");
    (
        ImgHw {
            rdram: rdram.to_vec(),
        },
        entry_addr,
    )
}

#[test]
fn process_dl_of_flat_quad_is_renderable() {
    let (hw, entry) = flat_color_hw();
    let mut r = headless_renderer();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let s: DlSummary = r.process_dl(&hw, entry, Microcode::F3dex2, &mut diags);
    assert!(diags.is_empty(), "clean DL emits no diags: {diags:?}");
    assert_eq!(s.tris, 2, "flat-color is a 2-triangle quad (6 indices / 3)");
    assert_eq!(s.errors, 0);
    assert_eq!(s.warns, 0);
    assert!(s.renderable, "a drawable framebuffer was produced");
}

#[test]
fn process_dl_of_out_of_bounds_entry_reports_error_without_panic() {
    // Empty RDRAM: the first bounds check fails → a DlPastRdram Error diag, no geometry, no panic.
    let hw = ImgHw { rdram: Vec::new() };
    let mut r = headless_renderer();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let s = r.process_dl(&hw, 0, Microcode::F3dex2, &mut diags);
    assert!(
        s.errors >= 1,
        "out-of-bounds DL is an Error diag (RG: DlPastRdram → Error)"
    );
    assert_eq!(s.tris, 0);
    assert!(!s.renderable, "draw-nothing produced no framebuffer (RC)");
    assert_eq!(
        diags.len() as u32,
        s.warns + s.errors,
        "every diag was streamed to the sink"
    );
}
