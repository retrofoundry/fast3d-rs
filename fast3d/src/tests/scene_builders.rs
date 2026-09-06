mod lighting;
pub(crate) use lighting::{chrome_icosphere, lights, lookat};
mod framebuffers;
pub(crate) use framebuffers::{fill_texrect, hud_over_3d, offscreen_then_sample, texrect};

use n64_gbi::{consts::*, encode::*, gu::*};

use super::dl_builder::{pack_texels, seg, Built, Command, DlBuilder, TexelFormat};
use super::fixtures::{Fixture, TextureSpec};

fn vertex([x, y, z]: [i16; 3], [s, t]: [i16; 2], [r, g, b, a]: [u8; 4]) -> VtxColored {
    VtxColored {
        x,
        y,
        z,
        flag: 0,
        s,
        t,
        r,
        g,
        b,
        a,
    }
}

fn viewport(b: &mut DlBuilder) -> u32 {
    b.viewport(Vp {
        vscale: [640, 480, 511, 0],
        vtrans: [640, 480, 511, 0],
    })
}

fn passthrough(selector: u32) -> CcPass {
    CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: selector,
    }
}

fn alpha(selector: u32) -> CcPass {
    CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: selector,
    }
}

fn combine(color: CcPass, alpha: CcPass) -> Command {
    gdp_set_combine_lerp(color, alpha, color, alpha)
}

fn modulate() -> CcPass {
    CcPass {
        a: 1,
        b: ZERO_C,
        c: 4,
        d: ZERO_C,
    }
}

fn quad(b: &mut DlBuilder, radius: i16, top_first: bool, textured: bool, color: [u8; 4]) -> u32 {
    let xy = if top_first {
        [
            [-radius, radius],
            [-radius, -radius],
            [radius, -radius],
            [radius, radius],
        ]
    } else {
        [
            [-radius, -radius],
            [radius, -radius],
            [radius, radius],
            [-radius, radius],
        ]
    };
    let uv = if top_first {
        [[0, 0], [0, 1024], [1024, 1024], [1024, 0]]
    } else {
        [[0, 0], [1024, 0], [1024, 1024], [0, 1024]]
    };
    let vertices: Vec<_> = xy
        .into_iter()
        .zip(uv)
        .map(|([x, y], st)| vertex([x, y, 0], if textured { st } else { [0; 2] }, color))
        .collect();
    b.vertices(&vertices)
}

fn setup(b: &mut DlBuilder, scale: f32, model: Mtx4, geometry: u32) -> Vec<Command> {
    let projection = b.matrix(gu_scale(scale, scale, scale));
    let model = b.matrix(model);
    let vp = viewport(b);
    vec![
        gsp_matrix(projection, true, true, false),
        gsp_matrix(model, false, true, false),
        gsp_viewport(vp),
        gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH | geometry),
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
    ]
}

