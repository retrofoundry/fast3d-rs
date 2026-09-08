use crate::hle::{gbi::GbiUcode, interp::interpret, mem::GbiDataFormat};
use crate::{DiagKind, RdramImage};
use n64_gbi::encode::*;

fn commands(source: u32, target: u32, texture: u32) -> Vec<(u32, u32)> {
    let mut commands = vec![
        gdp_set_color_image(0, 2, 64, source),
        gdp_set_scissor(0, 0, 0, 256, 256),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(0xfb81fb81),
        gdp_fill_rectangle(0, 0, 252, 252),
        gdp_set_color_image(0, 2, 64, target),
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 64, texture),
        gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 252, 252),
    ];
    commands.extend(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
    ));
    commands.push(gsp_enddl());
    commands
}

fn run(commands: &[(u32, u32)]) -> crate::hle::interp::InterpResult {
    let bytes: Vec<_> = commands
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    interpret(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
        None,
    )
}

#[test]
fn fb_alias_interior_or_same_target_is_diagnostic() {
    for (target, texture) in [(0x20000, 0x10002), (0x10000, 0x10000)] {
        let result = run(&commands(0x10000, target, texture));
        assert!(
            result
                .diags
                .iter()
                .any(|d| d.at == 80 && d.kind.severity() == crate::Severity::Error),
            "{:?}",
            result.diags
        );
        assert_eq!(result.dropped_runs, 1);
        assert!(!result
            .scene
            .framebuffer_pairs
            .iter()
            .flat_map(|p| &p.ops)
            .any(|op| matches!(op, crate::scene::SceneOp::TexRect { .. })));
    }
}

#[test]
fn fb_texture_load_overlap_never_reads_stale_ram() {
    for load in [gdp_load_block(0, 0, 0, 15, 0), (0xf4000000, 12 << 12)] {
        let mut commands = commands(0x10000, 0x20000, 0x10000);
        commands.insert(10, load);
        let result = run(&commands);
        assert!(
            !result
                .diags
                .iter()
                .any(|d| matches!(d.kind, DiagKind::MemoryRead { .. })),
            "GPU-only address must never reach Rdram: {:?}",
            result.diags
        );
        assert!(result.diags.iter().any(|d| d.at == 80));
        assert_eq!(result.dropped_runs, 1);
        assert_eq!(result.termination, crate::inspect::WalkTermination::End);
    }
}

#[test]
fn fb_format_change_does_not_reuse_old_view() {
    let mut commands = commands(0x10000, 0x20000, 0x10000);
    commands[7] = gdp_set_texture_image(0, 3, 64, 0x10000);
    let result = run(&commands);
    assert!(result
        .diags
        .iter()
        .any(|d| d.at == 80 && d.kind.severity() == crate::Severity::Error));
    assert_eq!(result.dropped_runs, 1);
}

#[test]
fn exact_base_fb_alias_keeps_compatibility() {
    let result = run(&commands(0x10000, 0x20000, 0x10000));
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert!(result.scene.framebuffer_pairs[1]
        .ops
        .iter()
        .any(|op| matches!(
            op,
            crate::scene::SceneOp::TexRect {
                fb_source: Some(0x10000),
                ..
            }
        )));
}

#[test]
fn framebuffer_range_overflow() {
    use crate::render::framebuffers::{targets::*, ImageLayout};
    let mut targets = TargetDescriptors::default();
    let layout = ImageLayout {
        width: 64,
        fmt: 0,
        siz: 2,
    };
    assert_eq!(
        targets.record(u64::MAX - 63, layout, 1, false),
        Err(DiagKind::FramebufferRangeOverflow {
            address: u64::MAX - 63,
            length: 128
        })
    );
    assert!(targets.get(u64::MAX - 63, false).is_none());
    assert!(targets.record(u64::MAX - 128, layout, 1, false).is_ok());
    let mut scene = run(&commands(0x10000, 0x20000, 0x10000)).scene;
    scene.framebuffer_pairs[0].color_image.addr = u64::MAX - 1;
    let inputs = crate::render::inputs::RenderInputs::new(&scene, (64, 64), [0; 2]).unwrap();
    assert!(inputs
        .diagnostics
        .iter()
        .any(|d| d.at == 32 && matches!(d.kind, DiagKind::FramebufferRangeOverflow { .. })));
}

