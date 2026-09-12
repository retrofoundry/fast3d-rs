use super::coverage_semantics::{cases, Case, Kind, RdpEdges, Rule};
use super::dl_builder::{Built, Command, DlBuilder};
use crate::capture::{Fixture, Provenance};
use crate::Microcode;
use n64_gbi::{consts::*, encode::*};

fn scene(case: &Case) -> Built {
    let mut b = DlBuilder::new();
    let mut projection = n64_gbi::gu::gu_scale(1.0 / 512.0, 1.0 / 512.0, 1.0 / 128.0);
    if matches!(
        case.kind,
        Kind::Perspective | Kind::PerspectiveNegative | Kind::Lod
    ) {
        projection[2][2] = 0.0;
        projection[2][3] = 1.0 / 128.0;
    }
    let projection = b.matrix(projection);
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [512, 512, 511, 0],
        vtrans: [(case.width * 2) as i16, (case.height * 2) as i16, 511, 0],
    });
    let matrix = if case.f3d { gsp_matrix_f3d } else { gsp_matrix };
    let cycle = if case.f3d {
        gdp_set_cycle_type_f3d
    } else {
        gdp_set_cycle_type
    };
    let mode = if case.f3d {
        gdp_set_render_mode_f3d
    } else {
        gdp_set_render_mode
    };
    let high = if case.f3d {
        gsp_setothermode_h_f3d
    } else {
        gdp_set_other_mode_h
    };
    let low = if case.f3d {
        gsp_setothermode_l_f3d
    } else {
        gdp_set_other_mode_l
    };
    let clear = if case.f3d {
        gsp_clear_geometrymode_f3d
    } else {
        gsp_clear_geometrymode
    };
    let set = if case.f3d {
        gsp_set_geometrymode_f3d
    } else {
        gsp_set_geometrymode
    };
    let vertex = if case.f3d { gsp_vertex_f3d } else { gsp_vertex };
    let triangle = if case.f3d {
        gsp_1triangle_f3d
    } else {
        gsp_1triangle
    };
    let color = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    let mut dl = vec![
        gdp_set_depth_image(0x0020_0000),
        gdp_set_color_image(0, 2, case.width, 0x0020_0000),
        gdp_set_scissor(0, 0, 0, case.width * 4, case.height * 4),
        cycle(3),
        gdp_set_fill_color(0xfffc_fffc),
        gdp_fill_rectangle(0, 0, (case.width - 1) * 4, (case.height - 1) * 4),
        gdp_pipe_sync(),
        gdp_set_color_image(
            0,
            if case.width % 2 == 1 { 3 } else { 2 },
            case.width,
            0x0010_0000,
        ),
        gdp_set_fill_color(if case.width % 2 == 1 {
            0x0000_00ff
        } else {
            0x0001_0001
        }),
        gdp_fill_rectangle(0, 0, (case.width - 1) * 4, (case.height - 1) * 4),
        gdp_pipe_sync(),
        clear(u32::MAX),
        set(if case.f3d {
            0x0080_0204
        } else {
            G_CLIPPING | G_SHADE | G_SHADING_SMOOTH
        }),
        matrix(projection, true, true, false),
        matrix(model, false, true, false),
        if case.f3d {
            gsp_viewport_f3d(viewport)
        } else {
            gsp_viewport(viewport)
        },
        cycle(0),
        high(4, 2, 3 << 4),
        high(6, 2, 3 << 6),
        high(12, 2, 0),
        low(0, 2, 0),
        mode(
            G_RM_OPA_SURF | if case.aa { AA_EN } else { 0 },
            G_RM_OPA_SURF2,
        ),
        gdp_set_combine_lerp(color, alpha, color, alpha),
        gdp_set_scissor(
            0,
            case.scissor[0] * 4,
            case.scissor[1] * 4,
            case.scissor[2] * 4,
            case.scissor[3] * 4,
        ),
    ];
    if matches!(
        case.kind,
        Kind::Perspective | Kind::PerspectiveNegative | Kind::SharedUv
    ) {
        let texture: Vec<_> = (0..32 * 32)
            .flat_map(|i| {
                let color = [0xf801u16, 0x07c1, 0x003f][(i % 32 / 4 + 2 * (i / 32 / 4)) % 3];
                color.to_be_bytes()
            })
            .collect();
        let address = b.bytes(8, &texture);
        dl.extend(gdp_load_texture_block(0, 2, 32, 32, address, 2, 5, 2, 5));
        let tex = CcPass { d: 1, ..color };
        dl.extend([
            gsp_texture(32768, 32768, 0, 0, true),
            high(19, 1, 1 << 19),
            gdp_set_combine_lerp(tex, alpha, tex, alpha),
        ]);
    }
    if case.kind == Kind::Lod {
        for level in 0..2u32 {
            let texture: Vec<_> = (0..32 * 32)
                .flat_map(|i| {
                    [0xf801u16, 0x07c1, 0x003f][((i % 32 / 4 + level) % 3) as usize].to_be_bytes()
                })
                .collect();
            let address = b.bytes(8, &texture);
            dl.extend([
                gdp_set_texture_image(0, 2, 1, address),
                gdp_set_tile(0, 2, 0, level * 256, 7, 0, 0, 0, 0, 0, 0, 0),
                gdp_load_sync(),
                gdp_load_block(7, 0, 0, 1023, 256),
                gdp_pipe_sync(),
                gdp_set_tile(0, 2, 8, level * 256, level, 0, 0, 5, 0, 0, 5, 0),
                gdp_set_tile_size(level, 0, 0, 124, 124),
            ]);
        }
        let tex = CcPass { d: 1, ..color };
        dl.extend([
            gsp_texture(32768, 32768, 1, 0, true),
            high(19, 1, 1 << 19),
            high(16, 1, 1 << 16),
            gdp_set_combine_lerp(tex, alpha, tex, alpha),
        ]);
    }
    if case.kind == Kind::Rects {
        dl.extend([
            gdp_pipe_sync(),
            cycle(3),
            gdp_set_fill_color(0xf801_f801),
            gdp_fill_rectangle(32, 32, 156, 156),
            gdp_pipe_sync(),
        ]);
        for (cyc, texel, bounds) in [
            (0, 0x07c1u16, [224, 32, 352, 160]),
            (2, 0x003fu16, [416, 32, 540, 156]),
            (1, 0xf801u16, [609, 33, 737, 161]),
        ] {
            let address = b.bytes(8, &texel.to_be_bytes().repeat(32 * 32));
            dl.extend(gdp_load_texture_block(0, 2, 32, 32, address, 2, 5, 2, 5));
            let tex = CcPass { d: 1, ..color };
            let tex_alpha = CcPass { d: 1, ..alpha };
            dl.extend([
                gdp_set_tile(0, 2, 8, 0, 1, 0, 2, 5, 0, 2, 5, 0),
                gdp_set_tile_size(1, 0, 0, 124, 124),
                cycle(cyc),
                gdp_set_combine_lerp(tex, tex_alpha, tex, tex_alpha),
            ]);
            dl.extend(gsp_texture_rectangle(
                bounds[0],
                bounds[1],
                bounds[2],
                bounds[3],
                0,
                0,
                0,
                if cyc == 2 { 4096 } else { 1024 },
                1024,
                false,
            ));
            dl.push(gdp_pipe_sync());
        }
        dl.extend([cycle(0), gdp_set_combine_lerp(color, alpha, color, alpha)]);
    }
    for (draw_index, draw) in case.draws.iter().enumerate() {
        if case.kind == Kind::SharedAlpha {
            dl.push(mode(G_RM_CLD_SURF, G_RM_CLD_SURF2));
        }
        if case.kind == Kind::Dither {
            dl.push(low(0, 2, 3));
        }
        if case.kind == Kind::Alpha {
            dl.extend([low(0, 2, 1), gdp_set_blend_color(128)]);
        }
        if case.kind == Kind::Fog {
            let one = CcPass { d: 6, ..alpha };
            dl.extend([
                set(G_FOG),
                gdp_set_fog_color(0x00ff_00ff),
                gsp_fog_position(500, 1000),
                cycle(1),
                mode(G_RM_FOG_SHADE_A, G_RM_OPA_SURF2),
                gdp_set_combine_lerp(color, one, color, one),
            ]);
        }
        if case.kind == Kind::Depth {
            dl.extend([
                gdp_pipe_sync(),
                set(G_ZBUFFER),
                mode(
                    G_RM_OPA_SURF | Z_CMP | if draw_index == 2 { ZMODE_DEC } else { Z_UPD },
                    G_RM_OPA_SURF2,
                ),
            ]);
        }
        let cull = match (case.f3d, draw.cull) {
            (_, 0) => 0,
            (true, -1) => 0x2000,
            (true, 1) => 0x1000,
            (false, -1) => G_CULL_BACK,
            (false, 1) => G_CULL_FRONT,
            _ => unreachable!(),
        };
        let vertices = b.vertices(&std::array::from_fn::<_, 3, _>(|i| {
            let [x, y] = draw.triangle[i];
            let w = if case.kind == Kind::Lod {
                [1, 4, 1][i]
            } else if matches!(case.kind, Kind::Perspective | Kind::PerspectiveNegative) {
                [1, 2, 4][i]
            } else {
                1
            };
            let rgba = if case.kind == Kind::Shade {
                [[0, 0, 0, 255], [255, 0, 0, 255], [0, 255, 0, 255]][i]
            } else {
                draw.color
            };
            VtxColored {
                x: ((x - i64::from(case.width) * 2) * w) as i16,
                y: ((i64::from(case.height) * 2 - y) * w) as i16,
                z: if case.kind == Kind::Fog {
                    [0, 64, 0][i]
                } else if case.kind == Kind::Depth {
                    ((x + y) / 16 - 96 - if draw_index == 1 { 16 } else { 0 }) as i16
                } else if matches!(
                    case.kind,
                    Kind::Perspective | Kind::PerspectiveNegative | Kind::Lod
                ) {
                    ((w - 1) * 128) as i16
                } else if case.kind == Kind::Near && i == 0 {
                    -256
                } else {
                    0
                },
                flag: 0,
                s: if case.kind == Kind::SharedUv {
                    if draw_index == 0 {
                        [0, 1024, 0][i]
                    } else {
                        [1024, 1984, 0][i]
                    }
                } else if case.kind == Kind::PerspectiveNegative {
                    [1904, 368, 1904][i]
                } else if case.kind == Kind::Lod {
                    [320, 6464, 320][i]
                } else {
                    [80, 1616, 80][i]
                },
                t: if case.kind == Kind::SharedUv {
                    if draw_index == 0 {
                        [0, 0, 1024][i]
                    } else {
                        [0, 1984, 1024][i]
                    }
                } else if case.kind == Kind::PerspectiveNegative {
                    [1808, 1808, 272][i]
                } else if case.kind == Kind::Lod {
                    0
                } else {
                    [176, 176, 1712][i]
                },
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: if matches!(case.kind, Kind::Dither | Kind::SharedAlpha) {
                    128
                } else if case.kind == Kind::Alpha {
                    [0, 255, 0][i]
                } else {
                    rgba[3]
                },
            }
        }));
        dl.extend([
            clear(if case.f3d { 0x3000 } else { G_CULL_BOTH }),
            set(cull),
            vertex(0, 3, vertices),
        ]);
        if case.modify_xy {
            for (i, [x, y]) in draw.triangle.into_iter().enumerate() {
                dl.push(gsp_modifyvertex(
                    i as u16,
                    G_MWO_POINT_XYSCREEN,
                    ((x as u32 & 0xffff) << 16) | (y as u32 & 0xffff),
                ));
            }
        }
        dl.push(triangle(0, 1, 2));
    }
    dl.extend([
        gdp_pipe_sync(),
        (0xe900_0000, 0),
        if case.f3d {
            gsp_enddl_f3d()
        } else {
            gsp_enddl()
        },
    ]);
    b.list("main", &dl);
    b.finish("main")
}