fn texture_pixels(spec: TextureSpec) -> (usize, Vec<[u8; 4]>) {
    use TextureSpec::*;
    match spec {
        White(side) => (side as usize, vec![[255; 4]; (side * side) as usize]),
        OrangeBlue => (
            32,
            (0..1024)
                .map(|i| {
                    if i / 32 < 16 {
                        [200, 100, 50, 255]
                    } else {
                        [50, 100, 200, 255]
                    }
                })
                .collect(),
        ),
        Rgba16Quad | OpaqueQuad => (
            4,
            (0..16)
                .map(|i| {
                    let mut pixel = match (i / 8, i % 2) {
                        (0, 0) => [255, 0, 0, 255],
                        (0, _) => [0, 255, 0, 255],
                        (_, 0) => [0, 0, 255, 0],
                        _ => [255, 255, 0, 0],
                    };
                    if spec == OpaqueQuad {
                        pixel[3] = 255;
                    }
                    pixel
                })
                .collect(),
        ),
        Ci4Palette => {
            let rgb = [
                [255, 0, 0],
                [255, 128, 0],
                [255, 255, 0],
                [128, 255, 0],
                [0, 255, 0],
                [0, 255, 128],
                [0, 255, 255],
                [0, 128, 255],
                [0, 0, 255],
                [128, 0, 255],
                [255, 0, 255],
                [255, 0, 128],
                [255, 255, 128],
                [128, 255, 255],
                [255, 128, 255],
                [128, 128, 255],
            ];
            (
                32,
                (0..1024)
                    .map(|i| {
                        let cell = (i / 32 / 8) * 4 + i % 32 / 8;
                        let [r, g, b] = rgb[cell];
                        [r, g, b, if cell % 2 == 0 { 255 } else { 0 }]
                    })
                    .collect(),
            )
        }
        Orange | Blue => (
            32,
            vec![
                if spec == Orange {
                    [200, 100, 50, 255]
                } else {
                    [40, 60, 220, 255]
                };
                1024
            ],
        ),
        IntensityRamp => (
            4,
            (0..16)
                .map(|i| {
                    let v = (i / 4 * 85) as u8;
                    [v, v, v, 255]
                })
                .collect(),
        ),
        WrapQuad => (
            4,
            (0..16)
                .map(|i| match (i / 8, i % 4 / 2) {
                    (0, 0) => [255, 0, 0, 255],
                    (0, _) => [0, 255, 0, 255],
                    (_, 0) => [0, 0, 255, 255],
                    _ => [255, 255, 0, 255],
                })
                .collect(),
        ),
        Ci8Palette => (
            32,
            (0..1024)
                .map(|i| {
                    let row = i / 32;
                    let v = (row * 8) as u8;
                    [v, v, v, if row % 2 == 0 { 255 } else { 0 }]
                })
                .collect(),
        ),
        ThreeColorTlut => (3, vec![[0, 0, 0, 255], [255, 0, 0, 255], [0, 255, 0, 255]]),
        AlphaTexrect => (
            2,
            vec![
                [255, 0, 0, 255],
                [255, 0, 0, 255],
                [255, 0, 0, 0],
                [255, 0, 0, 0],
            ],
        ),
        Empty => (1, vec![]),
    }
}

fn load_texture(b: &mut DlBuilder, spec: TextureSpec, ci4: bool, side: u32) -> Vec<Command> {
    texture_block(
        b,
        spec,
        if ci4 {
            TexelFormat::Ci4
        } else {
            TexelFormat::Rgba16
        },
        [side, side],
        [0, side.ilog2()],
        true,
    )
}

fn texture_block(
    b: &mut DlBuilder,
    spec: TextureSpec,
    format: TexelFormat,
    [width, height]: [u32; 2],
    [mode, mask]: [u32; 2],
    enable: bool,
) -> Vec<Command> {
    let (pixel_width, pixels) = texture_pixels(spec);
    let packed = pack_texels(format, pixel_width, &pixels);
    let texels = b.bytes(8, &packed.texels);
    let (fmt, siz) = match format {
        TexelFormat::Rgba16 => (0, 2),
        TexelFormat::Ci4 => (2, 0),
        TexelFormat::Ci8 => (2, 1),
        TexelFormat::Ia16 => (3, 2),
        TexelFormat::Ia8 => (3, 1),
        TexelFormat::Ia4 => (3, 0),
        TexelFormat::I8 => (4, 1),
        TexelFormat::I4 => (4, 0),
    };
    let mut commands = Vec::new();
    if fmt == 2 {
        let palette = b.bytes(8, &packed.palette);
        commands.extend([
            gdp_set_texture_image(0, 2, 1, palette),
            gdp_load_sync(),
            gdp_load_tlut(7, (packed.palette.len() as u32 / 2 - 1) << 2),
            gdp_pipe_sync(),
        ]);
    }
    commands.extend(gdp_load_texture_block(
        fmt, siz, width, height, texels, mode, mask, mode, mask,
    ));
    if enable {
        commands.push(gsp_texture(0xffff, 0xffff, 0, 0, true));
    }
    commands
}

fn end(mut b: DlBuilder, mut commands: Vec<Command>) -> Built {
    commands.push(gsp_enddl());
    b.list("main", &commands);
    b.finish("main")
}

fn triangles(commands: &mut Vec<Command>, start: u8) {
    commands.extend([
        gsp_1triangle(start, start + 1, start + 2),
        gsp_1triangle(start, start + 2, start + 3),
    ]);
}

fn textured_material(commands: &mut Vec<Command>) {
    commands.extend([
        combine(modulate(), alpha(4)),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gdp_set_env_color(0x000000ff),
    ]);
}

