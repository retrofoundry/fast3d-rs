use super::*;
use crate::capture::MemorySpan;
use n64_gbi::encode::*;

fn task(commands: &[(u32, u32)], order: u32) -> Task {
    Task {
        entry: 0,
        microcode: Microcode::F3dex2,
        data_format: DataFormat::Fixed,
        order,
        source: SourceLayout {
            memory: MemoryLayout::IMAGE,
            segments: [0; 16],
        },
        spans: vec![MemorySpan {
            address: 0,
            bytes: commands
                .iter()
                .flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
                .collect(),
        }],
    }
}

fn fixture() -> Fixture {
    let mut fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    fixture.tasks = vec![
        task(
            &[
                gdp_set_color_image(0, 2, 64, 0),
                gdp_set_scissor(0, 0, 0, 256, 192),
                gdp_set_fill_color(0xf801f801),
                gsp_enddl(),
            ],
            0,
        ),
        task(&[gdp_fill_rectangle(0, 0, 252, 188), gsp_enddl()], 1),
    ];
    fixture
}

#[test]
fn framebuffer_primers_do_not_inherit_the_fixtures_final_registers() {
    let mut fixture = fixture();
    fixture.tasks[1] = task(
        &[
            gdp_fill_rectangle(0, 0, 252, 188),
            gdp_set_depth_image(0),
            gsp_enddl(),
        ],
        1,
    );
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(output
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
}

#[test]
fn replay_resets_prior_registers_but_inherits_between_recorded_tasks() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let expected = pollster::block_on(fixture.replay(device.clone(), queue.clone())).unwrap();
    assert!(expected
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
    for policy in [ClearPolicy::PerFrame, ClearPolicy::Persist] {
        let mut renderer = fixture.renderer(device.clone(), queue.clone(), policy);
        let poison = task(
            &[
                gdp_set_depth_image(0),
                gdp_set_convert(1, 2, 3, 4, 5, 6),
                gdp_set_key_r(7, 8, 9),
                gdp_set_cycle_type(3),
                gsp_enddl(),
            ],
            0,
        );
        for _ in 0..2 {
            let hardware = ReplayHardware::new(&poison, None).unwrap();
            renderer.process_dl(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink);
            let output = pollster::block_on(fixture.render_frame(&mut renderer)).unwrap();
            assert_eq!(output.rgba8, expected.rgba8);
            assert_eq!(output.summaries, expected.summaries);
            assert_eq!(output.diagnostics, expected.diagnostics);
        }
    }
}

#[test]
fn capture_rejects_unrecorded_starting_registers_without_resetting_live_state() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device, queue, ClearPolicy::PerFrame);
    let setup = ReplayHardware::new(&fixture.tasks[0], None).unwrap();
    renderer.process_dl(&setup, 0, Microcode::F3dex2, &mut crate::NopSink);
    let before = renderer.rdp.clone();
    let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
    assert_eq!(renderer.rdp, before);
    let draw = ReplayHardware::new(&fixture.tasks[1], None).unwrap();
    capture
        .process_dl(
            &mut renderer,
            &draw,
            0,
            Microcode::F3dex2,
            DataFormat::Fixed,
            &mut crate::NopSink,
        )
        .unwrap();
    let error = capture.finish(None).unwrap_err();
    assert!(error.to_string().contains("prior RDP state"), "{error}");
    assert_eq!(renderer.rdp.color_image.addr, 0);
    assert!(renderer.inner.has_fb(0));
}

#[test]
fn capture_accepts_default_start_equivalent_tasks_after_previous_work() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
    for _ in 0..2 {
        let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
        for task in &fixture.tasks {
            let hardware = ReplayHardware::new(task, None).unwrap();
            let summary = capture
                .process_dl(
                    &mut renderer,
                    &hardware,
                    0,
                    Microcode::F3dex2,
                    DataFormat::Fixed,
                    &mut crate::NopSink,
                )
                .unwrap();
            assert_eq!(summary.renderable, task.order == 1);
        }
        let captured = capture.finish(None).unwrap();
        let output = pollster::block_on(captured.replay(device.clone(), queue.clone())).unwrap();
        assert!(output
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255, 0, 0, 255]));
    }
}

fn full_fill() -> Task {
    task(
        &[
            gdp_set_color_image(0, 2, 64, 0),
            gdp_set_scissor(0, 0, 0, 256, 192),
            gdp_set_fill_color(0xf801f801),
            gdp_fill_rectangle(0, 0, 252, 188),
            gsp_enddl(),
        ],
        0,
    )
}