fn fixture(case: &Case) -> Fixture {
    let mut built = scene(case);
    if case.name == "coverage-retained-height" {
        let mut first = case.clone();
        first.height = 256;
        first.scissor = [0, 0, 256, 256];
        let mut prefix = scene(&first);
        let offset = prefix.rdram.len().next_multiple_of(16) as u32;
        prefix.rdram.resize(offset as usize, 0);
        for command in built.rdram[built.entry as usize..].as_chunks_mut::<8>().0 {
            if matches!(command[0], 0xda | 0xdc | 0x01) {
                let address = u32::from_be_bytes(command[4..].try_into().unwrap());
                command[4..].copy_from_slice(&(address + offset).to_be_bytes());
            }
        }
        prefix.rdram.extend(built.rdram);
        let entry = prefix.rdram.len() as u32;
        for (w0, w1) in [
            gsp_displaylist(prefix.entry),
            gsp_displaylist(built.entry + offset),
            gsp_enddl(),
        ] {
            prefix.rdram.extend(w0.to_be_bytes());
            prefix.rdram.extend(w1.to_be_bytes());
        }
        built = Built {
            rdram: prefix.rdram,
            entry,
        };
    }
    super::capture_fixture::make_image(built.rdram,built.entry,
        if case.f3d {Microcode::F3d} else {Microcode::F3dex2},case.width,case.height,
        Provenance { command_vector: format!("{}; literal quarter-pixel geometry; A=centre, B=integer; AA={}; interaction={:?}",case.name,case.aa,case.kind),
            synthetic_data: format!("Authored IMAGE commands; viewport scale 128, translation (W/2,H/2), raster {}x{}; object xy=w*(quarter_x-2W,2H-quarter_y). Independent expectations in tests/common/coverage_semantics.rs. {:?}",case.width,case.height,case.draws),
            ..Default::default() })
}