fn rectangle(x: [i16; 2], y: [i16; 2], z: i16, uv: i16, color: [u8; 4]) -> [VtxColored; 4] {
    [[0, 0], [1, 0], [1, 1], [0, 1]]
        .map(|[i, j]| vertex([x[i], y[j], z], [i as i16 * uv, j as i16 * uv], color))
}

pub(crate) fn flat_color(f: &Fixture) -> Built {
    let translucent = f.name == "flat-color--translucent";
    let mut b = DlBuilder::new();
    let vertices = quad(
        &mut b,
        if translucent { 128 } else { 48 },
        !translucent,
        false,
        if f.name == "flat-color--vertex-colors" {
            [96, 128, 160, 144]
        } else {
            [255; 4]
        },
    );
    let mut commands = setup(
        &mut b,
        if translucent { 1. / 128. } else { 1. / 64. },
        gu_mtx_ident(),
        0,
    );
    if translucent {
        commands[5] = gdp_set_render_mode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2);
    }
    commands.extend([
        combine(passthrough(3), alpha(if translucent { 3 } else { 4 })),
        gdp_set_prim_color(0, 0, if translucent { 0xff000080 } else { 0x40c8ffff }),
        gsp_vertex(0, 4, vertices),
    ]);
    if translucent {
        commands.extend([gsp_1triangle(0, 1, 2), gsp_1triangle(0, 2, 3)]);
    } else {
        commands.push(gsp_2triangles(0, 1, 2, 0, 2, 3));
    }
    commands.push(gsp_enddl());
    b.list("main", &commands);
    b.finish("main")
}

pub(crate) fn textured_quad(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = quad(&mut b, 48, false, true, [255; 4]);
    let model = gu_rotate(f32::from_bits(f.time_bits) * 60., 0., 0., 1.);
    let mut commands = setup(&mut b, 1. / 128., model, 0);
    commands.push(combine(modulate(), alpha(4)));
    if f.name == "textured-quad--blend-color" {
        commands.push(gdp_set_blend_color(0x12345678));
    } else {
        commands.extend([
            gdp_set_prim_color(0, 0, 0xffffffff),
            gdp_set_env_color(0x000000ff),
        ]);
    }
    let side = if matches!(f.texture, TextureSpec::Rgba16Quad | TextureSpec::OpaqueQuad) {
        4
    } else {
        32
    };
    commands.extend(load_texture(&mut b, f.texture, false, side));
    commands.extend([
        gsp_vertex(0, 4, vertices),
        gsp_1triangle(0, 1, 2),
        gsp_1triangle(0, 2, 3),
        gsp_enddl(),
    ]);
    b.list("main", &commands);
    b.finish("main")
}

pub(crate) fn ci4_grid(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = quad(&mut b, 48, false, true, [255; 4]);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    commands.push(gdp_set_other_mode_h(14, 2, 0x8000));
    commands.push(if f.name == "ci4-grid--canary" {
        gdp_set_combine_lerp(
            passthrough(ZERO_C),
            alpha(ZERO_A),
            CcPass {
                a: 6,
                b: ZERO_C,
                c: 8,
                d: ZERO_C,
            },
            alpha(4),
        )
    } else {
        combine(modulate(), alpha(4))
    });
    commands.extend([
        gdp_set_prim_color(0, 0, 0xffffffff),
        gdp_set_env_color(0x000000ff),
    ]);
    commands.extend(load_texture(&mut b, f.texture, true, 32));
    commands.extend([
        gsp_vertex(0, 4, vertices),
        gsp_1triangle(0, 1, 2),
        gsp_1triangle(0, 2, 3),
        gsp_enddl(),
    ]);
    b.list("main", &commands);
    b.finish("main")
}

pub(crate) fn segmented_sub_dl(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = quad(&mut b, 32, true, true, [255; 4]);
    b.list(
        "quad",
        &[
            gsp_vertex(0, 4, vertices),
            gsp_2triangles(0, 1, 2, 0, 2, 3),
            gsp_enddl(),
        ],
    );
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), G_CULL_BACK);
    commands.extend([
        combine(modulate(), alpha(4)),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gdp_set_env_color(0x000000ff),
    ]);
    commands.extend(load_texture(&mut b, f.texture, false, 32));
    commands.push(b.segment(6, "quad"));
    for x in [-64., 64.] {
        let translation = b.matrix(gu_translate(x, 0., 0.));
        commands.extend([
            gsp_matrix(translation, false, false, true),
            gsp_displaylist(seg(6, 0)),
            gsp_popmatrix(1),
        ]);
    }
    commands.push(gsp_enddl());
    b.list("main", &commands);
    b.finish("main")
}

