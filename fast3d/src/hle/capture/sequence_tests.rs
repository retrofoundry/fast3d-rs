use super::*;
use crate::capture::{CaptureSequence, Sequence};
use crate::tests::dl_builder::DlBuilder;
use crate::DepthResetPolicy;
use n64_gbi::{consts::*, encode::*};

fn draw_task() -> Task {
    let mut b = DlBuilder::new();
    let projection = b.matrix(n64_gbi::gu::gu_scale(1.0 / 256.0, 1.0 / 256.0, 1.0 / 128.0));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [1024, 1024, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    let vertices = b.vertices(
        &[(-128, 88), (-128, -88), (128, -88), (128, 88)].map(|(x, y)| VtxColored {
            x,
            y,
            z: 64,
            flag: 0,
            s: 0,
            t: 0,
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        }),
    );
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
    b.list(
        "main",
        &[
            gdp_set_depth_image(0x200000),
            gdp_set_color_image(0, 2, 320, 0x100000),
            gdp_set_scissor(0, 0, 0, 1280, 960),
            gdp_set_cycle_type(3),
            gdp_set_fill_color(0x00010001),
            gdp_fill_rectangle(0, 0, 1276, 956),
            gsp_clear_geometrymode(u32::MAX),
            gsp_set_geometrymode(G_CLIPPING | G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER),
            gsp_matrix(projection, true, true, false),
            gsp_matrix(model, false, true, false),
            gsp_viewport(viewport),
            gdp_set_cycle_type(0),
            gdp_set_other_mode_h(4, 2, 3 << 4),
            gdp_set_other_mode_h(6, 2, 3 << 6),
            gdp_set_other_mode_l(0, 2, 0),
            gdp_set_combine_lerp(color, alpha, color, alpha),
            gdp_set_render_mode(G_RM_OPA_SURF | Z_CMP | Z_UPD, G_RM_OPA_SURF2),
            gsp_vertex(0, 4, vertices),
            gsp_2triangles(0, 1, 2, 0, 2, 3),
            gsp_enddl(),
        ],
    );
    let built = b.finish("main");
    Task {
        entry: built.entry.into(),
        microcode: Microcode::F3dex2,
        data_format: DataFormat::Fixed,
        order: 0,
        source: SourceLayout {
            memory: MemoryLayout::IMAGE,
            segments: [0; 16],
        },
        spans: vec![MemorySpan {
            address: 0,
            bytes: built.rdram,
        }],
    }
}

fn depth_sequence() -> Sequence {
    let mut fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    fixture.frame.serial = 1;
    fixture.frame.width = 320;
    fixture.frame.height = 240;
    fixture.frame.config.clear_policy = ClearPolicy::Persist;
    fixture.frame.dither_seed = 123;
    let mut clear = draw_task();
    clear.entry = 0;
    clear.spans[0].bytes = [
        gdp_set_depth_image(0x200000),
        gdp_set_color_image(0, 2, 320, 0x200000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(0),
        gdp_fill_rectangle(0, 0, 1276, 956),
        gsp_enddl(),
    ]
    .iter()
    .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
    .collect();
    let mut draw = draw_task();
    draw.order = 1;
    fixture.tasks = vec![clear, draw];
    let mut second = fixture.clone();
    second.frame.serial = 2;
    second.tasks = vec![draw_task()];
    Sequence {
        frames: vec![fixture, second],
        warmup_frames: 0,
        presentations: vec![1, 2],
    }
}

fn center(output: &ReplayOutput) -> &[u8] {
    &output.rgba8[(120 * 320 + 160) * 4..(120 * 320 + 160) * 4 + 4]
}

fn switch_second_frame_color(sequence: &mut Sequence) {
    let task = &mut sequence.frames[1].tasks[0];
    let (w0, w1) = gdp_set_color_image(0, 2, 320, 0x300000);
    task.spans[0].bytes.splice(
        task.entry as usize..task.entry as usize,
        w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()),
    );
}

#[test]
fn sequence_cimg_depth_control_mutations() {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let original = depth_sequence();
    let mut switched = original.clone();
    switch_second_frame_color(&mut switched);
    let mut no_depth_write = original.clone();
    for task in no_depth_write
        .frames
        .iter_mut()
        .flat_map(|frame| &mut frame.tasks)
    {
        for command in task.spans[0].bytes[task.entry as usize..]
            .as_chunks_mut::<8>()
            .0
        {
            if u32::from_be_bytes(command[..4].try_into().unwrap()) == gdp_set_render_mode(0, 0).0 {
                let mode = u32::from_be_bytes(command[4..].try_into().unwrap());
                command[4..].copy_from_slice(&(mode & !Z_UPD).to_be_bytes());
            }
        }
    }
    let black = [0, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let mut observed = Vec::new();
    for (name, sequence, policy, expected) in [
        (
            "original",
            &original,
            DepthResetPolicy::Never,
            [black, black],
        ),
        (
            "original",
            &original,
            DepthResetPolicy::ColorImageSwitch,
            [blue, black],
        ),
        ("switch", &switched, DepthResetPolicy::Never, [black, black]),
        (
            "switch",
            &switched,
            DepthResetPolicy::ColorImageSwitch,
            [blue, blue],
        ),
        (
            "no Z_UPD",
            &no_depth_write,
            DepthResetPolicy::Never,
            [black, black],
        ),
        (
            "no Z_UPD",
            &no_depth_write,
            DepthResetPolicy::ColorImageSwitch,
            [blue, blue],
        ),
    ] {
        let output =
            pollster::block_on(sequence.replay(device.clone(), queue.clone(), policy)).unwrap();
        let centers: Vec<_> = output
            .presentations
            .iter()
            .map(|p| center(&p.output).to_vec())
            .collect();
        eprintln!("{name}, {policy:?}: {centers:?}");
        observed.push((name, policy, centers, expected));
        assert!(output
            .frames
            .iter()
            .flat_map(|frame| frame.diagnostics.iter().flatten())
            .next()
            .is_none());
        for presentation in &output.presentations {
            assert_eq!(&presentation.output.rgba8[..4], black);
        }
    }
    for (name, policy, centers, expected) in observed {
        assert_eq!(centers, expected, "{name}, {policy:?}");
    }
}

#[test]
fn sequence_identical_cimg_keeps_epoch_across_frames() {
    for switch in [false, true] {
        let mut sequence = depth_sequence();
        if switch {
            switch_second_frame_color(&mut sequence);
        }
        let mut rdp = Default::default();
        let mut framebuffers = Default::default();
        let mut epochs = Vec::new();
        for task in sequence.frames.iter().flat_map(|frame| &frame.tasks) {
            let result = task.interpret_with_framebuffers(rdp, framebuffers).unwrap();
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            epochs.push(result.scene.framebuffer_pairs[0].color_image_epoch);
            rdp = result.rdp;
            framebuffers = result.framebuffers;
        }
        assert_eq!(epochs, if switch { [1, 2, 4] } else { [1, 2, 2] });
    }
}

#[test]
fn single_frame_rejects_prior_depth_dependency() {
    let fixture = depth_sequence().frames.remove(1);
    let (device, queue) = crate::render::headless_device_forced_fallback();
    assert_eq!(
        pollster::block_on(fixture.replay(device, queue)).unwrap_err(),
        CaptureError::ClearPolicyMismatch
    );
}

#[test]
fn capture_sequence_replays_from_reset() {
    let source = depth_sequence();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer =
        source.frames[0].renderer(device.clone(), queue.clone(), ClearPolicy::Persist);
    renderer.begin_frame();
    let mut capture = CaptureSequence::begin(&mut renderer, 123).unwrap();
    for fixture in &source.frames {
        capture
            .begin_frame(&mut renderer, fixture.provenance.clone())
            .unwrap();
        for task in &fixture.tasks {
            let hardware = ReplayHardware::new(task, None).unwrap();
            capture
                .frame_mut()
                .unwrap()
                .process_dl(
                    &mut renderer,
                    &hardware,
                    task.entry,
                    task.microcode,
                    task.data_format,
                    &mut crate::NopSink,
                )
                .unwrap();
        }
        capture.present_last(&mut renderer).unwrap();
    }
    let sequence = capture.finish(0, vec![1, 2]).unwrap();
    assert_eq!(sequence.frames[0].frame.serial, 1);
    assert_eq!(sequence.frames[1].frame.serial, 2);
    let sequence = Sequence::from_bytes(&sequence.to_bytes().unwrap()).unwrap();
    let output =
        pollster::block_on(sequence.replay(device, queue, DepthResetPolicy::Never)).unwrap();
    assert_eq!(output.presentations.len(), 2);
    for presentation in &output.presentations {
        assert_eq!(center(&presentation.output), [0, 0, 0, 255]);
        assert!(presentation
            .output
            .diagnostics
            .iter()
            .flatten()
            .next()
            .is_none());
    }
    assert!(output.frames[0]
        .commands
        .iter()
        .any(|event| event.command.w0 >> 24 == 0xfe));
}

#[test]
fn sequence_depth_controls_distinguish_task_and_frame_boundaries() {
    let mut sequence = depth_sequence();
    switch_second_frame_color(&mut sequence);
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for (policy, expected) in [
        (DepthResetPolicy::Never, [[0, 0, 0, 255], [0, 0, 0, 255]]),
        (
            DepthResetPolicy::ColorImageSwitch,
            [[0, 0, 255, 255], [0, 0, 255, 255]],
        ),
        (
            DepthResetPolicy::TaskBoundary,
            [[0, 0, 255, 255], [0, 0, 255, 255]],
        ),
        (
            DepthResetPolicy::FrameBoundary,
            [[0, 0, 0, 255], [0, 0, 255, 255]],
        ),
    ] {
        let output =
            pollster::block_on(sequence.replay(device.clone(), queue.clone(), policy)).unwrap();
        for (presentation, expected) in output.presentations.iter().zip(expected) {
            assert_eq!(
                center(&presentation.output),
                expected,
                "{policy:?}, serial {}",
                presentation.serial
            );
            assert_eq!(&presentation.output.rgba8[..4], [0, 0, 0, 255]);
        }
    }
}

#[test]
fn v1_capture_still_replays() {
    let fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output = pollster::block_on(fixture.replay(device, queue)).unwrap();
    assert!(output
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [255, 0, 0, 255]));
}

#[test]
fn sequence_command_log_retains_repeated_high_address_fetches() {
    let mut task =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap"))
            .unwrap()
            .tasks
            .remove(0);
    let child = task.entry;
    task.entry = 0x9000_0000_0000;
    task.spans.push(MemorySpan {
        address: task.entry,
        bytes: [
            [0x0600_0000u64, child],
            [0x0600_0000, child],
            [0xb800_0000, 0],
        ]
        .into_iter()
        .flatten()
        .flat_map(u64::to_le_bytes)
        .collect(),
    });
    task.spans.sort_by_key(|span| span.address);
    let hardware = ReplayHardware::new(&task, None).unwrap();
    let result = crate::hle::interp::interpret_with_state(
        hardware.rdram(),
        task.entry,
        task.microcode.into(),
        task.data_format,
        Default::default(),
        None,
    );
    hardware.check().unwrap();
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let commands = hardware.commands.into_inner();
    assert!(commands.len().is_multiple_of(2));
    let half = commands.len() / 2;
    assert!(half > 0);
    assert_eq!(&commands[..half], &commands[half..]);
    assert!(commands.iter().all(|command| command.pc > u32::MAX.into()));
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.w0 >> 24 == 0xff)
            .count(),
        2
    );
}

