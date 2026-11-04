//! P3.8: `Renderer::process_dl` walks a DL into the persistent store and returns a `DlSummary`.

use crate::render::workload::TargetId;
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

fn store_pixels(r: &mut Renderer, addr: u64) -> Vec<u8> {
    target_pixels(r, TargetId::Guest(addr))
}

fn target_pixels(r: &mut Renderer, target: TargetId) -> Vec<u8> {
    let device = r.device().clone();
    let queue = r.queue().clone();
    super::common::pixels_from_render(
        &device,
        &queue,
        64,
        64,
        wgpu::TextureFormat::Rgba8Unorm,
        |view| {
            let mut encoder = r.device().create_command_encoder(&Default::default());
            r.inner.scanout(&device, &mut encoder, view, target, None);
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
        for scene in &ordinary.frame_scenes.clone() {
            let targets = (!scene.draw_runs.is_empty())
                .then_some(TargetId::Legacy)
                .into_iter()
                .chain(
                    scene
                        .framebuffer_pairs
                        .iter()
                        .filter(|p| !p.is_depth_clear)
                        .map(|p| TargetId::Guest(p.color_image.addr)),
                );
            for addr in targets {
                assert!(ordinary.inner.has_fb(addr));
                assert!(observed.inner.has_fb(addr));
                assert_eq!(
                    target_pixels(&mut observed, addr),
                    target_pixels(&mut ordinary, addr),
                    "{name}: {addr:?}"
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
            gdp_set_cycle_type(crate::hle::consts::G_CYC_FILL),
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
fn fillrect_primitive_alpha_and_cycle_edges() {
    use n64_gbi::consts::rdp::*;
    use n64_gbi::encode::*;

    let color = CcPass {
        a: 15,
        b: 15,
        c: 31,
        d: 3,
    };
    let alpha = CcPass {
        a: 7,
        b: 7,
        c: 7,
        d: 3,
    };
    let mode1 = IM_RD | CVG_DST_FULL | FORCE_BL | gbl_c1(CLR_IN, A_IN, CLR_MEM, B_1MA);
    let mode2 = gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA);
    let mut renderer = headless_renderer();
    for cycle in [0, G_CYC_FILL] {
        for prim_alpha in [0, 255] {
            renderer.begin_frame();
            let mut diags = Vec::new();
            renderer.process_dl(
                &fill_hw(0x100000, 0x07c107c1),
                0,
                Microcode::F3dex2,
                &mut diags,
            );
            assert!(diags.is_empty(), "{diags:?}");
            let before = store_pixels(&mut renderer, 0x100000);
            assert!(before
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 255, 0, 255]));
            let hw = ImgHw {
                rdram: [
                    gdp_set_cycle_type(cycle),
                    gdp_set_render_mode(mode1, mode2),
                    gdp_set_combine_lerp(color, alpha, color, alpha),
                    gdp_set_prim_color(0, 0, 0xff000000 | prim_alpha),
                    gdp_set_fill_color(0x003f003f),
                    gdp_fill_rectangle(0, 0, 40, 40),
                    gsp_enddl(),
                ]
                .into_iter()
                .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
                .collect(),
            };
            let summary = renderer.process_dl(&hw, 0, Microcode::F3dex2, &mut diags);
            assert!(diags.is_empty(), "{diags:?}");
            assert!(summary.renderable);
            let pixels = store_pixels(&mut renderer, 0x100000);
            for y in 0..64 {
                for x in 0..64 {
                    let expected = if cycle == G_CYC_FILL && x <= 10 && y <= 10 {
                        [0, 0, 255, 255]
                    } else if cycle == 0 && x < 10 && y < 10 {
                        if prim_alpha == 0 {
                            [0, 255, 0, 0]
                        } else {
                            [255, 0, 0, 255]
                        }
                    } else {
                        [0, 255, 0, 255]
                    };
                    assert_eq!(
                        crate::tests::common::pixel(&pixels, 64, x, y),
                        expected,
                        "cycle {cycle}, alpha {prim_alpha}, pixel ({x}, {y})"
                    );
                }
            }
        }
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
    let before = [
        store_pixels(&mut r, 0x100000),
        store_pixels(&mut r, 0x200000),
    ];
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
        assert_eq!(summary.commands, 5);
        assert_eq!(summary.commands, count);
        assert_eq!(summary.termination, WalkTermination::ObserverStopped);
        assert!(!summary.renderable);
        assert_eq!(r.frame_scenes, scenes);
        assert!(!r.last_backend_was_image);
        assert_eq!(r.last_scanout_addr, Some(TargetId::Guest(0x200000)));
        assert_eq!(store_pixels(&mut r, 0x100000), before[0]);
        assert_eq!(store_pixels(&mut r, 0x200000), before[1]);
        assert!(!r.inner.has_fb(0x300000));
    }
    r.process_dl(
        &fill_hw(0x100000, 0x003f003f),
        0,
        Microcode::F3dex2,
        &mut crate::NopSink,
    );
    assert_ne!(store_pixels(&mut r, 0x100000), before[0]);
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

#[path = "rdp_state.rs"]
mod rdp_state;