fn perspective(b: &mut DlBuilder, eye_z: f32, rotation: f32) -> Vec<Command> {
    let (projection, norm) = gu_perspective(45., 1.3333, 10., 1000., 1.);
    let projection = b.matrix(projection);
    let view = b.matrix(gu_look_at(0., 0., eye_z, 0., 0., 0., 0., 1., 0.));
    let model = b.matrix(gu_rotate(rotation, 0., 1., 0.));
    let vp = viewport(b);
    vec![
        gsp_matrix(projection, true, true, false),
        gsp_persp_normalize(norm),
        gsp_matrix(view, false, true, false),
        gsp_matrix(model, false, false, false),
        gsp_viewport(vp),
        gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER),
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2),
        combine(passthrough(4), alpha(4)),
    ]
}

fn faces(commands: &mut Vec<Command>, quads: &[[u8; 4]]) {
    commands.extend(
        quads
            .iter()
            .map(|&[a, b, c, d]| gsp_2triangles(a, b, c, a, c, d)),
    );
}

pub(crate) fn perspective_cube(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let positions = [
        [-30, -30, -30],
        [30, -30, -30],
        [30, 30, -30],
        [-30, 30, -30],
        [-30, -30, 30],
        [30, -30, 30],
        [30, 30, 30],
        [-30, 30, 30],
    ];
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [255, 0, 255, 255],
        [0, 255, 255, 255],
        [255; 4],
        [64, 64, 64, 255],
    ];
    let vertices = b.vertices(
        &positions
            .into_iter()
            .zip(colors)
            .map(|(p, c)| vertex(p, [0; 2], c))
            .collect::<Vec<_>>(),
    );
    let mut commands = perspective(&mut b, 150., f32::from_bits(f.time_bits) * 45.);
    commands.push(gsp_vertex(0, 8, vertices));
    faces(
        &mut commands,
        &[
            [0, 1, 2, 3],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [3, 2, 6, 7],
            [0, 3, 7, 4],
            [1, 2, 6, 5],
        ],
    );
    commands.push(gsp_enddl());
    b.list("main", &commands);
    b.finish("main")
}

pub(crate) fn morphcube(f: &Fixture) -> Built {
    let time = f32::from_bits(f.time_bits);
    let weight = (1. - time.cos()) / 2.;
    let mut cube = Vec::new();
    for (z, xs) in [(40, [-40, 0, 40]), (-40, [40, 0, -40])] {
        for x in xs {
            for y in [-40, 0, 40] {
                cube.push([x, y, z]);
            }
        }
    }
    for x in [40, -40] {
        for y in [-40, 0, 40] {
            cube.push([x, y, 0]);
        }
    }
    cube.extend([[0, 40, 0], [0, -40, 0]]);
    let vertices: Vec<_> = cube
        .into_iter()
        .map(|p| {
            let radius = match p.iter().filter(|&&v| v != 0).count() {
                3 => 23,
                2 => 28,
                _ => 40,
            };
            let sphere = p.map(|v: i16| v.signum() * radius);
            let pos = std::array::from_fn(|i| {
                (p[i] as f32 + (sphere[i] - p[i]) as f32 * weight).round() as i16
            });
            let [r, g, b] = p.map(|v| (150 + v * 7 / 4) as u8);
            vertex(pos, [0; 2], [r, g, b, 255])
        })
        .collect();
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&vertices);
    let mut commands = perspective(&mut b, 200., time * 30.);
    commands.push(gsp_vertex(0, 26, vertices));
    faces(
        &mut commands,
        &[
            [0, 3, 4, 1],
            [1, 4, 5, 2],
            [3, 6, 7, 4],
            [4, 7, 8, 5],
            [9, 12, 13, 10],
            [10, 13, 14, 11],
            [12, 15, 16, 13],
            [13, 16, 17, 14],
            [6, 18, 19, 7],
            [7, 19, 20, 8],
            [18, 9, 10, 19],
            [19, 10, 11, 20],
            [15, 21, 22, 16],
            [16, 22, 23, 17],
            [21, 0, 1, 22],
            [22, 1, 2, 23],
            [2, 5, 24, 23],
            [23, 24, 14, 17],
            [5, 8, 20, 24],
            [24, 20, 11, 14],
            [15, 12, 25, 21],
            [21, 25, 3, 0],
            [12, 9, 18, 25],
            [25, 18, 6, 3],
        ],
    );
    commands.push(gsp_enddl());
    b.list("main", &commands);
    b.finish("main")
}

