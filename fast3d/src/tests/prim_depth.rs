use super::dl_builder::DlBuilder;
use crate::diag::{DiagKind, Diagnostic};
use crate::hle::interpret_rdram;
use crate::scene::{PrimitiveDepth, SceneOp};
use n64_gbi::encode::*;

#[derive(Clone, Copy, Debug)]
enum Draw {
    Triangle,
    Tri2,
    Quad,
    TexRect,
    TexRectFlip,
}

const DRAWS: [Draw; 5] = [
    Draw::Triangle,
    Draw::Tri2,
    Draw::Quad,
    Draw::TexRect,
    Draw::TexRectFlip,
];

impl Draw {
    fn commands(self) -> Vec<(u32, u32)> {
        match self {
            Self::Triangle => vec![gsp_1triangle(0, 1, 2)],
            Self::Tri2 => vec![gsp_2triangles(0, 1, 2, 0, 2, 3)],
            Self::Quad => vec![gsp_quad(0, 1, 2, 3)],
            Self::TexRect | Self::TexRectFlip => gsp_texture_rectangle(
                32 * 4,
                32 * 4,
                96 * 4,
                96 * 4,
                0,
                0,
                0,
                1024,
                1024,
                matches!(self, Self::TexRectFlip),
            )
            .to_vec(),
        }
    }
}

fn setup(cycle: u32, paired: bool) -> (DlBuilder, Vec<(u32, u32)>) {
    let mut dl = DlBuilder::new();
    let vertices = dl.vertices(
        &[[-1, -1], [1, -1], [1, 1], [-1, 1]].map(|[x, y]| VtxColored {
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
        }),
    );
    let color = CcPass {
        a: 15,
        b: 15,
        c: 31,
        d: 3,
    };
    let alpha = CcPass {
        a: 7,
        b: 7,
        c: 7,
        d: 3,
    };
    let mut commands = vec![
        gsp_vertex(0, 4, vertices),
        gdp_set_combine_lerp(color, alpha, color, alpha),
        gdp_set_prim_color(0, 0, 0xff00_00ff),
        gdp_set_cycle_type(cycle),
        gdp_set_render_mode(0x0f0a_4000, 0),
        gdp_set_other_mode_l(2, 1, 0),
    ];
    if paired {
        commands.extend([
            gdp_set_color_image(0, 2, 320, 0x10000),
            gdp_set_scissor(0, 0, 0, 320 * 4, 240 * 4),
        ]);
    }
    (dl, commands)
}

fn walk(mut dl: DlBuilder, mut commands: Vec<(u32, u32)>) -> crate::hle::InterpResult {
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpret_rdram(&built.rdram, built.entry)
}

#[test]
fn primdepth_snapshot_per_draw() {
    for cycle in [0, 1] {
        for paired in [false, true] {
            let (dl, mut commands) = setup(cycle, paired);
            for value in [0x1234_abcd, 0x1234_abcd, 0x1234_ffff, 0x8000_ffff] {
                commands.push((0xee00_0000, value));
                commands.extend(Draw::Triangle.commands());
            }
            commands.push((0xee00_0000, 0xffff_0001));
            let result = walk(dl, commands);
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            let runs: Vec<_> = if paired {
                result.scene.framebuffer_pairs[0]
                    .ops
                    .iter()
                    .filter_map(|op| match op {
                        crate::scene::SceneOp::Tris(run) => Some(run),
                        _ => None,
                    })
                    .collect()
            } else {
                result.scene.draw_runs.iter().collect()
            };
            assert_eq!(runs.len(), 3);
            assert_eq!(
                runs.iter().map(|run| run.index_count).collect::<Vec<_>>(),
                [6, 3, 3]
            );
            assert_eq!(
                runs.iter().map(|run| run.prim_depth).collect::<Vec<_>>(),
                [
                    PrimitiveDepth {
                        z: 0x1234,
                        dz: 0xabcd
                    },
                    PrimitiveDepth {
                        z: 0x1234,
                        dz: 0xffff
                    },
                    PrimitiveDepth {
                        z: 0x8000,
                        dz: 0xffff
                    },
                ]
            );
            assert_eq!(result.rdp.prim_depth, PrimitiveDepth { z: 0xffff, dz: 1 });
        }
        let (dl, mut commands) = setup(cycle, true);
        for (draw, next_depth) in [
            (Draw::Triangle, 0x8000_ffff),
            (Draw::TexRect, 0x1234_abcd),
            (Draw::TexRectFlip, 0xfeed_beef),
        ] {
            commands.extend(draw.commands());
            commands.push((0xee00_0000, next_depth));
        }
        commands.push((0xee00_0000, 0xffff_0001));
        let result = walk(dl, commands);
        let snapshots: Vec<_> = result.scene.framebuffer_pairs[0]
            .ops
            .iter()
            .map(|op| match op {
                SceneOp::Tris(run) => run.prim_depth,
                SceneOp::TexRect { prim_depth, .. } => *prim_depth,
                _ => panic!("unexpected op: {op:?}"),
            })
            .collect();
        assert_eq!(
            snapshots,
            [
                PrimitiveDepth { z: 0, dz: 0 },
                PrimitiveDepth {
                    z: 0x8000,
                    dz: 0xffff
                },
                PrimitiveDepth {
                    z: 0x1234,
                    dz: 0xabcd
                },
            ]
        );
        assert!(result.diags.is_empty());
    }
}

