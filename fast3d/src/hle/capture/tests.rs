use super::*;
use crate::{DataFormat, Hardware, Rdram, RdramImage};

struct Image(Vec<u8>);
impl Hardware for Image {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(&self.0)
    }
}

#[test]
fn capture_missing_span_is_error() {
    let hw = Image(vec![0xdf, 0, 0, 0, 0, 0, 0, 0]);
    let recorder = RecordingHardware::new(&hw);
    let mem = recorder.rdram();
    assert_eq!(mem.read_command(0).w0, 0xdf00_0000);
    drop(mem);
    let task = recorder
        .finish(0, crate::Microcode::F3dex2, DataFormat::Fixed, 0)
        .unwrap();
    let replay = ReplayHardware::new(&task, None).unwrap();
    assert_eq!(replay.rdram().read_command(0).w0, 0xdf00_0000);
    assert!(replay.check().is_ok());
    assert!(!replay.rdram().in_bounds(8, 8));
    assert!(matches!(
        replay.check(),
        Err(CaptureError::MissingSpan {
            address: 8,
            length: 8
        })
    ));
    assert_eq!(&*replay.rdram().read_bytes(7, 2), &[0, 0]);
    assert!(replay.check().is_err());
}

fn frame() -> Frame {
    Frame {
        serial: 42,
        dither_seed: 17,
        width: 65,
        height: 48,
        vi: None,
        dual_source_blending: false,
        config: crate::RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: crate::ClearPolicy::Persist,
            power_preference: wgpu::PowerPreference::LowPower,
        },
    }
}

fn fixture(task: Task) -> Fixture {
    Fixture {
        frame: frame(),
        tasks: vec![task],
        provenance: Provenance {
            synthetic_data: "literal test payloads".into(),
            ..Default::default()
        },
    }
}

