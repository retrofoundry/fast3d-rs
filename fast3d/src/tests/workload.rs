use crate::render::workload::{TargetId, Workload};
use crate::scene::{FramebufferPair, SceneOp, Scissor};
use crate::tests::dl_builder::DlBuilder;
use n64_gbi::{consts::*, encode::*};

#[test]
fn high_poly_submits_one_triangle_operation() {
    for paired in [false, true] {
        let mut scene = super::common::scene_from_fixture("high-poly");
        let origins = scene.draw_origins.clone();
        let run = scene.draw_runs[0];
        assert_eq!(scene.indices.len(), 181 * 3);
        assert_eq!(origins.len(), 91);
        if paired {
            scene.framebuffer_pairs.push(FramebufferPair {
                ops: scene.draw_runs.drain(..).map(SceneOp::Tris).collect(),
                active_scissor: origins[0].scissor,
                ..Default::default()
            });
        }
        let workload = Workload::new(&scene);
        assert_eq!(workload.targets.len(), 1);
        let operations = &workload.targets[0].operations;
        eprintln!(
            "high-poly paired={paired}: triangles=181 workload_ops={}",
            operations.len()
        );
        assert_eq!(
            operations.len(),
            1,
            "unchanged triangle state must stay batched"
        );
        assert_eq!(operations[0].draw, SceneOp::Tris(run));
        assert_eq!(operations[0].pc, Some(origins[0].pc));
        assert_eq!(scene.draw_origins, origins);
    }
}

#[test]
fn adjacent_runs_coalesce_only_with_identical_state_and_contiguous_indices() {
    let original = super::common::scene_from_fixture("high-poly");
    let first = crate::scene::DrawRun {
        index_count: 3,
        ..original.draw_runs[0]
    };
    let second = crate::scene::DrawRun {
        index_start: 3,
        ..first
    };
    let mut variants = vec![second; 7];
    variants[0].material_index += 1;
    variants[1].render_mode_index += 1;
    variants[2].fog_color[0] ^= 255;
    variants[3].prim_depth.z += 1;
    variants[4].prim_depth.dz += 1;
    variants[5].cull = if first.cull == crate::scene::CullKind::None {
        crate::scene::CullKind::Cull
    } else {
        crate::scene::CullKind::None
    };
    variants[6].index_start += 3;
    for next in std::iter::once(second).chain(variants) {
        let mut scene = original.clone();
        scene.draw_runs = vec![first, next];
        let workload = Workload::new(&scene);
        let operations = &workload.targets[0].operations;
        if next == second {
            assert_eq!(operations.len(), 1);
            assert_eq!(
                operations[0].draw,
                SceneOp::Tris(crate::scene::DrawRun {
                    index_count: 6,
                    ..first
                })
            );
        } else {
            assert_eq!(operations.len(), 2, "state/range change: {next:?}");
            assert_eq!(operations[0].draw, SceneOp::Tris(first));
            assert_eq!(operations[1].draw, SceneOp::Tris(next));
        }
    }
}

#[test]
fn batching_preserves_unknown_origin_boundaries() {
    let mut scene = super::common::scene_from_fixture("high-poly");
    let first = scene.draw_origins[0].clone();
    let third = scene.draw_origins[2].clone();
    scene.draw_origins = vec![first.clone(), third.clone()];
    scene.draw_runs[0].index_count = third.indices.end;
    let workload = Workload::new(&scene);
    let operations = &workload.targets[0].operations;
    assert_eq!(operations.len(), 3);
    assert_eq!(
        operations.iter().map(|op| op.pc).collect::<Vec<_>>(),
        [Some(first.pc), None, Some(third.pc)]
    );
    scene.draw_origins.clear();
    let workload = Workload::new(&scene);
    assert_eq!(workload.targets[0].operations.len(), 1);
    assert_eq!(workload.targets[0].operations[0].pc, None);
}

