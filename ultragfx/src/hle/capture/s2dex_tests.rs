use super::*;
use crate::hle::{interp, s2dex_bg_tests};
use crate::inspect::{WalkObserver, WalkStep, WalkTermination};
use std::ops::ControlFlow;

#[derive(Default)]
struct Steps(Vec<String>);
impl WalkObserver for Steps {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self.0.push(format!("{step:?}"));
        ControlFlow::Continue(())
    }
}

fn walk(
    memory: impl Rdram,
    entry: u64,
    bindings: &[(u64, Microcode)],
    steps: &mut Steps,
) -> interp::InterpResult {
    interp::interpret_with_bindings(
        memory,
        entry,
        Microcode::F3dex2.into(),
        DataFormat::Float,
        Default::default(),
        Default::default(),
        Some(steps),
        Default::default(),
        bindings,
    )
}

#[cfg(target_pointer_width = "64")]
#[test]
fn capture_v2_s2dex_replays_after_source_drop() {
    let (fixture, live, steps, pixels) = {
        let (source, _) = s2dex_bg_tests::fixture(64, 32);
        let mut object = [0; 40];
        for offset in (0..16).step_by(2).chain([20, 24, 26, 28, 30]) {
            let value =
                u16::from_be_bytes(source[0x200 + offset..0x202 + offset].try_into().unwrap());
            let target = if offset < 16 { offset } else { offset + 4 };
            object[target..target + 2].copy_from_slice(&value.to_ne_bytes());
        }
        object[26..28].copy_from_slice(&source[0x216..0x218]);
        object[16..24].copy_from_slice(&(source.as_ptr() as u64 + 0x1000).to_ne_bytes());
        let mut commands = vec![0xe1000000u64, 0xdead0000, 0xdd0007ff, 0x1234567887654321];
        for (index, bytes) in source[..80].as_chunks::<8>().0.iter().enumerate() {
            if index == 2 {
                commands.extend([
                    0xef300cf0, 0, 0xf7000000, 0x00010001, 0xf60000fc, 124, 0xe1000000, 0,
                ]);
            }
            let w0 = u32::from_be_bytes(bytes[..4].try_into().unwrap());
            let w1 = u32::from_be_bytes(bytes[4..].try_into().unwrap());
            let address = match w0 >> 24 {
                0xfd => source.as_ptr() as u64 + w1 as u64,
                0x09 => object.as_ptr() as u64,
                _ => w1 as u64,
            };
            commands.extend([w0 as u64, address]);
        }
        commands.truncate(commands.len() - 2);
        commands.extend([
            0xe1000000,
            0xdead0000,
            0xdd0007ff,
            0x1234567887654322,
            0xdf000000,
            0,
        ]);
        let bindings = vec![
            (0x1234567887654321, Microcode::S2dex2),
            (0x1234567887654322, Microcode::F3dex2),
        ];
        let entry = commands.as_ptr() as u64;
        let memory = unsafe { crate::hle::host_mem::HostMemory::new(crate::HostRam::new(&[])) };
        let mut steps = Steps::default();
        let live = walk(memory, entry, &bindings, &mut steps);
        assert!(live.diags.is_empty(), "{:?}", live.diags);
        assert_eq!(live.termination, WalkTermination::End);
        let mut template =
            Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap"))
                .unwrap();
        template.frame.width = 64;
        template.frame.height = 32;
        let (device, queue) = crate::render::headless_device_forced_fallback();
        let mut renderer = template.renderer(device, queue, crate::ClearPolicy::PerFrame);
        let mut capture = CaptureFrame::begin(&mut renderer, 0, 17, Provenance::default());
        let mut diagnostics = Vec::new();
        unsafe {
            capture
                .process_dl_host_with_bindings(
                    &mut renderer,
                    crate::HostRam::new(&[]),
                    entry,
                    Microcode::F3dex2,
                    DataFormat::Float,
                    &mut diagnostics,
                    &bindings,
                )
                .unwrap();
        }
        assert!(diagnostics.is_empty());
        assert_eq!(renderer.rdp, live.rdp);
        let target = renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let fixture = capture
            .present_last_to(&mut renderer, &target.create_view(&Default::default()))
            .unwrap();
        let pixels =
            pollster::block_on(crate::hle::capture::replay::read_rgba8(&renderer, &target))
                .unwrap();
        (fixture, live, steps, pixels)
    };
    let bytes = fixture.to_bytes().unwrap();
    assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
    let fixture = Fixture::from_bytes(&bytes).unwrap();
    let task = &fixture.tasks[0];
    assert_eq!(task.source.memory.obj_bg_abi, ObjBgAbi::Host64);
    let replay = ReplayHardware::new(task, None).unwrap();
    let mut replay_steps = Steps::default();
    let result = walk(
        replay.rdram(),
        task.entry,
        &task.bindings,
        &mut replay_steps,
    );
    replay.check().unwrap();
    assert_eq!(result, live);
    assert_eq!(replay_steps.0, steps.0);
    let output = pollster::block_on(fixture.replay_headless()).unwrap();
    assert_eq!(output.rgba8, pixels);
    assert!(output.diagnostics.iter().all(Vec::is_empty));
    let (source, _) = s2dex_bg_tests::fixture(64, 32);
    for y in 0..31 {
        for x in 0..63 {
            assert_eq!(
                &pixels[(y * 64 + x) * 4..][..4],
                &s2dex_bg_tests::palette_pixel(&source, x, y, 64)
            );
        }
    }
}

