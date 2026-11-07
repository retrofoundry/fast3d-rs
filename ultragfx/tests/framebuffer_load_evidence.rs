#![cfg(feature = "capture")]

use ultragfx::capture::{Fixture, ReplayHardware};
use ultragfx::Hardware;

#[allow(dead_code)]
#[path = "common/framebuffer_load_semantics.rs"]
mod semantics;
use semantics::{Case, CASES};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct Continue;

impl ultragfx::inspect::WalkObserver for Continue {
    fn command(&mut self, _: ultragfx::inspect::WalkStep<'_>) -> std::ops::ControlFlow<()> {
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
        let result = ultragfx::inspect::walk(
            hardware.rdram(),
            task.entry,
            task.microcode,
            task.data_format,
            &mut Continue,
        );
        hardware.check().unwrap();
        assert_eq!(result.termination, ultragfx::inspect::WalkTermination::End);
        assert!(
            result.diagnostics.is_empty(),
            "{}: {:?}",
            case.name(),
            result.diagnostics
        );
        assert_eq!(result.dropped_runs, 0);
        assert!(fixture.final_color_image().is_ok());
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
        for policy in [
            ultragfx::ClearPolicy::PerFrame,
            ultragfx::ClearPolicy::Persist,
        ] {
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
            assert!(
                output.diagnostics.iter().all(Vec::is_empty),
                "{}: {:?}",
                case.name(),
                output.diagnostics
            );
            assert_eq!(output.summaries.len(), 1);
            assert!(output.summaries[0].renderable);
            assert_eq!(output.summaries[0].errors, 0);
            assert_eq!(output.rgba8.len(), 320 * 240 * 4);
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
fn f0_shortcut_samples_texture_coordinates() {
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