#[test]
fn rectangles_and_scissor_commands_split_identical_triangle_state() {
    let original = super::common::scene_from_fixture("high-poly");
    let run = crate::scene::DrawRun {
        index_count: 3,
        ..original.draw_runs[0]
    };
    let next = crate::scene::DrawRun {
        index_start: 3,
        ..run
    };
    let scissor = original.draw_origins[0].scissor;
    let changed_scissor = Scissor { ulx: 8, ..scissor };
    for boundary in [
        vec![SceneOp::FillRect {
            rect: Default::default(),
            color_raw: 0,
            convert: [0; 6],
            key: Default::default(),
        }],
        vec![SceneOp::TexRect {
            rect: Default::default(),
            tile: 0,
            uls: 0,
            ult: 0,
            dsdx: 0,
            dtdy: 0,
            flip: false,
            copy_mode: false,
            material_index: run.material_index,
            render_mode_index: run.render_mode_index,
            fog_color: run.fog_color,
            prim_depth: run.prim_depth,
            fb_source: None,
        }],
        vec![SceneOp::SetScissor(changed_scissor)],
        vec![
            SceneOp::SetScissor(changed_scissor),
            SceneOp::SetScissor(scissor),
        ],
    ] {
        let mut scene = original.clone();
        scene.draw_runs.clear();
        let mut ops = vec![SceneOp::Tris(run)];
        ops.extend(boundary.clone());
        ops.push(SceneOp::Tris(next));
        scene.framebuffer_pairs.push(FramebufferPair {
            ops,
            active_scissor: scissor,
            ..Default::default()
        });
        let workload = Workload::new(&scene);
        let operations = &workload.targets[0].operations;
        let is_scissor = matches!(boundary[0], SceneOp::SetScissor(_));
        assert_eq!(operations.len(), if is_scissor { 2 } else { 3 });
        assert_eq!(operations[0].draw, SceneOp::Tris(run));
        assert_eq!(operations.last().unwrap().draw, SceneOp::Tris(next));
        if !is_scissor {
            assert_eq!(operations[1].draw, boundary[0]);
        }
        assert_eq!(
            operations.last().unwrap().scissor,
            if boundary == [SceneOp::SetScissor(changed_scissor)] {
                changed_scissor
            } else {
                scissor
            }
        );
    }
}

#[test]
fn returning_to_a_target_keeps_separate_workloads() {
    let mut scene = super::common::scene_from_fixture("high-poly");
    let run = scene.draw_runs.remove(0);
    for (index, address) in [0x1000, 0x2000, 0x1000].into_iter().enumerate() {
        scene.framebuffer_pairs.push(FramebufferPair {
            color_image: crate::scene::ColorImage {
                addr: address,
                ..Default::default()
            },
            active_scissor: scene.draw_origins[0].scissor,
            ops: vec![SceneOp::Tris(crate::scene::DrawRun {
                index_start: index as u32 * 3,
                index_count: 3,
                ..run
            })],
            ..Default::default()
        });
    }
    let workload = Workload::new(&scene);
    assert_eq!(
        workload
            .targets
            .iter()
            .map(|target| target.id)
            .collect::<Vec<_>>(),
        [
            TargetId::Guest(0x1000),
            TargetId::Guest(0x2000),
            TargetId::Guest(0x1000)
        ]
    );
    assert!(workload
        .targets
        .iter()
        .all(|target| target.operations.len() == 1));
}

#[test]
fn batching_does_not_reattribute_interpreter_diagnostics() {
    let original = super::common::scene_from_fixture("high-poly");
    let first_pc = original.draw_origins[0].pc;
    let diagnostic_pc = original.draw_origins[1].pc;
    let (bytes, entry) = super::fixtures::fixture("high-poly");
    let mut bytes = bytes.to_vec();
    // Replace the second TRI2 with an invalid vertex load, which leaves draw state intact.
    let (w0, w1) = (0x0100_1000u32, 0u32);
    let command = &mut bytes[diagnostic_pc as usize..diagnostic_pc as usize + 8];
    command[..4].copy_from_slice(&w0.to_be_bytes());
    command[4..].copy_from_slice(&w1.to_be_bytes());
    let result = crate::hle::interpret_rdram(&bytes, entry as u32);
    let workload = Workload::new(&result.scene);
    assert_eq!(workload.targets[0].operations.len(), 1);
    assert_eq!(workload.targets[0].operations[0].pc, Some(first_pc));
    assert_eq!(
        result.diags,
        [crate::Diagnostic {
            at: diagnostic_pc,
            kind: crate::DiagKind::VtxOutOfRange { count: 1, end: 0 },
        }]
    );
}

