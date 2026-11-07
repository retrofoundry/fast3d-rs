use super::*;

mod batches;

const POINTS: [[i16; 2]; 3] = [[13, 13], [1267, 27], [33, 118]];
const COUNTS: &[u8; 320 * 32] =
    include_bytes!("../../../../tests/fixtures/guest-triangle-e-counts.bin");

fn triangle(points: [[i16; 2]; 3], low: u32, reverse: bool, background: bool) -> Built {
    let mut b = DlBuilder::new();
    let projection = b.matrix([
        [1.0 / 1024.0, 0.0, 0.0, 0.0],
        [0.0, -1.0 / 128.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0, 1.0],
    ]);
    let viewport = b.viewport(Vp {
        vscale: [1024, 128, 511, 0],
        vtrans: [1024, 128, 511, 0],
    });
    let vertices = b.vertices(&points.map(|[x, y]| VtxColored {
        x,
        y,
        z: 0,
        flag: 0,
        s: 0,
        t: 0,
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    }));
    let mut dl = Vec::new();
    image(&mut dl, FB, 512, 64, 0, 2);
    if background {
        fill(&mut dl, 0x00010001, [0, 0, 511, 63]);
        if low & IM_RD != 0 {
            for x in (16..512).step_by(32) {
                fill(&mut dl, 0, [x, 4, x + 15, 7]);
            }
        }
    }
    dl.extend([
        (0xef00_0030 | G_TC_FILT, low),
        combine(3, 3),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gsp_viewport(viewport),
        gsp_matrix(projection, true, true, false),
        gsp_vertex(0, 3, vertices),
        if reverse {
            gsp_1triangle(2, 1, 0)
        } else {
            gsp_1triangle(0, 1, 2)
        },
    ]);
    built(b, dl)
}

fn accepted(r: &mut Renderer, memory: &mut Memory, task: Built) {
    assert_eq!(
        submit_guest(r, memory, task).termination,
        crate::inspect::WalkTermination::End
    );
    for patch in r.take_guest_writes() {
        memory.0[patch.address as usize..patch.address as usize + patch.bytes.len()]
            .copy_from_slice(&patch.bytes);
    }
}

