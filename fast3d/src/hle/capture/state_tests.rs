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
