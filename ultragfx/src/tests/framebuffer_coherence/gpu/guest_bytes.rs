use super::*;

#[test]
fn guest_bytes_last_writer_and_framebuffer_texture_agree() {
    for fallback in [false, true] {
        for known in [false, true] {
            let coverage = if known {
                CVG_DST_FULL
            } else {
                CVG_DST_SAVE | IM_RD
            };
            let mut r = renderer(fallback);
            r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut dl = Vec::new();
            image(&mut dl, FB, 4, 4, 0, 2);
            dl.extend([
                (0xef000030 | G_TC_FILT, coverage),
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0x13558dff),
                gdp_fill_rectangle(0, 0, 16, 16),
                (0xef0000f0 | G_TC_FILT, coverage),
                gdp_fill_rectangle(4, 0, 8, 16),
                (0xef000070 | G_TC_FILT, coverage),
            ]);
            image(&mut dl, OUTPUT, 8, 4, 4, 1);
            dl.extend([
                combine(1, 1),
                gdp_set_texture_image(4, 1, 8, FB),
                gdp_set_tile(4, 1, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
                gdp_load_sync(),
                (0xf4000000, (7 << 24) | (28 << 12) | 12),
                gdp_pipe_sync(),
                gdp_set_tile(4, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
                gdp_set_tile_size(0, 0, 0, 28, 12),
            ]);
            dl.extend(gsp_texture_rectangle(
                0, 0, 32, 16, 0, 0, 0, 1024, 1024, false,
            ));
            run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
            let expected = *include_bytes!("../../guest_bytes/last-writer.rgba16");
            for pixel in 0..16 {
                assert_eq!(
                    r.inner
                        .coherence
                        .as_ref()
                        .unwrap()
                        .stored_coverage_code(FB as u64 + pixel * 2),
                    known.then_some(7)
                );
            }
            assert_eq!(&memory.0[FB as usize..FB as usize + 32], &expected);
            assert_eq!(&memory.0[OUTPUT as usize..OUTPUT as usize + 32], &expected);
        }
    }
}

#[test]
fn guest_bytes_fill_and_copy_bypass_dither_and_copy_coverage_stays_unknown() {
    let mut r = renderer(true);
    r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
    let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
    for selector in 0..4 {
        r.reset();
        let mut b = DlBuilder::new();
        let texture = b.bytes(8, &[0x21, 0x09, 0x21, 0x08].repeat(2));
        let mut dl = Vec::new();
        image(&mut dl, FB, 4, 1, 0, 2);
        dl.extend([
            (0xef300000 | (selector << 6), 0),
            gdp_set_fill_color(0x12a31ae5),
            gdp_fill_rectangle(0, 0, 12, 0),
        ]);
        image(&mut dl, OUTPUT, 4, 1, 0, 2);
        dl.push((0xef200000 | (selector << 6), 0));
        dl.extend(gdp_load_texture_block(0, 2, 4, 1, texture, 2, 0, 2, 0));
        dl.extend(gsp_texture_rectangle(
            0, 0, 12, 0, 0, 0, 0, 4096, 1024, false,
        ));
        run_compatibility(&mut r, &mut memory, built(b, dl));
        assert_eq!(
            &memory.0[FB as usize..FB as usize + 8],
            &[0x12, 0xa3, 0x1a, 0xe5, 0x12, 0xa3, 0x1a, 0xe5]
        );
        assert_eq!(
            &memory.0[OUTPUT as usize..OUTPUT as usize + 8],
            &[0x21, 0x09, 0x21, 0x08, 0x21, 0x09, 0x21, 0x08]
        );
        for pixel in 0..4 {
            assert_eq!(
                r.inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .stored_coverage_code(OUTPUT as u64 + pixel * 2),
                None
            );
        }
    }
}

#[test]
fn guest_bytes_native_snapshot_survives_source_overwrite() {
    let mut r = renderer(true);
    r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
    let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
    let mut dl = Vec::new();
    image(&mut dl, FB, 4, 1, 0, 2);
    fill(&mut dl, 0x12a312a3, [0, 0, 3, 0]);
    run(&mut r, &mut memory, built(DlBuilder::new(), dl));
    let mut dl = Vec::new();
    image(&mut dl, OUTPUT, 8, 1, 4, 1);
    dl.extend([
        (0xef000030 | G_TC_FILT, CVG_DST_FULL),
        combine(1, 1),
        gdp_set_texture_image(4, 1, 8, FB),
        gdp_set_tile(4, 1, 1, 0, 7, 0, 2, 0, 0, 2, 0, 0),
        gdp_load_sync(),
        (0xf4000000, (7 << 24) | (28 << 12)),
        gdp_pipe_sync(),
        gdp_set_tile(4, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 28, 0),
    ]);
    image(&mut dl, FB, 4, 1, 0, 2);
    fill(&mut dl, 0xf801f801, [0, 0, 3, 0]);
    image(&mut dl, OUTPUT, 8, 1, 4, 1);
    dl.extend([(0xef000030 | G_TC_FILT, CVG_DST_FULL), combine(1, 1)]);
    dl.extend(gsp_texture_rectangle(
        0, 0, 32, 4, 0, 0, 0, 1024, 1024, false,
    ));
    run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
    assert_eq!(
        &memory.0[OUTPUT as usize..OUTPUT as usize + 8],
        &[0x12, 0xa3, 0x12, 0xa3, 0x12, 0xa3, 0x12, 0xa3]
    );
}

#[test]
fn guest_bytes_copy_presents_modern_color_at_both_scales() {
    for scale in [1, 2] {
        for writeback in [false, true] {
            let mut r = renderer(true);
            r.reconfigure(crate::RendererConfig {
                resolution_multiplier: scale,
                ..r.config
            });
            r.set_guest_writeback_with_encoding(writeback, crate::FramebufferEncoding::BigEndian);
            let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
            let mut dl = Vec::new();
            image(&mut dl, FB, 4, 1, 0, 2);
            dl.extend([
                (0xef000030 | G_TC_FILT, CVG_DST_SAVE | IM_RD),
                combine(3, 3),
                gdp_set_prim_color(0, 0, 0x13558dff),
                gdp_fill_rectangle(0, 0, 16, 4),
            ]);
            run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
            let mut dl = Vec::new();
            image(&mut dl, OUTPUT, 4, 1, 0, 2);
            dl.push((0xef2000c0, 0));
            dl.extend(gdp_load_texture_block(0, 2, 4, 1, FB, 2, 0, 2, 0));
            dl.extend(gsp_texture_rectangle(
                0, 0, 12, 0, 0, 0, 0, 4096, 1024, false,
            ));
            run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
            let expected = [19, 85, 141, 255].repeat((4 * scale * scale) as usize);
            let source = &r.frame_scenes.last().unwrap().materials[0].texture;
            let crate::hle::texture_request::TextureSource::Framebuffer(request) = source else {
                panic!("expected framebuffer texture");
            };
            assert!(request.image_route());
            assert_eq!(
                pixels(&mut r, OUTPUT, 4 * scale, scale),
                expected,
                "scale={scale}, writeback={writeback}"
            );
        }
    }
}

#[test]
fn guest_bytes_copy_preserves_framebuffer_words() {
    for scale in [1, 2] {
        for native in [false, true] {
            for materialized in [false, true] {
                let mut r = renderer(true);
                r.reconfigure(crate::RendererConfig {
                    resolution_multiplier: scale,
                    ..r.config
                });
                r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
                let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
                let mut dl = Vec::new();
                image(&mut dl, FB, 4, 1, 0, 2);
                if native {
                    fill(&mut dl, 0x12a31ae4, [0, 0, 3, 0]);
                    run(&mut r, &mut memory, built(DlBuilder::new(), dl));
                } else {
                    dl.extend([
                        (0xef000030 | G_TC_FILT, CVG_DST_SAVE | IM_RD),
                        combine(3, 3),
                        gdp_set_prim_color(0, 0, 0x13558dff),
                        gdp_fill_rectangle(0, 0, 16, 4),
                    ]);
                    run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
                }
                let expected: &[u8] = if native {
                    &[0x12, 0xa3, 0x1a, 0xe4, 0x12, 0xa3, 0x1a, 0xe4]
                } else {
                    &[0x1a, 0xe5, 0x12, 0xa3, 0x1a, 0xe5, 0x12, 0xa3]
                };
                assert_eq!(&memory.0[FB as usize..FB as usize + 8], expected);
                let mut dl = Vec::new();
                image(&mut dl, OUTPUT, 4, 1, 0, 2);
                dl.push((0xef2000c0, 0));
                if materialized {
                    dl.extend(gdp_load_texture_block(4, 1, 8, 1, FB, 2, 0, 2, 0));
                    dl.push(gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0));
                    dl.push(gdp_set_tile_size(0, 0, 0, 12, 0));
                } else {
                    dl.extend(gdp_load_texture_block(0, 2, 4, 1, FB, 2, 0, 2, 0));
                }
                dl.extend(gsp_texture_rectangle(
                    0, 0, 12, 0, 0, 0, 0, 4096, 1024, false,
                ));
                run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
                assert_eq!(
                    &memory.0[OUTPUT as usize..OUTPUT as usize + 8],
                    expected,
                    "native={native}, materialized={materialized}"
                );
                for pixel in 0..4 {
                    assert_eq!(
                        r.inner
                            .coherence
                            .as_ref()
                            .unwrap()
                            .stored_coverage_code(OUTPUT as u64 + pixel * 2),
                        None
                    );
                }
            }
        }
    }
}

