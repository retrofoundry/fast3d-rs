use crate::render::workload::{TargetId, Workload};
use crate::scene::Scissor;
use crate::tests::dl_builder::DlBuilder;
use n64_gbi::{consts::*, encode::*};

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