#[test]
fn capture_s2dex_missing_object_texture_or_tlut_span_fails() {
    let (bytes, _) = s2dex_bg_tests::fixture(320, 240);
    let hardware = Image(bytes);
    let recording = RecordingHardware::new(&hardware);
    let result = interp::interpret(
        recording.rdram(),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty());
    let task = recording
        .finish(0, Microcode::S2dex2, DataFormat::Fixed, 0)
        .unwrap();
    for (address, length) in [(0x210, 4), (0x1000, 1928), (0x400, 512)] {
        let mut missing = task.clone();
        missing.spans = task
            .spans
            .iter()
            .flat_map(|span| {
                let mut spans = Vec::new();
                let end = span.address + span.bytes.len() as u64;
                if address < span.address || address >= end {
                    return vec![span.clone()];
                }
                if address > span.address {
                    spans.push(MemorySpan {
                        address: span.address,
                        bytes: span.bytes[..(address - span.address) as usize].to_vec(),
                    });
                }
                if address + 1 < end {
                    spans.push(MemorySpan {
                        address: address + 1,
                        bytes: span.bytes[(address + 1 - span.address) as usize..].to_vec(),
                    });
                }
                spans
            })
            .collect();
        assert_eq!(
            missing.interpret(Default::default()).unwrap_err(),
            CaptureError::MissingSpan { address, length }
        );
    }
}

#[test]
fn capture_v1_fixture_remains_readable() {
    let bytes = include_bytes!("../../../tests/fixtures/host64-fill.f3dcap");
    assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
    let fixture = Fixture::from_bytes(bytes).unwrap();
    assert!(fixture.tasks.iter().all(|task| task.bindings.is_empty()));
    assert_eq!(fixture.tasks[0].microcode, Microcode::F3d);
    assert_eq!(fixture.tasks[0].source.memory.obj_bg_abi, ObjBgAbi::Host64);
    let expected = fixture.tasks[0].interpret(Default::default()).unwrap();
    let encoded = fixture.to_bytes().unwrap();
    assert_eq!(&encoded[8..12], &3u32.to_le_bytes());
    assert_eq!(
        Fixture::from_bytes(&encoded).unwrap().tasks[0]
            .interpret(Default::default())
            .unwrap(),
        expected
    );
}

