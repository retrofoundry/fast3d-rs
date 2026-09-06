use crate::capture::{Fixture, Provenance};
use crate::Microcode;
use n64_gbi::{consts::*, encode::*};

use super::dl_builder::{Built, Command, DlBuilder};

#[derive(Clone, Copy, Debug)]
enum Case {
    Rgba,
    St,
    Xy,
    Z,
    Quad,
}

const CASES: [Case; 5] = [Case::Rgba, Case::St, Case::Xy, Case::Z, Case::Quad];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const YELLOW: [u8; 4] = [255, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Rgba => "f3dex2-modify-rgba",
            Self::St => "f3dex2-modify-st",
            Self::Xy => "f3dex2-modify-xy",
            Self::Z => "f3dex2-modify-z",
            Self::Quad => "f3dex2-quad-winding",
        }
    }
}

fn vertex(x: i16, y: i16, z: i16, [r, g, b, a]: [u8; 4]) -> VtxColored {
    VtxColored {
        x: x - 160,
        y: 120 - y,
        z,
        flag: 0,
        s: 64,
        t: 64,
        r,
        g,
        b,
        a,
    }
}

fn rectangle(b: &mut DlBuilder, left: i16, right: i16, z: i16, color: [u8; 4]) -> u32 {
    b.vertices(
        &[(left, 40), (left, 112), (right, 112), (right, 40)].map(|(x, y)| vertex(x, y, z, color)),
    )
}

fn combine(selector: u32) -> Command {
    let color = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: selector,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: selector,
    };
    gdp_set_combine_lerp(color, alpha, color, alpha)
}

fn setup(b: &mut DlBuilder) -> Vec<Command> {
    let projection = b.matrix(n64_gbi::gu::gu_scale(1.0 / 128.0, 1.0 / 128.0, 1.0 / 128.0));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [512, 512, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    vec![
        gdp_set_depth_image(0x0020_0000),
        gdp_set_color_image(0, 2, 320, 0x0020_0000),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        gdp_set_cycle_type(3),
        gdp_set_fill_color(0xfffc_fffc),
        gdp_fill_rectangle(0, 0, 1276, 956),
        gdp_pipe_sync(),
        gdp_set_color_image(0, 2, 320, 0x0010_0000),
        gdp_set_fill_color(0x0001_0001),
        gdp_fill_rectangle(0, 0, 1276, 956),
        gdp_pipe_sync(),
        gsp_clear_geometrymode(u32::MAX),
        gsp_set_geometrymode(G_CLIPPING | G_SHADE | G_SHADING_SMOOTH),
        gsp_matrix(projection, true, true, false),
        gsp_matrix(model, false, true, false),
        gsp_viewport(viewport),
        gdp_set_cycle_type(0),
        gdp_set_other_mode_h(4, 2, 3 << 4),
        gdp_set_other_mode_h(6, 2, 3 << 6),
        gdp_set_other_mode_h(12, 2, 0),
        gdp_set_other_mode_l(0, 2, 0),
        gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2),
        combine(4),
    ]
}

fn scene(case: Case) -> Built {
    let mut b = DlBuilder::new();
    let mut dl = setup(&mut b);
    if matches!(case, Case::Quad) {
        for (top, cull) in [(32, 0), (104, G_CULL_BACK), (176, G_CULL_FRONT)] {
            let vertices = b.vertices(&[
                vertex(40, top, 0, RED),
                vertex(104, top, 0, RED),
                vertex(40, top + 48, 0, RED),
                vertex(176, top, 0, GREEN),
                vertex(176, top + 48, 0, GREEN),
                vertex(240, top, 0, GREEN),
            ]);
            dl.extend([
                gsp_clear_geometrymode(G_CULL_BOTH),
                gsp_set_geometrymode(cull),
                gsp_vertex(0, 6, vertices),
                (0x0700_0204, 0x0006_080a),
            ]);
        }
    } else {
        if matches!(case, Case::St) {
            let texels: Vec<_> = (0..32 * 8)
                .flat_map(|i| {
                    let texel: u16 = match i % 32 {
                        0..8 => 0xf801,
                        8..16 => 0x07c1,
                        _ => 0x003f,
                    };
                    texel.to_be_bytes()
                })
                .collect();
            let texture = b.bytes(8, &texels);
            dl.extend(gdp_load_texture_block(0, 2, 32, 8, texture, 2, 3, 2, 5));
            dl.extend([gsp_texture(32768, 32768, 0, 0, true), combine(1)]);
        }
        if matches!(case, Case::Z) {
            let background = rectangle(&mut b, 40, 248, 0, BLUE);
            dl.extend([
                gsp_set_geometrymode(G_ZBUFFER),
                gdp_set_render_mode(G_RM_OPA_SURF | Z_CMP | Z_UPD, G_RM_OPA_SURF2),
                gsp_vertex(0, 4, background),
                gsp_2triangles(0, 1, 2, 0, 2, 3),
            ]);
        }
        let vertices = rectangle(
            &mut b,
            40,
            if matches!(case, Case::Xy) { 112 } else { 248 },
            if matches!(case, Case::Z) { -32 } else { 0 },
            if matches!(case, Case::Xy) {
                YELLOW
            } else {
                RED
            },
        );
        dl.push(gsp_vertex(0, 4, vertices));
        if matches!(case, Case::Xy) {
            dl.push(gsp_2triangles(0, 1, 2, 0, 2, 3));
            for (slot, (x, y)) in [(176u32, 40u32), (176, 112), (248, 112), (248, 40)]
                .into_iter()
                .enumerate()
            {
                dl.push(gsp_modifyvertex(
                    slot as u16,
                    G_MWO_POINT_XYSCREEN,
                    (x * 4) << 16 | (y * 4),
                ));
            }
            dl.push(gsp_2triangles(0, 1, 2, 0, 2, 3));
        } else {
            for (strip, (left, right)) in
                [(40, 104), (104, 176), (176, 248)].into_iter().enumerate()
            {
                if strip != 0 {
                    let (attr, value) = match case {
                        Case::Rgba => (
                            G_MWO_POINT_RGBA,
                            if strip == 1 { 0x00ff_00ff } else { 0x0000_ffff },
                        ),
                        Case::St => (
                            G_MWO_POINT_ST,
                            if strip == 1 { 0x0180_0040 } else { 0x0300_0040 },
                        ),
                        Case::Z => (
                            G_MWO_POINT_ZSCREEN,
                            if strip == 1 { 0x0000_c000 } else { 0x0000_4000 },
                        ),
                        _ => unreachable!(),
                    };
                    for slot in 0..4 {
                        dl.push(gsp_modifyvertex(slot, attr, value));
                    }
                }
                dl.extend([
                    gdp_pipe_sync(),
                    gdp_set_scissor(0, left * 4, 0, right * 4, 960),
                    gsp_2triangles(0, 1, 2, 0, 2, 3),
                ]);
            }
        }
    }
    dl.extend([gdp_pipe_sync(), (0xe900_0000, 0), gsp_enddl()]);
    b.list("main", &dl);
    b.finish("main")
}

