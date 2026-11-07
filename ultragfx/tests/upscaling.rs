#[cfg(feature = "capture")]
use ultragfx::ScalePolicy;
use ultragfx::{ClearPolicy, PresentTarget, Renderer, RendererConfig};

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
    use ultragfx::capture::{Fixture, ReplayOptions, ReplaySource};
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

#[cfg(feature = "capture")]
async fn live_auto_and_reduced_scanout() {
    use n64_gbi::encode::*;
    use ultragfx::{Hardware, RdramImage, ViRegisters};
    struct Image(Vec<u8>, ViRegisters);
    impl Hardware for Image {
        fn rdram(&self) -> impl ultragfx::Rdram + '_ {
            RdramImage::new(&self.0)
        }
        fn vi(&self) -> Option<ViRegisters> {
            Some(self.1)
        }
    }
    let commands = [
        gdp_set_color_image(0, 2, 7, 0x1000),
        gdp_set_scissor(0, 0, 0, 28, 44),
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_fill_color(0x07c107c1),
        gdp_fill_rectangle(0, 0, 24, 40),
        gdp_set_fill_color(0xf801f801),
        gdp_fill_rectangle(0, 12, 24, 28),
        gsp_enddl(),
    ];
    let mut memory = vec![0; 8192];
    for (chunk, (a, b)) in memory.chunks_mut(8).zip(commands) {
        chunk[..4].copy_from_slice(&a.to_be_bytes());
        chunk[4..].copy_from_slice(&b.to_be_bytes());
    }
    let mut hw = Image(
        memory,
        ViRegisters {
            origin: 0x1000 + 3 * 14,
            width: 7,
            status: 2,
            v_start: 10,
            y_scale: 1024,
            ..Default::default()
        },
    );
    let adapter = wgpu::Instance::default()
        .request_adapter(&Default::default())
        .await
        .unwrap();
    let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
    let mut renderer = Renderer::with_device(
        device.clone(),
        queue.clone(),
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 7,
            height: 11,
        },
        config(1),
    );
    let mut diagnostics = Vec::new();
    assert!(
        renderer
            .process_dl(&hw, 0, ultragfx::Microcode::F3dex2, &mut diagnostics)
            .renderable,
        "{diagnostics:?}"
    );
    renderer.set_scale_policy(ScalePolicy::Auto).unwrap();
    renderer.set_output_content_height(121);
    renderer.begin_frame_with_vi(hw.vi()).unwrap();
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
    assert_eq!(renderer.auto_reference_height(), Some(60));
    hw.1.v_start = 400;
    renderer.set_output_content_height(241);
    let mid_frame = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    renderer.present_to(&hw, &mid_frame.create_view(&Default::default()));
    assert_eq!(renderer.effective_resolution_multiplier(), 3);
    hw.1.v_start = 10;
    for factor in [1, 2, 4] {
        renderer.set_reduction_factor(factor).unwrap();
        assert_eq!(renderer.reduction_factor(), factor);
        let (width, height) = ((21 / factor).max(1), (15 / factor).max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        renderer.present_to(&hw, &texture.create_view(&Default::default()));
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(height) * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        #[cfg(not(target_arch = "wasm32"))]
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.await.unwrap().unwrap();
        let data = buffer.slice(..).get_mapped_range();
        for row in data.chunks(256) {
            for pixel in row[..width as usize * 4].as_chunks::<4>().0 {
                assert_eq!(*pixel, [255, 0, 0, 255]);
            }
        }
    }
    renderer.begin_frame_with_vi(hw.vi()).unwrap();
    assert_eq!(renderer.effective_resolution_multiplier(), 4);
    renderer.set_scale_policy(ScalePolicy::Fixed(2)).unwrap();
    assert_eq!(renderer.effective_resolution_multiplier(), 4);
    renderer.begin_frame();
    assert_eq!(renderer.effective_resolution_multiplier(), 2);
    for invalid in [0, 5, u32::MAX] {
        assert!(renderer
            .set_scale_policy(ScalePolicy::Fixed(invalid))
            .is_err());
    }
    assert_eq!(renderer.requested_scale_policy(), ScalePolicy::Fixed(2));
    assert!(renderer.set_reduction_factor(0).is_err());
    assert_eq!(renderer.reduction_factor(), 4);
}

#[cfg(all(feature = "capture", not(target_arch = "wasm32")))]
#[test]
fn live_scale_and_box_reduction() {
    pollster::block_on(live_auto_and_reduced_scanout());
}

#[cfg(all(feature = "capture", target_arch = "wasm32"))]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn live_scale_and_box_reduction() {
    live_auto_and_reduced_scanout().await;
}
