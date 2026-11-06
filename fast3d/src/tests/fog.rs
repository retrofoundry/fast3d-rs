use super::common::{clear_color_rgb, pixel, render_to_pixels};
use crate::hle::consts::rdp::*;
use crate::hle::gbi::GbiUcode;
use crate::hle::interp::interpret;
use crate::hle::mem::{GbiDataFormat, RdramImage};
use crate::hle::{Scene, SceneOp};
use crate::render::{headless_device, headless_device_forced_fallback, SceneRenderer};
use n64_gbi::encode::{mtx_to_bytes, VtxColored};

use super::render::{render_scene_to_rgba8, run_compute_outputs};

const FACTOR_A: (u32, u32) = (0xBC00_0008, 0x0724_F9DC);
const FACTOR_B: (u32, u32) = (0xBC00_0008, 0x0500_FC00);
const COLOR_A: (u32, u32) = (0xF800_0000, 0x0F41_64FF);
const COLOR_B: (u32, u32) = (0xF800_0000, 0x0550_4BFF);
const LOAD: (u32, u32) = (0x0400_0010, 0x40);
const TRI: (u32, u32) = (0xBF00_0000, 0);

fn scene_memory(commands: &[(u32, u32)]) -> (Vec<u8>, u32) {
    let mut bytes = vec![0; 0x40];
    for (x, y) in [(-10, 10), (10, 10), (10, -10), (-10, -10)] {
        bytes.extend_from_slice(
            &VtxColored {
                x,
                y,
                z: 9,
                flag: 0,
                s: 0,
                t: 0,
                r: 0,
                g: 0,
                b: 0,
                a: 17,
            }
            .to_bytes(),
        );
    }
    bytes.extend_from_slice(&mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 10.0],
    ]));
    bytes.resize(0x100, 0);
    for (w0, w1) in [
        (0x0102_0040u32, 0x80u32),
        (0xB700_0000, 0x0001_0004),
        (0xBA00_1402, 0x0010_0000),
        (0xB900_031D, 0xC811_2078),
        (0xFA00_0000, 0x0000_00FF),
        (0xFC00_0000, 0x0001_86C3),
    ]
    .into_iter()
    .chain(commands.iter().copied())
    .chain([(0xB800_0000, 0)])
    {
        bytes.extend_from_slice(&w0.to_be_bytes());
        bytes.extend_from_slice(&w1.to_be_bytes());
    }
    (bytes, 0x100)
}

fn interpret_memory(bytes: Vec<u8>, entry: u32) -> Scene {
    let result = interpret(
        RdramImage::new(&bytes),
        entry.into(),
        GbiUcode::F3d,
        GbiDataFormat::Fixed,
        None,
    );
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene
}

fn scene(commands: &[(u32, u32)]) -> Scene {
    let (bytes, entry) = scene_memory(commands);
    interpret_memory(bytes, entry)
}

#[test]
fn fog_factors_are_vertex_load_state() {
    let scene = scene(&[
        FACTOR_A,
        LOAD,
        FACTOR_B,
        LOAD,
        FACTOR_A,
        LOAD,
        (0xB600_0000, 0x0001_0000),
        LOAD,
        (0xBC00_0008, 0xFFFF_8000),
    ]);
    assert_eq!(scene.fog, [1, 2, 1, 0]);
    assert_eq!(scene.fog_table, [[1828, -1572], [1280, -1024]]);
}

#[test]
fn fog_color_is_draw_state() {
    for paired in [false, true] {
        let mut commands = Vec::new();
        if paired {
            commands.extend([(0xFF10_013F, 0x0010_0000), (0xED00_0000, 0x0050_03C0)]);
        }
        commands.extend([
            FACTOR_A, LOAD, COLOR_A, TRI, TRI, FACTOR_B, TRI, COLOR_B, TRI, COLOR_A, TRI, COLOR_B,
        ]);
        let scene = scene(&commands);
        let runs: Vec<_> = if paired {
            scene.framebuffer_pairs[0]
                .ops
                .iter()
                .filter_map(|op| match op {
                    SceneOp::Tris(run) => Some(run),
                    _ => None,
                })
                .collect()
        } else {
            scene.draw_runs.iter().collect()
        };
        assert_eq!(
            runs.iter().map(|r| r.index_count).collect::<Vec<_>>(),
            [9, 3, 3]
        );
        assert_eq!(
            runs.iter().map(|r| r.fog_color).collect::<Vec<_>>(),
            [[15, 65, 100, 255], [5, 80, 75, 255], [15, 65, 100, 255]]
        );
        assert_eq!(scene.materials.len(), 1);
        assert_eq!(scene.render_modes.len(), 1);
    }
}