#[test]
fn sequence_warmup_preserves_registers_and_presentation_serials() {
    let mut sequence = depth_sequence();
    let frame = &mut sequence.frames[1];
    frame.tasks[0].entry = 0;
    frame.tasks[0].spans[0].bytes = [
        gdp_set_cycle_type(3),
        gdp_set_fill_color(0xf801_f801),
        gdp_fill_rectangle(0, 0, 60, 60),
        gsp_enddl(),
    ]
    .into_iter()
    .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
    .collect();
    sequence.warmup_frames = 1;
    sequence.presentations = vec![2];
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let output =
        pollster::block_on(sequence.replay(device, queue, DepthResetPolicy::Never)).unwrap();
    assert_eq!(output.frames.len(), 2);
    assert_eq!(output.presentations.len(), 1);
    assert_eq!(output.presentations[0].serial, 2);
    assert_eq!(center(&output.presentations[0].output), [0, 0, 0, 255]);
    assert_eq!(&output.presentations[0].output.rgba8[..4], [255, 0, 0, 255]);
    assert!(output
        .frames
        .iter()
        .flat_map(|frame| frame.diagnostics.iter().flatten())
        .next()
        .is_none());
}

#[test]
fn single_frame_rejects_prior_rgba32_color_dependency() {
    let mut fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    for span in &mut fixture.tasks[0].spans {
        for command in span.bytes.as_chunks_mut::<16>().0.iter_mut() {
            let mut w0 = u64::from_le_bytes(command[..8].try_into().unwrap());
            match w0 >> 24 {
                0xff => w0 = (w0 & !(3 << 19)) | (3 << 19),
                0xf6 => w0 = 0xf603_c03c,
                _ => continue,
            }
            command[..8].copy_from_slice(&w0.to_le_bytes());
        }
    }
    let (device, queue) = crate::render::headless_device_forced_fallback();
    assert_eq!(
        pollster::block_on(fixture.replay(device, queue)).unwrap_err(),
        CaptureError::ClearPolicyMismatch
    );
}