#[test]
fn overflowing_texture_load_never_reaches_guest_memory() {
    for command in [
        gdp_load_block(0, 0, 0, 3, 0),
        (0xf4000000, 0),
        gdp_load_tlut(0, 3),
    ] {
        let words = [command, gsp_enddl()];
        let bytes: Vec<_> = words
            .iter()
            .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
            .collect();
        let rdp = crate::hle::rdp::Rdp {
            tex_image: (0, 2, 3, u64::MAX - 3),
            ..Default::default()
        };
        let result = crate::hle::interp::interpret_with_state(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
            rdp,
            None,
        );
        assert_eq!(
            result.diags,
            [crate::Diagnostic {
                at: 0,
                kind: DiagKind::FramebufferRangeOverflow {
                    address: u64::MAX - 3,
                    length: 8
                },
            }]
        );
        assert_eq!(result.termination, crate::inspect::WalkTermination::End);
    }
}

#[test]
fn framebuffer_history_does_not_change_guest_register_equality() {
    let words = commands(0x10000, 0x20000, 0x10000);
    let first = run(&words);
    let snapshot = first.rdp.clone();
    let bytes: Vec<_> = words
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    let mut previous = first;
    for _ in 0..3 {
        previous = crate::hle::interp::interpret_with_framebuffers(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3dex2,
            GbiDataFormat::Fixed,
            previous.rdp,
            previous.framebuffers,
            None,
        );
        assert!(previous.diags.is_empty(), "{:?}", previous.diags);
        assert_eq!(previous.rdp, snapshot);
    }
    let loads = [
        gdp_set_texture_image(0, 2, 64, 0x10000),
        gdp_load_block(0, 0, 0, 3, 0),
        gsp_enddl(),
    ];
    let bytes: Vec<_> = loads
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    let retained = crate::hle::interp::interpret_with_framebuffers(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
        previous.rdp,
        previous.framebuffers,
        None,
    );
    assert!(matches!(
        retained.diags[0].kind,
        DiagKind::UnsupportedFramebufferAccess {
            reason: crate::FramebufferAccess::TextureLoad,
            ..
        }
    ));
    let fresh = run(&loads);
    assert!(matches!(fresh.diags[0].kind, DiagKind::MemoryRead { .. }));
}

#[test]
fn framebuffer_source_generation_and_extent_follow_writes() {
    use crate::render::framebuffers::{targets::*, ImageLayout};
    let mut targets = TargetDescriptors::default();
    let layout = ImageLayout {
        width: 64,
        fmt: 0,
        siz: 2,
    };
    let first = targets.record(0x10000, layout, 32, false).unwrap();
    let grown = targets.record(0x10000, layout, 64, false).unwrap();
    assert_eq!(first.generation, grown.generation);
    assert_eq!(grown.height, 64);
    targets.record(0x11000, layout, 16, false).unwrap();
    assert!(matches!(
        targets.source(0x10000, 0x20000, layout, 16),
        Err(DiagKind::UnsupportedFramebufferAccess {
            reason: crate::FramebufferAccess::Overlap,
            ..
        })
    ));
    let replacement = targets
        .record(0x10000, ImageLayout { siz: 3, ..layout }, 16, false)
        .unwrap();
    assert!(replacement.generation > first.generation);
    assert!(targets.source(0x10000, 0x20000, layout, 16).is_err());
    let mut scene = run(&commands(0x10000, 0x20000, 0x10000)).scene;
    if let crate::scene::SceneOp::TexRect { fb_source, .. } = &mut scene.framebuffer_pairs[1].ops[0]
    {
        *fb_source = Some(0x30000);
    }
    let inputs = crate::render::inputs::RenderInputs::new(&scene, (64, 64), [0; 2]).unwrap();
    assert_eq!(inputs.dropped_runs, 1);
    assert!(inputs.diagnostics.iter().any(|d| d.at == 80
        && matches!(
            d.kind,
            DiagKind::UnsupportedFramebufferAccess {
                reason: crate::FramebufferAccess::MissingSource,
                ..
            }
        )));
}

#[test]
fn rejected_framebuffer_load_covers_masked_tmem_taps() {
    use crate::hle::{rdp::TileDescriptor, tmem::Tmem};
    let mut tmem = Tmem::default();
    tmem.write_tile(&[0xff; 8], 0, 1, 1, 1, 8, 2);
    let diagnostic = crate::Diagnostic {
        at: 0x1234,
        kind: DiagKind::UnsupportedFramebufferAccess {
            address: 0x10000,
            reason: crate::FramebufferAccess::TextureLoad,
        },
    };
    tmem.reject_load(diagnostic, |bank| {
        bank.write_tile(&[0; 8], 1, 1, 1, 1, 8, 2)
    });
    let tile = TileDescriptor {
        width: 4,
        height: 1,
        line: 1,
        siz: 2,
        masks: 3,
        ..Default::default()
    };
    assert_eq!(tmem.rejection(&tile), Some(diagnostic));
    assert_eq!(tmem.rejection(&TileDescriptor { cms: 2, ..tile }), None);
}

