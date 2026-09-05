use crate::hle::gbi::GbiUcode;
use crate::hle::interp::interpret;
use crate::hle::math::identity;
use crate::hle::mem::{GbiDataFormat, RdramImage};
use crate::hle::Scene;
use crate::render::{headless_device, triangle_inv_tex_size};
use n64_gbi::encode::*;

use super::render::{render_scene_to_rgba8, run_compute_outputs};

const TEXEL_TOLERANCE: f64 = 1.0 / 1024.0;
// WARP's acos lands ~1.3e-3 texel off at the metal scale; the cubic it replaces is off by >0.2.
const ACOS_TEXEL_TOLERANCE: f64 = 1.0 / 256.0;

fn assert_uv(got: [f32; 2], expected: [f64; 2]) {
    assert_uv_within(got, expected, TEXEL_TOLERANCE);
}

fn assert_uv_within(got: [f32; 2], expected: [f64; 2], tolerance: f64) {
    for axis in 0..2 {
        assert!(
            (f64::from(got[axis]) - expected[axis]).abs() <= tolerance,
            "axis {axis}: expected {} texels, got {}",
            expected[axis],
            got[axis]
        );
    }
}

fn math_scene(mode: u32, dots: &[f32]) -> Scene {
    let n = dots.len();
    Scene {
        raw_pos: vec![[0.0; 3]; n],
        raw_st: vec![[0.0; 2]; n],
        cn: vec![0xff00_007f; n],
        mtx_index: vec![0; n],
        viewport_index: vec![0; n],
        texcoord_index: vec![0; n],
        light_index: vec![0; n],
        light_count: vec![0; n],
        texgen_mode: vec![mode; n],
        lookat_index: (0..n as u32).collect(),
        mvp_table: vec![identity()],
        viewport_table: vec![([160.0, 120.0, 0.0], [160.0, 120.0, 0.5])],
        texcoord_table: vec![[0.0; 2]],
        texgen_scale_table: vec![[0x0f80 as f32 / 65536.0, 0x07c0 as f32 / 65536.0]],
        lookat_table: dots
            .iter()
            .map(|&d| {
                let y = (1.0 - d * d).max(0.0).sqrt();
                ([d, y, 0.0], [-d, y, 0.0])
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn texgen_metal_scale_yields_62_by_31() {
    let dots = [-1.25, -1.0, -0.75, -0.5, 0.0, 0.5, 0.75, 1.0, 1.25];
    let scene = math_scene(1, &dots);
    let (device, queue, _) = headless_device();
    let output = run_compute_outputs(&device, &queue, &scene);
    for (vertex, d) in output.iter().zip(dots) {
        let d = f64::from(d).clamp(-1.0, 1.0);
        assert_uv(vertex.uv, [(d + 1.0) * 31.0, (1.0 - d) * 15.5]);
    }
    let captured = captured_state_scene();
    for (vertex, expected) in run_compute_outputs(&device, &queue, &captured).iter().zip([
        [62.0, 15.5],
        [31.0, 31.0],
        [62.0, 62.0],
    ]) {
        assert_uv(vertex.uv, expected);
    }
}

#[test]
fn texgen_linear_matches_acos() {
    let dots = [
        -1.25, -1.0, -0.875, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 0.875, 1.0, 1.25,
    ];
    let scene = math_scene(2, &dots);
    let (device, queue, _) = headless_device();
    for (vertex, d) in run_compute_outputs(&device, &queue, &scene)
        .iter()
        .zip(dots)
    {
        let d = f64::from(d).clamp(-1.0, 1.0);
        assert_uv_within(
            vertex.uv,
            [
                (-d).acos() * 62.0 / std::f64::consts::PI,
                d.acos() * 31.0 / std::f64::consts::PI,
            ],
            ACOS_TEXEL_TOLERANCE,
        );
    }
}

fn captured_state_scene() -> Scene {
    use crate::hle::consts::{G_LIGHTING, G_MTX_LOAD, G_TEXTURE_GEN};
    use crate::hle::rsp::Rsp;
    let mut bytes = mtx_to_bytes([
        [0.0, 1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .to_vec();
    bytes.extend_from_slice(
        &VtxColored {
            x: 0,
            y: 0,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 127,
            g: 0,
            b: 0,
            a: 255,
        }
        .to_bytes(),
    );
    bytes.resize(112, 0);
    bytes[88] = 127;
    bytes[105] = 127;
    let mem = RdramImage::new(&bytes);
    let mut rsp = Rsp::default();
    let mut rdp = crate::hle::rdp::Rdp::default();
    let mut scene = Scene::default();
    rsp.modify_geometry_mode(!0, G_LIGHTING | G_TEXTURE_GEN);
    rsp.set_lookat(&mem, 0, 80);
    rsp.set_lookat(&mem, 1, 96);
    rsp.set_texture(0, 0, true, 0x0f80, 0x07c0);
    rsp.set_vertex(&mem, 64, 1, 0, &rdp, &mut scene);
    rsp.matrix(&mem, 0, G_MTX_LOAD);
    rsp.set_vertex(&mem, 64, 1, 1, &rdp, &mut scene);
    rsp.set_lookat(&mem, 0, 96);
    rsp.set_texture(0, 0, true, 0x0f80, 0x0f80);
    rdp.tiles[0].width = 64;
    rdp.tiles[0].height = 32;
    rdp.tiles[0].uls = 12;
    rdp.tiles[0].ult = 20;
    rsp.set_vertex(&mem, 64, 1, 2, &rdp, &mut scene);
    rsp.finish(&mut scene);
    scene
}

#[test]
fn texgen_captures_scale_and_rotated_lookat_at_load() {
    let scene = captured_state_scene();
    let axes: Vec<_> = scene
        .lookat_index
        .iter()
        .map(|&i| scene.lookat_table[i as usize])
        .collect();
    assert_eq!(
        axes,
        [
            ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
            ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        ]
    );
    let scales: Vec<_> = scene
        .texcoord_index
        .iter()
        .map(|&i| scene.texgen_scale_table[i as usize])
        .collect();
    assert_eq!(
        scales,
        [
            [31.0 / 512.0, 31.0 / 1024.0],
            [31.0 / 512.0, 31.0 / 1024.0],
            [31.0 / 512.0; 2]
        ]
    );
}

fn write_commands(bytes: &mut [u8], offset: usize, commands: &[(u32, u32)]) {
    for (dest, &(w0, w1)) in bytes[offset..]
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(commands)
    {
        dest[..4].copy_from_slice(&w0.to_be_bytes());
        dest[4..].copy_from_slice(&w1.to_be_bytes());
    }
}

fn interpret_commands(mut bytes: Vec<u8>, commands: &[(u32, u32)]) -> Scene {
    bytes.resize(0x6000 + commands.len() * 8, 0);
    write_commands(&mut bytes, 0x6000, commands);
    interpret_memory(bytes, 0x6000)
}

fn interpret_memory(bytes: Vec<u8>, entry: u32) -> Scene {
    let mut mem = RdramImage::new(&bytes);
    mem.set_segment(4, 0);
    let result = interpret(mem, entry.into(), GbiUcode::F3d, GbiDataFormat::Fixed);
    assert!(result.diags.is_empty(), "{:?}", result.diags);
    result.scene
}

fn coordinate_texture(bytes: &mut [u8]) {
    for t in 0..32u16 {
        for s in 0..64u16 {
            let texel = ((s >> 1) << 11) | (t << 6) | (((s & 1) * 31) << 1) | 1;
            let offset = 0x90 + usize::from(t * 64 + s) * 2;
            bytes[offset..offset + 2].copy_from_slice(&texel.to_be_bytes());
        }
    }
}

fn assert_pixel(got: &[u8], expected: [u8; 4]) {
    for (channel, (&got, expected)) in got.iter().zip(expected).enumerate() {
        assert!(
            got.abs_diff(expected) <= 1,
            "channel {channel}: expected {expected}, got {got}"
        );
    }
}

fn prologue() -> Vec<(u32, u32)> {
    vec![
        gsp_matrix_f3d(0, true, true, false),
        gsp_set_geometrymode_f3d(0x0002_0204),
        gsp_lookat_f3d(0, 0x1100),
        gsp_lookat_f3d(1, 0x1110),
        gdp_set_cycle_type_f3d(0),
        gdp_set_render_mode_f3d(
            crate::hle::consts::G_RM_OPA_SURF,
            crate::hle::consts::G_RM_OPA_SURF2,
        ),
        (0xfb00_0000, 0x0000_00ff),
    ]
}

fn fixture_memory() -> Vec<u8> {
    let mut bytes = vec![0; 0x6000];
    let mut projection = identity();
    projection[0][0] = 1.0 / 128.0;
    projection[1][1] = 1.0 / 128.0;
    bytes[..64].copy_from_slice(&mtx_to_bytes(projection));
    bytes[0x1108] = 127;
    bytes[0x1119] = 127;
    bytes[0x1120..0x1140].fill(127);
    coordinate_texture(&mut bytes);
    bytes
}

fn mixed_scene(texgen_first: bool) -> Scene {
    let mut bytes = fixture_memory();
    let positions = [(-96, -64), (96, -64), (96, 64), (-96, 64)];
    for (i, (x, y)) in positions.into_iter().enumerate() {
        bytes[0x2000 + i * 16..0x2010 + i * 16].copy_from_slice(
            &VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 1984,
                t: 992,
                r: 0,
                g: 0,
                b: 127,
                a: 255,
            }
            .to_bytes(),
        );
    }
    let mut commands = prologue();
    commands.extend([
        (0xfcff_ffff, 0xfffc_fa7d),
        gsp_texture_f3d(0x0f80, 0x07c0, 0, 0, true),
        gsp_set_geometrymode_f3d(0x0004_0000),
        gsp_vertex_f3d(0, 2, 0x2000),
        gsp_clear_geometrymode_f3d(0x0004_0000),
        gsp_texture_f3d(0x8000, 0x8000, 0, 0, true),
        gsp_vertex_f3d(2, 2, 0x2020),
    ]);
    // sm64 can load vertices before setting the render tile's dimensions.
    commands.extend(gdp_load_texture_block(0, 2, 64, 32, 0x90, 0, 5, 0, 6));
    commands.extend(if texgen_first {
        [gsp_1triangle_f3d(0, 1, 2), gsp_1triangle_f3d(2, 3, 0)]
    } else {
        [gsp_1triangle_f3d(2, 3, 0), gsp_1triangle_f3d(0, 1, 2)]
    });
    commands.push(gsp_enddl_f3d());
    interpret_commands(bytes, &commands)
}

#[test]
fn texgen_normalizes_by_draw_time_tile_size() {
    let scene = mixed_scene(true);
    let run = &scene.draw_runs[0];
    let mut mat = scene.materials[run.material_index as usize].clone();
    assert_eq!((mat.tex_w, mat.tex_h), (64, 32));
    assert_eq!(
        triangle_inv_tex_size(&mat),
        [1.0 / 64.0, 1.0 / 32.0, 0.0, 0.0]
    );
    mat.tex_w = 32;
    mat.tex_h = 16;
    assert_eq!(
        triangle_inv_tex_size(&mat),
        [1.0 / 32.0, 1.0 / 16.0, 0.0, 0.0]
    );
}

#[test]
fn texgen_mixed_vertices_share_texel_units() {
    for texgen_first in [true, false] {
        let scene = mixed_scene(texgen_first);
        assert_eq!(scene.draw_runs.len(), 1);
        assert_eq!(scene.draw_runs[0].index_count, 6);
        let (device, queue, _) = headless_device();
        for vertex in run_compute_outputs(&device, &queue, &scene) {
            assert_uv(vertex.uv, [31.0, 15.5]);
        }
        let pixels = render_scene_to_rgba8(&scene, 256, 256);
        for (x, y) in [(64, 96), (192, 96), (64, 160), (192, 160)] {
            let offset = (y * 256 + x) * 4;
            assert_pixel(&pixels[offset..offset + 4], [123, 123, 255, 255]);
        }
    }
}

// n64decomp/sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422, actors/mario/model.inc.c.
// mario_metal_butt: PipeSync, SetGeometryMode(G_TEXTURE_GEN),
// SetCombineMode(G_CC_DECALFADE), LoadTextureBlock(RGBA16, 64x32, wrap, masks 6/5),
// Texture(0x0F80, 0x07C0), Light(1), Light(2), DisplayList(mario_butt_dl), EndDisplayList.
// Addresses below select synthetic payloads; counts, indices and list nesting are preserved.
const MARIO_METAL_BUTT: &[(u32, u32)] = &[
    (0xe700_0000, 0x0000_0000),
    (0xb700_0000, 0x0004_0000),
    (0xfcff_ffff, 0xfffc_fa7d),
    (0xfd10_0000, 0x0400_0090),
    (0xf510_0000, 0x0701_4060),
    (0xe600_0000, 0x0000_0000),
    (0xf300_0000, 0x077f_f080),
    (0xe700_0000, 0x0000_0000),
    (0xf510_2000, 0x0001_4060),
    (0xf200_0000, 0x000f_c07c),
    (0xbb00_0001, 0x0f80_07c0),
    (0x0386_0000, 0x0400_1120),
    (0x0388_0000, 0x0400_1130),
    (0x0600_0000, 0x0400_4000),
    (0xb800_0000, 0x0000_0000),
];

const MARIO_BUTT_DL: &[(u32, u32)] = &[
    (0x04e0_0000, 0x0400_2000),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x003c_4650),
    (0xbf00_0000, 0x005a_3264),
    (0xbf00_0000, 0x0028_6e64),
    (0xbf00_0000, 0x0078_828c),
    (0x04d0_0000, 0x0400_2100),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x003c_4650),
    (0xbf00_0000, 0x005a_646e),
    (0xbf00_0000, 0x0078_1e82),
    (0xbf00_0000, 0x003c_5078),
    (0x04f0_0000, 0x0400_2200),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x003c_0a46),
    (0xbf00_0000, 0x0050_5a64),
    (0xbf00_0000, 0x006e_5a78),
    (0xbf00_0000, 0x0082_8c96),
    (0x04e0_0000, 0x0400_2300),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x0028_3c32),
    (0xbf00_0000, 0x0046_5014),
    (0xbf00_0000, 0x005a_4664),
    (0xbf00_0000, 0x006e_7846),
    (0xbf00_0000, 0x0082_6e8c),
    (0x04e0_0000, 0x0400_2400),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_0a00),
    (0xbf00_0000, 0x0028_323c),
    (0xbf00_0000, 0x0014_4650),
    (0xbf00_0000, 0x005a_646e),
    (0xbf00_0000, 0x0078_828c),
    (0x04e0_0000, 0x0400_2500),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x003c_4650),
    (0xbf00_0000, 0x005a_6446),
    (0xbf00_0000, 0x006e_645a),
    (0xbf00_0000, 0x0078_828c),
    (0xbf00_0000, 0x0000_8278),
    (0x04e0_0000, 0x0400_2600),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_140a),
    (0xbf00_0000, 0x0028_323c),
    (0xbf00_0000, 0x0046_141e),
    (0xbf00_0000, 0x0050_5a64),
    (0xbf00_0000, 0x005a_506e),
    (0xbf00_0000, 0x006e_7882),
    (0xbf00_0000, 0x0082_8c0a),
    (0x04f0_0000, 0x0400_2700),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x003c_4650),
    (0xbf00_0000, 0x005a_6432),
    (0xbf00_0000, 0x0028_6e78),
    (0xbf00_0000, 0x003c_6e82),
    (0xbf00_0000, 0x0082_1e8c),
    (0xbf00_0000, 0x0096_140a),
    (0x04f0_0000, 0x0400_2800),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x001e_2832),
    (0xbf00_0000, 0x000a_003c),
    (0xbf00_0000, 0x0046_505a),
    (0xbf00_0000, 0x0064_6e78),
    (0xbf00_0000, 0x0000_828c),
    (0xbf00_0000, 0x0096_008c),
    (0x04d0_0000, 0x0400_2900),
    (0xbf00_0000, 0x0000_0a14),
    (0xbf00_0000, 0x0014_1e00),
    (0xbf00_0000, 0x001e_2800),
    (0xbf00_0000, 0x0000_2832),
    (0xbf00_0000, 0x0032_3c46),
    (0xbf00_0000, 0x0032_4600),
    (0xbf00_0000, 0x0050_5a64),
    (0xbf00_0000, 0x005a_6e64),
    (0xbf00_0000, 0x005a_0a6e),
    (0xbf00_0000, 0x006e_7864),
    (0xbf00_0000, 0x0064_8250),
    (0xb800_0000, 0x0000_0000),
];