pub(crate) fn texture_ramp(f: &Fixture) -> Built {
    let family = f.name.split("--").next().unwrap();
    let format = match family {
        "ci4-canary" => TexelFormat::Ci4,
        "ci8-canary" | "ci8-ramp" => TexelFormat::Ci8,
        "i4-ramp" => TexelFormat::I4,
        "i8-ramp" => TexelFormat::I8,
        "ia4-ramp" => TexelFormat::Ia4,
        "ia8-ramp" => TexelFormat::Ia8,
        "ia16-ramp" => TexelFormat::Ia16,
        _ => unreachable!(),
    };
    let side: u32 = if f.texture == TextureSpec::IntensityRamp {
        4
    } else {
        32
    };
    let mut b = DlBuilder::new();
    let vertices = quad(&mut b, 48, false, true, [255; 4]);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    if matches!(format, TexelFormat::Ci4 | TexelFormat::Ci8) {
        commands.push(gdp_set_other_mode_h(14, 2, 0x8000));
    }
    textured_material(&mut commands);
    if f.name.contains("canary") {
        let i = commands.len() - 3;
        commands[i] = gdp_set_combine_lerp(
            passthrough(ZERO_C),
            alpha(ZERO_A),
            CcPass {
                a: 6,
                b: ZERO_C,
                c: 8,
                d: ZERO_C,
            },
            alpha(4),
        );
    }
    commands.extend(texture_block(
        &mut b,
        f.texture,
        format,
        [side, side],
        [0, side.ilog2()],
        true,
    ));
    commands.push(gsp_vertex(0, 4, vertices));
    triangles(&mut commands, 0);
    end(b, commands)
}

pub(crate) fn repeat(f: &Fixture) -> Built {
    let small = f.source.starts_with("goldens::");
    let side: u32 = if small { 4 } else { 32 };
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&rectangle(
        [-48, 48],
        [-48, 48],
        0,
        if small { 256 } else { 2048 },
        [255; 4],
    ));
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    textured_material(&mut commands);
    commands.extend(texture_block(
        &mut b,
        f.texture,
        TexelFormat::Rgba16,
        [side, side],
        [
            u32::from(f.name.starts_with("mirror")),
            if small { 0 } else { 5 },
        ],
        true,
    ));
    commands.push(gsp_vertex(0, 4, vertices));
    triangles(&mut commands, 0);
    end(b, commands)
}

pub(crate) fn alpha_threshold(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut vertices = rectangle([-128, 128], [-128, 128], 0, 128, [255; 4]);
    vertices[0].a = 0;
    vertices[3].a = 0;
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    commands[5] = gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2);
    commands.extend([gdp_set_other_mode_l(0, 2, 1), gdp_set_blend_color(128)]);
    textured_material(&mut commands);
    commands.extend(load_texture(&mut b, f.texture, false, 4));
    commands.push(gsp_vertex(0, 4, vertices));
    triangles(&mut commands, 0);
    end(b, commands)
}

pub(crate) fn backface_culling(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(
        &[[0, 48, 0], [-48, -48, 0], [48, -48, 0]].map(|p| vertex(p, [0; 2], [80, 220, 100, 255])),
    );
    let mut commands = setup(
        &mut b,
        1. / 64.,
        gu_rotate(f32::from_bits(f.time_bits) * 45., 0., 1., 0.),
        G_CULL_BACK,
    );
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 3, vertices),
        gsp_1triangle(0, 1, 2),
    ]);
    end(b, commands)
}

