use fast3d::{ClearPolicy, PresentTarget, Renderer, RendererConfig};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn config(scale: u32) -> RendererConfig {
    RendererConfig {
        resolution_multiplier: scale,
        sample_count: 1,
        present_mode: wgpu::PresentMode::Fifo,
        format: None,
        clear_policy: ClearPolicy::Persist,
        power_preference: wgpu::PowerPreference::LowPower,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_scale_validation() {
    for scale in [0, 5, u32::MAX] {
        assert!(config(scale).validate().is_err());
    }
    for scale in 1..=4 {
        assert!(config(scale).validate().is_ok());
    }
}

struct Warnings(std::sync::Mutex<Vec<String>>);
static WARNINGS: Warnings = Warnings(std::sync::Mutex::new(Vec::new()));

impl log::Log for Warnings {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() == log::Level::Warn
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

async fn effective_scale_survives_reset_and_reconfigure() {
    log::set_logger(&WARNINGS).unwrap();
    log::set_max_level(log::LevelFilter::Warn);
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let target = || PresentTarget::Headless {
        format: wgpu::TextureFormat::Rgba8Unorm,
        width: 32,
        height: 24,
    };
    for scale in [0, 5, u32::MAX] {
        assert!(config(scale).validate().is_err());
        let renderer =
            Renderer::with_device(device.clone(), queue.clone(), target(), config(scale));
        assert_eq!(renderer.effective_resolution_multiplier(), 1);
        assert!(WARNINGS
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|message| message.contains(&format!("got {scale}; treated as 1"))));
    }
    let mut renderer = Renderer::with_device(device, queue, target(), config(2));
    for scale in 1..=4 {
        renderer.reconfigure(config(scale));
        assert_eq!(renderer.effective_resolution_multiplier(), scale);
        renderer.reset();
        assert_eq!(renderer.effective_resolution_multiplier(), scale);
        renderer.resize(100, 100);
        assert_eq!(renderer.effective_resolution_multiplier(), scale);
    }
    renderer.reconfigure(config(0));
    assert_eq!(renderer.effective_resolution_multiplier(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn public_effective_scale() {
    pollster::block_on(effective_scale_survives_reset_and_reconfigure());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn public_effective_scale() {
    effective_scale_survives_reset_and_reconfigure().await;
}

#[cfg(feature = "capture")]
async fn replay_scale_and_source() {
    use fast3d::capture::{Fixture, ReplayOptions, ReplaySource};
    let fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    let base = fixture.replay_headless().await.unwrap();
    for scale in 1..=4 {
        let native = fixture
            .replay_headless_with_options(ReplayOptions {
                resolution_multiplier: Some(scale),
                source: ReplaySource::Native,
            })
            .await
            .unwrap();
        assert_eq!(native.rgba8, base.rgba8);
        assert_eq!((native.width, native.height), (base.width, base.height));
        assert_eq!(native.effective_resolution_multiplier, scale);
        let presentation = fixture
            .replay_headless_with_options(ReplayOptions {
                resolution_multiplier: Some(scale),
                source: ReplaySource::Presentation,
            })
            .await
            .unwrap();
        assert_eq!(
            (presentation.width, presentation.height),
            (64 * scale, 48 * scale)
        );
        assert_eq!(presentation.source, ReplaySource::Presentation);
        assert_eq!(presentation.summaries, native.summaries);
        assert_eq!(presentation.diagnostics, native.diagnostics);
        assert!(presentation
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255, 0, 0, 255]));
    }
    for scale in [0, 5, u32::MAX] {
        assert!(fixture
            .replay_headless_with_options(ReplayOptions {
                resolution_multiplier: Some(scale),
                ..Default::default()
            })
            .await
            .is_err());
    }
}

#[cfg(all(feature = "capture", not(target_arch = "wasm32")))]
#[test]
fn replay_fixed_scale_sources() {
    pollster::block_on(replay_scale_and_source());
}

#[cfg(all(feature = "capture", target_arch = "wasm32"))]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn replay_fixed_scale_sources() {
    replay_scale_and_source().await;
}