#[test]
fn fixture_container_rejects_corruption() {
    let hw = Image(vec![0xdf, 0, 0, 0, 0, 0, 0, 0]);
    let recording = RecordingHardware::new(&hw);
    recording.rdram().read_command(0);
    let f = fixture(
        recording
            .finish(0, crate::Microcode::F3dex2, DataFormat::Fixed, 0)
            .unwrap(),
    );
    let b = f.to_bytes().unwrap();
    assert_eq!(&b[..16], b"F3DCAP\0\0\x01\0\0\0\x01\x02\x03\x04");
    assert_eq!(Fixture::from_bytes(&b).unwrap(), f);
    for n in 0..b.len() {
        assert!(Fixture::from_bytes(&b[..n]).is_err(), "truncation at {n}");
    }
    for offset in [0, 8, 12, 16, 28, 60, 64, 68, 72, 76, 80] {
        let mut corrupt = b.clone();
        corrupt[offset] = 255;
        assert!(
            Fixture::from_bytes(&corrupt).is_err(),
            "corruption at {offset}"
        );
    }
    let mut bad = f.clone();
    bad.tasks[0].spans.push(MemorySpan {
        address: 7,
        bytes: vec![0],
    });
    assert!(bad.to_bytes().is_err());
    bad.tasks[0].spans = vec![MemorySpan {
        address: u64::MAX,
        bytes: vec![0],
    }];
    assert!(bad.to_bytes().is_err());
    let mut bad_offset = b.clone();
    let directory_offset = b.len() - 8 - 24;
    bad_offset[directory_offset + 16..directory_offset + 24]
        .copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Fixture::from_bytes(&bad_offset).is_err());
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod host {
    use super::*;
    use crate::HostRam;
    struct Host {
        segments: [u64; 16],
    }
    impl Hardware for Host {
        fn rdram(&self) -> impl Rdram + '_ {
            let mut r = unsafe { HostRam::new(&[]) };
            r.segments = self.segments;
            r
        }
    }
    fn host() -> Host {
        Host { segments: [0; 16] }
    }
    fn task(recording: RecordingHardware<'_, Host>, entry: u64, format: DataFormat) -> Task {
        recording
            .finish(entry, crate::Microcode::F3dex2, format, 0)
            .unwrap()
    }

    #[test]
    fn capture_host64_replays_after_source_drop() {
        let hw = host();
        let body = Box::new([0xdf00_0000usize, 0xdead_beef_7654_3210usize]);
        let entry = Box::new([0xde00_0000usize, body.as_ptr() as usize, 0xdf00_0000, 0]);
        let at = entry.as_ptr() as u64;
        assert!(at > u32::MAX as u64);
        let rec = RecordingHardware::new(&hw);
        let live = crate::hle::interpret(
            hw.rdram(),
            at,
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        let recorded = crate::hle::interpret(
            rec.rdram(),
            at,
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        assert_eq!(recorded.scene, live.scene);
        assert_eq!(recorded.diags, live.diags);
        assert_eq!(recorded.commands, live.commands);
        let task = task(rec, at, DataFormat::Fixed);
        let body_at = body.as_ptr() as u64;
        drop(entry);
        drop(body);
        let bytes = fixture(task).to_bytes().unwrap();
        let f = Fixture::from_bytes(&bytes).unwrap();
        let replay = ReplayHardware::new(&f.tasks[0], None).unwrap();
        let c = replay.rdram().read_command(body_at);
        assert_eq!(
            (c.w0, c.w1, c.w1_addr),
            (0xdf00_0000, 0x7654_3210, 0xdead_beef_7654_3210)
        );
        let got = crate::hle::interpret(
            replay.rdram(),
            at,
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        replay.check().unwrap();
        assert_eq!(got.commands, 3);
        assert_eq!(got.commands, live.commands);
        assert_eq!(got.diags, live.diags);
        assert_eq!(got.scene, live.scene);
    }

    #[test]
    fn capture_host64_preserves_interior_aliases_and_segments() {
        let data = Box::new([11u8, 12, 13, 14, 15, 16, 17, 18, 19]);
        let base = data.as_ptr() as u64;
        let mut hw = host();
        hw.segments[2] = base;
        let rec = RecordingHardware::new(&hw);
        let mut mem = rec.rdram();
        assert_eq!(mem.resolve_masked(0x0200_0003), base + 3);
        assert_eq!(&*mem.read_bytes(base + 3, 5), &[14, 15, 16, 17, 18]);
        assert_eq!(&*mem.read_bytes(base + 1, 5), &[12, 13, 14, 15, 16]);
        mem.set_segment(3, base + 1);
        assert_eq!(mem.read_u8(mem.resolve(0x0300_0005)), 17);
        drop(mem);
        let t = task(rec, base + 3, DataFormat::Fixed);
        assert_eq!(
            t.spans,
            vec![MemorySpan {
                address: base + 1,
                bytes: vec![12, 13, 14, 15, 16, 17, 18]
            }]
        );
        drop(data);
        let replay = ReplayHardware::new(&t, None).unwrap();
        let mut mem = replay.rdram();
        assert_eq!(mem.resolve_masked(0x0200_0003), base + 3);
        assert_eq!(mem.read_u8(mem.resolve(0x0200_0003)), 14);
        mem.set_segment(3, base + 1);
        assert_eq!(mem.read_u8(mem.resolve(0x0300_0005)), 17);
        assert_eq!(
            mem.resolve_masked(0xfedc_ba98_7654_3217),
            0xfedc_ba98_7654_3217
        );
        replay.check().unwrap();
    }

    #[test]
    fn capture_host_layout_fixed_and_float() {
        for format in [DataFormat::Fixed, DataFormat::Float] {
            let hw = host();
            let rec = RecordingHardware::new(&hw);
            let expected = [
                [-1.5, 2.25, -3.75, 4.5],
                [5.125, -6.25, 7.5, -8.75],
                [9., 10., -11., 12.],
                [13.5, -14.25, 15.75, 1.],
            ];
            let mut matrix = Vec::new();
            match format {
                DataFormat::Float => {
                    for v in expected.iter().flatten() {
                        matrix.extend_from_slice(&f32::to_ne_bytes(*v));
                    }
                }
                DataFormat::Fixed => {
                    let words = [
                        0xfffe0002u32,
                        0xfffc0004,
                        0x0005fff9,
                        0x0007fff7,
                        0x0009000a,
                        0xfff5000c,
                        0x000dfff1,
                        0x000f0001,
                        0x80004000,
                        0x40008000,
                        0x2000c000,
                        0x80004000,
                        0,
                        0,
                        0x8000c000,
                        0xc0000000,
                    ];
                    for w in words {
                        matrix.extend_from_slice(&w.to_ne_bytes());
                    }
                }
            }
            let stride = if format == DataFormat::Float { 24 } else { 16 };
            let mut vertices = vec![std::mem::MaybeUninit::<u8>::uninit(); stride * 2];
            let p = vertices.as_mut_ptr().cast::<u8>();
            for i in 0..2 {
                let mut fields = Vec::new();
                match format {
                    DataFormat::Float => {
                        for v in [-12.5f32, 27.25, i as f32 + 0.5] {
                            fields.extend_from_slice(&v.to_ne_bytes());
                        }
                    }
                    DataFormat::Fixed => {
                        for v in [-12i16, 27, i as i16] {
                            fields.extend_from_slice(&v.to_ne_bytes());
                        }
                    }
                }
                let st_offset = if format == DataFormat::Float { 14 } else { 8 };
                unsafe {
                    std::ptr::copy_nonoverlapping(fields.as_ptr(), p.add(i * stride), fields.len());
                    std::ptr::copy_nonoverlapping(
                        (-321i16).to_ne_bytes().as_ptr(),
                        p.add(i * stride + st_offset),
                        2,
                    );
                    std::ptr::copy_nonoverlapping(
                        (1234i16).to_ne_bytes().as_ptr(),
                        p.add(i * stride + st_offset + 2),
                        2,
                    );
                    std::ptr::copy_nonoverlapping(
                        [128, 17, 255, 64].as_ptr(),
                        p.add(i * stride + st_offset + 4),
                        4,
                    );
                }
            }
            let a = matrix.as_ptr() as u64;
            let b = p as u64;
            let live = hw.rdram();
            let mem = rec.rdram();
            assert_eq!(live.read_matrix(a, format), expected);
            assert_eq!(mem.read_matrix(a, format), expected);
            assert_eq!(mem.vertex_stride(format), stride as u64);
            for i in 0..2 {
                let v = mem.read_vertex(b + i * stride as u64, format);
                let expected_pos = if format == DataFormat::Float {
                    [-12.5, 27.25, i as f32 + 0.5]
                } else {
                    [-12., 27., i as f32]
                };
                assert_eq!(v.pos, expected_pos);
                assert_eq!(v.st, [-321, 1234]);
                assert_eq!(v.rgba, [128, 17, 255, 64]);
                assert_eq!(live.read_vertex(b + i * stride as u64, format).pos, v.pos);
            }
            drop(mem);
            let t = task(rec, a, format);
            assert_eq!(
                t.spans.iter().map(|s| s.bytes.len()).sum::<usize>(),
                64 + if format == DataFormat::Float { 40 } else { 28 }
            );
            drop(matrix);
            drop(vertices);
            let replay = ReplayHardware::new(&t, None).unwrap();
            let mem = replay.rdram();
            assert_eq!(mem.read_matrix(a, format), expected);
            assert_eq!(mem.read_vertex(b + stride as u64, format).st, [-321, 1234]);
            assert_eq!(mem.read_vertex(b, format).rgba, [128, 17, 255, 64]);
            replay.check().unwrap();
        }
    }

    #[test]
    fn capture_records_typed_reads_and_texrect_continuations() {
        let hw = host();
        let matrix = Box::new([
            0x00010000u32,
            0,
            1,
            0,
            0,
            0x00010000,
            0,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]);
        let vertex = Box::new([-12i16, 23, 45, 0, -321, 1234, 0x0201, -253]);
        let commands = Box::new([
            0xda380007usize,
            matrix.as_ptr() as usize,
            0x01001002,
            vertex.as_ptr() as usize,
            0xff10013f,
            0x234567001,
            0xe45003c0,
            0x0001401c,
            0xe1000000,
            0x000b000d,
            0xf1000000,
            0x04000200,
            0xf8000000,
            0xdeadbeef,
            0xdf000000,
            0,
        ]);
        let entry = commands.as_ptr() as u64;
        let rec = RecordingHardware::new(&hw);
        let live = crate::hle::interpret(
            rec.rdram(),
            entry,
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        let t = task(rec, entry, DataFormat::Fixed);
        assert_eq!(live.scene.raw_pos, vec![[-12., 23., 45.]]);
        assert_eq!(live.commands, 6);
        assert_eq!(live.rdp.fog_color, [222, 173, 190, 239]);
        let m = matrix.as_ptr() as u64;
        let v = vertex.as_ptr() as u64;
        drop(commands);
        drop(matrix);
        drop(vertex);
        let replay = ReplayHardware::new(&t, None).unwrap();
        let got = crate::hle::interpret(
            replay.rdram(),
            entry,
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        replay.check().unwrap();
        assert_eq!(got.diags, live.diags);
        assert_eq!(got.commands, live.commands);
        assert_eq!(got.scene, live.scene);
        assert_eq!(replay.rdram().read_command(entry + 4 * 16).w1, 0x000b000d);
        assert_eq!(replay.rdram().read_command(entry + 5 * 16).w1, 0x04000200);
        assert_eq!(
            replay.rdram().read_matrix(m, DataFormat::Fixed)[0],
            [1., 0., 0., 0.]
        );
        assert_eq!(
            replay.rdram().read_vertex(v, DataFormat::Fixed).st,
            [-321, 1234]
        );
        replay.check().unwrap();
        for missing in [entry + 4 * 16, m, v] {
            let mut broken = t.clone();
            let span = broken
                .spans
                .iter()
                .position(|s| s.address <= missing && missing < s.address + s.bytes.len() as u64)
                .unwrap();
            let old = broken.spans.remove(span);
            let offset = (missing - old.address) as usize;
            if offset > 0 {
                broken.spans.insert(
                    span,
                    MemorySpan {
                        address: old.address,
                        bytes: old.bytes[..offset].to_vec(),
                    },
                );
            }
            let replay = ReplayHardware::new(&broken, None).unwrap();
            crate::hle::interpret(
                replay.rdram(),
                entry,
                crate::hle::gbi::GbiUcode::F3dex2,
                DataFormat::Fixed,
                None,
            );
            assert!(
                replay.check().is_err(),
                "missing typed/continuation bytes at {missing:#x}"
            );
        }
    }

    #[test]
    fn capture_task_snapshots_allow_memory_reuse() {
        let hw = host();
        let mut bytes = Box::new([11u8, 22, 33, 44]);
        let address = bytes.as_ptr() as u64;
        let first = RecordingHardware::new(&hw);
        assert_eq!(&*first.rdram().read_bytes(address, 4), &[11, 22, 33, 44]);
        let first = first
            .finish(address, crate::Microcode::F3d, DataFormat::Fixed, 0)
            .unwrap();
        bytes[1] = 99;
        let second = RecordingHardware::new(&hw);
        assert_eq!(&*second.rdram().read_bytes(address, 4), &[11, 99, 33, 44]);
        let second = second
            .finish(address, crate::Microcode::F3d, DataFormat::Float, 1)
            .unwrap();
        let mut f = fixture(first);
        f.tasks.push(second);
        drop(bytes);
        let f = Fixture::from_bytes(&f.to_bytes().unwrap()).unwrap();
        for (task, expected) in f.tasks.iter().zip([22, 99]) {
            let replay = ReplayHardware::new(task, None).unwrap();
            assert_eq!(replay.rdram().read_u8(address + 1), expected);
            replay.check().unwrap();
        }
    }

    #[test]
    fn capture_conflicting_reads_invalidate_task() {
        let hw = host();
        let mut bytes = Box::new([11u8, 22, 33, 44]);
        let address = bytes.as_ptr() as u64;
        let rec = RecordingHardware::new(&hw);
        let mem = rec.rdram();
        mem.read_bytes(address, 3);
        bytes[1] = 99;
        mem.read_bytes(address + 1, 3);
        drop(mem);
        assert_eq!(
            rec.finish(address, crate::Microcode::F3d, DataFormat::Fixed, 0),
            Err(CaptureError::ConflictingRead {
                address: address + 1
            })
        );
    }
}

#[test]
fn capture_checked_in_high_address_fixture_walks() {
    let fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    let task = &fixture.tasks[0];
    let replay = ReplayHardware::new(task, fixture.frame.vi).unwrap();
    assert_eq!(task.entry, 0x0000_0001_2345_6000);
    let result = crate::hle::interpret(
        replay.rdram(),
        task.entry,
        task.microcode.into(),
        task.data_format,
        None,
    );
    replay.check().unwrap();
    assert_eq!(result.commands, 6);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let pairs = &result.scene.framebuffer_pairs;
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].color_image.addr, 0x0000_0002_3456_7000);
    assert_eq!(pairs[0].color_image.width, 64);
    assert_eq!(pairs[0].size_extent.1, 48);
    assert!(matches!(
        pairs[0].ops.last(),
        Some(crate::hle::SceneOp::FillRect {
            color_raw: 0xf801_f801,
            ..
        })
    ));
}

#[test]
fn capture_replayed_task_records_identical_spans() {
    let mut image = Image(vec![0; 0x30]);
    for (address, w0, w1) in [
        (0, 0xde00_0000u32, 0x20u32),
        (8, 0xdf00_0000, 0),
        (0x20, 0xff10_003f, 0x0010_0000),
        (0x28, 0xdf00_0000, 0),
    ] {
        image.0[address..address + 4].copy_from_slice(&w0.to_be_bytes());
        image.0[address + 4..address + 8].copy_from_slice(&w1.to_be_bytes());
    }
    let recording = RecordingHardware::new(&image);
    let result = crate::hle::interpret(
        recording.rdram(),
        0,
        Microcode::F3dex2.into(),
        DataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let image_task = recording
        .finish(0, Microcode::F3dex2, DataFormat::Fixed, 7)
        .unwrap();
    assert_eq!(image_task.spans.len(), 2);
    drop(image);
    let host_fixture =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    for mut task in [image_task, host_fixture.tasks[0].clone()] {
        task.source.segments[4] = task.entry;
        let replay = ReplayHardware::new(&task, None).unwrap();
        let recording = RecordingHardware::new(&replay);
        let result = crate::hle::interpret(
            recording.rdram(),
            task.entry,
            task.microcode.into(),
            task.data_format,
            None,
        );
        replay.check().unwrap();
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let recorded = recording
            .finish(task.entry, task.microcode, task.data_format, task.order)
            .unwrap();
        assert_eq!(recorded.spans, task.spans);
        assert_eq!(recorded, task);

        let mut memory = replay.rdram();
        assert_eq!(memory.capture_layout(), Some(task.source));
        memory.set_segment(4, 0x1234_5678);
        let mut updated_source = task.source;
        updated_source.segments[4] = 0x1234_5678;
        assert_eq!(memory.capture_layout(), Some(updated_source));
    }
}

#[test]
fn capture_image_preserves_masking_endianness_and_typed_reads() {
    struct SegmentedImage(Vec<u8>);
    impl Hardware for SegmentedImage {
        fn rdram(&self) -> impl Rdram + '_ {
            let mut mem = RdramImage::new(&self.0);
            mem.segments[2] = 0x20;
            mem
        }
    }
    let mut hw = SegmentedImage(vec![0; 128]);
    let expected = [
        [-1.5, 2.25, 3., 4.],
        [5., 6., 7., 8.],
        [9., 10., 11., 12.],
        [13., 14., 15., 1.],
    ];
    hw.0[32..96].copy_from_slice(&n64_gbi::encode::mtx_to_bytes(expected));
    hw.0[0..8].copy_from_slice(&[0xdf, 0, 0, 0, 0xfe, 0xdc, 0xba, 0x98]);
    hw.0[96..112].copy_from_slice(&[
        0xff, 0x80, 0, 23, 1, 0, 99, 99, 0xfe, 0xdc, 0x12, 0x34, 1, 2, 3, 255,
    ]);
    let rec = RecordingHardware::new(&hw);
    let mem = rec.rdram();
    assert_eq!(mem.resolve_masked(0x0200_0007), 32);
    assert_eq!(mem.resolve(0x0200_0007), 39);
    assert_eq!(mem.read_command(0).w1_addr, 0xfedc_ba98);
    assert_eq!(mem.read_matrix(32, DataFormat::Fixed), expected);
    assert_eq!(
        mem.read_vertex(96, DataFormat::Fixed).pos,
        [-128., 23., 256.]
    );
    assert_eq!(mem.read_i8(96), -1);
    assert_eq!(mem.read_u16(106), 0x1234);
    drop(mem);
    let t = rec
        .finish(0, Microcode::F3dex2, DataFormat::Fixed, 0)
        .unwrap();
    drop(hw);
    let replay = ReplayHardware::new(&t, None).unwrap();
    let mem = replay.rdram();
    assert_eq!(mem.resolve_masked(0x0200_0007), 32);
    assert_eq!(mem.resolve(0x0200_0007), 39);
    assert_eq!(mem.read_command(0).w1_addr, 0xfedc_ba98);
    assert_eq!(mem.read_matrix(32, DataFormat::Fixed), expected);
    assert_eq!(mem.read_vertex(96, DataFormat::Fixed).st, [-292, 0x1234]);
    assert_eq!(mem.read_i8(96), -1);
    assert_eq!(mem.read_u16(106), 0x1234);
    replay.check().unwrap();
}

#[test]
fn replay_adjacent_spans_are_one_address_interval() {
    let mut f =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    let span = f.tasks[0].spans.remove(0);
    f.tasks[0].spans = vec![
        MemorySpan {
            address: span.address,
            bytes: span.bytes[..19].to_vec(),
        },
        MemorySpan {
            address: span.address + 19,
            bytes: span.bytes[19..].to_vec(),
        },
    ];
    let replay = ReplayHardware::new(&f.tasks[0], None).unwrap();
    assert!(replay.rdram().in_bounds(span.address + 16, 16));
    let command = replay.rdram().read_command(span.address + 16);
    assert_eq!(command.w1_addr, 0x234567000);
    replay.check().unwrap();
}

#[test]
fn replay_host_big_endian_uses_recorded_byte_order() {
    let mut f =
        Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap")).unwrap();
    f.tasks[0].source.memory.byte_order = ByteOrder::Big;
    for word in f.tasks[0].spans[0].bytes.as_chunks_mut::<8>().0.iter_mut() {
        word.reverse();
    }
    let replay = ReplayHardware::new(&f.tasks[0], None).unwrap();
    let mem = replay.rdram();
    let command = mem.read_command(f.tasks[0].entry + 16);
    assert_eq!(
        (command.w0, command.w1, command.w1_addr),
        (0xff10003f, 0x34567000, 0x234567000)
    );
    replay.check().unwrap();
}

#[test]
fn capture_missing_span_near_address_limit_does_not_panic() {
    for commands in [
        vec![0xdc000008u64, u64::MAX, 0xdf000000, 0],
        vec![0xdc00000a, u64::MAX, 0xdf000000, 0],
        vec![0x01003006, u64::MAX - 20, 0xdf000000, 0],
        vec![0xfd10003f, u64::MAX, 0xf4004004, 0x07008008, 0xdf000000, 0],
    ] {
        let mut f =
            Fixture::from_bytes(include_bytes!("../../../tests/fixtures/host64-fill.f3dcap"))
                .unwrap();
        let task = &mut f.tasks[0];
        task.microcode = Microcode::F3dex2;
        task.spans[0].bytes = commands.iter().flat_map(|w| w.to_le_bytes()).collect();
        let task = &Fixture::from_bytes(&f.to_bytes().unwrap()).unwrap().tasks[0];
        let replay = ReplayHardware::new(task, None).unwrap();
        crate::hle::interpret(
            replay.rdram(),
            task.entry,
            task.microcode.into(),
            task.data_format,
            None,
        );
        assert!(matches!(
            replay.check(),
            Err(CaptureError::MissingSpan { .. })
        ));
    }
}

#[test]
fn final_color_image_follows_nested_lists_and_segments() {
    let words: [(u32, u32); 7] = [
        (0xff10_013f, 0x0010_0000),
        (0x0600_0000, 0x0400_0018),
        (0xb800_0000, 0),
        (0xbc00_0806, 0x0020_0000),
        (0xff18_00ff, 0x0200_0100),
        (0xb800_0000, 0),
        (0xff10_003f, 0x0030_0000),
    ];
    let task = Task {
        entry: 0x100,
        microcode: Microcode::F3d,
        data_format: DataFormat::Fixed,
        order: 0,
        source: SourceLayout {
            memory: MemoryLayout::IMAGE,
            segments: {
                let mut segments = [0; 16];
                segments[4] = 0x100;
                segments
            },
        },
        spans: vec![MemorySpan {
            address: 0x100,
            bytes: words
                .into_iter()
                .flat_map(|(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
                .collect(),
        }],
    };
    let color = task.final_color_image().unwrap();
    assert_eq!(
        (color.addr, color.width, color.fmt, color.siz),
        (0x0020_0100, 256, 0, 3)
    );
    let mut incomplete = task;
    incomplete.spans[0].bytes.truncate(32);
    assert!(incomplete.final_color_image().is_err());
}
