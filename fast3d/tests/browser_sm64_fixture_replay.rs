#![cfg(feature = "capture")]

use fast3d::capture::Fixture;
#[path = "common/sm64_semantics.rs"]
mod sm64_semantics;
use sm64_semantics::{Case, CASES};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn bytes(case: Case) -> &'static [u8] {
    match case {
        Case::Host64Fill => include_bytes!("fixtures/host64-fill.f3dcap"),
        Case::CombinerSelector => include_bytes!("fixtures/combiner-env-alpha.f3dcap"),
        Case::PowerMeterPoint => include_bytes!("fixtures/sm64-power-meter-point.f3dcap"),
        Case::CastleTrilerp => include_bytes!("fixtures/sm64-castle-trilerp.f3dcap"),
        Case::TransparentMario => include_bytes!("fixtures/sm64-transparent-mario.f3dcap"),
    }
}

async fn replay_corpus() {
    for case in CASES {
        let fixture = Fixture::from_bytes(bytes(case)).unwrap();
        assert!(
            !fixture.frame.dual_source_blending,
            "{} must request Features::empty()",
            case.filename()
        );
        let output = fixture
            .replay_headless()
            .await
            .expect("browser corpus requires a WebGPU adapter and successful replay");
        let info = output
            .adapter_info
            .as_ref()
            .expect("replay must report adapter_info");
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_test::console_log!(
            "sm64 fixture adapter: {:?} {} ({})",
            info.backend,
            info.name,
            case.filename()
        );
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "sm64 fixture adapter: {:?} {} ({})",
            info.backend,
            info.name,
            case.filename()
        );
        #[cfg(target_arch = "wasm32")]
        assert_eq!(info.backend, wgpu::Backend::BrowserWebGpu);
        assert_eq!((output.width, output.height), case.dimensions());
        assert_eq!(output.summaries.len(), fixture.tasks.len());
        assert_eq!(output.diagnostics.len(), fixture.tasks.len());
        assert!(
            output
                .summaries
                .iter()
                .all(|s| s.renderable && s.errors == 0),
            "{}: {:?}",
            case.filename(),
            output.summaries
        );
        assert!(
            output.diagnostics.iter().all(Vec::is_empty),
            "{}: {:?}",
            case.filename(),
            output.diagnostics
        );
        case.assert_pixels(&output.rgba8);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn browser_sm64_fixture_replay() {
    pollster::block_on(replay_corpus());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn browser_sm64_fixture_replay() {
    replay_corpus().await;
}