pub(crate) fn matrix_stack(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(
        &[[0, 20, 0], [-20, -20, 0], [20, -20, 0]].map(|p| vertex(p, [0; 2], [255, 80, 80, 255])),
    );
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    commands.push(combine(passthrough(4), alpha(4)));
    for x in [-60., 0., 60.] {
        let matrix = b.matrix(gu_translate(x, 0., 0.));
        commands.extend([
            gsp_matrix(matrix, false, false, true),
            gsp_vertex(0, 3, vertices),
            gsp_1triangle(0, 1, 2),
            gsp_popmatrix(1),
        ]);
    }
    end(b, commands)
}

pub(crate) fn two_cycle_combiner(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut vertices = rectangle([-48, 48], [-48, 48], 0, 0, [255; 4]);
    for (v, color) in vertices.iter_mut().zip([
        [0, 0, 255, 255],
        [255; 4],
        [0, 255, 0, 255],
        [255, 0, 0, 255],
    ]) {
        *v = vertex([v.x, v.y, v.z], [0; 2], color);
    }
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 64., gu_mtx_ident(), G_ZBUFFER);
    commands[4] = gdp_set_cycle_type(1);
    commands[5] = gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2);
    commands.extend([
        gdp_set_combine_lerp(
            passthrough(4),
            alpha(4),
            CcPass {
                a: 0,
                b: ZERO_C,
                c: 3,
                d: ZERO_C,
            },
            alpha(0),
        ),
        gdp_set_prim_color(0, 0, 0xffa040ff),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
    ]);
    end(b, commands)
}

pub(crate) fn multi_material(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut vertices = Vec::new();
    for (x, uv, color) in [
        ([-128, -43], 128, [255; 4]),
        ([-43, 43], 0, [255; 4]),
        ([43, 128], 128, [255, 165, 0, 255]),
    ] {
        vertices.extend(rectangle(x, [-128, 128], 0, uv, color));
    }
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    textured_material(&mut commands);
    let load = load_texture(&mut b, f.texture, false, 4);
    commands.extend(&load);
    commands.push(gsp_vertex(0, 12, vertices));
    triangles(&mut commands, 0);
    commands.extend([
        gdp_set_render_mode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2),
        combine(passthrough(3), alpha(4)),
        gdp_set_prim_color(0, 0, 0x0000ff80),
    ]);
    triangles(&mut commands, 4);
    commands.extend([
        gdp_set_render_mode(G_RM_AA_ZB_TEX_EDGE, G_RM_AA_ZB_TEX_EDGE2),
        combine(modulate(), alpha(1)),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gdp_set_env_color(0x000000ff),
    ]);
    commands.extend(load);
    triangles(&mut commands, 8);
    end(b, commands)
}

pub(crate) fn onetri(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&[
        vertex([-48, 48, 0], [0; 2], [0, 255, 0, 255]),
        vertex([-48, -48, 0], [0; 2], [255, 0, 0, 255]),
        vertex([48, -48, 0], [0; 2], [0, 0, 255, 255]),
        vertex([48, 48, 0], [0; 2], [0, 0, 0, 255]),
    ]);
    let mut commands = setup(
        &mut b,
        1. / 128.,
        gu_rotate(f32::from_bits(f.time_bits) * 60., 0., 0., 1.),
        0,
    );
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
    ]);
    end(b, commands)
}

pub(crate) fn colored_triangle(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(&[
        vertex([-48, -48, 0], [0; 2], [255, 0, 0, 255]),
        vertex([48, -48, 0], [0; 2], [0, 255, 0, 255]),
        vertex([0, 48, 0], [0; 2], [0, 0, 255, 255]),
    ]);
    let mut commands = setup(&mut b, 1. / 64., gu_mtx_ident(), 0);
    if f.name.ends_with("missing-render-mode") {
        commands.truncate(4);
    } else {
        commands.insert(3, gsp_clear_geometrymode(G_LIGHTING | G_CULL_BACK));
    }
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 3, vertices),
        gsp_1triangle(0, 1, 2),
    ]);
    end(b, commands)
}

pub(crate) fn framebuffer_extent(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices = b.vertices(
        &[[-32, 32, 0], [-32, 0, 0], [0, 0, 0], [0, 32, 0]].map(|p| vertex(p, [0; 2], [255; 4])),
    );
    let mut commands = setup(&mut b, 1. / 64., gu_mtx_ident(), 0);
    commands.extend([
        combine(passthrough(3), alpha(4)),
        gdp_set_prim_color(0, 0, 0xffffffff),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
    ]);
    end(b, commands)
}