fn reused_scene(rgba: bool) -> Scene {
    scene(&[
        FACTOR_B,
        LOAD,
        FACTOR_A,
        LOAD,
        TRI,
        FACTOR_B,
        TRI,
        if rgba {
            (0xBC00_100C, 0x1122_3344)
        } else {
            (0xBC00_140C, 0x0040_0080)
        },
        TRI,
    ])
}

#[test]
fn fog_reused_vertex_keeps_factor() {
    let scene = reused_scene(false);
    assert_eq!(scene.indices, [1, 1, 1, 1, 1, 1, 2, 2, 2]);
    assert_eq!(scene.fog, [1, 2, 2]);
}

#[test]
fn fog_modify_rgba_preserves_supplied_alpha() {
    let scene = reused_scene(true);
    assert_eq!(scene.cn[2], 0x4433_2211);
    assert_eq!(scene.fog, [1, 2, 0]);
}

#[test]
fn fog_signed_clip_z_gpu_readback() {
    let (mut bytes, entry) = scene_memory(&[(0xBC00_0008, 0x0080_0080), LOAD]);
    bytes[0x44..0x46].copy_from_slice(&(-3i16).to_be_bytes());
    bytes[0x80..0xC0].copy_from_slice(&mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 4.0],
    ]));
    assert_alphas(&interpret_memory(bytes, entry), &[32.0]);
}

#[test]
fn fog_ucode_reload_gpu_readback() {
    use n64_gbi::encode::{
        gsp_enddl, gsp_load_ucode, gsp_matrix, gsp_set_geometrymode, gsp_vertex,
    };

    for target in [crate::Microcode::F3dex2, crate::Microcode::F3dex2NoN] {
        for roundtrip in [true, false] {
            let (mut bytes, entry) = scene_memory(&[]);
            bytes.truncate(entry as usize);
            let mut commands = vec![
                gsp_matrix(0x80, true, true, false),
                gsp_set_geometrymode(0x0001_0000),
                (0xDB08_0000, FACTOR_B.1),
                gsp_vertex(0, 1, 0x40),
            ];
            if roundtrip {
                commands.extend(gsp_load_ucode(0x1000, 0x8000, 0x800));
            }
            commands.extend(gsp_load_ucode(0x2000, 0x9000, 0x800));
            commands.extend([
                gsp_matrix(0x80, true, true, false),
                gsp_vertex(0, 1, 0x40),
                (0xDB08_0000, FACTOR_B.1),
                gsp_vertex(0, 1, 0x40),
                gsp_enddl(),
            ]);
            for (w0, w1) in commands {
                bytes.extend_from_slice(&w0.to_be_bytes());
                bytes.extend_from_slice(&w1.to_be_bytes());
            }
            let result = crate::hle::interp::interpret_with_bindings(
                RdramImage::new(&bytes),
                entry.into(),
                target.into(),
                GbiDataFormat::Fixed,
                Default::default(),
                Default::default(),
                None,
                Default::default(),
                &[(0x1000, crate::Microcode::S2dex2), (0x2000, target)],
            );
            assert_alphas(&result.scene, &[128.0, 0.0, 128.0]);
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            assert_eq!(result.termination, crate::inspect::WalkTermination::End);
        }
    }
}

#[test]
fn fog_factors_gpu_readback() {
    let scene = scene(&[
        FACTOR_A,
        LOAD,
        FACTOR_B,
        LOAD,
        (0xB600_0000, 0x0001_0000),
        LOAD,
    ]);
    assert_alphas(&scene, &[73.2, 128.0, 17.0]);
}