#[test]
fn capture_bindings_are_task_local() {
    let mut fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    let base = fixture.tasks[0].clone();
    fixture.frame.serial = 1;
    fixture.tasks = [Microcode::S2dex2, Microcode::F3dex2]
        .into_iter()
        .enumerate()
        .map(|(order, microcode)| {
            let mut task = base.clone();
            task.order = order as u32;
            task.microcode = Microcode::F3dex2;
            task.bindings = vec![(0x123456789abcdef0, microcode)];
            let mut prefix: Vec<u8> = [0xe1000000u64, 0xdead0000, 0xdd0007ff, 0x123456789abcdef0]
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect();
            prefix.extend(task.spans[0].bytes.clone());
            task.spans[0].bytes = prefix;
            for command in task.spans[0].bytes.as_chunks_mut::<16>().0 {
                if u64::from_le_bytes(command[..8].try_into().unwrap()) >> 24 == 0xba {
                    command[..8].copy_from_slice(&0xef300cf0u64.to_le_bytes());
                    command[8..].copy_from_slice(&0u64.to_le_bytes());
                }
            }
            let i = task.spans[0].bytes.len() - 16;
            task.spans[0].bytes[i..i + 8].copy_from_slice(&0xdf000000u64.to_le_bytes());
            let command = if microcode == Microcode::S2dex2 {
                0x0b000000u64
            } else {
                0xd9000000u64
            };
            task.spans[0]
                .bytes
                .splice(i..i, [command, 12].into_iter().flat_map(u64::to_le_bytes));
            task
        })
        .collect();
    let fixture = Fixture::from_bytes(&fixture.to_bytes().unwrap()).unwrap();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = fixture.renderer(device.clone(), queue.clone(), crate::ClearPolicy::Persist);
    let mut capture = CaptureFrame::begin(
        &mut renderer,
        0,
        fixture.frame.dither_seed,
        Provenance::default(),
    );
    for task in &fixture.tasks {
        let hardware = ReplayHardware::new(task, None).unwrap();
        let mut diags = Vec::new();
        capture
            .process_dl_with_bindings(
                &mut renderer,
                &hardware,
                task.entry,
                task.microcode,
                task.data_format,
                &mut diags,
                &task.bindings,
            )
            .unwrap();
        hardware.check().unwrap();
        assert!(diags.is_empty(), "{diags:?}");
        assert!(task.interpret(Default::default()).unwrap().diags.is_empty());
    }
    let admitted = capture.present_last(&mut renderer).unwrap();
    assert_eq!(admitted.tasks[0].bindings, fixture.tasks[0].bindings);
    assert_eq!(admitted.tasks[1].bindings, fixture.tasks[1].bindings);
    let output = pollster::block_on(fixture.replay(device.clone(), queue.clone())).unwrap();
    assert!(output.diagnostics.iter().all(Vec::is_empty));
    let sequence = Sequence {
        frames: vec![fixture],
        warmup_frames: 0,
        presentations: vec![1],
    };
    let sequence = Sequence::from_bytes(&sequence.to_bytes().unwrap()).unwrap();
    let replay = pollster::block_on(sequence.replay(
        device.clone(),
        queue.clone(),
        crate::DepthResetPolicy::Never,
    ))
    .unwrap();
    assert_eq!(replay.presentations[0].output.rgba8, output.rgba8);
    assert!(replay.frames[0].diagnostics.iter().all(Vec::is_empty));
    #[cfg(feature = "profiling")]
    {
        let mut measured = Vec::new();
        pollster::block_on(sequence.measure(
            device,
            queue,
            MeasurementOptions {
                readback: true,
                command_trace: true,
                ..Default::default()
            },
            |frame| measured.push(frame),
        ))
        .unwrap();
        assert_eq!(measured.len(), 1);
        assert_eq!(measured[0].output.rgba8, output.rgba8);
        assert_eq!(measured[0].output.diagnostics, output.diagnostics);
        assert_eq!(measured[0].output.summaries, output.summaries);
    }
}

#[test]
fn capture_v2_preserves_enum_ids_and_rejects_invalid_metadata() {
    let (bytes, _) = s2dex_bg_tests::fixture(64, 32);
    let hardware = Image(bytes);
    let recording = RecordingHardware::new(&hardware);
    let result = interp::interpret(
        recording.rdram(),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty());
    let task = recording
        .finish_with_bindings(
            0,
            Microcode::S2dex2,
            DataFormat::Fixed,
            0,
            &[
                (0xfeedbeef_12345678, Microcode::F3dex2),
                (0x12345678, Microcode::F3d),
                (0xfeedbeef, Microcode::S2dex2),
            ],
        )
        .unwrap();
    let mut fixture = fixture(task);
    fixture.provenance = Provenance::default();
    let bytes = fixture.to_bytes().unwrap();
    let task_at = 132;
    assert_eq!(&bytes[task_at + 4..task_at + 8], &2u32.to_le_bytes());
    assert_eq!(&bytes[task_at + 29..task_at + 32], &[0; 3]);
    assert_eq!(&bytes[task_at + 160..task_at + 164], &0u32.to_le_bytes());
    assert_eq!(&bytes[task_at + 164..task_at + 168], &3u32.to_le_bytes());
    for (index, id) in [0u32, 1, 2].into_iter().enumerate() {
        let at = task_at + 176 + index * 12;
        assert_eq!(&bytes[at..at + 4], &id.to_le_bytes());
    }
    assert_eq!(Fixture::from_bytes(&bytes).unwrap(), fixture);
    for offset in [task_at + 4, task_at + 29, task_at + 160, task_at + 176] {
        let mut corrupt = bytes.clone();
        corrupt[offset] = 255;
        assert!(Fixture::from_bytes(&corrupt).is_err(), "offset {offset}");
    }
    let mut wrong_abi = bytes.clone();
    wrong_abi[task_at + 160] = 1;
    assert!(Fixture::from_bytes(&wrong_abi).is_err());
    for length in 0..bytes.len() {
        assert!(Fixture::from_bytes(&bytes[..length]).is_err());
    }
}
