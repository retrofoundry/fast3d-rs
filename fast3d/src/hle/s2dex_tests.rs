use crate::inspect::{WalkObserver, WalkStep, WalkTermination};
use crate::{DataFormat, Microcode, RdramImage};
use std::ops::ControlFlow;

fn image(commands: &[(u32, u32)]) -> Vec<u8> {
    commands
        .iter()
        .flat_map(|&(a, b)| [a.to_be_bytes(), b.to_be_bytes()].concat())
        .collect()
}

#[derive(Default)]
struct Steps(Vec<(Microcode, Microcode, u32, bool, bool)>);
impl WalkObserver for Steps {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self.0.push((
            step.microcode,
            step.state.microcode,
            step.state.obj_render_mode,
            step.state.mvp_valid,
            step.state.fog_factors_valid,
        ));
        ControlFlow::Continue(())
    }
}

#[test]
fn non_switches_preserve_draw_clipping_and_inspect_identity() {
    use crate::tests::dl_builder::DlBuilder;
    use n64_gbi::encode::*;

    let mut b = DlBuilder::new();
    let matrix = b.matrix(super::math::identity());
    let vertices =
        b.vertices(
            &[[-1, -1, -2], [1, -1, -2], [0, 1, -2]].map(|[x, y, z]| VtxColored {
                x,
                y,
                z,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            }),
        );
    let mut commands = vec![gdp_set_combine_lerp(
        CcPass {
            a: 8,
            b: 8,
            c: 16,
            d: 4,
        },
        CcPass {
            a: 7,
            b: 7,
            c: 7,
            d: 4,
        },
        CcPass {
            a: 8,
            b: 8,
            c: 16,
            d: 4,
        },
        CcPass {
            a: 7,
            b: 7,
            c: 7,
            d: 4,
        },
    )];
    commands.push(gdp_set_render_mode(
        n64_gbi::consts::G_RM_OPA_SURF,
        n64_gbi::consts::G_RM_OPA_SURF2,
    ));
    for token in [None, Some(0x2000), Some(0x3000), Some(0x2000)] {
        if let Some(token) = token {
            commands.extend([
                (0xe1000000, 0x8000),
                (0xdd0007ff, 0x1000),
                (0xdd0007ff, token),
            ]);
        }
        commands.extend([
            gsp_matrix(matrix, false, true, false),
            gsp_vertex(0, 3, vertices),
            gsp_1triangle(0, 1, 2),
        ]);
    }
    commands.push(gsp_enddl());
    b.list("main", &commands);
    let built = b.finish("main");
    let mut seen = Vec::new();
    let result = super::interp::interpret_with_bindings(
        RdramImage::new(&built.rdram),
        built.entry.into(),
        Microcode::F3dex2NoN.into(),
        DataFormat::Fixed,
        Default::default(),
        Default::default(),
        Some(&mut |step: WalkStep<'_>| {
            if step.words[0].w0 >> 24 == 0x05 {
                seen.push((step.microcode, step.state.microcode));
            }
            ControlFlow::Continue(())
        }),
        Default::default(),
        &[
            (0x1000, Microcode::S2dex2),
            (0x2000, Microcode::F3dex2NoN),
            (0x3000, Microcode::F3dex2),
        ],
    );
    assert_eq!(result.termination, WalkTermination::End);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(
        seen,
        [
            Microcode::F3dex2NoN,
            Microcode::F3dex2NoN,
            Microcode::F3dex2,
            Microcode::F3dex2NoN
        ]
        .map(|m| (m, m))
    );
    assert_eq!(
        result
            .scene
            .draw_runs
            .iter()
            .map(|run| (run.unclipped_depth, run.index_count))
            .collect::<Vec<_>>(),
        vec![(true, 6), (false, 3), (true, 3)]
    );
}