const PATCH_NORMALS: [(i8, i8); 10] = [
    (-127, -127),
    (0, -127),
    (127, -127),
    (-127, 0),
    (0, 0),
    (127, 0),
    (-127, 127),
    (0, 127),
    (127, 127),
    (64, -32),
];

fn metal_butt_memory() -> (Vec<u8>, u32) {
    let mut bytes = fixture_memory();
    for (group, ((nx, ny), count)) in PATCH_NORMALS
        .into_iter()
        .zip([15, 14, 16, 15, 15, 15, 15, 16, 16, 14])
        .enumerate()
    {
        let left = -112 + (group % 5) as i16 * 48;
        let bottom = 48 - (group / 5) as i16 * 96;
        let corners = [
            (left, bottom),
            (left + 32, bottom),
            (left + 32, bottom + 32),
            (left, bottom),
            (left + 32, bottom + 32),
            (left, bottom + 32),
        ];
        for i in 0..count {
            let (x, y) = corners.get(i).copied().unwrap_or((left, bottom));
            let offset = 0x2000 + group * 0x100 + i * 16;
            bytes[offset..offset + 16].copy_from_slice(
                &VtxColored {
                    x,
                    y,
                    z: 0,
                    flag: 0,
                    s: 0,
                    t: 0,
                    r: nx as u8,
                    g: ny as u8,
                    b: 0,
                    a: 255,
                }
                .to_bytes(),
            );
        }
    }
    write_commands(&mut bytes, 0x4000, MARIO_BUTT_DL);
    write_commands(&mut bytes, 0x5000, MARIO_METAL_BUTT);
    let mut commands = prologue();
    commands.extend([gsp_displaylist_f3d(0x0400_5000), gsp_enddl_f3d()]);
    bytes.resize(0x6000 + commands.len() * 8, 0);
    write_commands(&mut bytes, 0x6000, &commands);
    (bytes, 0x6000)
}