#[test]
fn fog_reused_vertex_gpu_readback() {
    assert_alphas(&reused_scene(false), &[128.0, 73.2, 73.2]);
}

#[test]
fn fog_modify_rgba_gpu_readback() {
    assert_alphas(&reused_scene(true), &[128.0, 73.2, 68.0]);
}

fn assert_alphas(scene: &Scene, expected: &[f32]) {
    let (device, queue, _) = headless_device();
    let output = run_compute_outputs(&device, &queue, scene);
    assert_eq!(output.len(), expected.len());
    for (i, (vertex, &expected)) in output.iter().zip(expected).enumerate() {
        let actual = vertex.color[3] * 255.0;
        assert!(
            (actual - expected).abs() < 0.001,
            "vertex {i}: {actual} != {expected}"
        );
    }
}

// JRB command settings from areas/1/{5,2}/model.inc.c; geometry is synthetic.
fn jrb_commands(append_unused: bool) -> Vec<(u32, u32)> {
    let mut commands = vec![
        (0xFF10_013F, 0x0010_0000),
        (0xED00_0000, 0x0050_03C0),
        COLOR_A,
        FACTOR_A,
        (0x0430_0040, 0x40),
        (0xED00_0000, 0x0014_03C0),
        (0xBF00_0000, 0x0000_0A14),
        (0xBF00_0000, 0x0000_141E),
        FACTOR_B,
        COLOR_B,
        (0xED14_0000, 0x0028_03C0),
        (0xBF00_0000, 0x0000_0A14),
        (0xBF00_0000, 0x0000_141E),
        (0x0430_0040, 0x40),
        (0xED28_0000, 0x003C_03C0),
        (0xBF00_0000, 0x0000_0A14),
        (0xBF00_0000, 0x0000_141E),
        COLOR_A,
        FACTOR_A,
        (0xED3C_0000, 0x0050_03C0),
        (0xBF00_0000, 0x0000_0A14),
        (0xBF00_0000, 0x0000_141E),
    ];
    if append_unused {
        commands.extend([(0xF800_0000, 0xFF00_FF00), (0xBC00_0008, 0xFFFF_8000)]);
    }
    commands
}

fn jrb_memory(append_unused: bool) -> (Vec<u8>, u32) {
    scene_memory(&jrb_commands(append_unused))
}

fn jrb_scene(append_unused: bool) -> Scene {
    let (bytes, entry) = jrb_memory(append_unused);
    interpret_memory(bytes, entry)
}

#[test]
fn fixture_sm64_jrb_mixed_fog() {
    let scene = jrb_scene(false);
    assert_alphas(
        &scene,
        &[73.2, 73.2, 73.2, 73.2, 128.0, 128.0, 128.0, 128.0],
    );
    let pixels = render_scene_to_rgba8(&scene, 320, 240);
    for (x, expected) in [
        (40, [4, 19, 29, 255]),
        (120, [1, 23, 22, 255]),
        (200, [3, 40, 38, 255]),
        (280, [8, 33, 50, 255]),
    ] {
        let offset = (120 * 320 + x) * 4;
        assert_eq!(&pixels[offset..offset + 4], &expected, "x={x}");
    }
    assert_eq!(pixels, render_scene_to_rgba8(&jrb_scene(true), 320, 240));
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_jrb_mixed_fog_fixture() {
    let (bytes, scene_entry) = jrb_memory(false);
    super::capture_fixture::write(
        bytes,
        scene_entry,
        320,
        240,
        "jrb-mixed-fog.f3dcap",
        jrb_provenance(),
    );
}

fn rect_scene(append_unused: bool) -> Scene {
    let mut commands = vec![
        (0xFF10_013F, 0x0010_0000),
        (0xED00_0000, 0x0050_03C0),
        COLOR_A,
        (0xE428_03C0, 0),
        (0xB400_0000, 0),
        (0xB300_0000, 0x0400_0400),
        COLOR_B,
        (0xE450_03C0, 0x0028_0000),
        (0xB400_0000, 0),
        (0xB300_0000, 0x0400_0400),
    ];
    if append_unused {
        commands.push((0xF800_0000, 0xFF00_FF00));
    }
    scene(&commands)
}

#[test]
fn fog_texrect_color_gpu_readback() {
    let pixels = render_scene_to_rgba8(&rect_scene(false), 320, 240);
    for (x, expected) in [(80, [15, 65, 100, 255]), (240, [5, 80, 75, 255])] {
        let offset = (120 * 320 + x) * 4;
        assert_eq!(&pixels[offset..offset + 4], &expected);
    }
    assert_eq!(pixels, render_scene_to_rgba8(&rect_scene(true), 320, 240));
}

#[test]
fn fog_unused_commands_leave_scene_unchanged() {
    assert_eq!(jrb_scene(false), jrb_scene(true));
    assert_eq!(rect_scene(false), rect_scene(true));
    let rects = rect_scene(false);
    let colors: Vec<_> = rects.framebuffer_pairs[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            SceneOp::TexRect { fog_color, .. } => Some(*fog_color),
            _ => None,
        })
        .collect();
    assert_eq!(colors, [[15, 65, 100, 255], [5, 80, 75, 255]]);
}

