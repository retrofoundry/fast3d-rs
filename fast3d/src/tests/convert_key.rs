use super::dl_builder::DlBuilder;
use crate::diag::{ConvertInput, DiagKind, Diagnostic, KeyInput, Severity};
use crate::hle::interpret_rdram;
use n64_gbi::encode::*;

fn walk(commands: &[(u32, u32)]) -> crate::hle::InterpResult {
    let mut dl = DlBuilder::new();
    let mut commands = commands.to_vec();
    commands.push(gsp_enddl());
    dl.list("main", &commands);
    let built = dl.finish("main");
    interpret_rdram(&built.rdram, built.entry)
}

#[test]
fn convert_key_setters_alone_are_silent() {
    let result = walk(&[
        (0xec20_1ff0, 0x03fe_0100),
        (0xeb00_0000, 0x0abc_1234),
        (0xea12_3fed, 0x5678_9abc),
    ]);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    assert_eq!(result.summary(false).errors, 0);
    assert_eq!(result.summary(false).warns, 0);
    assert_eq!(result.dropped_runs, 0);
    let control = walk(&[(0, 0); 3]);
    assert_eq!(result.scene, control.scene);
    assert_eq!(result.summary(false), control.summary(false));
}

#[test]
fn convert_signed_nine_bit_literals() {
    for (words, value) in [
        ((0xec20_1008, 0x0402_0100), -256),
        ((0xec3f_ffff, 0xffff_ffff), -1),
        ((0xec00_0000, 0x0000_0000), 0),
        ((0xec1f_eff7, 0xfbfd_feff), 255),
    ] {
        let result = walk(&[words]);
        assert!(result.diags.is_empty());
        assert_eq!(result.rdp.convert, [value; 6]);
    }
}

#[test]
fn convert_k2_crosses_word_boundary() {
    for (words, value) in [
        ((0xec00_0000, 0xf800_0000), 31),
        ((0xec00_0001, 0x0000_0000), 32),
        ((0xec00_0007, 0xf800_0000), 255),
        ((0xec00_0008, 0x0000_0000), -256),
        ((0xec00_000f, 0xf800_0000), -1),
    ] {
        assert_eq!(walk(&[words]).rdp.convert, [0, 0, value, 0, 0, 0]);
    }
}

#[test]
fn keyr_keygb_preserve_other_channels() {
    use crate::hle::rdp::KeyChannel;
    let red = (0xeb00_0000, 0x0abc_1234);
    let gb = (0xea12_3fed, 0x5678_9abc);
    let expected = [
        KeyChannel {
            center: 0x12,
            scale: 0x34,
            width: 0xabc,
        },
        KeyChannel {
            center: 0x56,
            scale: 0x78,
            width: 0x123,
        },
        KeyChannel {
            center: 0x9a,
            scale: 0xbc,
            width: 0xfed,
        },
    ];
    for commands in [[red, gb], [gb, red]] {
        assert_eq!(walk(&commands).rdp.key, expected);
    }
    let result = walk(&[red, gb, (0xeb00_0000, 0x0fff_ffff)]);
    assert_eq!(result.rdp.key[1..], expected[1..]);
    assert_eq!(
        result.rdp.key[0],
        KeyChannel {
            center: 255,
            scale: 255,
            width: 4095
        }
    );
    let result = walk(&[gb, red, (0xea00_0000, 0)]);
    assert_eq!(
        result.rdp.key,
        [expected[0], KeyChannel::default(), KeyChannel::default()]
    );
}

#[test]
fn key_width_is_retained() {
    for (words, expected) in [
        (
            [(0xeb00_0000, 0x0800_0000), (0xea00_1fff, 0)],
            [0x800, 1, 0xfff],
        ),
        (
            [(0xebff_ffff, 0xf123_0000), (0xeaab_cdef, 0)],
            [0x123, 0xabc, 0xdef],
        ),
    ] {
        assert_eq!(walk(&words).rdp.key.map(|channel| channel.width), expected);
    }
}