#[test]
fn primdepth_source_draw_is_rejected() {
    for cycle in [0, 1] {
        for draw in DRAWS {
            let (mut dl, mut commands) = setup(cycle, true);
            commands.extend([(0xee00_0000, 0x8000_ffff), gdp_set_other_mode_l(2, 1, 4)]);
            let draw_offset = commands.len() as u64 * 8;
            commands.extend(draw.commands());
            commands.push(gsp_enddl());
            let entry = dl.list("main", &commands);
            let built = dl.finish("main");
            let result = interpret_rdram(&built.rdram, built.entry);
            assert!(result.scene.indices.is_empty(), "{draw:?}, cycle {cycle}");
            assert!(result.scene.draw_runs.is_empty());
            assert!(result.scene.framebuffer_pairs.is_empty());
            assert!(result.scene.materials.is_empty());
            assert_eq!(result.summary(false).errors, 1);
            assert_eq!(result.summary(false).warns, 0);
            assert_eq!(result.summary(false).dropped_runs, 1);
            assert_eq!(
                result.diags,
                [Diagnostic {
                    at: u64::from(entry) + draw_offset,
                    kind: DiagKind::UnsupportedPrimitiveDepthSource,
                }]
            );
        }
    }
}

#[test]
fn primdepth_source_rejects_repeated_draws_and_recovers() {
    for cycle in [0, 1] {
        for draw in DRAWS {
            for paired in [false, true] {
                if !paired && matches!(draw, Draw::TexRect | Draw::TexRectFlip) {
                    continue;
                }
                let (mut dl, mut commands) = setup(cycle, paired);
                commands.extend(draw.commands());
                commands.push(gdp_set_other_mode_l(2, 1, 4));
                let first_offset = commands.len() as u64 * 8;
                commands.extend(draw.commands());
                let second_offset = commands.len() as u64 * 8;
                commands.extend(draw.commands());
                commands.push(gdp_set_other_mode_l(2, 1, 0));
                commands.extend(draw.commands());
                commands.push(gsp_enddl());
                let entry = dl.list("main", &commands);
                let built = dl.finish("main");
                let result = interpret_rdram(&built.rdram, built.entry);
                assert_eq!(
                    result.diags,
                    [first_offset, second_offset].map(|offset| Diagnostic {
                        at: u64::from(entry) + offset,
                        kind: DiagKind::UnsupportedPrimitiveDepthSource,
                    })
                );
                assert_eq!(result.summary(true).errors, 2);
                assert_eq!(result.summary(true).dropped_runs, 2);
                assert_eq!(result.summary(true).warns, 0);
                let triangles = match draw {
                    Draw::Triangle => 2,
                    Draw::Tri2 | Draw::Quad => 4,
                    _ => 0,
                };
                assert_eq!(result.summary(true).tris, triangles);
                if matches!(draw, Draw::TexRect | Draw::TexRectFlip) {
                    assert_eq!(result.scene.framebuffer_pairs[0].ops.len(), 2);
                }
            }
        }
    }
}

fn pixel_z_scene(cycle: u32, draw: Draw, setters: bool) -> crate::hle::InterpResult {
    let (dl, mut commands) = setup(cycle, true);
    commands.extend([
        gsp_set_geometrymode(1),
        gdp_set_render_mode(0x0044_2078, 0),
        if setters {
            (0xee00_0000, 0xffff_8000)
        } else {
            (0, 0)
        },
    ]);
    commands.extend(draw.commands());
    commands.push(if setters {
        (0xee00_0000, 0x1234_abcd)
    } else {
        (0, 0)
    });
    walk(dl, commands)
}

