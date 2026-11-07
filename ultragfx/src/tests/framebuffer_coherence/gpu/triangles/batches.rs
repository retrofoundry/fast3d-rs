use super::*;

fn batch(points: [[i16; 2]; 3], low: u32, count: usize) -> Built {
    let mut task = triangle(points, low, false, true);
    let commands = task.rdram[task.entry as usize..].as_chunks_mut::<8>().0;
    let fill = commands.iter_mut().find(|c| c[0] == 0xf7).unwrap();
    fill[4..].fill(0);
    for _ in 1..count {
        task = insert(task, gsp_1triangle(0, 1, 2));
    }
    task
}

fn compare(low: u32, count: usize, expected: [u32; 3]) {
    for (fallback, encoding) in [
        (false, crate::FramebufferEncoding::Native),
        (true, crate::FramebufferEncoding::BigEndian),
    ] {
        let mut r = renderer(fallback);
        let mut cpu = renderer(fallback);
        r.set_guest_writeback_with_encoding(true, encoding);
        cpu.set_guest_writeback_with_encoding(true, encoding);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        let mut other = Memory(memory.0.clone());
        let mut expected = expected;
        if low & IM_RD != 0 {
            expected[2] = 18;
        }
        let points = [[1, 1], [3, 1], [2, 3]];
        accepted(&mut r, &mut memory, batch(points, low, count));
        run_compatibility(&mut cpu, &mut other, batch(points, low, count));
        assert_eq!(&memory.0[FB as usize..], &other.0[FB as usize..]);
        let coherence = r.inner.coherence.as_ref().unwrap();
        let (_, gpu) = coherence.native_fill_pixels(&r.device, &r.queue).unwrap();
        assert_eq!(gpu[0], expected);
        assert_eq!(gpu[1], [0, 0, 1]);
        for pixel in 0..512 * 64 {
            let address = u64::from(FB) + pixel * 2;
            assert_eq!(
                coherence.stored_coverage_code(address),
                cpu.inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .stored_coverage_code(address)
            );
        }
        let tris: Vec<_> = r.frame_scenes.last().unwrap().framebuffer_pairs[0]
            .ops
            .iter()
            .filter_map(|op| {
                if let crate::scene::SceneOp::Tris(run) = op {
                    Some(run.index_count)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(tris, [count as u32 * 3]);
    }
}

#[test]
fn guest_route_gpu_batch_overlapping_triangles_in_one_run() {
    compare(AA_EN | IM_RD, 2, [0xffff, 4, 2]);
}

#[test]
fn guest_route_gpu_batch_wrap_crosses_seven() {
    compare(AA_EN | IM_RD | CVG_DST_WRAP, 4, [0xfffe, 0, 2]);
}

#[test]
fn guest_route_gpu_batch_save_preserves_destination() {
    compare(AA_EN | IM_RD | CVG_DST_SAVE, 5, [0xfffe, 0, 2]);
}

#[test]
fn guest_route_gpu_batch_image_read_off_uses_seven() {
    compare(AA_EN | CVG_DST_WRAP, 5, [0xfffe, 1, 2]);
    compare(AA_EN | CVG_DST_SAVE, 5, [0xffff, 7, 2]);
}

#[test]
fn guest_route_gpu_batch_primitive_257_continues_history() {
    compare(AA_EN | IM_RD | CVG_DST_WRAP, 257, [0xfffe, 2, 2]);
}

#[test]
fn guest_route_gpu_batch_tiny_primitive_and_tile_caps_match() {
    let mut outputs = Vec::new();
    let mut cpu = renderer(true);
    let mut other = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
    run_compatibility(
        &mut cpu,
        &mut other,
        batch(POINTS, AA_EN | IM_RD | CVG_DST_WRAP, 257),
    );
    for limits in [None, Some((3, 5))] {
        let mut r = renderer(true);
        r.inner.coherence.as_mut().unwrap().native_batch_limits = limits;
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        accepted(
            &mut r,
            &mut memory,
            batch(POINTS, AA_EN | IM_RD | CVG_DST_WRAP, 257),
        );
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[4 * 512 + 137], [0xfffe, 1, 18]);
        assert_eq!(gpu[4 * 512 + 115], [0xfffe, 3, 18]);
        assert_eq!(&memory.0[FB as usize..], &other.0[FB as usize..]);
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
        outputs.push((memory.0, gpu));
    }
    assert_eq!(outputs[0], outputs[1]);
}

fn append(mut task: Built, commands: Vec<Command>) -> Built {
    let offset = task.rdram[task.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .position(|c| c[0] == 0xdf)
        .unwrap()
        * 8
        + task.entry as usize;
    task.rdram.splice(
        offset..offset,
        commands
            .into_iter()
            .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes())),
    );
    task
}

#[test]
fn guest_route_gpu_batch_orders_modes_colours_scissors_and_fills() {
    for limits in [None, Some((1, 7))] {
        let task = || {
            let task = insert(
                batch([[1, 1], [3, 1], [2, 3]], AA_EN | IM_RD | CVG_DST_WRAP, 1),
                gdp_set_prim_color(0, 0, 0xff0000ff),
            );
            let mut commands = vec![
                (0xef00_0030 | G_TC_FILT, AA_EN | IM_RD | CVG_DST_SAVE),
                gdp_set_prim_color(0, 0, 0x00ff00ff),
                gsp_1triangle(0, 1, 2),
                (0xef00_0030 | G_TC_FILT, AA_EN | IM_RD | CVG_DST_WRAP),
                gdp_set_prim_color(0, 0, 0x0000ffff),
                gsp_1triangle(0, 1, 2),
            ];
            fill(&mut commands, 0x12341234, [1, 0, 1, 0]);
            commands.extend([
                (0xef00_0030 | G_TC_FILT, AA_EN | IM_RD | CVG_DST_FULL),
                gdp_set_prim_color(0, 0, 0xffffffff),
                gdp_set_scissor(0, 4, 0, 512 * 4, 64 * 4),
                gsp_1triangle(0, 1, 2),
            ]);
            append(task, commands)
        };
        let mut r = renderer(true);
        r.inner.coherence.as_mut().unwrap().native_batch_limits = limits;
        let mut cpu = renderer(true);
        let mut memory = Memory(vec![0; OUTPUT as usize + 512 * 64 * 2]);
        let mut other = Memory(memory.0.clone());
        accepted(&mut r, &mut memory, task());
        run_compatibility(&mut cpu, &mut other, task());
        assert_eq!(&memory.0[FB as usize..], &other.0[FB as usize..]);
        let (_, gpu) = r
            .inner
            .coherence
            .as_ref()
            .unwrap()
            .native_fill_pixels(&r.device, &r.queue)
            .unwrap();
        assert_eq!(gpu[0], [0x003f, 4, 20]);
        assert_eq!(gpu[1], [0x1234, 0, 21]);
    }
}
