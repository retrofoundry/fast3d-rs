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

fn store_pixels(r: &Renderer, addr: u64) -> Vec<u8> {
    super::common::pixels_from_render(
        r.device(),
        r.queue(),
        64,
        64,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| {
            let mut encoder = r.device().create_command_encoder(&Default::default());
            r.inner.scanout(&mut encoder, view, addr);
            r.queue().submit(Some(encoder.finish()));
        },
    )
}

#[test]
fn observed_render_matches_summary_scenes_and_every_framebuffer() {
    use crate::inspect::{WalkStep, WalkTermination};
    use std::ops::ControlFlow;

    let mut ordinary = headless_renderer();
    let mut observed = headless_renderer();
    ordinary.begin_frame();
    observed.begin_frame();
    for name in [
        "flat-color--white1",
        "multi-material",
        "hud-over-3d",
        "fill-texrect",
        "offscreen-then-sample",
    ] {
        let (rdram, entry) = super::fixtures::fixture(name);
        let hw = ImgHw {
            rdram: rdram.to_vec(),
        };
        let (mut ordinary_diags, mut observed_diags) = (vec![], vec![]);
        let expected = ordinary.process_dl(&hw, entry, Microcode::F3dex2, &mut ordinary_diags);
        let mut count = 0;
        let actual = observed.process_dl_observed(
            &hw,
            entry,
            Microcode::F3dex2,
            &mut observed_diags,
            &mut |step: WalkStep<'_>| {
                assert_eq!(step.seq, count);
                count += 1;
                for &emission in step.emissions {
                    super::inspect::assert_emission(
                        ordinary.frame_scenes.last().unwrap(),
                        emission,
                    );
                }
                ControlFlow::Continue(())
            },
        );
        assert_eq!(actual, expected, "{name}");
        assert_eq!(actual.termination, WalkTermination::End);
        assert_eq!(count, actual.commands);
        assert_eq!(observed_diags, ordinary_diags);
        assert_eq!(observed.frame_scenes, ordinary.frame_scenes);
        assert_eq!(observed.last_scanout_addr, ordinary.last_scanout_addr);
        for scene in &ordinary.frame_scenes {
            let addresses: Vec<_> = if scene.framebuffer_pairs.is_empty() {
                vec![scene.color_image.addr]
            } else {
                scene
                    .framebuffer_pairs
                    .iter()
                    .filter(|p| !p.is_depth_clear)
                    .map(|p| p.color_image.addr)
                    .collect()
            };
            for addr in addresses {
                assert!(ordinary.inner.has_fb(addr));
                assert!(observed.inner.has_fb(addr));
                assert_eq!(
                    store_pixels(&observed, addr),
                    store_pixels(&ordinary, addr),
                    "{name}: {addr:#x}"
                );
            }
        }
    }
}

fn fill_hw(addr: u32, color: u32) -> ImgHw {
    use n64_gbi::encode::*;
    ImgHw {
        rdram: [
            gdp_set_color_image(0, 2, 64, addr),
            gdp_set_scissor(0, 0, 0, 256, 256),
            gdp_set_fill_color(color),
            gdp_fill_rectangle(0, 0, 252, 252),
            gsp_enddl(),
        ]
        .into_iter()
        .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
        .collect(),
    }
}

#[test]
fn cancelled_render_preserves_previous_dls_and_does_not_create_framebuffers() {
    use crate::inspect::{Emission, WalkStep, WalkTermination};
    use std::ops::ControlFlow;

    let mut r = headless_renderer();
    r.begin_frame();
    r.process_dl(
        &fill_hw(0x100000, 0xf801f801),
        0,
        Microcode::F3dex2,
        &mut crate::NopSink,
    );
    r.process_dl(
        &fill_hw(0x200000, 0x07c107c1),
        0,
        Microcode::F3dex2,
        &mut crate::NopSink,
    );
    r.last_backend_was_image = false;
    let scenes = r.frame_scenes.clone();
    let before = [store_pixels(&r, 0x100000), store_pixels(&r, 0x200000)];
    assert_ne!(before[0], before[1]);
    for addr in [0x100000, 0x300000] {
        let mut count = 0;
        let summary = r.process_dl_observed(
            &fill_hw(addr, 0x003f003f),
            0,
            Microcode::F3dex2,
            &mut crate::NopSink,
            &mut |step: WalkStep<'_>| {
                count += 1;
                if step
                    .emissions
                    .iter()
                    .any(|e| matches!(e, Emission::FillRect { .. }))
                {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        assert_eq!(summary.commands, 4);
        assert_eq!(summary.commands, count);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
        assert!(!summary.renderable);
        assert_eq!(r.frame_scenes, scenes);
        assert!(!r.last_backend_was_image);
        assert_eq!(r.last_scanout_addr, Some(0x200000));
        assert_eq!(store_pixels(&r, 0x100000), before[0]);
        assert_eq!(store_pixels(&r, 0x200000), before[1]);
        assert!(!r.inner.has_fb(0x300000));
    }
    r.process_dl(
        &fill_hw(0x100000, 0x003f003f),
        0,
        Microcode::F3dex2,
        &mut crate::NopSink,
    );
    assert_ne!(store_pixels(&r, 0x100000), before[0]);
}

#[test]
fn observed_renderer_has_no_cpu_inspection_cap() {
    use crate::inspect::{WalkStep, WalkTermination, MAX_DISPATCHES};
    use n64_gbi::encode::*;
    use std::ops::ControlFlow;

    let mut r = headless_renderer();
    let hw = ImgHw {
        rdram: std::iter::repeat_n(gdp_set_fill_color(0), MAX_DISPATCHES as usize)
            .chain([gsp_enddl()])
            .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
            .collect(),
    };
    let mut count = 0;
    let summary = r.process_dl_observed(
        &hw,
        0,
        Microcode::F3dex2,
        &mut crate::NopSink,
        &mut |_: WalkStep<'_>| {
            count += 1;
            ControlFlow::Continue(())
        },
    );
    assert_eq!(summary.commands, MAX_DISPATCHES + 1);
    assert_eq!(count, summary.commands);
    assert_eq!(summary.termination, WalkTermination::End);
}

#[path = "renderer_dl_prefix.rs"]
mod prefix;
