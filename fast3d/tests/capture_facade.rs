#![cfg(feature = "capture")]

use fast3d::capture::{CaptureError, Fixture};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn checked_in_high_address_fixture_renders() {
    let fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    let output = fixture.replay_headless().await.unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("capture replay adapter: {:?}", output.adapter_info);
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("capture replay adapter: {:?}", output.adapter_info);
    assert_eq!((output.width, output.height), (64, 48));
    assert_eq!(output.rgba8.len(), 64 * 48 * 4);
    assert!(output
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [255, 0, 0, 255]));
    assert_eq!(output.summaries.len(), 1);
    assert!(output.summaries[0].renderable);
    assert_eq!(output.summaries[0].errors, 0);
    assert_eq!(output.diagnostics, vec![vec![]]);
    assert!(output.adapter_info.is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn capture_checked_in_high_address_fixture_renders() {
    pollster::block_on(checked_in_high_address_fixture_renders());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn capture_checked_in_high_address_fixture_renders() {
    checked_in_high_address_fixture_renders().await;
}

async fn public_replay_rejects_missing_span() {
    let mut fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    let entry = fixture.tasks[0].entry;
    fixture.tasks[0].spans.clear();
    let error = fixture.replay_headless().await.unwrap_err();
    assert_eq!(
        error,
        CaptureError::MissingSpan {
            address: entry,
            length: 16
        }
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn capture_public_replay_rejects_missing_span() {
    pollster::block_on(public_replay_rejects_missing_span());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn capture_public_replay_rejects_missing_span() {
    public_replay_rejects_missing_span().await;
}

async fn public_replay_rejects_prior_framebuffer_dependence() {
    let mut fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    let mut changed = false;
    for span in &mut fixture.tasks[0].spans {
        for command in span.bytes.chunks_exact_mut(16) {
            let w0 = u64::from_le_bytes(command[..8].try_into().unwrap());
            if w0 >> 24 == 0xF6 {
                command[..8].copy_from_slice(&0xF603_C03Cu64.to_le_bytes());
                changed = true;
            }
        }
    }
    assert!(changed);
    assert_eq!(
        fixture.replay_headless().await.unwrap_err(),
        CaptureError::ClearPolicyMismatch,
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn capture_public_replay_rejects_prior_framebuffer_dependence() {
    pollster::block_on(public_replay_rejects_prior_framebuffer_dependence());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn capture_public_replay_rejects_prior_framebuffer_dependence() {
    public_replay_rejects_prior_framebuffer_dependence().await;
}

async fn image_vi_selects_captured_framebuffer() {
    use fast3d::capture::{CaptureFrame, Provenance};
    use fast3d::{
        ClearPolicy, DataFormat, Hardware, Microcode, PresentTarget, Rdram, RdramImage, Renderer,
        RendererConfig, ViRegisters,
    };

    struct ImageHardware {
        bytes: Vec<u8>,
        vi: ViRegisters,
    }
    impl Hardware for ImageHardware {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.bytes)
        }
        fn vi(&self) -> Option<ViRegisters> {
            Some(self.vi)
        }
    }

    let commands: [(u32, u32); 9] = [
        (0xBA00_1402, 0x0030_0000),
        (0xED00_0000, 0x0010_00C0),
        (0xFF10_003F, 0x0010_0000),
        (0xF700_0000, 0xF801_F801),
        (0xF60F_C0BC, 0),
        (0xFF10_003F, 0x0020_0000),
        (0xF700_0000, 0x07C1_07C1),
        (0xF60F_C0BC, 0),
        (0xB800_0000, 0),
    ];
    let vi = ViRegisters {
        status: 2,
        origin: 0x8010_0000,
        width: 64,
        x_scale: 1024,
        y_scale: 1024,
        h_start: 0x0040_0080,
        v_start: 0x0020_0080,
        v_current: 12,
    };
    let mut hardware = ImageHardware {
        bytes: commands
            .into_iter()
            .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
            .collect(),
        vi: ViRegisters {
            origin: 0x8020_0000,
            ..vi
        },
    };
    let (device, queue) = test_device().await;
    let mut renderer = Renderer::with_device(
        device.clone(),
        queue.clone(),
        PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 48,
        },
        RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::LowPower,
        },
    );
    let mut frame = CaptureFrame::begin(
        &mut renderer,
        17,
        23,
        Provenance {
            source_symbols: "literal BE-image fill commands".into(),
            synthetic_data: "64x48 red and green framebuffers".into(),
            ..Default::default()
        },
    );
    let mut diagnostics = Vec::new();
    let summary = frame
        .process_dl(
            &mut renderer,
            &hardware,
            0,
            Microcode::F3d,
            DataFormat::Fixed,
            &mut diagnostics,
        )
        .unwrap();
    hardware.vi = vi;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("capture-vi-target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 48,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut fixture = frame
        .present_to(
            &mut renderer,
            &hardware,
            &target.create_view(&Default::default()),
        )
        .unwrap();
    drop(hardware);
    assert_eq!(fixture.frame.vi, Some(vi));
    let first = fixture.replay(device.clone(), queue.clone()).await.unwrap();
    assert_eq!((first.width, first.height), (64, 48));
    assert_eq!(first.rgba8.len(), 64 * 48 * 4);
    assert_eq!(first.summaries, [summary]);
    assert_eq!(first.diagnostics, [diagnostics]);
    assert!(first
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [255, 0, 0, 255]));
    fixture.frame.vi.as_mut().unwrap().origin = 0x8020_0000;
    let second = fixture.replay(device, queue).await.unwrap();
    assert_eq!(second.rgba8.len(), 64 * 48 * 4);
    assert!(second
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [0, 255, 0, 255]));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn fixture_image_vi_selects_captured_framebuffer() {
    pollster::block_on(image_vi_selects_captured_framebuffer());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn fixture_image_vi_selects_captured_framebuffer() {
    image_vi_selects_captured_framebuffer().await;
}

async fn test_device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("capture facade tests require a GPU adapter");
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("capture facade adapter: {:?}", adapter.get_info());
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("capture facade adapter: {:?}", adapter.get_info());
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("capture-facade-test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .unwrap()
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod host {
    use super::*;
    use fast3d::capture::{CaptureFrame, Provenance};
    use fast3d::{
        ClearPolicy, DataFormat, DiagKind, Hardware, HostRam, Microcode, PresentTarget, Rdram,
        RdramImage, Renderer, RendererConfig,
    };

    struct HostGraph {
        root: Box<[[usize; 2]]>,
        child: Box<[[usize; 2]]>,
    }

    impl HostGraph {
        fn new() -> Self {
            let child = vec![
                [0xB800_0000, 0],
                [0xBA00_1402, 0x0030_0000],
                [0xED00_0000, 0x0010_40C0],
                [0xFF10_0040, 0x0000_0002_3456_7000],
                [0xF700_0000, 0xF801_F801],
                [0xF610_00BC, 0],
                [0xBC00_00FF, 0],
                [0xB800_0000, 0],
            ]
            .into_boxed_slice();
            let root = vec![
                [0x0600_0000, child[1..].as_ptr() as usize],
                [0xB800_0000, 0],
            ]
            .into_boxed_slice();
            Self { root, child }
        }

        fn entry(&self) -> u64 {
            self.root.as_ptr() as u64
        }
    }

    impl Hardware for HostGraph {
        fn rdram(&self) -> impl Rdram + '_ {
            // Both allocations stay owned by this borrow for the entire display-list walk.
            unsafe { HostRam::new(&[]) }
        }
    }

    struct PresentationHardware;
    impl Hardware for PresentationHardware {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&[])
        }
    }

    fn read_pixels(renderer: &Renderer, target: &wgpu::Texture) -> Vec<u8> {
        let stride = 512;
        let buffer = renderer.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("live-capture-readback"),
            size: stride * 48,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = renderer
            .device()
            .create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(48),
                },
            },
            target.size(),
        );
        renderer.queue().submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap()
            });
        renderer
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receiver.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        let pixels = mapped
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..65 * 4].iter().copied())
            .collect();
        drop(mapped);
        buffer.unmap();
        pixels
    }

    #[test]
    fn fixture_public_facade_matches_live_backend() {
        pollster::block_on(async {
            let (device, queue) = test_device().await;
            let new_renderer = || {
                Renderer::with_device(
                    device.clone(),
                    queue.clone(),
                    PresentTarget::Headless {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        width: 65,
                        height: 48,
                    },
                    RendererConfig {
                        resolution_multiplier: 1,
                        sample_count: 1,
                        present_mode: wgpu::PresentMode::Fifo,
                        format: Some(wgpu::TextureFormat::Rgba8Unorm),
                        clear_policy: ClearPolicy::Persist,
                        power_preference: wgpu::PowerPreference::LowPower,
                    },
                )
            };
            let mut renderer = new_renderer();
            let mut raw_renderer = new_renderer();
            raw_renderer.begin_frame();
            let mut hardware = HostGraph::new();
            assert!(hardware.entry() > u32::MAX as u64);
            let mut frame = CaptureFrame::begin(
                &mut renderer,
                42,
                123,
                Provenance {
                    source_symbols: "test-owned nested fill display lists".into(),
                    synthetic_data: "65x48 red fill followed by blue 17x17 rectangle".into(),
                    ..Default::default()
                },
            );
            let mut diagnostics = vec![Vec::new(), Vec::new()];
            let mut raw_diagnostics = vec![Vec::new(), Vec::new()];
            let raw_first = raw_renderer.process_dl(
                &hardware,
                hardware.entry(),
                Microcode::F3d,
                &mut raw_diagnostics[0],
            );
            let first = frame
                .process_dl(
                    &mut renderer,
                    &hardware,
                    hardware.entry(),
                    Microcode::F3d,
                    DataFormat::Fixed,
                    &mut diagnostics[0],
                )
                .unwrap();
            hardware.child[4][1] = 0x003F_003F;
            hardware.child[5] = [0xF606_0060, 0x0002_0020];
            let raw_second = raw_renderer.process_dl(
                &hardware,
                hardware.entry(),
                Microcode::F3d,
                &mut raw_diagnostics[1],
            );
            let second = frame
                .process_dl(
                    &mut renderer,
                    &hardware,
                    hardware.entry(),
                    Microcode::F3d,
                    DataFormat::Fixed,
                    &mut diagnostics[1],
                )
                .unwrap();
            drop(hardware);
            let target_descriptor = wgpu::TextureDescriptor {
                label: Some("live-capture-target"),
                size: wgpu::Extent3d {
                    width: 65,
                    height: 48,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            };
            let target = device.create_texture(&target_descriptor);
            let raw_target = device.create_texture(&target_descriptor);
            raw_renderer.present_to(
                &PresentationHardware,
                &raw_target.create_view(&Default::default()),
            );
            let fixture = frame
                .present_to(
                    &mut renderer,
                    &PresentationHardware,
                    &target.create_view(&Default::default()),
                )
                .unwrap();
            let live_pixels = read_pixels(&renderer, &target);
            let raw_live_pixels = read_pixels(&raw_renderer, &raw_target);
            for pixels in [&live_pixels, &raw_live_pixels] {
                assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
                assert_eq!(
                    &pixels[(16 * 65 + 16) * 4..(16 * 65 + 16) * 4 + 4],
                    &[0, 0, 255, 255]
                );
            }
            assert_eq!(live_pixels, raw_live_pixels);
            assert_eq!([first, second], [raw_first, raw_second]);
            assert_eq!(diagnostics, raw_diagnostics);
            assert_eq!(first.commands, 9);
            assert_eq!(second.commands, 9);
            assert_eq!(diagnostics[0].len(), 1);
            assert_eq!(diagnostics[0][0].kind, DiagKind::UnhandledMoveword(0xFF));
            let fixture = Fixture::from_bytes(&fixture.to_bytes().unwrap()).unwrap();
            assert_eq!(fixture.frame.serial, 42);
            assert_eq!(fixture.frame.dither_seed, 123);
            assert_eq!(
                fixture
                    .tasks
                    .iter()
                    .map(|task| task.order)
                    .collect::<Vec<_>>(),
                [0, 1]
            );
            let output = fixture.replay(device, queue).await.unwrap();
            assert_eq!((output.width, output.height), (65, 48));
            assert_eq!(output.rgba8, raw_live_pixels);
            assert_eq!(output.summaries, [raw_first, raw_second]);
            assert_eq!(output.diagnostics, raw_diagnostics);
        });
    }
}