#[test]
fn coverage_encoded_coordinates_and_commands() {
    assert_eq!(gsp_1triangle_f3d(0, 1, 2), (0xbf00_0000, 0x0000_0a14));
    assert_eq!(gsp_1triangle(0, 1, 2), (0x0500_0204, 0));
    assert_eq!(
        gsp_modifyvertex(1, G_MWO_POINT_XYSCREEN, 0x0021_0043),
        (0x0218_0002, 0x0021_0043)
    );
    for case in cases() {
        let built = scene(&case);
        let memory = &built.rdram;
        assert_eq!(&memory[16..24], &[0; 8]);
        assert_eq!(&memory[48..52], &[0, 128, 0, 0]);
        let commands: Vec<Command> = memory[built.entry as usize..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| {
                (
                    u32::from_be_bytes(c[..4].try_into().unwrap()),
                    u32::from_be_bytes(c[4..].try_into().unwrap()),
                )
            })
            .collect();
        let vp = commands
            .iter()
            .find(|c| c.0 == if case.f3d { 0x0380_0000 } else { 0xdc08_0008 })
            .unwrap()
            .1 as usize;
        assert_eq!(&memory[vp..vp + 8], &[2, 0, 2, 0, 1, 255, 0, 0]);
        assert_eq!(
            u16::from_be_bytes(memory[vp + 8..vp + 10].try_into().unwrap()),
            (case.width * 2) as u16
        );
        assert_eq!(
            u16::from_be_bytes(memory[vp + 10..vp + 12].try_into().unwrap()),
            (case.height * 2) as u16
        );
        let loads: Vec<_> = commands
            .iter()
            .filter(|c| c.0 >> 24 == if case.f3d { 4 } else { 1 })
            .collect();
        assert_eq!(loads.len(), case.draws.len());
        for (load, draw) in loads.into_iter().zip(&case.draws) {
            for (i, [x, y]) in draw.triangle.into_iter().enumerate() {
                let at = load.1 as usize + i * 16;
                let ox = i16::from_be_bytes(memory[at..at + 2].try_into().unwrap()) as i64;
                let oy = i16::from_be_bytes(memory[at + 2..at + 4].try_into().unwrap()) as i64;
                let w = if case.kind == Kind::Lod {
                    [1, 4, 1][i]
                } else if matches!(case.kind, Kind::Perspective | Kind::PerspectiveNegative) {
                    [1, 2, 4][i]
                } else {
                    1
                };
                assert_eq!(
                    [
                        ox / w + i64::from(case.width) * 2,
                        i64::from(case.height) * 2 - oy / w
                    ],
                    [x, y]
                );
            }
        }
        assert_eq!(commands[commands.len() - 2], (0xe900_0000, 0));
        let fixture = fixture(&case);
        assert_eq!(fixture.tasks.len(), 1);
    }
}

