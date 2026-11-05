use super::*;
use crate::hle::texture_request::TextureSource;

fn built(mut b: DlBuilder, mut dl: Vec<Command>) -> Built {
    dl.push(gsp_enddl());
    b.list("main", &dl);
    b.finish("main")
}

fn image(dl: &mut Vec<Command>, address: u32, width: u32, height: u32, siz: u32) {
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_color_image(0, siz, width, address),
        gdp_set_scissor(0, 0, 0, width * 4, height * 4),
    ]);
}

fn fill(dl: &mut Vec<Command>, word: u16, bounds: [u32; 4]) {
    dl.extend([
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_fill_color(u32::from(word) * 0x10001),
        gdp_fill_rectangle(bounds[0] * 4, bounds[1] * 4, bounds[2] * 4, bounds[3] * 4),
    ]);
}

fn load(
    dl: &mut Vec<Command>,
    fmt: u32,
    siz: u32,
    width: u32,
    address: u32,
    bounds: [u32; 4],
    line: u32,
) {
    dl.extend([
        gdp_set_texture_image(fmt, siz, width, address),
        gdp_set_tile(fmt, siz, line, 0, 7, 0, 2, 0, 0, 2, 0, 0),
        gdp_load_sync(),
        (
            0xf4000000 | ((bounds[0] * 4) << 12) | (bounds[1] * 4),
            (7 << 24) | ((bounds[2] * 4) << 12) | (bounds[3] * 4),
        ),
        gdp_set_tile(fmt, siz, line, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(
            0,
            0,
            0,
            (bounds[2] - bounds[0]) * 4,
            (bounds[3] - bounds[1]) * 4,
        ),
    ]);
}

fn draw(dl: &mut Vec<Command>, x: u32, y: u32, width: u32, height: u32) {
    dl.extend([
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_OPA_SURF | CVG_DST_FULL, G_RM_OPA_SURF2),
        gdp_set_other_mode_h(12, 2, 0),
        gdp_set_other_mode_h(4, 2, 3 << 4),
        gdp_set_other_mode_h(6, 2, 3 << 6),
    ]);
    dl.extend(gsp_texture_rectangle(
        x * 4,
        y * 4,
        (x + width) * 4,
        (y + height) * 4,
        0,
        0,
        0,
        1024,
        1024,
        false,
    ));
}

fn capture(built: &Built) -> Fixture {
    let mut fixture = fixture(&scene(Case::ExactRect), Case::ExactRect);
    fixture.tasks[0].entry = u64::from(built.entry);
    fixture.tasks[0].spans = vec![MemorySpan {
        address: 0,
        bytes: built.rdram.clone(),
    }];
    fixture
}

fn pixels(built: &Built) -> Vec<u8> {
    let output = pollster::block_on(capture(built).replay_headless()).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    output.rgba8
}

fn reinterpret(fmt: u32) -> Built {
    let mut b = DlBuilder::new();
    let mut dl = Vec::new();
    image(&mut dl, SOURCE, 4, 1, 2);
    for (x, word) in [0x2219, 0x8531, 0xdde7, 0x0997].into_iter().enumerate() {
        fill(&mut dl, word, [x as u32, 0, x as u32, 0]);
    }
    image(&mut dl, OUTPUT, 320, 240, 3);
    fill(&mut dl, 0, [0, 0, 319, 239]);
    if fmt == 2 {
        let palette: Vec<_> = (0..=255u8).flat_map(|index| [index, 255 - index]).collect();
        let address = b.bytes(8, &palette);
        dl.extend([
            gdp_set_texture_image(0, 2, 1, address),
            gdp_set_tile(0, 0, 0, 256, 7, 0, 0, 0, 0, 0, 0, 0),
            (0xf0000000, (7 << 24) | (255 << 14)),
            gdp_set_other_mode_h(14, 2, 3 << 14),
            gdp_set_texture_image(2, 2, 1, SOURCE),
            gdp_set_tile(2, 2, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0),
            gdp_load_block(7, 0, 0, 3, 2048),
            gdp_set_tile(2, 1, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0),
            gdp_set_tile_size(0, 0, 0, 28, 0),
        ]);
    } else {
        load(&mut dl, fmt, 2, 4, SOURCE, [0, 0, 3, 0], 1);
    }
    dl.push(combine(1));
    draw(&mut dl, 0, 0, if fmt == 2 { 8 } else { 4 }, 1);
    dl.push(alpha_to_rgb());
    draw(&mut dl, 0, 1, if fmt == 2 { 8 } else { 4 }, 1);
    built(b, dl)
}