#[test]
fn pixel_z_draw_state_unchanged() {
    for cycle in [0, 1] {
        for draw in DRAWS {
            let baseline = pixel_z_scene(cycle, draw, false);
            let mut changed = pixel_z_scene(cycle, draw, true);
            assert!(baseline.diags.is_empty());
            assert!(changed.diags.is_empty());
            assert_eq!(changed.summary(true), baseline.summary(true));
            for op in &mut changed.scene.framebuffer_pairs[0].ops {
                let depth = match op {
                    SceneOp::Tris(run) => &mut run.prim_depth,
                    SceneOp::TexRect { prim_depth, .. } => prim_depth,
                    _ => panic!("unexpected op: {op:?}"),
                };
                assert_eq!(
                    *depth,
                    PrimitiveDepth {
                        z: 0xffff,
                        dz: 0x8000
                    }
                );
                *depth = PrimitiveDepth::default();
            }
            assert_eq!(changed.scene, baseline.scene);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pixel_z_draws_unchanged() {
    use super::common::render_to_pixels;
    use crate::render::{headless_device, headless_device_forced_fallback, SceneRenderer};

    for fallback in [true, false] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
        for cycle in [0, 1] {
            for draw in DRAWS {
                let baseline = pixel_z_scene(cycle, draw, false);
                let changed = pixel_z_scene(cycle, draw, true);
                let before =
                    render_to_pixels(&device, &queue, &mut renderer, &baseline.scene, 320, 240);
                let after =
                    render_to_pixels(&device, &queue, &mut renderer, &changed.scene, 320, 240);
                assert!(
                    before.as_chunks::<4>().0.contains(&[255, 0, 0, 255]),
                    "{draw:?}, cycle {cycle}"
                );
                assert_eq!(
                    after, before,
                    "{draw:?}, cycle {cycle}, fallback {fallback}"
                );
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn primdepth_source_draw_is_rejected_pixels() {
    use super::common::render_to_pixels;
    use crate::render::{headless_device_forced_fallback, SceneRenderer};

    let (device, queue) = headless_device_forced_fallback();
    let mut renderer =
        SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, false);
    for cycle in [0, 1] {
        for draw in DRAWS {
            let (dl, mut commands) = setup(cycle, true);
            commands.push(gdp_set_other_mode_l(2, 1, 4));
            commands.extend(draw.commands());
            let rejected = walk(dl, commands);
            assert_eq!(rejected.summary(false).dropped_runs, 1);
            let (dl, commands) = setup(cycle, true);
            let empty = walk(dl, commands);
            let before = render_to_pixels(&device, &queue, &mut renderer, &empty.scene, 320, 240);
            let after = render_to_pixels(&device, &queue, &mut renderer, &rejected.scene, 320, 240);
            assert_eq!(after, before, "{draw:?}, cycle {cycle}");
            assert!(!after.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
        }
    }
}

#[test]
fn primdepth_words_and_roundtrip() {
    for (z, dz, expected) in [
        (0, 0, (0xee00_0000, 0)),
        (0x1234, 0xabcd, (0xee00_0000, 0x1234_abcd)),
        (0xffff, 0x8000, (0xee00_0000, 0xffff_8000)),
        (0, 0xffff, (0xee00_0000, 0x0000_ffff)),
    ] {
        assert_eq!(gdp_set_prim_depth(z, dz), expected);
        let mut dl = DlBuilder::new();
        dl.list("main", &[expected, gsp_enddl()]);
        let built = dl.finish("main");
        let result = interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.rdp.prim_depth.z, z as u16);
        assert_eq!(result.rdp.prim_depth.dz, dz as u16);
    }
}

#[test]
fn primdepth_setter_alone_is_silent() {
    let mut dl = DlBuilder::new();
    dl.list("baseline", &[(0, 0), (0, 0), gsp_enddl()]);
    dl.list(
        "setters",
        &[
            (0xee00_0000, 0x1234_abcd),
            (0xee00_0000, 0xffff_8000),
            gsp_enddl(),
        ],
    );
    let baseline = dl.address("baseline");
    let built = dl.finish("setters");
    let baseline = interpret_rdram(&built.rdram, baseline);
    let result = interpret_rdram(&built.rdram, built.entry);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.summary(false), baseline.summary(false));
    assert_eq!(result.scene, baseline.scene);
}