#[test]
#[ignore = "writes IMAGE captures and independent expectations to FAST3D_WRITE_FIXTURES"]
fn write_rt64_coverage_fixtures() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest = String::from(
        "scene\twidth\theight\tchannels\tthreshold\tmax_diff_pixels\txor_pixels\tparent_gate\trt64_quirk\n",
    );
    for case in cases() {
        let a = case.expected(Rule::A);
        let b = case.expected(Rule::B);
        let mask: Vec<u8> = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .flat_map(|(a, b)| if a == b { [0, 0, 0, 255] } else { [255; 4] })
            .collect();
        let changes = mask.as_chunks::<4>().0.iter().filter(|p| p[0] != 0).count();
        if case.name == "coverage-slopes-f3d" {
            let mut column = case.clone();
            column.draws.truncate(8);
            let mask: Vec<u8> = column
                .expected(Rule::B)
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    if *p == super::coverage_semantics::BLACK {
                        [0, 0, 0, 255]
                    } else {
                        [255; 4]
                    }
                })
                .collect();
            std::fs::write(
                dir.join(format!("{}.rt64-quirk-mask.rgba8", case.name)),
                mask,
            )
            .unwrap();
        }
        for (suffix, bytes) in [
            ("f3dcap", fixture(&case).to_bytes().unwrap()),
            ("expected-a.rgba8", a),
            ("expected-b.rgba8", b),
            ("xor.rgba8", mask),
        ] {
            std::fs::write(dir.join(format!("{}.{}", case.name, suffix)), bytes).unwrap();
        }
        if case.name.contains("ties") {
            let mut words = String::new();
            let mut ties = String::from(
                "primitive\tx\ty\tedge\tax\tay\tbx\tby\tleft16\tright16\tmask\tcvbit\n",
            );
            for (i, draw) in case.draws.iter().enumerate() {
                let rdp = RdpEdges::exact(draw.triangle);
                words.push_str(&format!("{i}\t{:08x?}\t{rdp:?}\n", rdp.words()));
                let mut samples = Vec::new();
                for y in 0..case.height as i64 {
                    for x in 0..case.width as i64 {
                        let mask = rdp.mask(x, y);
                        samples.push(mask);
                        for e in 0..3 {
                            let a = draw.triangle[e];
                            let b = draw.triangle[(e + 1) % 3];
                            if super::coverage_semantics::edge(a, b, [4 * x, 4 * y]) == 0
                                && (a[0].min(b[0])..=a[0].max(b[0])).contains(&(4 * x))
                                && (a[1].min(b[1])..=a[1].max(b[1])).contains(&(4 * y))
                            {
                                let endpoints = rdp
                                    .endpoints(4 * y)
                                    .map_or(("invalid-y".into(), "invalid-y".into()), |(l, r)| {
                                        (l.to_string(), r.to_string())
                                    });
                                ties.push_str(&format!(
                                    "{i}\t{x}\t{y}\t{e}\t{}\t{}\t{}\t{}\t{}\t{}\t{mask:02x}\t{}\n",
                                    a[0],
                                    a[1],
                                    b[0],
                                    b[1],
                                    endpoints.0,
                                    endpoints.1,
                                    mask >> 7
                                ));
                            }
                        }
                    }
                }
                std::fs::write(dir.join(format!("{}.{i}.rdp-mask.u8", case.name)), samples)
                    .unwrap();
            }
            std::fs::write(dir.join(format!("{}.rdp-edges.txt", case.name)), words).unwrap();
            std::fs::write(dir.join(format!("{}.ties.tsv", case.name)), ties).unwrap();
        }
        manifest.push_str(&format!(
            "{}\t{}\t{}\tRGB\t{}\t0\t{}\t{}\t{}\n",
            case.name,
            case.width,
            case.height,
            if matches!(
                case.kind,
                Kind::Shade | Kind::Fog | Kind::Alpha | Kind::SharedAlpha
            ) {
                "7"
            } else if case.kind == Kind::Dither {
                "statistical"
            } else {
                "0"
            },
            changes,
            if case.name == "coverage-retained-height" {
                "held-scanout"
            } else {
                "required"
            },
            if case.name == "coverage-slopes-f3d" {
                "rt64-4337374-f3d-left-column"
            } else {
                "none"
            },
        ));
    }
    std::fs::write(dir.join("coverage-manifest.tsv"), manifest).unwrap();
}

#[test]
fn coverage_fixture_bytes_match_builders() {
    for case in cases() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("{}.f3dcap", case.name));
        assert_eq!(
            std::fs::read(path).unwrap(),
            fixture(&case).to_bytes().unwrap(),
            "{}",
            case.name
        );
    }
}

#[test]
#[ignore = "writes the unchanged sphere IMAGE input for independent mask derivation"]
fn write_coverage_prediction_inputs() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let (bytes, entry) = super::fixtures::fixture("chrome-icosphere--orange");
    std::fs::write(dir.join("pairless-chrome-icosphere.rdram"), bytes).unwrap();
    std::fs::write(dir.join("pairless-chrome-icosphere.json"),format!("{{\"width\":64,\"height\":64,\"tasks\":[{{\"entry\":{entry},\"microcode\":\"f3dex2\",\"segments\":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}}]}}\n")).unwrap();
}