#[test]
fn switch_roundtrip_preserves_shared_state_and_return_stack() {
    let bytes = image(&[
        (0xde000000, 0x01000000),
        (0xfa000000, 0x12345678),
        (0xdf000000, 0),
        (0xe1000000, 0x8000),
        (0xdd0007ff, 0x1000),
        (0xe1000000, 0x9000),
        (0xdd0007ff, 0x2000),
        (0xdf000000, 0),
    ]);
    let mut rdp = super::rdp::Rdp {
        tex_image: (2, 1, 159, 0x3000),
        other_mode_h: 0x1234,
        other_mode_l: 0x5678,
        fog_color: [1, 2, 3, 4],
        ..Default::default()
    };
    rdp.tiles[7].uls = 12;
    let mut steps = Steps::default();
    let result = super::interp::interpret_with_bindings(
        RdramImage {
            bytes: &bytes,
            segments: {
                let mut segments = [0; 16];
                segments[1] = 24;
                segments
            },
        },
        0,
        Microcode::F3dex2.into(),
        DataFormat::Fixed,
        rdp.clone(),
        Default::default(),
        Some(&mut steps),
        Default::default(),
        &[(0x1000, Microcode::S2dex2), (0x2000, Microcode::F3dex2)],
    );
    assert_eq!(result.termination, WalkTermination::End);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.commands, 8);
    rdp.prim = [0x12, 0x34, 0x56, 0x78];
    assert_eq!(result.rdp, rdp);
    assert_eq!(result.rdp.prim, [0x12, 0x34, 0x56, 0x78]);
    assert_eq!(
        (steps.0[2].0, steps.0[2].1),
        (Microcode::F3dex2, Microcode::S2dex2)
    );
    assert_eq!(
        (steps.0[4].0, steps.0[4].1),
        (Microcode::S2dex2, Microcode::F3dex2)
    );
}

#[test]
fn switch_invalidates_rsp_derived_state() {
    let mut rsp = super::rsp::Rsp::default();
    let mut scene = crate::scene::Scene::default();
    let rdp = super::rdp::Rdp::default();
    let mut matrix = [0; 64];
    matrix[..2].copy_from_slice(&3i16.to_be_bytes());
    rsp.matrix(&RdramImage::new(&matrix), 0, 3).unwrap();
    let viewport = [
        0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0, 0, 0x04, 0x00, 0x05, 0x00, 0x06, 0x00, 0, 0,
    ];
    rsp.set_viewport(&RdramImage::new(&viewport), 0).unwrap();
    rsp.set_vertex(&RdramImage::new(&[0; 16]), 0, 1, 0, &rdp, &mut scene)
        .unwrap();
    assert!(rsp.cull_display_list(0, 0).is_ok());
    rsp.switch_microcode(super::gbi::GbiUcode::S2dex2);
    assert!(rsp.cull_display_list(0, 0).is_err());
    assert!(rsp.branch_z(0, 0).is_err());
    let state = rsp.inspect_state(&rdp, &[]);
    assert_eq!(state.modelview_depth, 2);
    assert_eq!(state.modelview[0][0], 3.0);
    assert_eq!(state.projection, &super::math::identity());
    assert_eq!(state.viewport_scale, [64., 128., 0.75]);
    assert_eq!(state.viewport_translation, [256., 320., 1.5]);
    assert!(!state.mvp_valid);
    let mut diags = Vec::new();
    rsp.prepare_vertex(&mut diags, 0);
    assert!(matches!(diags[0].kind, crate::DiagKind::UndefinedMvp));
    assert!(!rsp.inspect_state(&rdp, &[]).mvp_valid);
    rsp.matrix(&RdramImage::new(&[0; 64]), 0, 2).unwrap();
    assert!(rsp.inspect_state(&rdp, &[]).mvp_valid);
    rsp.finish(&mut scene);
    assert_eq!(scene.raw_pos.len(), 1);
    assert_eq!(scene.mvp_table.len(), 3);
}