fn record(capture: &mut CaptureFrame, renderer: &mut Renderer, task: &Task) -> Result<DlSummary> {
    let hardware = ReplayHardware::new(task, None).unwrap();
    capture.process_dl(
        renderer,
        &hardware,
        0,
        Microcode::F3dex2,
        DataFormat::Fixed,
        &mut crate::NopSink,
    )
}

fn output_target(renderer: &Renderer) -> wgpu::Texture {
    renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("capture-state-regression"),
        size: wgpu::Extent3d {
            width: 64,
            height: 48,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[test]
fn capture_rejects_reset_between_tasks_even_when_final_registers_match() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for reset in [Renderer::reset_rdp_state, Renderer::reset] {
        let mut renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
        let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
        record(&mut capture, &mut renderer, &full_fill()).unwrap();
        let before = renderer.rdp.clone();
        reset(&mut renderer);
        let draw = task(
            &[
                gdp_set_color_image(0, 2, 64, 0),
                gdp_set_scissor(0, 0, 0, 256, 192),
                gdp_fill_rectangle(0, 0, 252, 188),
                gdp_set_fill_color(0xf801f801),
                gsp_enddl(),
            ],
            1,
        );
        let result = record(&mut capture, &mut renderer, &draw);
        assert_eq!(renderer.rdp, before);
        let target = output_target(&renderer);
        let finished =
            capture.present_last_to(&mut renderer, &target.create_view(&Default::default()));
        assert!(pollster::block_on(read_rgba8(&renderer, &target))
            .unwrap()
            .iter()
            .all(|&byte| byte == 0));
        assert!(result.unwrap_err().to_string().contains("renderer changed"));
        assert!(finished
            .unwrap_err()
            .to_string()
            .contains("renderer changed"));
    }
}

#[test]
fn capture_accepts_unused_convert_and_key_state_in_fill() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
    let setup = task(
        &[
            gdp_set_convert(1, 2, 3, 4, 5, 6),
            gdp_set_key_r(7, 8, 9),
            gsp_enddl(),
        ],
        0,
    );
    let hardware = ReplayHardware::new(&setup, None).unwrap();
    renderer.process_dl(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink);
    let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
    record(&mut capture, &mut renderer, &full_fill()).unwrap();
    let target = output_target(&renderer);
    let captured = capture
        .present_last_to(&mut renderer, &target.create_view(&Default::default()))
        .unwrap();
    let live = pollster::block_on(read_rgba8(&renderer, &target)).unwrap();
    let replay = pollster::block_on(captured.replay(device, queue)).unwrap();
    assert_eq!(live, replay.rgba8);
    assert!(live
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
}

#[test]
fn capture_accepts_no_work_before_independent_fill() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device, queue, ClearPolicy::PerFrame);
    let fill = full_fill();
    let hardware = ReplayHardware::new(&fill, None).unwrap();
    renderer.process_dl(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink);
    let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
    assert!(
        !record(&mut capture, &mut renderer, &task(&[gsp_enddl()], 0))
            .unwrap()
            .renderable
    );
    record(&mut capture, &mut renderer, &fill).unwrap();
    capture.present_last(&mut renderer).unwrap();
}

#[test]
fn capture_rejects_mutations_at_every_completion_method() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for reset in [Renderer::reset_rdp_state, Renderer::reset] {
        for completion in 0..4 {
            let mut renderer =
                fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
            let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
            record(&mut capture, &mut renderer, &full_fill()).unwrap();
            reset(&mut renderer);
            let target = output_target(&renderer);
            let view = target.create_view(&Default::default());
            let hardware = PresentationHardware(None);
            let result = match completion {
                0 => capture.present(&mut renderer, &hardware),
                1 => capture.present_to(&mut renderer, &hardware, &view),
                2 => capture.present_last(&mut renderer),
                _ => capture.present_last_to(&mut renderer, &view),
            };
            assert!(result.unwrap_err().to_string().contains("renderer changed"));
        }
    }
}