pub(crate) fn fogworld(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices: Vec<_> = [([-80, 10], [80, 10], 110), ([20, 100], [10, -80], 0)]
        .into_iter()
        .flat_map(|(x, y, z)| rectangle(x, y, z, 0, [200, 50, 50, 255]))
        .collect();
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), G_FOG | G_ZBUFFER);
    commands[4] = gdp_set_cycle_type(1);
    commands[5] = gdp_set_render_mode(G_RM_FOG_SHADE_A, G_RM_AA_ZB_OPA_SURF2);
    commands.extend([
        gdp_set_fog_color(0x808080ff),
        gsp_fog_position(500, 1000),
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 8, vertices),
    ]);
    triangles(&mut commands, 0);
    triangles(&mut commands, 4);
    end(b, commands)
}

pub(crate) fn tron(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let vertices: Vec<_> = [
        ([-128, 43], [0, 220, 255, 128]),
        ([-43, 128], [255, 0, 220, 128]),
    ]
    .into_iter()
    .flat_map(|(x, color)| rectangle(x, [-128, 128], 0, 0, color))
    .collect();
    let vertices = b.vertices(&vertices);
    let mut commands = setup(
        &mut b,
        1. / 128.,
        gu_rotate(f32::from_bits(f.time_bits) * 30., 0., 0., 1.),
        0,
    );
    commands[5] = gdp_set_render_mode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2);
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 8, vertices),
    ]);
    triangles(&mut commands, 0);
    triangles(&mut commands, 4);
    end(b, commands)
}

pub(crate) fn decal(f: &Fixture) -> Built {
    let tilted = f.name.ends_with("in-front") || f.name.ends_with("behind");
    let mut b = DlBuilder::new();
    let mut vertices = rectangle([-128, 128], [-128, 128], 0, 0, [40, 40, 200, 255]).to_vec();
    vertices.extend(rectangle(
        [-128, 128],
        [if tilted { -128 } else { 0 }, 128],
        0,
        0,
        [240, 220, 40, 255],
    ));
    if tilted {
        for (i, v) in vertices.iter_mut().enumerate() {
            v.z = if v.y < 0 { -96 } else { 96 };
            if i >= 4 {
                v.z += if f.name.ends_with("behind") { 2 } else { -2 };
            }
        }
    } else {
        vertices.extend(rectangle([0, 128], [0, 128], -48, 0, [220, 40, 40, 255]));
    }
    let count = vertices.len() as u8;
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), 0);
    commands.pop();
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, count, vertices),
        gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2),
    ]);
    triangles(&mut commands, 0);
    if !tilted {
        triangles(&mut commands, 8);
    }
    commands.push(gdp_set_render_mode(
        G_RM_AA_ZB_OPA_DECAL,
        G_RM_AA_ZB_OPA_DECAL2,
    ));
    triangles(&mut commands, 4);
    end(b, commands)
}

pub(crate) fn high_poly(_: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut vertices: Vec<_> = [128, 85, 43, 0, -43, -85, -128]
        .into_iter()
        .flat_map(|y| [0, 43, 85, 128].map(|x| vertex([x, y, 0], [0; 2], [0, 50, 200, 255])))
        .collect();
    vertices.extend(
        [[-128, 128, 0], [-70, 128, 0], [-128, 70, 0]].map(|p| vertex(p, [0; 2], [255, 0, 0, 255])),
    );
    let vertices = b.vertices(&vertices);
    let mut commands = setup(&mut b, 1. / 128., gu_mtx_ident(), G_ZBUFFER);
    commands[5] = gdp_set_render_mode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2);
    commands.push(combine(passthrough(4), alpha(4)));
    for _ in 0..5 {
        commands.push(gsp_vertex(0, 28, vertices));
        for row in 0..6 {
            for col in 0..3 {
                let i = row * 4 + col;
                commands.push(gsp_2triangles(i, i + 1, i + 4, i + 1, i + 5, i + 4));
            }
        }
    }
    commands.extend([gsp_vertex(0, 31, vertices), gsp_1triangle(28, 29, 30)]);
    end(b, commands)
}

pub(crate) fn three_color_tlut(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let commands = texture_block(&mut b, f.texture, TexelFormat::Ci8, [3, 1], [0, 0], false);
    end(b, commands)
}