#[test]
fn s2dex_ignores_unsupported_command_from_before_switch() {
    let bytes = image(&[
        (0xd6000000, 0),
        (0xe1000000, 0x8000),
        (0xdd0007ff, 0x1000),
        (0xe3000a01, 0x200000),
        (0xdf000000, 0),
    ]);
    let result = super::interp::interpret_with_bindings(
        RdramImage::new(&bytes),
        0,
        Microcode::F3dex2.into(),
        DataFormat::Fixed,
        Default::default(),
        Default::default(),
        None,
        Default::default(),
        &[(0x1000, Microcode::S2dex2)],
    );
    assert_eq!(result.termination, WalkTermination::End);
    assert_eq!(result.rdp.other_mode_h, 0x200000);
}

#[test]
fn s2dex_fog_use_without_restore_is_reported() {
    let mut bytes = image(&[
        (0xdb080000, 0x0500fc00),
        (0xf8000000, 0x12345678),
        (0xd9000000, 0x10000),
        (0xe1000000, 0x8000),
        (0xdd0007ff, 0x1000),
        (0xdd0007ff, 0x2000),
        (0x01001002, 128),
        (0xdb080000, 0x0500fc00),
        (0x01001002, 128),
        (0xdf000000, 0),
    ]);
    bytes.resize(144, 0);
    let result = super::interp::interpret_with_bindings(
        RdramImage::new(&bytes),
        0,
        Microcode::F3dex2.into(),
        DataFormat::Fixed,
        Default::default(),
        Default::default(),
        None,
        Default::default(),
        &[(0x1000, Microcode::S2dex2), (0x2000, Microcode::F3dex2)],
    );
    assert_eq!(result.termination, WalkTermination::End);
    assert_eq!(
        result
            .diags
            .iter()
            .filter(|d| matches!(d.kind, crate::DiagKind::UndefinedFogFactors))
            .count(),
        1
    );
    assert_eq!(result.rdp.fog_color, [0x12, 0x34, 0x56, 0x78]);
    assert_eq!(result.scene.fog_table, vec![[1280, -1024]]);
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn load_ucode_full_width_aliases_without_blob_reads() {
    use super::host_mem::{HostMemory, HostRam};
    let token = 0x123456789abcdef0u64;
    for alias in [token, token.wrapping_sub(0x80000000)] {
        let words = [
            0xe1000000u64,
            0xdeadbeefdeadbeef,
            0xdd0007ff,
            alias,
            0xdf000000,
            0,
        ];
        let ram = unsafe { HostMemory::new(HostRam::new(&[])) };
        let mut observer = Steps::default();
        let result = crate::inspect::walk_with_bindings(
            ram,
            words.as_ptr() as u64,
            Microcode::F3dex2,
            DataFormat::Float,
            &mut observer,
            &[
                (token, Microcode::S2dex2),
                (token.wrapping_sub(0x80000000), Microcode::S2dex2),
            ],
        );
        assert_eq!(result.termination, WalkTermination::End);
        assert!(result.diagnostics.is_empty());
        assert_eq!(observer.0[2].0, Microcode::S2dex2);
    }
}

#[test]
fn s2dex_reload_resets_object_mode() {
    let mut rsp = super::rsp::Rsp::default();
    rsp.switch_microcode(super::gbi::GbiUcode::S2dex2);
    rsp.obj_render_mode = 8;
    rsp.switch_microcode(super::gbi::GbiUcode::S2dex2);
    assert_eq!(rsp.obj_render_mode, 0);
}

#[test]
fn profiled_walk_uses_per_walk_bindings() {
    let bytes = image(&[(0xe1000000, 0x8000), (0xdd0007ff, 0x1000), (0xdf000000, 0)]);
    let mut cpu = crate::profiling::CpuInterpreter::default();
    let (summary, diags) = cpu.process_with_bindings(
        RdramImage::new(&bytes),
        0,
        Microcode::F3dex2,
        DataFormat::Fixed,
        &[(0x1000, Microcode::S2dex2)],
    );
    assert_eq!(summary.termination, WalkTermination::End);
    assert!(diags.is_empty());
    let (summary, _) = cpu.process(
        RdramImage::new(&bytes),
        0,
        Microcode::F3dex2,
        DataFormat::Fixed,
    );
    assert_eq!(summary.termination, WalkTermination::Rejected);
}

#[derive(Default)]
struct Dispatches(Vec<(usize, usize, u32)>);
impl WalkObserver for Dispatches {
    fn command(&mut self, step: WalkStep<'_>) -> ControlFlow<()> {
        self.0.push((
            step.words.len(),
            step.emissions.len(),
            step.state.obj_render_mode,
        ));
        ControlFlow::Continue(())
    }
}

#[test]
fn s2dex_select_dl_rejects_without_consuming_sentinel() {
    let bytes = image(&[(0xe4000000, 0), (0x04000000, 0), (0xfa000000, 0x12345678)]);
    let mut observer = Dispatches::default();
    let result = super::interp::interpret(
        RdramImage::new(&bytes),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        Some(&mut observer),
    );
    assert_eq!(result.termination, WalkTermination::Rejected);
    assert_eq!(result.commands, 1);
    assert_eq!(observer.0, vec![(2, 0, 0)]);
    assert!(matches!(
        result.diags[0].kind,
        crate::DiagKind::UnsupportedCommand { opcode: 4, .. }
    ));
    assert_eq!(result.rdp.prim, [0; 4]);
    assert!(result.scene.framebuffer_pairs.is_empty());
}

#[test]
fn s2dex_stateful_unsupported_command_rejects_task() {
    let (device, queue, _) = crate::render::headless_device();
    let mut renderer = crate::Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 64,
            height: 64,
        },
        crate::RendererConfig {
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            clear_policy: crate::ClearPolicy::PerFrame,
            power_preference: wgpu::PowerPreference::LowPower,
        },
    );
    for opcode in [0x05, 0xdc, 0x01, 0x02, 0xda, 0x06, 0x07, 0x08, 0xdb] {
        let words = image(&[
            (0xff100003, 0x10000),
            (0xef300000, 0),
            (0xf7000000, 0xffffffff),
            (0xf600c00c, 0),
            (
                (opcode << 24) | if opcode == 0xdb { 0x080000 } else { 0 },
                0,
            ),
            (0xdf000000, 0),
        ]);
        let before = renderer.rdp.clone();
        let mut diags = Vec::new();
        let result = renderer.process_dl_memory_with_bindings(
            RdramImage::new(&words),
            0,
            Microcode::S2dex2,
            &mut diags,
            &[],
        );
        assert_eq!(
            result.termination,
            WalkTermination::Rejected,
            "opcode {opcode:x}"
        );
        assert!(!result.renderable);
        assert!(renderer
            .frame_scenes
            .iter()
            .all(|scene| *scene == crate::scene::Scene::default()));
        assert_eq!(renderer.rdp, before);
    }
}

#[test]
fn s2dex_shared_handlers_and_reload() {
    let bytes = image(&[
        (0x0b000000, 12),
        (0xdb060004, 0x8000),
        (0xe3000a01, 0x200000),
        (0xe0000000, 0),
        (0xe1000000, 0x1234),
        (0xdd0007ff, 0x1000),
        (0xdf000000, 0),
    ]);
    let mut observer = Dispatches::default();
    let result = super::interp::interpret_with_bindings(
        RdramImage::new(&bytes),
        0,
        Microcode::S2dex2.into(),
        DataFormat::Fixed,
        Default::default(),
        Default::default(),
        Some(&mut observer),
        Default::default(),
        &[(0x1000, Microcode::S2dex2)],
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.termination, WalkTermination::End);
    assert_eq!(observer.0[0].2, 12);
    assert_eq!(observer.0[5].2, 0);
    assert_eq!(result.rdp.other_mode_h, 0x200000);
}