fn fixture(case: Case) -> Fixture {
    let built = scene(case);
    super::capture_fixture::make_image(built.rdram, built.entry, Microcode::F3dex2, 320, 240, Provenance {
        decomp_revision: "libultra gbi.h; authored library-contract PR 4".into(),
        source_symbols: "gsSPModifyVertex, gsSP2Triangles, gsSP1Quadrangle".into(),
        command_vector: match case {
            Case::Rgba => "RGBA 02100000/00FF00FF then 02100000/0000FFFF, slots 0..3; red/green/blue strips",
            Case::St => "ST 02140000/01800040 then 02140000/03000040, slots 0..3; half texture scale at load, unit scale after modify; red/green/blue strips",
            Case::Xy => "XY 02180000/02C000A0 for slot 0; translate [40,112)x[40,112) to [176,248)x[40,112)",
            Case::Z => "Z 021C0000/0000C000 then 021C0000/00004000, slots 0..3; red foreground moves behind blue then in front; red/blue/red strips",
            Case::Quad => "07000204/0006080A: independent triangles (0,1,2), (3,4,5), opposite winding; no cull, cull back, cull front",
        }.into(),
        synthetic_data: "IMAGE BE commands, fixed vertices/matrices, 320x240 RGBA16 color at 0x100000 and cleared depth at 0x200000; flat primary colors, point-sampled texture; no game capture".into(),
    })
}

fn expected(case: Case, x: u32, y: u32) -> [u8; 4] {
    match case {
        Case::Quad => {
            for (top, red, green) in [(32, true, true), (104, false, true), (176, true, false)] {
                if (top..top + 48).contains(&y) {
                    for (left, visible, color) in [(40, red, RED), (176, green, GREEN)] {
                        if visible
                            && (left..left + 64).contains(&x)
                            && 3 * (2 * (x - left) + 1) + 4 * (2 * (y - top) + 1) < 384
                        {
                            return color;
                        }
                    }
                }
            }
            BLACK
        }
        Case::Xy
            if (40..112).contains(&y) && ((40..112).contains(&x) || (176..248).contains(&x)) =>
        {
            YELLOW
        }
        Case::Rgba | Case::St | Case::Z if (40..112).contains(&y) && (40..248).contains(&x) => {
            if x < 104 {
                RED
            } else if x < 176 {
                if matches!(case, Case::Z) {
                    BLUE
                } else {
                    GREEN
                }
            } else if matches!(case, Case::Z) {
                RED
            } else {
                BLUE
            }
        }
        _ => BLACK,
    }
}

#[test]
fn f3dex2_fixtures_are_exportable_images() {
    for case in CASES {
        let fixture = fixture(case);
        assert_eq!(fixture.tasks[0].microcode, Microcode::F3dex2);
        let task = &fixture.tasks[0];
        assert_eq!(task.source.memory, crate::capture::MemoryLayout::IMAGE);
        assert!(task.entry.is_multiple_of(8));
        assert!(task.entry < 8 * 1024 * 1024);
        assert!(task
            .spans
            .iter()
            .all(|span| span.address + span.bytes.len() as u64 <= 8 * 1024 * 1024));
    }
}

#[test]
fn f3dex2_fixture_pixels() {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    for case in CASES {
        let output =
            pollster::block_on(fixture(case).replay(device.clone(), queue.clone())).unwrap();
        assert!(
            output.diagnostics.iter().all(Vec::is_empty),
            "{case:?}: {:?}",
            output.diagnostics
        );
        for (i, pixel) in output.rgba8.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i as u32 % 320, i as u32 / 320);
            assert_eq!(*pixel, expected(case, x, y), "{case:?}: ({x}, {y})");
        }
    }
}

fn write(case: Case) {
    let output_dir = std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES");
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::write(
        std::path::Path::new(&output_dir).join(format!("{}.f3dcap", case.name())),
        fixture(case).to_bytes().unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_f3dex2_modify_rgba_fixture() {
    write(Case::Rgba);
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_f3dex2_modify_st_fixture() {
    write(Case::St);
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_f3dex2_modify_xy_fixture() {
    write(Case::Xy);
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_f3dex2_modify_z_fixture() {
    write(Case::Z);
}

#[test]
#[ignore = "writes an RT64 oracle fixture to FAST3D_WRITE_FIXTURES"]
fn write_rt64_f3dex2_quad_winding_fixture() {
    write(Case::Quad);
}