fn strips(copy: bool) -> Built {
    let mut dl = Vec::new();
    image(&mut dl, SOURCE, 320, 240, 2);
    fill(&mut dl, 0xf801, [0, 0, 159, 239]);
    fill(&mut dl, 0x07c1, [160, 0, 319, 239]);
    fill(&mut dl, 0x003f, [0, 234, 319, 239]);
    let stages = if copy {
        vec![(SOURCE, 0x400000, false), (0x400000, OUTPUT, true)]
    } else {
        vec![(SOURCE, OUTPUT, false)]
    };
    for (source, destination, copy) in stages {
        image(&mut dl, destination, 320, 240, 2);
        dl.extend([
            combine(1),
            gdp_set_other_mode_h(12, 2, 0),
            gdp_set_other_mode_h(4, 2, 3 << 4),
            gdp_set_other_mode_h(6, 2, 3 << 6),
        ]);
        for y in (0..240).step_by(6) {
            load(&mut dl, 0, 2, 320, source, [0, y, 319, y + 5], 80);
            dl.extend([
                gdp_set_cycle_type(if copy { 2 } else { 0 }),
                gdp_set_render_mode(
                    if copy {
                        0
                    } else {
                        G_RM_OPA_SURF | CVG_DST_FULL
                    },
                    if copy { 0 } else { G_RM_OPA_SURF2 },
                ),
            ]);
            dl.extend(gsp_texture_rectangle(
                0,
                y * 4,
                if copy { 319 * 4 } else { 320 * 4 },
                (y + if copy { 5 } else { 6 }) * 4,
                0,
                0,
                0,
                if copy { 4096 } else { 1024 },
                1024,
                false,
            ));
        }
    }
    built(DlBuilder::new(), dl)
}

#[test]
fn framebuffer_load_requests_keep_copy_identity_and_reinterpretation() {
    for built in [reinterpret(2), reinterpret(3), strips(false), strips(true)] {
        let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.dropped_runs, 0);
        assert!(result
            .scene
            .materials
            .iter()
            .any(|material| matches!(material.texture, TextureSource::Framebuffer(_))));
    }
    let built = reinterpret(4);
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.iter().any(|diag| matches!(
        diag.kind,
        crate::DiagKind::UnsupportedFramebufferAccess {
            reason: crate::FramebufferAccess::Reinterpretation,
            ..
        }
    )));
    assert_eq!(result.dropped_runs, 2);
}

#[test]
fn framebuffer_load_ia16_preserves_rt64_precision() {
    let pixels = pixels(&reinterpret(3));
    for (x, [intensity, alpha]) in [[33, 150], [132, 108], [222, 105], [8, 85]]
        .into_iter()
        .enumerate()
    {
        assert_eq!(&pixels[x * 4..x * 4 + 3], &[intensity; 3]);
        assert_eq!(&pixels[(320 + x) * 4..(320 + x) * 4 + 3], &[alpha; 3]);
    }
}

#[test]
fn framebuffer_load_ci8_uses_high_byte_first() {
    let pixels = pixels(&reinterpret(2));
    for (x, index) in [0x22, 0x19, 0x85, 0x31, 0xdd, 0xe7, 0x09, 0x97]
        .into_iter()
        .enumerate()
    {
        assert_eq!(&pixels[x * 4..x * 4 + 3], &[index; 3]);
        assert_eq!(&pixels[(320 + x) * 4..(320 + x) * 4 + 3], &[255 - index; 3]);
    }
}

#[test]
fn framebuffer_load_save_six_row_strips_and_copy_restore() {
    for copy in [false, true] {
        let pixels = pixels(&strips(copy));
        for y in 0..240 {
            for x in 0..320 {
                let expected = if y >= 234 {
                    [0, 0, 255]
                } else if x < 160 {
                    [255, 0, 0]
                } else {
                    [0, 255, 0]
                };
                assert_eq!(
                    &pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3],
                    &expected,
                    "copy={copy} ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn framebuffer_load_snapshot_survives_overwrite_and_reload() {
    for case in [Case::OffsetRect, Case::OffsetTriangle] {
        let mut original = scene(case);
        original.rdram.truncate(original.rdram.len() - 8);
        let mut dl = vec![combine(1)];
        load(&mut dl, 0, 2, 32, SOURCE, [0, 0, 3, 1], 2);
        draw(&mut dl, 232, 88, 4, 2);
        dl.push(gsp_enddl());
        original.rdram.extend(
            dl.into_iter()
                .flat_map(|(w0, w1)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes())),
        );
        let pixels = pixels(&original);
        for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (index as u32 % 320, index as u32 / 320);
            let expected = if (232..236).contains(&x) && (88..90).contains(&y) {
                REPLACEMENT
            } else {
                semantics::fast3d_expected(case, x, y)
            };
            assert_eq!(*pixel, expected, "{case:?} ({x}, {y})");
        }
    }
}

#[test]
fn framebuffer_load_in_later_task_uses_preserved_target() {
    let mut fixture = capture(&scene(Case::ExactRect));
    let mut dl = vec![];
    load(&mut dl, 0, 2, 32, SOURCE, [0, 0, 3, 1], 2);
    image(&mut dl, OUTPUT, 320, 240, 3);
    fill(&mut dl, 0, [0, 0, 319, 239]);
    dl.push(combine(1));
    draw(&mut dl, 0, 0, 4, 2);
    let later = built(DlBuilder::new(), dl);
    let mut task = capture(&later).tasks.remove(0);
    task.order = 1;
    fixture.tasks.push(task);
    let output = pollster::block_on(fixture.replay_headless()).unwrap();
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{:?}",
        output.diagnostics
    );
    for y in 0..2 {
        for x in 0..4 {
            assert_eq!(
                &output.rgba8[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3],
                &REPLACEMENT[..3]
            );
        }
    }
}

