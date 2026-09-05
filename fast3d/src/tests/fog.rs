use crate::hle::gbi::GbiUcode;
use crate::hle::interp::interpret;
use crate::hle::mem::{GbiDataFormat, RdramImage};
use crate::hle::{Scene, SceneOp};
use crate::render::headless_device;
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
