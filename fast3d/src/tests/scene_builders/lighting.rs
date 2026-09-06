use super::*;
use crate::tests::dl_builder::Light;

pub(crate) fn lookat(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let commands = b.set_look_at(gu_look_at_reflect([0., 0., 100., 0., 0., 0., 0., 1., 0.]));
    end(b, commands.to_vec())
}

fn lit_setup(b: &mut DlBuilder, f: &Fixture, chrome: bool) -> Vec<Command> {
    let (projection, norm) = gu_perspective(45., 1.3333, 10., 1000., 1.);
    let projection = b.matrix(projection);
    let view = b.matrix(gu_look_at(
        0.,
        if chrome { 0. } else { 80. },
        if chrome { 200. } else { 230. },
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
    ));
    let model = b.matrix(gu_rotate(f32::from_bits(f.time_bits) * 30., 0., 1., 0.));
    let vp = viewport(b);
    let mut commands = vec![
        gsp_matrix(projection, true, true, false),
        gsp_persp_normalize(norm),
        gsp_matrix(view, false, true, false),
        gsp_matrix(model, false, false, false),
        gsp_viewport(vp),
        gsp_set_geometrymode(
            G_LIGHTING
                | G_SHADE
                | G_SHADING_SMOOTH
                | G_ZBUFFER
                | if chrome { G_TEXTURE_GEN } else { 0 },
        ),
    ];
    commands.extend(b.set_lights(
        [5; 3],
        &[
            Light {
                color: [100, 100, 0],
                direction: [-32, -64, -32],
            },
            Light {
                color: [50, 50, 0],
                direction: [15, 30, 120],
            },
        ],
    ));
    if chrome {
        commands.extend(b.set_look_at(gu_look_at_reflect([0., 0., 200., 0., 0., 0., 0., 1., 0.])));
    }
    let selector = if chrome { 1 } else { 4 };
    commands.extend([
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2),
        combine(passthrough(selector), alpha(selector)),
    ]);
    commands
}

pub(crate) fn chrome_icosphere(f: &Fixture) -> Built {
    let phi = (1. + 5f64.sqrt()) / 2.;
    let corners = [
        [-1., phi, 0.],
        [1., phi, 0.],
        [-1., -phi, 0.],
        [1., -phi, 0.],
        [0., -1., phi],
        [0., 1., phi],
        [0., -1., -phi],
        [0., 1., -phi],
        [phi, 0., -1.],
        [phi, 0., 1.],
        [-phi, 0., -1.],
        [-phi, 0., 1.],
    ];
    let faces = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let mut positions = Vec::new();
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for [a, b, c] in faces {
        let mut lattice = [[0u8; 4]; 4];
        for (row, indices) in lattice.iter_mut().enumerate() {
            for (col, index) in indices.iter_mut().enumerate().take(4 - row) {
                let p: [f64; 3] = std::array::from_fn(|axis| {
                    corners[a][axis] * (3 - row - col) as f64
                        + corners[b][axis] * row as f64
                        + corners[c][axis] * col as f64
                });
                let length = p.iter().map(|v| v * v).sum::<f64>().sqrt();
                let position = p.map(|v| (v / length * 76.5).round() as i16);
                *index = positions
                    .iter()
                    .position(|p| *p == position)
                    .unwrap_or_else(|| {
                        positions.push(position);
                        let [r, g, b] = p.map(|v| (v / length * 127.).round() as i8 as u8);
                        vertices.push(vertex(position, [0; 2], [r, g, b, 255]));
                        vertices.len() - 1
                    }) as u8;
            }
        }
        for row in 0..3 {
            for col in 0..3 - row {
                triangles.push([
                    lattice[row][col],
                    lattice[row + 1][col],
                    lattice[row][col + 1],
                ]);
                if col < 2 - row {
                    triangles.push([
                        lattice[row + 1][col],
                        lattice[row + 1][col + 1],
                        lattice[row][col + 1],
                    ]);
                }
            }
        }
    }
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&vertices);
    let mut commands = lit_setup(&mut b, f, true);
    commands.extend(load_texture(&mut b, f.texture, false, 32));
    commands.push(gsp_vertex(0, positions.len() as u8, vertices));
    for pair in triangles.chunks_exact(2) {
        let [a, b, c] = pair[0];
        let [d, e, f] = pair[1];
        commands.push(gsp_2triangles(a, b, c, d, e, f));
    }
    end(b, commands)
}