#[test]
fn framebuffer_load_capture_omits_generated_pixels() {
    use crate::{capture::RecordingHardware, Hardware, Rdram, RdramImage};
    struct Image(Vec<u8>);
    impl Hardware for Image {
        fn rdram(&self) -> impl Rdram + '_ {
            RdramImage::new(&self.0)
        }
    }
    for case in [Case::OffsetRect, Case::OffsetTriangle, Case::ExactRect] {
        let built = scene(case);
        let hardware = Image(built.rdram);
        let recorder = RecordingHardware::new(&hardware);
        let result = crate::hle::interpret(
            recorder.rdram(),
            built.entry.into(),
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let task = recorder
            .finish(built.entry.into(), Microcode::F3dex2, DataFormat::Fixed, 0)
            .unwrap();
        assert!(task
            .spans
            .iter()
            .all(|span| span.address + span.bytes.len() as u64 <= u64::from(OUTPUT)));
        let replay = crate::capture::ReplayHardware::new(&task, None).unwrap();
        let replayed = crate::hle::interpret(
            replay.rdram(),
            built.entry.into(),
            crate::hle::gbi::GbiUcode::F3dex2,
            DataFormat::Fixed,
            None,
        );
        replay.check().unwrap();
        assert_eq!(result.scene, replayed.scene);
    }
}

#[test]
fn framebuffer_load_interior_320_stride() {
    let mut dl = Vec::new();
    image(&mut dl, SOURCE, 320, 240, 2);
    fill(&mut dl, 0x003f, [0, 0, 319, 239]);
    fill(&mut dl, 0xf801, [8, 3, 11, 3]);
    fill(&mut dl, 0x07c1, [8, 4, 11, 4]);
    image(&mut dl, OUTPUT, 320, 240, 3);
    fill(&mut dl, 0, [0, 0, 319, 239]);
    load(&mut dl, 0, 2, 320, SOURCE + 2 * 640 + 8, [4, 1, 7, 2], 2);
    dl.push(combine(1));
    draw(&mut dl, 0, 0, 4, 2);
    let pixels = pixels(&built(DlBuilder::new(), dl));
    for y in 0..2 {
        for x in 0..4 {
            assert_eq!(
                &pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3],
                if y == 0 { &[255, 0, 0] } else { &[0, 255, 0] }
            );
        }
    }
}

fn palette_copy() -> Built {
    let mut b = DlBuilder::new();
    let indices = b.bytes(8, &[0, 1, 2, 3, 0, 1, 2, 3]);
    let mut dl = Vec::new();
    image(&mut dl, SOURCE, 8, 3, 2);
    for (x, word) in [0xf801, 0x07c1, 0x003f, 0xffff].into_iter().enumerate() {
        fill(&mut dl, word, [x as u32 + 2, 1, x as u32 + 2, 2]);
    }
    dl.extend([
        gdp_set_texture_image(0, 2, 8, SOURCE),
        gdp_set_tile(0, 2, 1, 256, 7, 0, 0, 0, 0, 0, 0, 0),
        (0xf0000000 | (8 << 12) | 4, (7 << 24) | (20 << 12) | 8),
    ]);
    fill(&mut dl, 0x0001, [0, 0, 7, 2]);
    image(&mut dl, OUTPUT, 320, 240, 3);
    fill(&mut dl, 0, [0, 0, 319, 239]);
    load(&mut dl, 2, 1, 8, indices, [0, 0, 7, 0], 1);
    dl.extend([gdp_set_other_mode_h(14, 2, 2 << 14), combine(1)]);
    draw(&mut dl, 0, 0, 8, 1);
    built(b, dl)
}