#[test]
fn convert_key_words_and_roundtrip() {
    let convert = gdp_set_convert(-256, -1, 0, 255, -256, -1);
    assert_eq!(convert, (0xec20_1ff0, 0x03fe_01ff));
    let keyr = gdp_set_key_r(0x12, 0x34, 0xabc);
    let keygb = gdp_set_key_gb(0x56, 0x78, 0x123, 0x9a, 0xbc, 0xfed);
    assert_eq!(keyr, (0xeb00_0000, 0x0abc_1234));
    assert_eq!(keygb, (0xea12_3fed, 0x5678_9abc));
    let result = walk(&[convert, keyr, keygb]);
    assert_eq!(result.rdp.convert, [-256, -1, 0, 255, -256, -1]);
    assert_eq!(
        result.rdp.key.map(|channel| channel.center),
        [0x12, 0x56, 0x9a]
    );
    assert_eq!(
        result.rdp.key.map(|channel| channel.scale),
        [0x34, 0x78, 0xbc]
    );
    assert_eq!(
        result.rdp.key.map(|channel| channel.width),
        [0xabc, 0x123, 0xfed]
    );
}

#[test]
fn convert_negative_range_uses_signed_arithmetic() {
    for slot in 0..6 {
        for pattern in 256..512u64 {
            let payload = pattern * 512u64.pow(5 - slot as u32);
            let words = (
                0xec00_0000 + (payload / 0x1_0000_0000) as u32,
                (payload % 0x1_0000_0000) as u32,
            );
            let mut expected = [0; 6];
            expected[slot] = pattern as i16 - 512;
            assert_eq!(
                walk(&[words]).rdp.convert,
                expected,
                "slot {slot}, pattern {pattern}"
            );
        }
    }
}

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
                128,
                128,
                384,
                384,
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
            g: 0,
            b: 0,
            a: 255,
        }),
    );
    let mut commands = vec![
        gsp_vertex(0, 4, vertices),
        combine(None),
        gdp_set_prim_color(0, 0, 0xff00_00ff),
        gdp_set_cycle_type(cycle),
        gdp_set_render_mode(0x0f0a_4000, 0),
    ];
    if paired {
        commands.extend([
            gdp_set_color_image(0, 2, 320, 0x10000),
            gdp_set_scissor(0, 0, 0, 1280, 960),
        ]);
    }
    (dl, commands)
}

fn combine(input: Option<(usize, bool, u32)>) -> (u32, u32) {
    let mut color = [CcPass {
        a: 15,
        b: 15,
        c: 31,
        d: 3,
    }; 2];
    if let Some((cycle, multiply, selector)) = input {
        if multiply {
            color[cycle].c = selector;
        } else {
            color[cycle].b = selector;
        }
    }
    let alpha = CcPass {
        a: 7,
        b: 7,
        c: 7,
        d: 3,
    };
    gdp_set_combine_lerp(color[0], alpha, color[1], alpha)
}

fn finish(mut dl: DlBuilder, mut commands: Vec<(u32, u32)>) -> (crate::hle::InterpResult, u64) {
    commands.push(gsp_enddl());
    let entry = dl.list("main", &commands);
    let built = dl.finish("main");
    (interpret_rdram(&built.rdram, built.entry), u64::from(entry))
}

fn assert_rejected(cycle_type: u32, command: (u32, u32), kind: DiagKind) {
    for draw in DRAWS {
        let (dl, mut commands) = setup(cycle_type, true);
        commands.push(command);
        let offset = commands.len() as u64 * 8;
        commands.extend(draw.commands());
        let (result, entry) = finish(dl, commands);
        assert!(
            result.scene.materials.is_empty(),
            "{draw:?}, cycle {cycle_type}, command {command:?}"
        );
        assert!(result.scene.indices.is_empty());
        assert!(result.scene.draw_runs.is_empty());
        assert!(result.scene.framebuffer_pairs.is_empty());
        assert_eq!(result.summary(false).errors, 1);
        assert_eq!(result.summary(false).warns, 0);
        assert_eq!(result.dropped_runs, 1);
        assert_eq!(
            result.diags,
            [Diagnostic {
                at: entry + offset,
                kind
            }]
        );
        assert_eq!(kind.severity(), Severity::Error);
    }
}

#[test]
fn key_selector_active_cycle_is_rejected() {
    for (cycle_type, cycle) in [(0, 1), (1, 0), (1, 1)] {
        for (multiply, selector) in [(false, KeyInput::Center), (true, KeyInput::Scale)] {
            assert_rejected(
                cycle_type,
                combine(Some((cycle, multiply, 6))),
                DiagKind::UnsupportedKeyInput { selector },
            );
        }
    }
}

#[test]
fn k_selector_active_cycle_is_rejected() {
    for (cycle_type, cycle) in [(0, 1), (1, 0), (1, 1)] {
        for (multiply, code, selector) in
            [(false, 7, ConvertInput::K4), (true, 15, ConvertInput::K5)]
        {
            assert_rejected(
                cycle_type,
                combine(Some((cycle, multiply, code))),
                DiagKind::UnsupportedConvertInput { selector },
            );
        }
    }
}

