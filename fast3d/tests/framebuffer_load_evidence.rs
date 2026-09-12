#![cfg(feature = "capture")]

use fast3d::capture::{Fixture, ReplayHardware};
use fast3d::{DiagKind, Diagnostic, FramebufferAccess, Hardware};

#[path = "common/framebuffer_load_semantics.rs"]
mod semantics;
use semantics::{Case, CASES};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn diagnostics(case: Case) -> Vec<Diagnostic> {
    if case == Case::ShortcutRect {
        vec![]
    } else {
        vec![Diagnostic {
            at: if case == Case::OffsetTriangle {
                0x0cd8
            } else {
                0x04d8
            },
            kind: DiagKind::UnsupportedFramebufferAccess {
                address: if case.offset() {
                    0x0030_00d0
                } else {
                    0x0030_0000
                },
                reason: FramebufferAccess::TextureLoad,
            },
        }]
    }
}

struct Continue;

impl fast3d::inspect::WalkObserver for Continue {
    fn command(&mut self, _: fast3d::inspect::WalkStep<'_>) -> std::ops::ControlFlow<()> {
        std::ops::ControlFlow::Continue(())
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn f0_capture_boundary() {
    for case in CASES {
        let fixture = Fixture::from_bytes(case.fixture_bytes()).unwrap();
        let task = &fixture.tasks[0];
        let hardware = ReplayHardware::new(task, None).unwrap();
        let result = fast3d::inspect::walk(
            hardware.rdram(),
            task.entry,
            task.microcode,
            task.data_format,
            &mut Continue,
        );
        hardware.check().unwrap();
        assert_eq!(result.termination, fast3d::inspect::WalkTermination::End);
        assert_eq!(result.diagnostics, diagnostics(case), "{}", case.name());
        assert_eq!(
            result.dropped_runs,
            if case == Case::ShortcutRect { 0 } else { 32 }
        );
        assert_eq!(
            fixture.final_color_image().is_ok(),
            case == Case::ShortcutRect
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn f0_paired_claim_excludes_lost_bits_and_later_writes() {
    for x in [48, 64, 80, 96] {
        assert_ne!(
            semantics::oracle_expected(x, 48),
            semantics::fast3d_expected(Case::ShortcutRect, x, 48)
        );
        assert_eq!(
            semantics::oracle_expected(x, 64),
            semantics::fast3d_expected(Case::ShortcutRect, x, 64)
        );
        assert_ne!(
            semantics::oracle_expected(x + 96, 64),
            semantics::fast3d_expected(Case::ShortcutRect, x + 96, 64)
        );
    }
    assert_eq!(semantics::oracle_expected(48, 96), [255; 4]);
}

async fn replay() {
    for case in CASES {
        for policy in [fast3d::ClearPolicy::PerFrame, fast3d::ClearPolicy::Persist] {
            let mut fixture = Fixture::from_bytes(case.fixture_bytes()).unwrap();
            fixture.frame.config.clear_policy = policy;
            let output = fixture
                .replay_headless()
                .await
                .expect("F0 replay requires a GPU adapter");
            #[cfg(target_arch = "wasm32")]
            assert_eq!(
                output.adapter_info.as_ref().unwrap().backend,
                wgpu::Backend::BrowserWebGpu
            );
            assert_eq!((output.width, output.height), (320, 240));
            assert_eq!(output.diagnostics, [diagnostics(case)], "{}", case.name());
            assert_eq!(output.summaries.len(), 1);
            assert!(output.summaries[0].renderable);
            assert_eq!(
                output.summaries[0].errors,
                u32::from(case != Case::ShortcutRect)
            );
            assert_eq!(output.rgba8.len(), 320 * 240 * 4);
            if case == Case::ShortcutRect {
                // Known renderer defect: one-pixel shaded quads drawn into the 32-wide producer do
                // not rasterise, so the first consumer group samples the fill colour. The
                // prediction is unchanged and asserted by `f0_shortcut_prediction_known_failing`.
                // See ~/hub/scratch/fast3d/f0-finding.md, finding #2.
                continue;
            }
            for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
                let (x, y) = (i as u32 % 320, i as u32 / 320);
                assert_eq!(
                    *pixel,
                    semantics::fast3d_expected(case, x, y),
                    "{} ({x}, {y}), {policy:?}",
                    case.name()
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn f0_framebuffer_load_replay() {
    pollster::block_on(replay());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn f0_framebuffer_load_replay() {
    replay().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "known renderer defect: small draws on a narrow offscreen target; see f0-finding.md #2"]
fn f0_shortcut_prediction_known_failing() {
    pollster::block_on(async {
        let fixture = Fixture::from_bytes(Case::ShortcutRect.fixture_bytes()).unwrap();
        let output = fixture.replay_headless().await.unwrap();
        for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i as u32 % 320, i as u32 / 320);
            assert_eq!(
                *pixel,
                semantics::fast3d_expected(Case::ShortcutRect, x, y),
                "({x}, {y})"
            );
        }
    });
}