fn scissor_growth_fixture() -> Fixture {
    let mut fixture = depth_sequence().frames.remove(1);
    let task = &mut fixture.tasks[0];
    let entry = task.entry as usize;
    let commands = &mut task.spans[0].bytes[entry..];
    commands[2 * 8 + 4..3 * 8].copy_from_slice(&((1280u32 << 12) | 64).to_be_bytes());
    let (w0, w1) = gdp_set_scissor(0, 0, 0, 1280, 960);
    commands[12 * 8..12 * 8 + 4].copy_from_slice(&w0.to_be_bytes());
    commands[12 * 8 + 4..13 * 8].copy_from_slice(&w1.to_be_bytes());
    let result = task.interpret(Default::default()).unwrap();
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.scene.framebuffer_pairs[0].size_extent.1, 16);
    let targets = crate::render::workload::Workload::new(&result.scene).targets;
    assert_eq!(targets.last().unwrap().logical_extent.1, 240);
    fixture
}

#[test]
fn scissor_growth_repro_spans_later_rows() {
    scissor_growth_fixture();
}

#[test]
fn single_frame_rejects_prior_contents_in_scissor_growth_rows() {
    let fixture = scissor_growth_fixture();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    assert_eq!(
        pollster::block_on(fixture.replay(device, queue)).unwrap_err(),
        CaptureError::ClearPolicyMismatch
    );
}