#[test]
fn capture_rejects_unrecorded_submissions_frame_boundaries_and_restored_config() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for mutation in 0..6 {
        let mut renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
        let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
        record(&mut capture, &mut renderer, &full_fill()).unwrap();
        let noop = task(&[gsp_enddl()], 0);
        let hardware = ReplayHardware::new(&noop, None).unwrap();
        match mutation {
            0 => renderer.begin_frame(),
            1 => {
                renderer.process_dl(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink);
            }
            2 => {
                renderer.process_dl_prefix(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink, 1);
            }
            3 => {
                let config = renderer.config;
                renderer.reconfigure(RendererConfig {
                    clear_policy: ClearPolicy::Persist,
                    ..config
                });
                renderer.reconfigure(config);
            }
            4 => {
                renderer.set_data_format(DataFormat::Float);
                renderer.set_data_format(DataFormat::Fixed);
            }
            _ => renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame),
        }
        let first = record(&mut capture, &mut renderer, &full_fill()).unwrap_err();
        assert!(first.to_string().contains("renderer changed"));
        assert_eq!(
            record(&mut capture, &mut renderer, &full_fill()).unwrap_err(),
            first
        );
        assert_eq!(capture.present_last(&mut renderer).unwrap_err(), first);
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn capture_host_rejects_reset_and_still_executes_the_task() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for reset in [Renderer::reset_rdp_state, Renderer::reset] {
        let mut renderer = fixture.renderer(device.clone(), queue.clone(), ClearPolicy::PerFrame);
        let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
        record(&mut capture, &mut renderer, &full_fill()).unwrap();
        reset(&mut renderer);
        let words: Vec<[usize; 2]> = [
            gdp_set_color_image(0, 2, 64, 0),
            gdp_set_scissor(0, 0, 0, 256, 192),
            gdp_fill_rectangle(0, 0, 252, 188),
            gsp_enddl(),
        ]
        .map(|(a, b)| [a as usize, b as usize])
        .to_vec();
        // The words own every source byte until the native walk returns.
        let result = unsafe {
            capture.process_dl_host(
                &mut renderer,
                crate::HostRam::new(bytemuck::cast_slice(&words)),
                words.as_ptr() as u64,
                Microcode::F3dex2,
                DataFormat::Fixed,
                &mut crate::NopSink,
            )
        };
        let target = output_target(&renderer);
        let finished =
            capture.present_last_to(&mut renderer, &target.create_view(&Default::default()));
        assert!(pollster::block_on(read_rgba8(&renderer, &target))
            .unwrap()
            .iter()
            .all(|&byte| byte == 0));
        assert!(result.unwrap_err().to_string().contains("renderer changed"));
        assert!(finished
            .unwrap_err()
            .to_string()
            .contains("renderer changed"));
    }
}

#[test]
fn capture_rejects_unsupported_active_use_even_when_both_walks_do_no_work() {
    let fixture = fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device, queue, ClearPolicy::PerFrame);
    let color = CcPass {
        a: 15,
        b: 7,
        c: 31,
        d: 3,
    };
    let alpha = CcPass {
        a: 7,
        b: 7,
        c: 7,
        d: 3,
    };
    let setup = task(
        &[
            gdp_set_color_image(0, 2, 64, 0),
            gdp_set_combine_lerp(color, alpha, color, alpha),
            gsp_enddl(),
        ],
        0,
    );
    let hardware = ReplayHardware::new(&setup, None).unwrap();
    renderer.process_dl(&hardware, 0, Microcode::F3dex2, &mut crate::NopSink);
    let draw = task(&[gdp_fill_rectangle(0, 0, 252, 188), gsp_enddl()], 0);
    let actual = draw.interpret(renderer.rdp.clone()).unwrap();
    let expected = draw.interpret(Default::default()).unwrap();
    for walk in [&actual, &expected] {
        assert!(crate::render::inputs::RenderInputs::new(&walk.scene, (64, 48), [0, 0]).is_none());
    }
    assert_eq!(actual.summary(false), expected.summary(false));
    assert!(matches!(
        actual.diags[0].kind,
        crate::DiagKind::UnsupportedConvertInput { .. }
    ));
    assert_eq!(expected.diags[0].kind, crate::DiagKind::DrawBeforeCimg);
    let mut capture = CaptureFrame::begin(&mut renderer, 0, 0, Provenance::default());
    assert!(
        !record(&mut capture, &mut renderer, &draw)
            .unwrap()
            .renderable
    );
    assert!(capture
        .present_last(&mut renderer)
        .unwrap_err()
        .to_string()
        .contains("prior RDP state"));
}