#[test]
fn normalization_retains_legacy_scissors_zero_address_and_command_pcs() {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(
        &[VtxColored {
            x: 0,
            y: 0,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }; 3],
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
    let commands = [
        gdp_set_combine_lerp(color, alpha, color, alpha),
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gsp_vertex(0, 3, vertices),
        gsp_1triangle(0, 1, 2),
        gdp_set_scissor(0, 32, 64, 320, 384),
        gsp_1triangle(0, 1, 2),
        gdp_set_color_image(0, 2, 320, 0),
        gsp_1triangle(0, 1, 2),
        gsp_enddl(),
    ];
    let entry = b.list("main", &commands);
    let built = b.finish("main");
    let result = crate::hle::interpret_rdram(&built.rdram, entry);
    let workload = Workload::new(&result.scene);
    assert_eq!(workload.targets.len(), 2, "{:?}", result.diags);
    let legacy = &workload.targets[0];
    assert_eq!(legacy.id, TargetId::Legacy);
    assert_eq!(legacy.logical_extent, (320, 240));
    assert_eq!(legacy.operations.len(), 2);
    assert_eq!(
        legacy.operations[0].scissor,
        Scissor {
            lrx: 320,
            lry: 240,
            ..Scissor::default()
        }
    );
    assert_eq!(
        legacy.operations[1].scissor,
        Scissor {
            ulx: 8,
            uly: 16,
            lrx: 80,
            lry: 96,
            mode: 0
        }
    );
    assert_eq!(legacy.operations[0].pc, Some(u64::from(entry) + 32));
    assert_eq!(legacy.operations[1].pc, Some(u64::from(entry) + 48));
    assert_eq!(workload.targets[1].id, TargetId::Guest(0));
    assert_eq!(
        workload.targets[1].operations[0].pc,
        Some(u64::from(entry) + 64)
    );
    assert_eq!(result.scene.draw_runs.len(), 1);
}

#[test]
fn normalization_distinguishes_unset_and_empty_legacy_scissor() {
    let original = super::common::scene_from_fixture("flat-color");
    let workload = Workload::new(&original);
    assert!(workload.targets[0].operations.iter().all(|op| op.scissor
        == Scissor {
            lrx: 320,
            lry: 240,
            ..Default::default()
        }));
    let (bytes, entry) = super::fixtures::fixture("flat-color");
    let mut bytes = bytes.to_vec();
    let wrapper = bytes.len() as u32;
    for (w0, w1) in [
        gdp_set_scissor(0, 0, 0, 0, 0),
        (u32::from(G_DL) << 24, entry as u32),
        gsp_enddl(),
    ] {
        bytes.extend(w0.to_be_bytes());
        bytes.extend(w1.to_be_bytes());
    }
    let result = crate::hle::interpret_rdram(&bytes, wrapper);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let workload = Workload::new(&result.scene);
    assert!(!workload.targets[0].operations.is_empty());
    assert!(workload.targets[0]
        .operations
        .iter()
        .all(|op| op.scissor == Scissor::default()));
}

#[test]
fn normalization_keeps_zero_depth_image_address() {
    let mut b = DlBuilder::new();
    b.list(
        "main",
        &[
            gdp_set_depth_image(0),
            gdp_set_color_image(0, 2, 320, 0x1000),
            gdp_set_scissor(0, 0, 0, 1280, 960),
            gdp_set_cycle_type(3),
            gdp_fill_rectangle(0, 0, 4, 4),
            gdp_set_color_image(0, 2, 320, 0),
            gdp_fill_rectangle(0, 0, 4, 4),
            gsp_enddl(),
        ],
    );
    let built = b.finish("main");
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    let workload = Workload::new(&result.scene);
    assert_eq!(workload.targets[0].depth_image, Some(0));
    assert!(!workload.targets[0].depth_clear);
    assert_eq!(workload.targets[1].depth_image, Some(0));
    assert!(workload.targets[1].depth_clear);
}

#[test]
fn depth_alias_fill_requires_supported_layout_and_cycle() {
    for (fmt, size, cycle) in [(0, 2, 0), (0, 2, 1), (0, 2, 2), (0, 3, 3), (1, 2, 3)] {
        let mut b = DlBuilder::new();
        b.list(
            "main",
            &[
                gdp_set_depth_image(0x2000),
                gdp_set_color_image(fmt, size, 320, 0x2000),
                gdp_set_scissor(0, 0, 0, 1280, 960),
                gdp_set_cycle_type(cycle),
                gdp_fill_rectangle(0, 0, 4, 4),
                gsp_enddl(),
            ],
        );
        let built = b.finish("main");
        let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
        assert_eq!(result.diags.len(), 1);
        assert!(result.scene.framebuffer_pairs.is_empty());
        assert!(matches!(
            result.diags[0].kind,
            crate::DiagKind::UnsupportedDepthAlias { address: 0x2000 }
        ));
    }
}