#[test]
fn yuv_and_chroma_key_mode_are_rejected() {
    for cycle in [0, 1] {
        for mode in [
            gdp_set_other_mode_h(9, 3, 0),
            gdp_set_other_mode_h(9, 3, 0xa00),
        ] {
            assert_rejected(cycle, mode, DiagKind::UnsupportedTextureConversion);
        }
        assert_rejected(
            cycle,
            gdp_set_other_mode_h(8, 1, 0x100),
            DiagKind::UnsupportedChromaKey,
        );
        for mode in [0, 0xa00] {
            assert_rejected(
                cycle,
                (0xef00_0000 | cycle << 20 | mode, 0x0f0a_4000),
                DiagKind::UnsupportedTextureConversion,
            );
        }
        assert_rejected(
            cycle,
            (0xef00_0d00 | cycle << 20, 0x0f0a_4000),
            DiagKind::UnsupportedChromaKey,
        );
    }
}

#[test]
fn inactive_cycle_selector_does_not_reject() {
    for cycle_type in [0, 2, 3] {
        for cycle in 0..2 {
            if cycle_type == 0 && cycle == 1 {
                continue;
            }
            for (multiply, selector) in [(false, 6), (true, 6), (false, 7), (true, 15)] {
                for draw in DRAWS {
                    let (dl, mut commands) = setup(cycle_type, true);
                    if cycle_type == 2 {
                        commands.push(gdp_set_tile(0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0));
                    }
                    commands.push(combine(Some((cycle, multiply, selector))));
                    commands.extend(draw.commands());
                    let (result, _) = finish(dl, commands);
                    assert!(
                        result.diags.is_empty(),
                        "{draw:?}, cycle {cycle_type}: {:?}",
                        result.diags
                    );
                    assert_eq!(result.dropped_runs, 0);
                    assert!(!result.scene.materials.is_empty());
                }
            }
        }
    }
}