#[test]
fn framebuffer_load_tlut_coordinates_and_replication() {
    let built = palette_copy();
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let copy = result
        .scene
        .framebuffer_pairs
        .iter()
        .flat_map(|pair| &pair.ops)
        .find_map(|op| {
            if let crate::scene::SceneOp::FramebufferCopy(copy) = op {
                Some(copy)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        copy.spans
            .iter()
            .map(|span| (span.origin, span.width))
            .collect::<Vec<_>>(),
        [([2, 1], 4), ([2, 2], 4)]
    );
    let tags = result.rdp.tmem_bank.framebuffer_tags.as_ref().unwrap();
    for lane in 0..4 {
        assert_eq!(tags.bytes[2048 + lane * 2].pointer, 0);
        assert_eq!(tags.bytes[2049 + lane * 2].pointer, 1);
        assert_eq!(tags.bytes[2080 + lane * 2].pointer, 128);
        assert_eq!(tags.bytes[2081 + lane * 2].pointer, 129);
    }
}

#[test]
fn framebuffer_load_tlut_snapshots_rendered_palette() {
    let pixels = pixels(&palette_copy());
    for (x, rgb) in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255; 3]]
        .into_iter()
        .cycle()
        .take(8)
        .enumerate()
    {
        assert_eq!(&pixels[x * 4..x * 4 + 3], &rgb);
    }
}

#[test]
fn framebuffer_load_tags_follow_padding_swaps_and_guest_overwrites() {
    use crate::hle::{
        framebuffer_load::FramebufferCopy,
        rdp::{Rdp, TileDescriptor},
        texture_request::prepare,
    };
    use crate::render::framebuffers::{targets::ImageDescriptor, ImageLayout};
    use std::sync::Arc;
    let source = ImageDescriptor {
        address: SOURCE.into(),
        layout: ImageLayout {
            width: 320,
            fmt: 0,
            siz: 2,
        },
        height: 240,
        generation: 1,
        depth: false,
    };
    let tile = TileDescriptor {
        fmt: 0,
        siz: 2,
        width: 4,
        height: 2,
        line: 2,
        tmem_addr: 32,
        cms: 2,
        cmt: 2,
        ..Default::default()
    };
    let mut rdp = Rdp {
        texture_loaded: true,
        ..Default::default()
    };
    let copy = Arc::new(FramebufferCopy::new(
        1,
        source,
        u64::from(SOURCE) + 3 * 640 + 16,
        8,
        640,
        2,
    ));
    assert_eq!(
        copy.spans
            .iter()
            .map(|s| (s.origin, s.width))
            .collect::<Vec<_>>(),
        [([8, 3], 4), ([8, 4], 4)]
    );
    rdp.tmem_bank.copy_load(copy.clone(), |bank| {
        bank.write_tile(&[], 32, 2, 2, 1, 640, 2)
    });
    let first = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    let TextureSource::Framebuffer(first_request) = &first.source else {
        panic!("expected GPU request")
    };
    assert_eq!(first_request.tags.bytes[0x114].pointer, 128);
    assert_eq!(first_request.tags.bytes[0x110].pointer, 132);
    assert_eq!(first_request.tags.bytes[0x108].copy, 0);
    rdp.tmem_bank
        .write_block(&[0xf8, 1].repeat(4), 32, 0, 0, 1, 2);
    let tags = rdp.tmem_bank.framebuffer_tags.as_ref().unwrap();
    assert!(tags.bytes[0x100..0x108].iter().all(|tag| tag.copy == 0));
    assert_ne!(tags.bytes[0x114].copy, 0);
    assert_ne!(first_request.tags.bytes[0x100].copy, 0);
    let second = Arc::new(FramebufferCopy::new(
        2,
        source,
        u64::from(SOURCE) + 3 * 640 + 16,
        8,
        640,
        2,
    ));
    rdp.tmem_bank
        .copy_load(second, |bank| bank.write_tile(&[], 32, 2, 2, 1, 640, 2));
    let reloaded = prepare(&rdp, &tile, 0, &Default::default()).unwrap();
    assert_ne!(first, reloaded);
    rdp.tmem_bank
        .copy_load(copy, |bank| bank.write_tlut(&[], 1, 256));
    let tile = TileDescriptor {
        fmt: 2,
        siz: 0,
        palette: 1,
        tmem_addr: 64,
        ..tile
    };
    assert!(matches!(
        prepare(&rdp, &tile, 2, &Default::default()).unwrap().source,
        TextureSource::Encoded(_)
    ));
    assert!(matches!(
        prepare(
            &rdp,
            &TileDescriptor { palette: 0, ..tile },
            2,
            &Default::default()
        ),
        Err(crate::DiagKind::UnsupportedFramebufferAccess {
            reason: crate::FramebufferAccess::Reinterpretation,
            ..
        })
    ));
}