pub(crate) fn lights(f: &Fixture) -> Built {
    let mut vertices = Vec::new();
    for (radius, diagonal, y, axial, oblique, ny) in [
        (34, 24, -46, [-74, -14], [-43, -62], 102),
        (56, 40, -30, [-106, -6], [-71, -80], 69),
        (62, 44, -6, [-127, 0], [-90, -90], -5),
        (54, 38, 18, [-113, 5], [-84, -76], -58),
        (40, 28, 36, [-99, 7], [-75, -65], -79),
        (30, 21, 48, [-81, 6], [-62, -53], -97),
        (12, 8, 58, [-78, 9], [-61, -49], -100),
        (10, 7, 66, [-115, 7], [-86, -77], -53),
        (4, 3, 74, [-98, 19], [-83, -56], -78),
    ] {
        for quadrant in 0..4 {
            for (mut p, mut n) in [([radius, 0], axial), ([diagonal, diagonal], oblique)] {
                for _ in 0..quadrant {
                    p = [-p[1], p[0]];
                    n = [-n[1], n[0]];
                }
                vertices.push(vertex(
                    [p[0], y, p[1]],
                    [0; 2],
                    [n[0] as i8 as u8, ny as i8 as u8, n[1] as i8 as u8, 255],
                ));
            }
        }
    }
    vertices.push(vertex([0, 78, 0], [0; 2], [0, -127i8 as u8, 0, 255]));
    for [x, y, z, nx, ny, nz] in [
        [48, -13, 0, 48, -115, -24],
        [45, -3, -15, 20, -11, -125],
        [41, 14, -9, -11, 116, -51],
        [41, 14, 9, -5, 85, 94],
        [45, -3, 15, 32, -58, 109],
        [72, -4, 0, 71, -105, 7],
        [68, 4, -12, 32, -40, -116],
        [61, 17, -8, -28, 91, -84],
        [61, 17, 8, -26, 104, 68],
        [68, 4, 12, 36, -21, 120],
        [90, 16, 0, 106, -70, 9],
        [84, 20, -10, 47, -25, -115],
        [76, 27, -6, -62, 72, -84],
        [76, 27, 6, -66, 84, 68],
        [84, 20, 10, 40, -5, 120],
        [98, 37, 0, 119, -41, 19],
        [94, 39, -7, 61, -12, -111],
        [87, 43, -4, -69, 57, -90],
        [87, 43, 4, -90, 69, 58],
        [94, 39, 7, 30, 8, 123],
        [101, 48, 0, 123, 4, 33],
        [97, 49, -5, 81, 21, -96],
        [92, 52, -3, -45, 74, -93],
        [92, 52, 3, -82, 90, 38],
        [97, 49, 5, 23, 46, 116],
        [96, 50, 0, 47, 118, 0],
        [-41, 38, 0, -22, 123, -26],
        [-40, 32, -8, -3, 13, -126],
        [-39, 24, -5, 14, -117, -48],
        [-39, 24, 5, 11, -82, 96],
        [-40, 32, 8, -12, 68, 106],
        [-71, 33, 0, -77, 101, 3],
        [-67, 28, -8, -15, 47, -117],
        [-62, 21, -5, 57, -75, -85],
        [-62, 21, 5, 47, -91, 76],
        [-67, 28, 8, -38, 22, 119],
        [-82, 6, 0, -127, 2, -1],
        [-76, 6, -8, -47, 14, -117],
        [-68, 6, -5, 100, 7, -78],
        [-68, 6, 5, 99, -3, 80],
        [-76, 6, 8, -47, -12, 117],
        [-71, -20, 0, -89, -90, -2],
        [-68, -16, -8, -39, -21, -119],
        [-62, -9, -5, 61, 81, -76],
        [-62, -9, 5, 69, 69, 82],
        [-68, -16, 8, -22, -41, 118],
        [-47, -30, 0, -20, -123, 26],
        [-45, -24, -8, 0, -72, -105],
        [-42, -16, -5, 54, 65, -95],
        [-42, -16, 5, 65, 98, 47],
        [-45, -24, 8, 22, -22, 123],
        [-44, -22, 0, 119, -43, 0],
    ] {
        vertices.push(vertex(
            [x, y, z],
            [0; 2],
            [nx as i8 as u8, ny as i8 as u8, nz as i8 as u8, 255],
        ));
    }
    let mut triangles = Vec::new();
    for (start, rings, sides) in [(0, 9, 8), (73, 5, 5), (99, 5, 5)] {
        for row in 0..rings - 1 {
            for col in 0..sides {
                let a = start + row * sides + col;
                let b = start + row * sides + (col + 1) % sides;
                triangles.extend([[a, b, b + sides], [a, b + sides, a + sides]]);
            }
        }
        let top = start + (rings - 1) * sides;
        for col in 0..sides {
            triangles.push([top + col, top + (col + 1) % sides, top + sides]);
        }
    }
    let mut b = DlBuilder::new();
    let count = vertices.len() as u8;
    let vertices = b.vertices(&vertices);
    let mut commands = lit_setup(&mut b, f, false);
    commands.push(gsp_vertex(0, count, vertices));
    for pair in triangles.chunks_exact(2) {
        let [a, b, c] = pair[0];
        let [d, e, f] = pair[1];
        commands.push(gsp_2triangles(a, b, c, d, e, f));
    }
    end(b, commands)
}
