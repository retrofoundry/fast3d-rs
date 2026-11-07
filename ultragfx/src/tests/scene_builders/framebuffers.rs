use super::*;

fn target(address: u32) -> [Command; 2] {
    [
        gdp_set_color_image(0, 2, 64, address),
        gdp_set_scissor(0, 0, 0, 256, 256),
    ]
}

fn fill(color: u32) -> [Command; 3] {
    [
        gdp_set_cycle_type(3),
        gdp_set_fill_color(color),
        gdp_fill_rectangle(0, 0, 256, 256),
    ]
}

pub(crate) fn fill_texrect(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut commands = target(0x00100000).to_vec();
    let flip = f.name.starts_with("texrectflip");
    if !flip {
        commands.extend(fill(0x003f003f));
    }
    commands.push(gdp_set_cycle_type(2));
    commands.extend(texture_block(
        &mut b,
        f.texture,
        TexelFormat::Rgba16,
        [4, 4],
        [0, 2],
        false,
    ));
    commands.extend(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, flip,
    ));
    end(b, commands)
}

pub(crate) fn hud_over_3d(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut vertices = rectangle([-48, 48], [-48, 48], 0, 0, [255; 4]);
    for (v, color) in vertices.iter_mut().zip([
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ]) {
        *v = vertex([v.x, v.y, v.z], [0; 2], color);
    }
    let vertices = b.vertices(&vertices);
    let mut commands = target(0x00100000).to_vec();
    let mut geometry = setup(
        &mut b,
        1. / 128.,
        gu_rotate(f32::from_bits(f.time_bits) * 45., 0., 1., 0.),
        0,
    );
    let vp = b.viewport(Vp {
        vscale: [128, 128, 511, 0],
        vtrans: [128, 128, 511, 0],
    });
    geometry[2] = gsp_viewport(vp);
    commands.extend(geometry);
    commands.extend([
        combine(passthrough(4), alpha(4)),
        gsp_vertex(0, 4, vertices),
        gsp_2triangles(0, 1, 2, 0, 2, 3),
        gdp_set_cycle_type(2),
    ]);
    commands.extend(texture_block(
        &mut b,
        f.texture,
        TexelFormat::Rgba16,
        [4, 4],
        [0, 2],
        false,
    ));
    commands.extend(gsp_texture_rectangle(
        0, 0, 64, 64, 0, 0, 0, 1024, 1024, false,
    ));
    end(b, commands)
}

pub(crate) fn offscreen_then_sample(_: &Fixture) -> Built {
    let b = DlBuilder::new();
    let mut commands = target(0x00200000).to_vec();
    commands.extend(fill(0xfb81fb81));
    commands.extend(target(0x00100000));
    commands.extend([
        gdp_set_cycle_type(2),
        gdp_set_texture_image(0, 2, 64, 0x00200000),
        gdp_set_tile(0, 2, 16, 0, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 252, 252),
    ]);
    commands.extend(gsp_texture_rectangle(
        0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
    ));
    end(b, commands)
}

pub(crate) fn texrect(f: &Fixture) -> Built {
    let mut b = DlBuilder::new();
    let mut commands = target(0x00100000).to_vec();
    match f.name {
        "texrect--alpha-over-green" => {
            commands.extend(fill(0x07c107c1));
            commands.extend([
                gdp_set_cycle_type(0),
                gdp_set_render_mode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2),
                combine(passthrough(1), alpha(1)),
            ]);
            commands.extend(texture_block(
                &mut b,
                f.texture,
                TexelFormat::Rgba16,
                [2, 2],
                [0, 1],
                false,
            ));
            commands.extend(gsp_texture_rectangle(
                0, 0, 256, 256, 0, 0, 0, 1024, 1024, false,
            ));
        }
        "texrect--invalid-combiner" => {
            commands.extend([
                gdp_set_cycle_type(1),
                gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
                gdp_set_combine_lerp(
                    CcPass {
                        a: 7,
                        b: ZERO_C,
                        c: 4,
                        d: ZERO_C,
                    },
                    alpha(6),
                    passthrough(0),
                    alpha(6),
                ),
            ]);
            commands.extend(gsp_texture_rectangle(
                0, 0, 64, 64, 0, 0, 0, 1024, 1024, false,
            ));
            commands.push(combine(passthrough(3), alpha(6)));
            commands.extend(gsp_texture_rectangle(
                64, 64, 128, 128, 0, 0, 0, 1024, 1024, false,
            ));
        }
        "texrect--combiner-roles" => {
            commands.extend([
                gdp_set_cycle_type(1),
                gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
                combine(passthrough(3), alpha(6)),
            ]);
            commands.extend(gsp_texture_rectangle(0, 0, 252, 252, 0, 48, 0, 0, 0, false));
        }
        _ => unreachable!(),
    }
    end(b, commands)
}