#[test]
fn guest_bytes_copy_only_target_resolves_pending_source_words() {
    for scale in [1, 2] {
        let mut r = renderer(true);
        r.reconfigure(crate::RendererConfig {
            resolution_multiplier: scale,
            ..r.config
        });
        r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
        let mut dl = Vec::new();
        image(&mut dl, FB, 4, 1, 0, 2);
        dl.extend([
            (0xef000030 | G_TC_FILT, CVG_DST_SAVE | IM_RD),
            combine(3, 3),
            gdp_set_prim_color(0, 0, 0x13558dff),
            gdp_fill_rectangle(0, 0, 16, 4),
        ]);
        image(&mut dl, FB + 0x1000, 4, 1, 0, 2);
        dl.extend(gdp_load_texture_block(0, 2, 4, 1, FB, 2, 0, 2, 0));
        image(&mut dl, OUTPUT, 4, 1, 0, 2);
        dl.push((0xef2000c0, 0));
        dl.extend(gsp_texture_rectangle(
            0, 0, 12, 0, 0, 0, 0, 4096, 1024, false,
        ));
        let task = built(DlBuilder::new(), dl);
        let mut scene = crate::hle::interpret_rdram(&task.rdram, task.entry).scene;
        let pair = scene
            .framebuffer_pairs
            .iter()
            .position(|pair| {
                pair.ops
                    .iter()
                    .any(|op| matches!(op, crate::scene::SceneOp::FramebufferCopy(_)))
            })
            .unwrap();
        let index = scene.framebuffer_pairs[pair]
            .ops
            .iter()
            .position(|op| matches!(op, crate::scene::SceneOp::FramebufferCopy(_)))
            .unwrap();
        let copy = scene.framebuffer_pairs[pair].ops.remove(index);
        let mut load = scene.framebuffer_pairs[pair].clone();
        load.color_image.addr = u64::from(FB + 0x1000);
        load.ops = vec![copy];
        scene.framebuffer_pairs.insert(pair + 1, load);
        r.inner.coherence.as_mut().unwrap().begin_task();
        r.inner
            .render_into_store(&r.device, &r.queue, &scene, r.config.clear_policy);
        for patch in r.inner.coherence.as_mut().unwrap().finish(&r.device) {
            memory.0[patch.address as usize..patch.address as usize + patch.bytes.len()]
                .copy_from_slice(&patch.bytes);
        }
        let expected = [0x1a, 0xe5, 0x12, 0xa3, 0x1a, 0xe5, 0x12, 0xa3];
        assert_eq!(&memory.0[FB as usize..FB as usize + 8], &expected);
        assert_eq!(&memory.0[OUTPUT as usize..OUTPUT as usize + 8], &expected);
    }
}