#[test]
fn sequence_cimg_reset_occurs_within_one_task() {
    let mut sequence = depth_sequence();
    let clear = &sequence.frames[0].tasks[0].spans[0].bytes;
    let mut combined = draw_task();
    let entry = combined.entry as usize;
    combined.spans[0]
        .bytes
        .splice(entry..entry, clear[..clear.len() - 8].iter().copied());
    sequence.frames.truncate(1);
    sequence.frames[0].tasks = vec![combined];
    sequence.presentations = vec![1];
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for (policy, expected) in [
        (DepthResetPolicy::Never, [0, 0, 0, 255]),
        (DepthResetPolicy::TaskBoundary, [0, 0, 0, 255]),
        (DepthResetPolicy::FrameBoundary, [0, 0, 0, 255]),
        (DepthResetPolicy::ColorImageSwitch, [0, 0, 255, 255]),
    ] {
        let output =
            pollster::block_on(sequence.replay(device.clone(), queue.clone(), policy)).unwrap();
        let presentation = &output.presentations[0].output;
        assert_eq!(center(presentation), expected, "{policy:?}");
        assert_eq!(&presentation.rgba8[..4], [0, 0, 0, 255]);
        assert_eq!(presentation.summaries.len(), 1);
        assert!(presentation.diagnostics.iter().flatten().next().is_none());
    }
}
