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
fn capture_admission_checks_prior_framebuffer_history_without_changing_rdp() {
    let mut fixture = fixture();
    let address = 0x10000;
    let mut load = task(
        &[
            gdp_set_texture_image(0, 2, 4, address),
            gdp_set_tile(0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_block(0, 0, 0, 3, 0),
            gsp_enddl(),
        ],
        0,
    );
    load.spans.push(MemorySpan {
        address: address.into(),
        bytes: vec![0xff; 8],
    });
    fixture.tasks = vec![load];
    for known in [false, true] {
        let mut initial_framebuffers = crate::hle::interp::FramebufferState::default();
        if known {
            initial_framebuffers
                .targets
                .record(
                    address.into(),
                    crate::render::framebuffers::ImageLayout {
                        width: 4,
                        fmt: 0,
                        siz: 2,
                    },
                    1,
                    false,
                )
                .unwrap();
        }
        let capture = CaptureFrame {
            fixture: fixture.clone(),
            error: None,
            initial_rdp: Default::default(),
            initial_framebuffers,
            expected_generation: std::rc::Rc::new(()),
            sequence: false,
        };
        let result = capture.finish(None);
        if known {
            assert!(
                matches!(result, Err(CaptureError::Invalid(message)) if message.contains("framebuffer history"))
            );
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
    }
}

#[test]
fn capture_admission_checks_prior_framebuffer_extent_during_preparation() {
    let mut fixture = fixture();
    let address = 0x10000;
    let fill = task(
        &[
            gdp_set_color_image(0, 2, 64, address),
            gdp_set_scissor(0, 0, 0, 256, 256),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(0xf801f801),
            gdp_fill_rectangle(0, 0, 252, 252),
            gsp_enddl(),
        ],
        0,
    );
    let mut copy = vec![
        gdp_set_color_image(0, 2, 64, 0x12000),
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 64, address),
        gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 252, 252),
    ];
    copy.extend(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
    ));
    copy.push(gsp_enddl());
    fixture.tasks = vec![fill, task(&copy, 1)];
    for height in [64, 128] {
        let mut initial_framebuffers = crate::hle::interp::FramebufferState::default();
        initial_framebuffers
            .targets
            .record(
                address.into(),
                crate::render::framebuffers::ImageLayout {
                    width: 64,
                    fmt: 0,
                    siz: 2,
                },
                height,
                false,
            )
            .unwrap();
        let fill = fixture.tasks[0]
            .interpret_with_framebuffers(Default::default(), initial_framebuffers.clone())
            .unwrap();
        assert!(fill.diags.is_empty(), "{:?}", fill.diags);
        let copy = fixture.tasks[1]
            .interpret_with_framebuffers(fill.rdp, fill.framebuffers)
            .unwrap();
        assert!(copy.diags.is_empty(), "{:?}", copy.diags);
        assert!(copy.scene.framebuffer_pairs[0]
            .ops
            .iter()
            .any(|op| matches!(
                op,
                crate::scene::SceneOp::TexRect {
                    fb_source: Some(0x10000),
                    ..
                }
            )));
        let capture = CaptureFrame {
            fixture: fixture.clone(),
            error: None,
            initial_rdp: Default::default(),
            initial_framebuffers,
            expected_generation: std::rc::Rc::new(()),
            sequence: false,
        };
        let result = capture.finish(None);
        if height == 128 {
            assert!(
                matches!(result, Err(CaptureError::Invalid(message)) if message.contains("framebuffer history"))
            );
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
    }
}

#[test]
fn capture_admission_does_not_record_skipped_legacy_depth_targets() {
    use n64_gbi::consts::*;

    let mut fixture = fixture();
    fixture.frame.width = 64;
    fixture.frame.height = 48;
    let depth = 0x10000;
    let color = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    let mut legacy = task(
        &[
            gdp_set_depth_image(depth),
            gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER),
            gdp_set_combine_lerp(color, alpha, color, alpha),
            gdp_set_render_mode(G_RM_OPA_SURF | Z_CMP | Z_UPD, G_RM_OPA_SURF2),
            gsp_vertex(0, 3, 0x1000),
            gsp_1triangle(0, 1, 2),
            gsp_enddl(),
        ],
        0,
    );
    legacy.spans.push(MemorySpan {
        address: 0x1000,
        bytes: [(-1, -1), (1, -1), (0, 1)]
            .into_iter()
            .flat_map(|(x, y)| {
                VtxColored {
                    x,
                    y,
                    z: 0,
                    flag: 0,
                    s: 0,
                    t: 0,
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                }
                .to_bytes()
            })
            .collect(),
    });
    let interpreted = legacy.interpret(Default::default()).unwrap();
    assert!(interpreted.diags.is_empty(), "{:?}", interpreted.diags);
    assert_eq!(interpreted.scene.indices.len(), 3);
    assert!(interpreted.scene.framebuffer_pairs.is_empty());
    let mut load = task(
        &[
            gdp_set_texture_image(0, 2, 4, depth),
            gdp_set_tile(0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_block(0, 0, 0, 3, 0),
            gsp_enddl(),
        ],
        1,
    );
    load.spans.push(MemorySpan {
        address: depth.into(),
        bytes: vec![0xff; 8],
    });
    fixture.tasks = vec![legacy, load];
    for known in [false, true] {
        let mut initial_framebuffers = crate::hle::interp::FramebufferState::default();
        if known {
            initial_framebuffers
                .targets
                .record(
                    depth.into(),
                    crate::render::framebuffers::ImageLayout {
                        width: 320,
                        fmt: 0,
                        siz: 2,
                    },
                    240,
                    true,
                )
                .unwrap();
        }
        let capture = CaptureFrame {
            fixture: fixture.clone(),
            error: None,
            initial_rdp: Default::default(),
            initial_framebuffers,
            expected_generation: std::rc::Rc::new(()),
            sequence: false,
        };
        let result = capture.finish(None);
        if known {
            assert!(
                matches!(result, Err(CaptureError::Invalid(message)) if message.contains("framebuffer history"))
            );
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
    }
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