#[test]
fn guest_bytes_copy_to_eight_bit_targets_uses_guest_samples() {
    for fmt in 2..=4 {
        let mut r = renderer(true);
        r.set_guest_writeback_with_encoding(true, crate::FramebufferEncoding::BigEndian);
        let mut memory = Memory(vec![0; OUTPUT as usize + 320 * 240 * 2]);
        let mut dl = Vec::new();
        image(&mut dl, FB, 4, 1, 0, 2);
        dl.extend([
            (0xef000030 | G_TC_FILT, CVG_DST_SAVE | IM_RD),
            combine(3, 3),
            gdp_set_prim_color(0, 0, 0x13558dff),
            gdp_fill_rectangle(0, 0, 16, 4),
        ]);
        run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
        let mut dl = Vec::new();
        image(&mut dl, OUTPUT, 4, 1, fmt, 1);
        dl.push((0xef2000c0, 0));
        dl.extend(gdp_load_texture_block(0, 2, 4, 1, FB, 2, 0, 2, 0));
        dl.extend(gsp_texture_rectangle(
            0, 0, 12, 0, 0, 0, 0, 4096, 1024, false,
        ));
        run_compatibility(&mut r, &mut memory, built(DlBuilder::new(), dl));
        assert_eq!(
            &memory.0[OUTPUT as usize..OUTPUT as usize + 4],
            &[24, 82, 24, 82],
            "fmt={fmt}"
        );
        assert_eq!(
            pixels(&mut r, OUTPUT, 4, 1),
            [19, 85, 141, 255].repeat(4),
            "fmt={fmt}"
        );
    }
}