fn metal_butt_scene() -> Scene {
    let (bytes, entry) = metal_butt_memory();
    interpret_memory(bytes, entry)
}

#[test]
fn fixture_sm64_mario_metal_butt_state() {
    let scene = metal_butt_scene();
    assert_eq!(scene.raw_pos.len(), 151);
    assert_eq!(scene.indices.len() / 3, 72);
    assert!(scene.texgen_mode.iter().all(|&mode| mode == 1));
    assert_eq!(scene.draw_runs.len(), 1);
    let mat = &scene.materials[scene.draw_runs[0].material_index as usize];
    assert!(mat.tex_enable);
    assert_eq!((mat.tex_w, mat.tex_h), (64, 32));
    for &index in &scene.texcoord_index {
        assert_eq!(
            scene.texgen_scale_table[index as usize],
            [31.0 / 512.0, 31.0 / 1024.0]
        );
    }
}

#[test]
fn fixture_sm64_mario_metal_butt() {
    let scene = metal_butt_scene();
    let pixels = render_scene_to_rgba8(&scene, 256, 256);
    let expected_pixels = [
        [0, 0, 0, 255],
        [123, 0, 255, 255],
        [255, 0, 0, 255],
        [0, 123, 0, 255],
        [123, 123, 255, 255],
        [255, 123, 0, 255],
        [0, 255, 0, 255],
        [123, 255, 255, 255],
        [255, 255, 0, 255],
        [189, 90, 0, 255],
    ];
    for (group, expected) in expected_pixels.into_iter().enumerate() {
        let x = 32 + (group % 5) * 48;
        let y = 64 + (group / 5) * 96;
        for (dx, dy) in [(-7, -5), (5, -7), (-5, 7), (7, 5)] {
            let offset = (((y as isize + dy) as usize * 256) + (x as isize + dx) as usize) * 4;
            assert_pixel(&pixels[offset..offset + 4], expected);
        }
    }
}

#[cfg(feature = "capture")]
#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_mario_metal_butt_fixture() {
    let (bytes, scene_entry) = metal_butt_memory();
    super::capture_fixture::write(
        bytes,
        scene_entry,
        320,
        240,
        "mario-metal-butt.f3dcap",
        crate::capture::Provenance {
            decomp_revision: "sm64 1372ae1bb7cbedc03df366393188f4f05dcfc422".into(),
            source_symbols: "actors/mario/model.inc.c: mario_metal_butt, mario_butt_dl"
                .into(),
            command_vector: "SM64 display-list commands with capture-only framebuffer wrapper"
                .into(),
            synthetic_data: "Synthetic vertices, normals, lights, look-at vectors, matrix, and coordinate texture"
                .into(),
        },
    );
}