#[test]
fn rejected_register_inputs_recover_after_cached_draw() {
    for cycle in [0, 1] {
        for (command, reset, kind) in [
            (
                combine(Some((1, false, 6))),
                combine(None),
                DiagKind::UnsupportedKeyInput {
                    selector: KeyInput::Center,
                },
            ),
            (
                combine(Some((1, true, 6))),
                combine(None),
                DiagKind::UnsupportedKeyInput {
                    selector: KeyInput::Scale,
                },
            ),
            (
                combine(Some((1, false, 7))),
                combine(None),
                DiagKind::UnsupportedConvertInput {
                    selector: ConvertInput::K4,
                },
            ),
            (
                combine(Some((1, true, 15))),
                combine(None),
                DiagKind::UnsupportedConvertInput {
                    selector: ConvertInput::K5,
                },
            ),
            (
                gdp_set_other_mode_h(9, 3, 0),
                gdp_set_other_mode_h(9, 3, 0xc00),
                DiagKind::UnsupportedTextureConversion,
            ),
            (
                gdp_set_other_mode_h(9, 3, 0xa00),
                gdp_set_other_mode_h(9, 3, 0xc00),
                DiagKind::UnsupportedTextureConversion,
            ),
            (
                gdp_set_other_mode_h(8, 1, 0x100),
                gdp_set_other_mode_h(8, 1, 0),
                DiagKind::UnsupportedChromaKey,
            ),
        ] {
            for draw in DRAWS {
                for paired in [false, true] {
                    if !paired && matches!(draw, Draw::TexRect | Draw::TexRectFlip) {
                        continue;
                    }
                    let (dl, mut commands) = setup(cycle, paired);
                    commands.extend(draw.commands());
                    commands.push(command);
                    let first = commands.len() as u64 * 8;
                    commands.extend(draw.commands());
                    let second = commands.len() as u64 * 8;
                    commands.extend(draw.commands());
                    commands.push(reset);
                    commands.extend(draw.commands());
                    let (result, entry) = finish(dl, commands);
                    assert_eq!(
                        result.diags,
                        [first, second].map(|offset| Diagnostic {
                            at: entry + offset,
                            kind
                        })
                    );
                    assert_eq!(result.summary(true).errors, 2);
                    assert_eq!(result.summary(true).warns, 0);
                    assert_eq!(result.dropped_runs, 2);
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
}

#[test]
fn convert_key_snapshot_per_draw() {
    use crate::scene::SceneOp;
    for cycle in [0, 1] {
        for paired in [false, true] {
            let (dl, mut commands) = setup(cycle, paired);
            for command in [
                (0xec20_1ff0, 0x03fe_01ff),
                (0xec20_1ff0, 0x03fe_01ff),
                (0xeb00_0000, 0x0abc_1234),
                (0xea12_3fed, 0x5678_9abc),
                (0xeb00_0000, 0x0abd_1234),
            ] {
                commands.push(command);
                commands.extend(Draw::Triangle.commands());
            }
            commands.extend([(0xec00_0000, 0), (0xeb00_0000, 0), (0xea00_0000, 0)]);
            let (result, _) = finish(dl, commands);
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            assert_eq!(result.scene.materials.len(), 4);
            assert_eq!(result.rdp.convert, [0; 6]);
            assert_eq!(result.rdp.key, [crate::hle::rdp::KeyChannel::default(); 3]);
            for mat in &result.scene.materials {
                assert_eq!(mat.convert, [-256, -1, 0, 255, -256, -1]);
            }
            let keys: Vec<_> = result
                .scene
                .materials
                .iter()
                .map(|mat| {
                    mat.key
                        .map(|channel| (channel.center, channel.scale, channel.width))
                })
                .collect();
            assert_eq!(
                keys,
                [
                    [(0, 0, 0); 3],
                    [(0x12, 0x34, 0xabc), (0, 0, 0), (0, 0, 0)],
                    [
                        (0x12, 0x34, 0xabc),
                        (0x56, 0x78, 0x123),
                        (0x9a, 0xbc, 0xfed)
                    ],
                    [
                        (0x12, 0x34, 0xabd),
                        (0x56, 0x78, 0x123),
                        (0x9a, 0xbc, 0xfed)
                    ],
                ]
            );
            let runs: Vec<_> = if paired {
                result.scene.framebuffer_pairs[0]
                    .ops
                    .iter()
                    .filter_map(|op| {
                        if let SceneOp::Tris(run) = op {
                            Some(run)
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                result.scene.draw_runs.iter().collect()
            };
            assert_eq!(
                runs.iter()
                    .map(|run| (run.material_index, run.index_count))
                    .collect::<Vec<_>>(),
                [(0, 6), (1, 3), (2, 3), (3, 3)]
            );
        }
        let (dl, mut commands) = setup(cycle, true);
        for (draw, value) in [
            (Draw::Triangle, -256),
            (Draw::TexRect, -1),
            (Draw::TexRectFlip, 255),
        ] {
            commands.extend([
                gdp_set_convert(value, value, value, value, value, value),
                gdp_set_key_r(1, 2, (value + 256) as u32),
                gdp_set_key_gb(3, 4, 5, 6, 7, 8),
            ]);
            commands.extend(draw.commands());
        }
        commands.extend([(0xec00_0000, 0), (0xeb00_0000, 0), (0xea00_0000, 0)]);
        let (result, _) = finish(dl, commands);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let snapshots: Vec<_> = result.scene.framebuffer_pairs[0]
            .ops
            .iter()
            .map(|op| {
                let index = match op {
                    SceneOp::Tris(run) => run.material_index,
                    SceneOp::TexRect { material_index, .. } => *material_index,
                    _ => panic!("{op:?}"),
                };
                let material = &result.scene.materials[index as usize];
                (material.convert, material.key.map(|channel| channel.width))
            })
            .collect();
        assert_eq!(
            snapshots,
            [
                ([-256; 6], [0, 5, 8]),
                ([-1; 6], [255, 5, 8]),
                ([255; 6], [511, 5, 8])
            ]
        );
    }
}

#[test]
fn fill_rect_keeps_register_snapshots_and_rejects_consuming_modes() {
    let (dl, mut commands) = setup(3, true);
    for value in [-256, -1, 255] {
        commands.extend([
            gdp_set_convert(value, value, value, value, value, value),
            gdp_set_key_r(1, 2, (value + 256) as u32),
            gdp_set_key_gb(3, 4, 5, 6, 7, 8),
            gdp_fill_rectangle(0, 0, 10, 10),
        ]);
    }
    commands.extend([(0xec00_0000, 0), (0xeb00_0000, 0), (0xea00_0000, 0)]);
    let (result, _) = finish(dl, commands);
    assert!(result.diags.is_empty());
    let snapshots: Vec<_> = result.scene.framebuffer_pairs[0]
        .ops
        .iter()
        .map(|op| match op {
            crate::scene::SceneOp::FillRect { convert, key, .. } => {
                (*convert, key.map(|channel| channel.width))
            }
            _ => panic!("{op:?}"),
        })
        .collect();
    assert_eq!(
        snapshots,
        [
            ([-256; 6], [0, 5, 8]),
            ([-1; 6], [255, 5, 8]),
            ([255; 6], [511, 5, 8])
        ]
    );
    for cycle in [0, 1, 3] {
        for (command, kind) in [
            (
                gdp_set_other_mode_h(9, 3, 0),
                DiagKind::UnsupportedTextureConversion,
            ),
            (
                gdp_set_other_mode_h(8, 1, 0x100),
                DiagKind::UnsupportedChromaKey,
            ),
        ] {
            let (dl, mut commands) = setup(cycle, true);
            commands.push(command);
            let offset = commands.len() as u64 * 8;
            commands.push(gdp_fill_rectangle(0, 0, 10, 10));
            let (result, entry) = finish(dl, commands);
            assert_eq!(
                result.diags,
                [Diagnostic {
                    at: entry + offset,
                    kind
                }]
            );
            assert_eq!(result.dropped_runs, 1);
            assert!(result.scene.framebuffer_pairs.is_empty());
        }
    }
}

fn unused_register_scene(cycle: u32, draw: Draw, setters: bool) -> crate::hle::InterpResult {
    let (dl, mut commands) = setup(cycle, true);
    let before = [
        (0xec20_1ff0, 0x03fe_01ff),
        (0xeb00_0000, 0x0abc_1234),
        (0xea12_3fed, 0x5678_9abc),
    ];
    commands.extend(if setters { before } else { [(0, 0); 3] });
    commands.extend(draw.commands());
    commands.extend(if setters {
        [(0xec00_0000, 0), (0xeb00_0000, 0), (0xea00_0000, 0)]
    } else {
        [(0, 0); 3]
    });
    finish(dl, commands).0
}

#[test]
fn unused_register_setters_preserve_draw_state() {
    for cycle in [0, 1] {
        for draw in DRAWS {
            let baseline = unused_register_scene(cycle, draw, false);
            let mut changed = unused_register_scene(cycle, draw, true);
            assert!(baseline.diags.is_empty());
            assert!(changed.diags.is_empty());
            assert_eq!(changed.summary(true), baseline.summary(true));
            for mat in &mut changed.scene.materials {
                assert_eq!(mat.convert, [-256, -1, 0, 255, -256, -1]);
                assert_eq!(mat.key.map(|channel| channel.width), [0xabc, 0x123, 0xfed]);
                mat.convert = [0; 6];
                mat.key = Default::default();
            }
            assert_eq!(changed.scene, baseline.scene);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn unused_register_setters_preserve_pixels() {
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
                let baseline = unused_register_scene(cycle, draw, false);
                let changed = unused_register_scene(cycle, draw, true);
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
fn unsupported_register_inputs_render_no_pixels() {
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
        for (cycle_type, cycle) in [(0, 1), (1, 0), (1, 1)] {
            for draw in DRAWS {
                for command in [
                    combine(Some((cycle, false, 6))),
                    combine(Some((cycle, true, 6))),
                    combine(Some((cycle, false, 7))),
                    combine(Some((cycle, true, 15))),
                    gdp_set_other_mode_h(9, 3, 0),
                    gdp_set_other_mode_h(9, 3, 0xa00),
                    gdp_set_other_mode_h(8, 1, 0x100),
                ] {
                    let (dl, mut commands) = setup(cycle_type, true);
                    commands.push(command);
                    commands.extend(draw.commands());
                    let (rejected, _) = finish(dl, commands);
                    assert_eq!(rejected.dropped_runs, 1);
                    let (dl, commands) = setup(cycle_type, true);
                    let (empty, _) = finish(dl, commands);
                    let before =
                        render_to_pixels(&device, &queue, &mut renderer, &empty.scene, 320, 240);
                    let after =
                        render_to_pixels(&device, &queue, &mut renderer, &rejected.scene, 320, 240);
                    assert_eq!(
                        after, before,
                        "{draw:?}, cycle {cycle_type}, command {command:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn f3d_registers_and_conversion_mode_use_shared_rdp_state() {
    use crate::hle::{
        gbi::GbiUcode,
        mem::{GbiDataFormat, RdramImage},
    };
    for (mode, kind) in [
        ((0xba00_0903, 0), DiagKind::UnsupportedTextureConversion),
        ((0xba00_0903, 0xa00), DiagKind::UnsupportedTextureConversion),
        ((0xba00_0801, 0x100), DiagKind::UnsupportedChromaKey),
    ] {
        let commands = [
            (0xec20_1ff0, 0x03fe_01ff),
            (0xeb00_0000, 0x0abc_1234),
            (0xea12_3fed, 0x5678_9abc),
            (0xff10_013f, 0x10000),
            mode,
            (0xf600_4004, 0),
            (0xb800_0000, 0),
        ];
        let bytes: Vec<_> = commands
            .into_iter()
            .flat_map(|(w0, w1): (u32, u32)| w0.to_be_bytes().into_iter().chain(w1.to_be_bytes()))
            .collect();
        let result = crate::hle::interpret(
            RdramImage::new(&bytes),
            0,
            GbiUcode::F3d,
            GbiDataFormat::Fixed,
        );
        assert_eq!(result.rdp.convert, [-256, -1, 0, 255, -256, -1]);
        assert_eq!(
            result.rdp.key.map(|channel| channel.width),
            [0xabc, 0x123, 0xfed]
        );
        assert_eq!(result.diags, [Diagnostic { at: 40, kind }]);
        assert_eq!(result.summary(false).errors, 1);
        assert_eq!(result.dropped_runs, 1);
        assert!(result.scene.framebuffer_pairs.is_empty());
    }
}

#[test]
fn textconv_partial_writes_reject_and_unrelated_writes_preserve_mode() {
    for (commands, rejected) in [
        (vec![gdp_set_other_mode_h(9, 1, 0)], true),
        (
            vec![
                gdp_set_other_mode_h(9, 3, 0xc00),
                gdp_set_other_mode_h(10, 1, 0),
            ],
            true,
        ),
        (
            vec![gdp_set_other_mode_h(9, 3, 0), gdp_set_cycle_type(1)],
            true,
        ),
        (
            vec![gdp_set_other_mode_h(9, 3, 0xc00), gdp_set_cycle_type(1)],
            false,
        ),
        (vec![gdp_set_cycle_type(1)], false),
    ] {
        let (dl, mut prefix) = setup(0, true);
        prefix.extend(commands);
        let offset = prefix.len() as u64 * 8;
        prefix.extend(Draw::Triangle.commands());
        let (result, entry) = finish(dl, prefix);
        if rejected {
            assert_eq!(
                result.diags,
                [Diagnostic {
                    at: entry + offset,
                    kind: DiagKind::UnsupportedTextureConversion
                }]
            );
            assert_eq!(result.dropped_runs, 1);
        } else {
            assert!(result.diags.is_empty());
            assert_eq!(result.summary(true).tris, 1);
        }
    }
}

#[test]
fn fill_rect_only_adds_rejection_for_convert_key_inputs() {
    for cycle_type in [0, 1] {
        for (command, expected) in [
            ((0xfc7f_feff, 0xfffd_f6fb), None),
            (
                combine(Some((1, false, 6))),
                Some(DiagKind::UnsupportedKeyInput {
                    selector: KeyInput::Center,
                }),
            ),
            (
                combine(Some((1, true, 6))),
                Some(DiagKind::UnsupportedKeyInput {
                    selector: KeyInput::Scale,
                }),
            ),
            (
                combine(Some((1, false, 7))),
                Some(DiagKind::UnsupportedConvertInput {
                    selector: ConvertInput::K4,
                }),
            ),
            (
                combine(Some((1, true, 15))),
                Some(DiagKind::UnsupportedConvertInput {
                    selector: ConvertInput::K5,
                }),
            ),
        ] {
            let (dl, mut commands) = setup(cycle_type, true);
            commands.push(command);
            let offset = commands.len() as u64 * 8;
            commands.push(gdp_fill_rectangle(0, 0, 10, 10));
            let (result, entry) = finish(dl, commands);
            if let Some(kind) = expected {
                assert_eq!(
                    result.diags,
                    [Diagnostic {
                        at: entry + offset,
                        kind
                    }]
                );
                assert_eq!(result.dropped_runs, 1);
                assert!(result.scene.framebuffer_pairs.is_empty());
            } else {
                assert!(result.diags.is_empty(), "{:?}", result.diags);
                assert_eq!(result.dropped_runs, 0);
                assert_eq!(result.scene.framebuffer_pairs[0].ops.len(), 1);
            }
        }
    }
}