#[test]
fn guest_route_gpu_triangle_e_calibration_both_windings_and_w3_parity() {
    assert_eq!(
        COUNTS
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        (0..=8).collect()
    );
    for fallback in [false, true] {
        for reverse in [false, true] {
            for low in [AA_EN, AA_EN | CVG_DST_FULL, G_ZS_PRIM] {
                let mut r = renderer(fallback);
                let mut cpu = renderer(fallback);
                let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
                let mut other = Memory(memory.0.clone());
                accepted(&mut r, &mut memory, triangle(POINTS, low, reverse, false));
                run_compatibility(&mut cpu, &mut other, triangle(POINTS, low, reverse, false));
                assert_eq!(
                    &memory.0[FB as usize..FB as usize + 512 * 64 * 2],
                    &other.0[FB as usize..FB as usize + 512 * 64 * 2]
                );
                let (_, gpu) = r
                    .inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .native_fill_pixels(&r.device, &r.queue)
                    .unwrap();
                for y in 0..32 {
                    for x in 0..320 {
                        let n = COUNTS[y * 320 + x];
                        let pixel = y * 512 + x;
                        let owned = r
                            .inner
                            .coherence
                            .as_ref()
                            .unwrap()
                            .stored_coverage_code(u64::from(FB) + pixel as u64 * 2);
                        assert_eq!(
                            owned,
                            cpu.inner
                                .coherence
                                .as_ref()
                                .unwrap()
                                .stored_coverage_code(u64::from(FB) + pixel as u64 * 2)
                        );
                        if low & AA_EN != 0 {
                            let expected = if n == 0 {
                                [0, 0, 0]
                            } else if low & CVG_DST_FULL != 0 {
                                [0xffff, 7, 1]
                            } else {
                                [0xfffe | u32::from(n >= 5), u32::from(n - 1), 1]
                            };
                            assert_eq!(
                                gpu[pixel], expected,
                                "({x},{y}), n={n}, reverse={reverse}, low={low:x}"
                            );
                            assert_eq!(
                                owned,
                                (n != 0).then(|| if low & CVG_DST_FULL != 0 { 7 } else { n - 1 })
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn guest_route_gpu_triangle_aa_only_pixel_and_hardware_edge_row() {
    for encoding in [
        crate::FramebufferEncoding::Native,
        crate::FramebufferEncoding::BigEndian,
    ] {
        let mut r = renderer(false);
        r.set_guest_writeback_with_encoding(true, encoding);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        accepted(
            &mut r,
            &mut memory,
            triangle(POINTS, AA_EN | IM_RD, false, true),
        );
        let mut cpu = renderer(false);
        cpu.set_guest_writeback_with_encoding(true, encoding);
        let mut other = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        run_compatibility(
            &mut cpu,
            &mut other,
            triangle(POINTS, AA_EN | IM_RD, false, true),
        );
        assert_eq!(
            &memory.0[FB as usize..FB as usize + 512 * 64 * 2],
            &other.0[FB as usize..FB as usize + 512 * 64 * 2]
        );
        for pixel in 0..512 * 64 {
            let address = u64::from(FB) + pixel * 2;
            assert_eq!(
                r.inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .stored_coverage_code(address),
                cpu.inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .stored_coverage_code(address)
            );
        }
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[4 * 512 + 115], [0xfffe, 3, 18]);
        assert_eq!(gpu[4 * 512 + 137], [0xfffe, 0, 18]);
        assert_eq!(gpu[3 * 512 + 2], [0x0001, 7, 1]);
        let expected = [
            0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff,
            0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff,
            0xffff, 0xffff, 0xffff, 0xffff, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe,
            0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xffff, 0xffff,
            0xffff, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe,
            0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe, 0xfffe,
            0xfffe, 0xfffe, 0x0001, 0x0001,
        ];
        for (offset, expected) in expected.into_iter().enumerate() {
            assert_eq!(gpu[4 * 512 + 70 + offset][0], expected);
        }
        let p = FB as usize + (4 * 512 + 115) * 2;
        assert_eq!(
            memory.0[p..p + 2],
            if encoding == crate::FramebufferEncoding::BigEndian {
                0xfffeu16.to_be_bytes()
            } else {
                0xfffeu16.to_ne_bytes()
            }
        );
        assert_eq!(
            &pixels(&mut r, FB, 512, 64)[(4 * 512 + 115) * 4..][..3],
            [0, 0, 0]
        );
    }
}

#[test]
fn guest_route_gpu_triangle_native_rejection_and_degeneracy() {
    for low in [G_ZS_PRIM, AA_EN] {
        let mut r = renderer(true);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        accepted(
            &mut r,
            &mut memory,
            triangle([[1, 1], [3, 1], [2, 3]], low, false, true),
        );
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(
            gpu[0],
            if low & AA_EN == 0 {
                [1, 7, 1]
            } else {
                [0xfffe, 1, 2]
            }
        );
        accepted(
            &mut r,
            &mut memory,
            triangle([[0, 0], [4, 4], [8, 8]], low, false, false),
        );
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert!(gpu.iter().all(|p| *p == [0; 3]));
    }
}

fn insert(mut task: Built, command: Command) -> Built {
    let offset = task.rdram[task.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .position(|c| c[0] == 0x05)
        .unwrap()
        * 8
        + task.entry as usize;
    task.rdram.splice(
        offset..offset,
        command
            .0
            .to_be_bytes()
            .into_iter()
            .chain(command.1.to_be_bytes()),
    );
    task
}

#[test]
fn guest_route_gpu_triangle_setup_boundaries_culling_and_w3_parity() {
    let points = [
        [[-4096, -8192], [4095, 1], [1, 8191]],
        [[-4096, 1], [4095, 2], [1, 255]],
        [[4095, 1], [-4096, 2], [1, 255]],
        [[13, -5], [1267, 27], [-33, 118]],
        [[0, 0], [2047, 0], [0, 255]],
        [[0, 0], [0, 255], [2047, 255]],
        [[0, 0], [0, 0], [4, 4]],
    ];
    let mut r = renderer(true);
    let mut cpu = renderer(true);
    for points in points {
        for reverse in [false, true] {
            r.reset();
            cpu.reset();
            let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
            let mut other = Memory(memory.0.clone());
            accepted(&mut r, &mut memory, triangle(points, AA_EN, reverse, false));
            run_compatibility(
                &mut cpu,
                &mut other,
                triangle(points, AA_EN, reverse, false),
            );
            assert_eq!(
                &memory.0[FB as usize..FB as usize + 512 * 64 * 2],
                &other.0[FB as usize..FB as usize + 512 * 64 * 2],
                "{points:?} {reverse}"
            );
            for p in 0..512 * 64 {
                let address = u64::from(FB) + p * 2;
                assert_eq!(
                    r.inner
                        .coherence
                        .as_ref()
                        .unwrap()
                        .stored_coverage_code(address),
                    cpu.inner
                        .coherence
                        .as_ref()
                        .unwrap()
                        .stored_coverage_code(address)
                );
            }
        }
    }
    for reverse in [false, true] {
        r.reset();
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        accepted(
            &mut r,
            &mut memory,
            insert(
                triangle([[0, 0], [16, 0], [0, 16]], AA_EN, reverse, false),
                gsp_set_geometrymode(G_CULL_BACK),
            ),
        );
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[0], if reverse { [0xffff, 7, 1] } else { [0, 0, 0] });
    }
}

#[test]
fn guest_route_gpu_triangle_culling_matches_w3_float_area_tie() {
    let points = [[-4096, -8192], [0, -1], [4095, 8188]];
    for reverse in [false, true] {
        let mut r = renderer(true);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        accepted(
            &mut r,
            &mut memory,
            insert(
                triangle(points, AA_EN, reverse, false),
                gsp_set_geometrymode(G_CULL_BACK),
            ),
        );
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[0], [0, 0, 0]);
        assert!(r.inner.coherence.as_ref().unwrap().owners.ranges.is_empty());
    }
}

fn matrix_halfword(mut task: Built, offset: usize, value: u16) -> Built {
    let command = task.rdram[task.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .find(|c| c[0] == 0xda)
        .unwrap();
    let address = u32::from_be_bytes(command[4..].try_into().unwrap()) as usize;
    task.rdram[address + offset..address + offset + 2].copy_from_slice(&value.to_be_bytes());
    task
}

#[test]
fn guest_route_gpu_triangle_routes_unimplemented_inputs_atomically() {
    let mut tasks = vec![
        triangle(POINTS, AA_EN | IM_RD, false, false),
        triangle(POINTS, AA_EN | IM_RD | CVG_DST_WRAP, false, false),
        triangle(POINTS, AA_EN | IM_RD | CVG_DST_SAVE, false, false),
        insert(triangle(POINTS, AA_EN, false, true), combine(4, 4)),
        insert(
            triangle(POINTS, AA_EN, false, true),
            gdp_set_prim_color(0, 0, 0x4080c0ff),
        ),
        insert(
            triangle(POINTS, AA_EN, false, true),
            gdp_set_prim_color(0, 0, 0xffffff80),
        ),
        insert(
            triangle(POINTS, AA_EN, false, true),
            gdp_fill_rectangle(0, 0, 4, 4),
        ),
        matrix_halfword(triangle(POINTS, AA_EN, false, true), 56, 16),
        matrix_halfword(
            triangle([[0, 0], [16, 0], [0, 16]], AA_EN, false, true),
            30,
            2,
        ),
        {
            let mut task = triangle(POINTS, AA_EN, false, true);
            let command = task.rdram[task.entry as usize..]
                .as_chunks_mut::<8>()
                .0
                .iter_mut()
                .find(|c| c[0] == 0xff)
                .unwrap();
            command[2..4].copy_from_slice(&510u16.to_be_bytes());
            task
        },
    ];
    for (rgb, alpha, key) in [(1, 3, 0), (0, 2, 0), (0, 3, 1)] {
        tasks.push(insert(
            triangle(POINTS, AA_EN, false, true),
            (
                0xef00_0000 | G_TC_FILT | (rgb << 6) | (alpha << 4) | (key << 8),
                AA_EN,
            ),
        ));
    }
    for (case, task) in tasks.into_iter().enumerate() {
        let mut r = renderer(false);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        let state = r.rdp.clone();
        memory.0[..FB as usize].copy_from_slice(&task.rdram[..FB as usize]);
        assert_native_unavailable(&mut r, &memory, task.entry.into());
        assert!(r.take_guest_writes().is_empty());
        assert!(r.inner.coherence.as_ref().unwrap().owners.ranges.is_empty());
        assert!(!r.inner.has_fb(u64::from(FB)));
        assert_eq!(r.rdp, state);
        let mut cpu = renderer(false);
        let mut other = Memory(memory.0.clone());
        let recorder = crate::profiling::Recorder::new(crate::profiling::Mode::Detailed);
        r.inner.profiling = recorder.clone();
        assert_eq!(
            r.process_dl(
                &memory,
                task.entry.into(),
                crate::Microcode::F3dex2,
                &mut crate::NopSink
            )
            .termination,
            crate::inspect::WalkTermination::End,
            "case {case}"
        );
        for patch in r.take_guest_writes() {
            memory.0[patch.address as usize..patch.address as usize + patch.bytes.len()]
                .copy_from_slice(&patch.bytes);
        }
        run_compatibility(&mut cpu, &mut other, task);
        assert_eq!(memory.0, other.0, "case {case}");
        assert_eq!(r.rdp, cpu.rdp, "case {case}");
        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.timings["interpretation"].calls, 1);
        assert_eq!(snapshot.counters["tasks.guest.compatibility"], 1);
        assert!(!snapshot.counters.contains_key("tasks.guest.native"));
    }
}

#[test]
fn guest_route_gpu_triangle_fill_overwrite_keeps_gpu_writer_order() {
    let mut task = triangle([[0, 0], [16, 0], [0, 16]], AA_EN, false, true);
    let offset = task.rdram[task.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .position(|c| c[0] == 0x05)
        .unwrap()
        * 8
        + task.entry as usize
        + 8;
    let mut commands = Vec::new();
    fill(&mut commands, 0x12341234, [0, 0, 0, 0]);
    commands.push(gdp_set_render_mode(G_ZS_PRIM, 0));
    task.rdram.splice(
        offset..offset,
        commands
            .into_iter()
            .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes())),
    );
    let mut r = renderer(true);
    let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
    accepted(&mut r, &mut memory, task);
    let (_, gpu) = r
        .inner
        .coherence
        .as_ref()
        .unwrap()
        .native_fill_pixels(&r.device, &r.queue)
        .unwrap();
    assert_eq!(gpu[0], [0x1234, 0, 3]);
    assert_eq!(gpu[1], [0xffff, 7, 2]);
    assert_eq!(word(&memory, FB, 0), 0x1234);
    assert_eq!(word(&memory, FB, 1), 0xffff);
}

#[test]
fn guest_route_gpu_triangle_constant_endpoint_colours() {
    for (rgb, alpha, command, expected) in [
        (3, 3, gdp_set_prim_color(0, 0, 0xff0000ff), 0xf801),
        (3, 3, gdp_set_prim_color(0, 0, 0x00ff00ff), 0x07c1),
        (5, 5, gdp_set_env_color(0x0000ffff), 0x003f),
        (6, 6, gdp_pipe_sync(), 0xffff),
        (7, 6, gdp_pipe_sync(), 0x0001),
    ] {
        let mut r = renderer(false);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        let task = insert(
            insert(
                triangle([[0, 0], [16, 0], [0, 16]], AA_EN, false, false),
                combine(rgb, alpha),
            ),
            command,
        );
        accepted(&mut r, &mut memory, task);
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[0], [expected, 7, 1]);
        assert_eq!(word(&memory, FB, 0), expected as u16);
    }
}