#[cfg(feature = "capture")]
fn jrb_provenance() -> crate::capture::Provenance {
    crate::capture::Provenance {
        decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
        source_symbols: "levels/jrb/areas/1/5/model.inc.c: jrb_seg7_dl_070069B0; levels/jrb/areas/1/2/model.inc.c: jrb_seg7_dl_07004940".into(),
        command_vector: "JRB fog state sequence with capture-only framebuffer wrapper".into(),
        synthetic_data: "Synthetic quad geometry, matrix, vertex colors, and alpha payloads"
            .into(),
    }
}

#[cfg(feature = "capture")]
pub(super) fn jrb_fixture() -> crate::capture::Fixture {
    let (bytes, entry) = jrb_memory(false);
    super::capture_fixture::make(bytes, entry, 320, 240, jrb_provenance())
}

#[test]
fn fog_blender_p_and_m_match_the_equation() {
    for fallback in [false, true] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
        for (mode, cycles, fog_in_p, alpha) in [
            (0x0af0_42b0, 0, false, 255),
            (0x0af0_42b0 & !FORCE_BL, 0, false, 255),
            (G_RM_FOG_SHADE_A | FORCE_BL, 0, true, 255),
            (
                gbl_c1(CLR_IN, A_SHADE, CLR_FOG, B_1MA) | G_RM_AA_ZB_OPA_SURF2,
                1,
                false,
                255,
            ),
            (G_RM_FOG_SHADE_A | G_RM_AA_ZB_OPA_SURF2, 1, true, 255),
            (
                gbl_c1(CLR_IN, A_SHADE, CLR_FOG, B_1MA)
                    | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)
                    | FORCE_BL,
                1,
                false,
                128,
            ),
            (
                G_RM_FOG_SHADE_A | gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA) | FORCE_BL,
                1,
                true,
                128,
            ),
        ] {
            for decal in [false, true] {
                for factor in [0, 64, 128, 192, 255] {
                    let mut commands = Vec::new();
                    if decal {
                        let [r, g, b] = clear_color_rgb();
                        commands.extend([
                            (0xB700_0000, 1),
                            (0xB900_031D, G_RM_OPA_SURF | Z_CMP | Z_UPD),
                            (0xFA00_0000, u32::from_be_bytes([r, g, b, 255])),
                            (0x0430_0040, 0x40),
                            (0xBF00_0000, 0x0000_0a14),
                            (0xBF00_0000, 0x0000_141e),
                        ]);
                    }
                    commands.extend([
                        (0xBA00_1402, cycles << 20),
                        (
                            0xB900_031D,
                            mode | if decal { ZMODE_DEC | Z_CMP } else { 0 },
                        ),
                        (0xFA00_0000, 0xc864_3200 | alpha),
                        (0xF800_0000, 0x1446_8cff),
                        (0xBC00_0008, factor),
                        (0x0430_0040, 0x40),
                        (0xBF00_0000, 0x0000_0a14),
                        (0xBF00_0000, 0x0000_141e),
                    ]);
                    let pixels =
                        render_to_pixels(&device, &queue, &mut renderer, &scene(&commands), 64, 64);
                    let actual = pixel(&pixels, 64, 32, 32);
                    for channel in 0..3 {
                        let (p, m) = if fog_in_p {
                            ([20, 70, 140], [200, 100, 50])
                        } else {
                            ([200, 100, 50], [20, 70, 140])
                        };
                        let a = factor as f32 / 255.0;
                        let expected = if cycles == 0 && mode & FORCE_BL == 0 {
                            p[channel] as f32
                        } else {
                            p[channel] as f32 * a + m[channel] as f32 * (1.0 - a)
                        };
                        let blend_alpha = alpha as f32 / 255.0;
                        let expected = expected * blend_alpha
                            + f32::from(clear_color_rgb()[channel]) * (1.0 - blend_alpha);
                        assert!((f32::from(actual[channel]) - expected).abs() <= 1.0,
                        "mode={mode:#010x} cycles={cycles} factor={factor} decal={decal} fallback={fallback} channel={channel}: {actual:?}, expected {expected}");
                    }
                    assert_eq!(
                        actual[3],
                        if dual && alpha < 255 {
                            255
                        } else {
                            alpha as u8
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn fog_one_cycle_bypass_gpu_readback() {
    let mut failures = Vec::new();
    for fallback in [false, true] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        // RGB literals from matrix-hw-01; RGBA16 compares stored RGB5, not coverage.
        for (name, size, mode, fog, primitive, expected) in [
            (
                "fog-02",
                3,
                0xc811_0004,
                0xd838_1860,
                0x5b81_d5ff,
                [216, 56, 24],
            ),
            (
                "fog-19",
                3,
                0xc811_004c,
                0xd838_1860,
                0x5b81_d5ff,
                [216, 56, 24],
            ),
            (
                "fog-37",
                2,
                0xc811_0004,
                0x1122_3360,
                0x5b81_d5ff,
                [2, 4, 6],
            ),
            (
                "fog-38",
                2,
                0xc811_1004,
                0xd838_1860,
                0x5b81_d580,
                [27, 7, 3],
            ),
        ] {
            let mut renderer =
                SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
            let (mut bytes, entry) = scene_memory(&[
                (0xff00_013f | size << 19, 0x0010_0000),
                (0xed00_0000, 0x0050_03c0),
                (0xef00_0cf0, mode),
                (0xfcff_ffff, 0xfffe_773b),
                (0xfa00_0000, primitive),
                (0xf800_0000, fog),
                (0xb600_0000, 0x0001_0000),
                (0x0430_0040, 0x40),
                (0xbf00_0000, 0x0000_0a14),
                (0xbf00_0000, 0x0000_141e),
            ]);
            for vertex in bytes[0x40..0x80].as_chunks_mut::<16>().0 {
                vertex[12..16].copy_from_slice(&[40, 104, 200, 128]);
            }
            let pixels = render_to_pixels(
                &device,
                &queue,
                &mut renderer,
                &interpret_memory(bytes, entry),
                320,
                240,
            );
            let actual = pixel(&pixels, 320, 160, 100);
            let rgb = [actual[0], actual[1], actual[2]].map(|channel| {
                if size == 2 {
                    channel >> 3
                } else {
                    channel
                }
            });
            if rgb != expected {
                failures.push(format!(
                    "{name} fallback={fallback} dual={dual}: {actual:?}, RGB {rgb:?} != {expected:?}"
                ));
            }
            assert!(
                renderer.diagnostics.is_empty(),
                "{:?}",
                renderer.diagnostics
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

type FogPixelCase = (u8, u8, u8, [[u8; 3]; 2]);

const INPUT_RGB: [u8; 3] = [200, 100, 50];
const FOG_RGB: [u8; 3] = [20, 70, 140];

fn assert_local_fog_equations(
    a: u32,
    b: u32,
    input: [u8; 3],
    fog: [u8; 3],
    cases: &[FogPixelCase],
) {
    for fallback in [false, true] {
        let (device, queue, dual) = if fallback {
            let (device, queue) = headless_device_forced_fallback();
            (device, queue, false)
        } else {
            headless_device()
        };
        let mut renderer =
            SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
        for &(alpha, fog_alpha, shade_alpha, shifted) in cases {
            for (cycles, force) in [(0, false), (0, true), (1, false), (1, true)] {
                for fog_in_p in [true, false] {
                    let (p, m) = if fog_in_p {
                        (CLR_FOG, CLR_IN)
                    } else {
                        (CLR_IN, CLR_FOG)
                    };
                    let mode = gbl_c1(p, a, m, b)
                        | if force { FORCE_BL } else { 0 }
                        | if cycles == 1 {
                            if force {
                                gbl_c2(CLR_IN, A_IN, CLR_MEM, B_1MA)
                            } else {
                                G_RM_OPA_SURF2
                            }
                        } else {
                            0
                        };
                    for decal in [false, true] {
                        let mut commands = Vec::new();
                        if decal {
                            let [r, g, b] = clear_color_rgb();
                            commands.extend([
                                (0xB700_0000, 1),
                                (0xB900_031D, G_RM_OPA_SURF | Z_CMP | Z_UPD),
                                (0xFA00_0000, u32::from_be_bytes([r, g, b, 255])),
                                (0x0430_0040, 0x40),
                                (0xBF00_0000, 0x0000_0a14),
                                (0xBF00_0000, 0x0000_141e),
                            ]);
                        }
                        commands.extend([
                            (0xBA00_1402, cycles << 20),
                            (
                                0xB900_031D,
                                mode | if decal { ZMODE_DEC | Z_CMP } else { 0 },
                            ),
                            (
                                0xFA00_0000,
                                u32::from_be_bytes([input[0], input[1], input[2], alpha]),
                            ),
                            (
                                0xF800_0000,
                                u32::from_be_bytes([fog[0], fog[1], fog[2], fog_alpha]),
                            ),
                            (0xBC00_0008, u32::from(shade_alpha)),
                            (0x0430_0040, 0x40),
                            (0xBF00_0000, 0x0000_0a14),
                            (0xBF00_0000, 0x0000_141e),
                        ]);
                        let pixels = render_to_pixels(
                            &device,
                            &queue,
                            &mut renderer,
                            &scene(&commands),
                            64,
                            64,
                        );
                        let actual = pixel(&pixels, 64, 32, 32);
                        let expected = if force || cycles == 1 {
                            shifted[usize::from(!fog_in_p)]
                        } else if fog_in_p {
                            fog
                        } else {
                            input
                        };
                        for channel in 0..3 {
                            let blend_alpha = if cycles == 1 && force {
                                f32::from(alpha) / 255.0
                            } else {
                                1.0
                            };
                            let expected = f32::from(expected[channel]) * blend_alpha
                                + f32::from(clear_color_rgb()[channel]) * (1.0 - blend_alpha);
                            let tolerance = if cycles == 1 && force { 1.0 } else { 0.0 };
                            assert!((f32::from(actual[channel]) - expected).abs() <= tolerance,
                                "a={a} b={b} alpha={alpha} fog_alpha={fog_alpha} shade_alpha={shade_alpha} cycles={cycles} force={force} fog_in_p={fog_in_p} decal={decal} fallback={fallback}: {actual:?}, channel={channel} expected={expected}");
                        }
                        assert_eq!(
                            actual[3],
                            if dual && cycles == 1 && force {
                                255
                            } else {
                                alpha
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fog_prim_a_uses_fog_alpha_independently_of_shade_alpha() {
    // G_RM_FOG_PRIM_A = GBL_c1(FOG, A_FOG, IN, 1MA); literals use angrylion's 5-bit weights.
    let mut cases = Vec::new();
    for (fog_alpha, expected) in [
        (0, [[200, 100, 50], [20, 70, 140]]),
        (64, [[155, 92, 72], [65, 77, 117]]),
        (128, [[110, 85, 95], [110, 85, 95]]),
        (192, [[65, 77, 117], [155, 92, 72]]),
        (255, [[25, 70, 137], [194, 99, 52]]),
    ] {
        for shade_alpha in [0, 73, 128, 255] {
            for alpha in [64, 255] {
                cases.push((alpha, fog_alpha, shade_alpha, expected));
            }
        }
    }
    assert_local_fog_equations(A_FOG, B_1MA, INPUT_RGB, FOG_RGB, &cases);
}

#[test]
fn fog_input_and_zero_alpha_use_complementary_b() {
    let rows = [
        (0, [[200, 100, 50], [20, 70, 140]]),
        (128, [[110, 85, 95], [110, 85, 95]]),
        (255, [[25, 70, 137], [194, 99, 52]]),
    ];
    for a in [A_IN, A_0] {
        let cases: Vec<_> = rows
            .iter()
            .map(|&(alpha, expected)| {
                let expected = if a == A_0 { rows[0].1 } else { expected };
                (alpha, 17, 73, expected)
            })
            .collect();
        assert_local_fog_equations(a, B_1MA, INPUT_RGB, FOG_RGB, &cases);
    }
}

#[test]
fn fog_local_alpha_and_noncomplementary_b_follow_blender_rounding() {
    for (b, rows) in [
        (
            B_1,
            [
                (0, [[200, 100, 50], [20, 70, 140]]),
                (128, [[210, 135, 120], [120, 120, 165]]),
                (255, [[219, 167, 185], [213, 166, 188]]),
            ],
        ),
        (
            B_0,
            [
                (0, [[6, 3, 1], [0, 2, 4]]),
                (128, [[16, 38, 71], [100, 52, 29]]),
                (255, [[25, 70, 137], [194, 99, 52]]),
            ],
        ),
    ] {
        for a in [A_IN, A_FOG, A_SHADE, A_0] {
            let cases: Vec<_> = rows
                .iter()
                .take(if a == A_0 { 1 } else { rows.len() })
                .map(|&(value, shifted)| {
                    (
                        if a == A_IN { value } else { 255 },
                        if a == A_FOG { value } else { 17 },
                        if a == A_SHADE { value } else { 73 },
                        shifted,
                    )
                })
                .collect();
            assert_local_fog_equations(a, b, INPUT_RGB, FOG_RGB, &cases);
        }
    }
}

#[test]
fn fog_blender_matches_the_hardware_worked_example() {
    // P = M = 128 and A = 128: B = 1 gives (128*16 + 128*32) >> 5, B = 0 gives (128*16 + 128) >> 5.
    for (b, value) in [(B_1, 192), (B_0, 68)] {
        assert_local_fog_equations(
            A_IN,
            b,
            [128; 3],
            [128; 3],
            &[(128, 17, 73, [[value; 3]; 2])],
        );
    }
}

fn assert_hardware_blender(mode: u32, cycles: u32, expected: [u8; 3], fallback: bool) {
    let (device, queue, dual) = if fallback {
        let (device, queue) = headless_device_forced_fallback();
        (device, queue, false)
    } else {
        headless_device()
    };
    eprintln!("blender mode={mode:#010x} cycles={cycles} fallback={fallback} dual={dual}");
    let mut renderer = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    for decal in [false, true] {
        let commands = [
            (0xBA00_1402, 0),
            (0xB900_031D, G_RM_OPA_SURF | G_ZS_PRIM | Z_CMP | Z_UPD),
            (0xB700_0000, 1),
            (0xFA00_0000, 0x1848_78E0),
            (0x0430_0040, 0x40),
            (0xBF00_0000, 0x0000_0A14),
            (0xBF00_0000, 0x0000_141E),
            (0xBA00_1402, cycles.min(1) << 20),
            (
                0xB900_031D,
                mode | if decal { ZMODE_DEC | Z_CMP } else { 0 },
            ),
            (0xFC3C_FFFF, 0xFFFE_773B),
            (0xF800_0000, 0xD838_1860),
            (0xF900_0000, 0x5090_D080),
            (0xFA00_0000, 0x5B81_D5FF),
            (0xFB00_0000, 0xDB21_4D40),
            (0xBC00_0008, 128),
            (0x0430_0040, 0x40),
            (0xBF00_0000, 0x0000_0A14),
            (0xBF00_0000, 0x0000_141E),
        ];
        let (mut bytes, entry) = scene_memory(&commands);
        for i in 0..4 {
            bytes[0x40 + i * 16 + 12..0x40 + i * 16 + 16].copy_from_slice(&[40, 104, 200, 128]);
        }
        let mut result = interpret(
            RdramImage::new(&bytes),
            entry.into(),
            GbiUcode::F3d,
            GbiDataFormat::Fixed,
            None,
        );
        assert!(
            result
                .diags
                .iter()
                .all(|d| d.kind == crate::DiagKind::NonCanonicalBlend),
            "{:?}",
            result.diags
        );
        if cycles == 2 {
            // Exercise the COPY shader gate without the interpreter's mandatory texture setup.
            result.scene.materials.last_mut().unwrap().cycle_type = 2;
        }
        let pixels = render_to_pixels(&device, &queue, &mut renderer, &result.scene, 64, 64);
        assert_eq!(
            pixel(&pixels, 64, 32, 32)[..3],
            expected,
            "mode={mode:#010x} cycles={cycles} decal={decal} fallback={fallback}"
        );
    }
}

#[test]
fn fog_blender_hardware_second_cycle_dual() {
    assert_hardware_blender(0x3E08_4004, 1, [128, 80, 112], false);
}

#[test]
fn fog_blender_hardware_second_cycle_fallback() {
    assert_hardware_blender(0x3E08_4004, 1, [128, 80, 112], true);
}

#[test]
fn fog_blender_hardware_blend_rgb_dual() {
    assert_hardware_blender(0x8411_4004, 0, [55, 119, 203], false);
}

#[test]
fn fog_blender_hardware_blend_rgb_fallback() {
    assert_hardware_blender(0x8411_4004, 0, [55, 119, 203], true);
}

#[test]
fn fog_blender_hardware_memory_rgb_dual() {
    assert_hardware_blender(0x44D1_4044, 0, [144, 62, 60], false);
}

#[test]
fn fog_blender_hardware_memory_rgb_fallback() {
    assert_hardware_blender(0x44D1_4044, 0, [144, 62, 60], true);
}

#[test]
fn fog_blender_second_cycle_consumes_first_result() {
    for fallback in [false, true] {
        assert_hardware_blender(
            gbl_c1(CLR_BL, A_FOG, CLR_IN, B_1MA)
                | gbl_c2(CLR_FOG, A_SHADE, CLR_IN, B_1MA)
                | FORCE_BL
                | G_ZS_PRIM,
            1,
            [136, 87, 113],
            fallback,
        );
    }
}

#[test]
fn fog_blender_blend_rgb_in_m() {
    for fallback in [false, true] {
        assert_hardware_blender(
            gbl_c1(CLR_IN, A_FOG, CLR_BL, B_1MA) | FORCE_BL | G_ZS_PRIM,
            0,
            [65, 129, 205],
            fallback,
        );
    }
}

#[test]
fn fog_blender_second_cycle_memory_does_not_apply_local_fog() {
    assert_hardware_blender(
        gbl_c1(CLR_IN, A_0, CLR_IN, B_1)
            | gbl_c2(CLR_FOG, A_FOG, CLR_MEM, B_1MA)
            | FORCE_BL
            | G_ZS_PRIM,
        1,
        [40, 104, 200],
        true,
    );
}

#[test]
fn fog_blender_copy_ignores_blend_rgb_input() {
    assert_hardware_blender(0x8411_4004, 2, [255, 255, 255], true);
}

#[test]
fn fog_blender_copy_ignores_memory_rgb_input() {
    assert_hardware_blender(0x44D1_4044, 2, [255, 255, 255], true);
}