#[test]
fn retained_tmem_ignores_unrelated_framebuffer_texture_image() {
    let mut rdp = crate::hle::rdp::Rdp::default();
    rdp.tmem_bank.write_tile(&[0xff; 8], 0, 1, 1, 1, 8, 2);
    rdp.texture_loaded = true;
    let mut dl = commands(0x10000, 0x20000, 0x20000);
    dl[8] = gdp_set_tile(0, 2, 1, 0, 0, 0, 2, 0, 0, 2, 0, 0);
    dl[9] = gdp_set_tile_size(0, 0, 0, 12, 0);
    let bytes: Vec<_> = dl
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    let result = crate::hle::interp::interpret_with_framebuffers(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
        rdp,
        Default::default(),
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert!(result.scene.framebuffer_pairs[1]
        .ops
        .iter()
        .any(|op| matches!(
            op,
            crate::scene::SceneOp::TexRect {
                fb_source: None,
                ..
            }
        )));
    assert!(result
        .scene
        .materials
        .last()
        .unwrap()
        .texture
        .iter()
        .all(|byte| *byte == 255));
}

#[test]
fn exact_base_fb_alias_keeps_compatibility_pixels() {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, false);
    let scene = crate::tests::common::scene_from_fixture("offscreen-then-sample--white1");
    let pixels =
        crate::tests::common::render_to_pixels(&device, &queue, &mut renderer, &scene, 64, 64);
    assert!(pixels == include_bytes!("../../goldens/2d-offscreen-then-sample.bin"));
    assert!(
        renderer.diagnostics.is_empty(),
        "{:?}",
        renderer.diagnostics
    );
}

#[test]
fn framebuffer_load_rejection_preserves_unrelated_draws_and_real_faults() {
    let mut dl = commands(0x10000, 0x20000, 0x10000);
    dl.insert(10, gdp_load_block(0, 0, 0, 15, 0));
    let result = run(&dl);
    assert!(result
        .scene
        .framebuffer_pairs
        .iter()
        .flat_map(|pair| &pair.ops)
        .any(|op| matches!(op, crate::scene::SceneOp::FillRect { .. })));
    dl[7] = gdp_set_texture_image(0, 2, 64, 0x30000);
    let failed = run(&dl);
    assert!(failed.scene.framebuffer_pairs.is_empty());
    assert!(failed
        .diags
        .iter()
        .any(|d| d.at == 80 && matches!(d.kind, DiagKind::MemoryRead { .. })));
}

#[test]
fn legacy_named_depth_is_a_known_gpu_texture_range() {
    use crate::hle::{
        rdp::Rdp,
        rsp::{PairRec, Rsp, Scene},
    };
    let mut rdp = Rdp {
        depth_image: Some(0x10000),
        ..Default::default()
    };
    let mut scene = Scene::default();
    let mut rec = PairRec::default();
    crate::hle::rsp::record_tri(
        &mut Rsp::default(),
        &mut rdp,
        &mut scene,
        &mut rec,
        0,
        0,
        0,
        0,
        0,
    );
    let dl = [
        gdp_set_texture_image(0, 2, 320, 0x10000),
        gdp_load_block(0, 0, 0, 15, 0),
        gsp_enddl(),
    ];
    let bytes: Vec<_> = dl
        .iter()
        .flat_map(|(a, b)| a.to_be_bytes().into_iter().chain(b.to_be_bytes()))
        .collect();
    let result = crate::hle::interp::interpret_with_framebuffers(
        RdramImage::new(&bytes),
        0,
        GbiUcode::F3dex2,
        GbiDataFormat::Fixed,
        rdp,
        rec.framebuffers,
        None,
    );
    assert!(result.diags.iter().any(|d| d.at == 8
        && matches!(
            d.kind,
            DiagKind::UnsupportedFramebufferAccess {
                reason: crate::FramebufferAccess::TextureLoad,
                ..
            }
        )));
    assert!(!result
        .diags
        .iter()
        .any(|d| matches!(d.kind, DiagKind::MemoryRead { .. })));
}
