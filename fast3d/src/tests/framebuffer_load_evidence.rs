use super::dl_builder::{Built, Command, DlBuilder};
use crate::capture::{Fixture, Frame, MemoryLayout, MemorySpan, Provenance, SourceLayout, Task};
use crate::{DataFormat, DiagKind, Diagnostic, FramebufferAccess, Microcode, RendererConfig};
use n64_gbi::{consts::*, encode::*};

#[path = "../../tests/common/framebuffer_load_semantics.rs"]
mod semantics;
use semantics::{Case, CASES, PRODUCER, REPLACEMENT, UNPACKED};

const SOURCE: u32 = 0x0030_0000;
const OUTPUT: u32 = 0x0010_0000;
const OFFSET_LOAD: Command = (0xf401_0004, 0x0701_c008);
const EXACT_LOAD: Command = (0xf400_0000, 0x0700_c004);
const PACKED: [u16; 8] = [
    0x12a3, 0x4433, 0xf0d9, 0x6d8b, 0x2219, 0x8531, 0xdde7, 0x0997,
];

fn combine(selector: u32) -> Command {
    let rgb = CcPass {
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
    gdp_set_combine_lerp(rgb, alpha, rgb, alpha)
}

fn alpha_to_rgb() -> Command {
    let rgb = CcPass {
        a: 6,
        b: ZERO_C,
        c: 8,
        d: ZERO_C,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 1,
    };
    gdp_set_combine_lerp(rgb, alpha, rgb, alpha)
}

fn quad(b: &mut DlBuilder, dl: &mut Vec<Command>, rect: [i16; 4], rgba: [u8; 4], st: [i16; 2]) {
    let [left, top, right, bottom] = rect;
    let [r, g, blue, a] = rgba;
    let vertices = b.vertices(
        &[(left, top), (left, bottom), (right, bottom), (right, top)].map(|(x, y)| VtxColored {
            x: x - 160,
            y: 120 - y,
            z: 0,
            flag: 0,
            s: st[0],
            t: st[1],
            r,
            g,
            b: blue,
            a,
        }),
    );
    dl.extend([gsp_vertex(0, 4, vertices), gsp_2triangles(0, 1, 2, 0, 2, 3)]);
}

fn target(dl: &mut Vec<Command>, source: bool) {
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_color_image(
            0,
            if source { 2 } else { 3 },
            if source { 32 } else { 320 },
            if source { SOURCE } else { OUTPUT },
        ),
        gdp_set_scissor(
            0,
            0,
            0,
            if source { 128 } else { 1280 },
            if source { 32 } else { 960 },
        ),
    ]);
}

fn clear(dl: &mut Vec<Command>, source: bool) {
    target(dl, source);
    dl.extend([
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_fill_color(if source { 0x0001_0001 } else { 0x0000_00ff }),
        gdp_fill_rectangle(
            0,
            0,
            if source { 124 } else { 1276 },
            if source { 28 } else { 956 },
        ),
        gdp_pipe_sync(),
        gdp_set_cycle_type(0),
        gdp_set_render_mode(G_RM_OPA_SURF | CVG_DST_FULL, G_RM_OPA_SURF2),
    ]);
}

fn consumers(b: &mut DlBuilder, dl: &mut Vec<Command>, case: Case, left: i16) {
    let copy = case == Case::ShortcutRect;
    dl.extend([
        gdp_pipe_sync(),
        gdp_set_cycle_type(if copy { 2 } else { 0 }),
    ]);
    for (top, combiner) in [(40, combine(1)), (88, alpha_to_rgb())] {
        if copy && top == 88 {
            continue;
        }
        dl.extend([gdp_pipe_sync(), combiner]);
        for i in 0..8i16 {
            let (col, row) = (i % 4, i / 4);
            let (x, y) = (left + col * 16, top + row * 16);
            if case == Case::OffsetTriangle {
                quad(
                    b,
                    dl,
                    [x, y, x + 16, y + 16],
                    [255; 4],
                    [col * 64 + 32, row * 64 + 32],
                );
            } else {
                dl.extend(gsp_texture_rectangle(
                    x as u32 * 4,
                    y as u32 * 4,
                    (x + if copy { 15 } else { 16 }) as u32 * 4,
                    (y + if copy { 15 } else { 16 }) as u32 * 4,
                    0,
                    (col * 32 + 16) as u32,
                    (row * 32 + 16) as u32,
                    0,
                    0,
                    false,
                ));
            }
        }
    }
}

fn scene(case: Case) -> Built {
    let mut b = DlBuilder::new();
    let projection = b.matrix(n64_gbi::gu::gu_scale(1.0 / 256.0, 1.0 / 256.0, 1.0 / 128.0));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [1024, 1024, 511, 0],
        vtrans: [640, 480, 511, 0],
    });
    let mut dl = vec![
        gsp_clear_geometrymode(u32::MAX),
        gsp_set_geometrymode(G_CLIPPING | G_SHADE | G_SHADING_SMOOTH),
        gsp_matrix(projection, true, true, false),
        gsp_matrix(model, false, true, false),
        gsp_viewport(viewport),
        gdp_set_other_mode_h(4, 2, 3 << 4),
        gdp_set_other_mode_h(6, 2, 3 << 6),
        gdp_set_other_mode_h(12, 2, 0),
        gdp_set_other_mode_h(14, 2, 0),
        gdp_set_other_mode_h(
            19,
            1,
            if case == Case::OffsetTriangle {
                1 << 19
            } else {
                0
            },
        ),
        gdp_set_other_mode_l(0, 2, 0),
    ];
    clear(&mut dl, false);
    clear(&mut dl, true);
    dl.push(combine(4));
    for (i, rgba) in PRODUCER.into_iter().enumerate() {
        let (x, y) = (
            i as i16 % 4 + if case.offset() { 8 } else { 0 },
            i as i16 / 4 + if case.offset() { 3 } else { 0 },
        );
        quad(&mut b, &mut dl, [x, y, x + 1, y + 1], rgba, [0; 2]);
    }
    target(&mut dl, false);
    dl.push(gdp_set_texture_image(
        0,
        2,
        32,
        SOURCE + if case.offset() { 0x88 } else { 0 },
    ));
    if case != Case::ShortcutRect {
        dl.extend([
            (0xe800_0000, 0),
            gdp_set_tile(0, 2, 2, 0x20, 7, 0, 2, 0, 0, 2, 0, 0),
            gdp_load_sync(),
            if case.offset() {
                OFFSET_LOAD
            } else {
                EXACT_LOAD
            },
            gdp_pipe_sync(),
        ]);
    }
    dl.extend([
        (0xe800_0000, 0),
        gdp_set_tile(0, 2, 2, 0x20, 0, 0, 2, 0, 0, 2, 0, 0),
        gdp_set_tile_size(0, 0, 0, 12, 4),
        gsp_texture(32768, 32768, 0, 0, true),
    ]);
    consumers(&mut b, &mut dl, case, 40);
    target(&mut dl, true);
    dl.extend([gdp_set_cycle_type(0), combine(4)]);
    quad(&mut b, &mut dl, [0, 0, 32, 8], REPLACEMENT, [0; 2]);
    target(&mut dl, false);
    consumers(&mut b, &mut dl, case, 136);
    dl.extend([gdp_pipe_sync(), gdp_set_cycle_type(0), combine(4)]);
    quad(
        &mut b,
        &mut dl,
        [232, 40, 264, 72],
        [0, 0, 255, 255],
        [0; 2],
    );
    dl.extend([gdp_pipe_sync(), (0xe900_0000, 0), gsp_enddl()]);
    b.list("main", &dl);
    b.finish("main")
}

fn rejection(built: &Built, case: Case) -> Diagnostic {
    let load = if case.offset() {
        OFFSET_LOAD
    } else {
        EXACT_LOAD
    };
    let bytes: Vec<_> = [load.0, load.1]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    let index = built.rdram[built.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .position(|word| word.as_slice() == bytes)
        .unwrap();
    Diagnostic {
        at: u64::from(built.entry) + index as u64 * 8,
        kind: DiagKind::UnsupportedFramebufferAccess {
            address: u64::from(SOURCE) + if case.offset() { 0xd0 } else { 0 },
            reason: FramebufferAccess::TextureLoad,
        },
    }
}

fn assert_boundary(built: &Built, case: Case) -> crate::hle::InterpResult {
    let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
    let expected = if case == Case::ShortcutRect {
        vec![]
    } else {
        vec![rejection(built, case)]
    };
    assert_eq!(result.diags, expected, "{case:?}");
    assert_eq!(result.termination, crate::inspect::WalkTermination::End);
    assert_eq!(result.scene.color_image.addr, u64::from(OUTPUT));
    assert_eq!(
        (
            result.scene.color_image.width,
            result.scene.color_image.fmt,
            result.scene.color_image.siz
        ),
        (320, 0, 3)
    );
    result
}

#[test]
fn f0_real_load_is_an_expected_rejection() {
    for case in CASES {
        let built = scene(case);
        assert!(built.rdram.len() < OUTPUT as usize);
        let result = assert_boundary(&built, case);
        assert_eq!(
            result.dropped_runs,
            match case {
                Case::ShortcutRect => 0,
                _ => 32,
            }
        );
    }
}

#[test]
fn f0_checked_captures_match_authored_commands() {
    for case in CASES {
        let built = scene(case);
        assert_eq!(
            fixture(&built, case).to_bytes().unwrap(),
            case.fixture_bytes()
        );
    }
}

fn commands(built: &Built) -> Vec<Command> {
    built.rdram[built.entry as usize..]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|word| {
            (
                u32::from_be_bytes(word[..4].try_into().unwrap()),
                u32::from_be_bytes(word[4..].try_into().unwrap()),
            )
        })
        .collect()
}

#[test]
fn f0_commands_pin_load_and_producer_write_order() {
    for case in CASES {
        let built = scene(case);
        let commands = commands(&built);
        let switches: Vec<_> = commands
            .iter()
            .enumerate()
            .filter(|(_, (w0, _))| w0 >> 24 == 0xff)
            .collect();
        assert_eq!(
            switches
                .iter()
                .map(|(_, (_, address))| *address)
                .collect::<Vec<_>>(),
            [OUTPUT, SOURCE, OUTPUT, SOURCE, OUTPUT]
        );
        let first = switches[2].0;
        let overwrite = switches[3].0;
        let second = switches[4].0;
        let loads: Vec<_> = commands
            .iter()
            .enumerate()
            .filter(|(_, (w0, _))| matches!(w0 >> 24, 0xf3 | 0xf4 | 0xf0))
            .collect();
        if case == Case::ShortcutRect {
            assert!(loads.is_empty());
        } else {
            assert_eq!(loads.len(), 1);
            let (index, &load) = loads[0];
            assert!(first < index && index < overwrite);
            assert_eq!(
                load,
                if case.offset() {
                    OFFSET_LOAD
                } else {
                    EXACT_LOAD
                }
            );
            assert_eq!(
                &commands[index - 3..index],
                &[
                    (0xe800_0000, 0),
                    (0xf510_0420, 0x0708_0200),
                    (0xe600_0000, 0)
                ]
            );
            assert_eq!(commands[index + 1], (0xe700_0000, 0));
        }
        let draw_opcode = if case == Case::OffsetTriangle {
            0x06
        } else {
            0xe4
        };
        for phase in [
            &commands[first..overwrite - 1],
            &commands[second..commands.len() - 6],
        ] {
            assert_eq!(
                phase
                    .iter()
                    .filter(|(w0, _)| w0 >> 24 == draw_opcode)
                    .count(),
                if case == Case::ShortcutRect { 8 } else { 16 }
            );
        }
        assert_eq!(commands[overwrite - 1], (0xe700_0000, 0));
        assert_eq!(commands[second - 1], (0xe700_0000, 0));
        let rewrite = &commands[overwrite..second - 1];
        assert_eq!(rewrite.len(), 6);
        assert_eq!(rewrite[3], combine(4));
        assert_eq!(rewrite[5], (0x0600_0204, 0x0000_0406));
        let vertices = rewrite[4].1 as usize;
        for vertex in built.rdram[vertices..vertices + 64].as_chunks::<16>().0 {
            assert_eq!(&vertex[12..], &REPLACEMENT);
        }
        assert_eq!(
            &commands[commands.len() - 3..],
            &[(0xe700_0000, 0), (0xe900_0000, 0), (0xdf00_0000, 0)]
        );
    }
}

#[test]
fn f0_same_load_commands_decode_literal_ram_control() {
    for case in [Case::OffsetRect, Case::OffsetTriangle, Case::ExactRect] {
        let mut built = scene(case);
        let image = commands(&built)
            .iter()
            .position(|(w0, _)| w0 >> 24 == 0xfd)
            .unwrap();
        let at = built.entry as usize + image * 8 + 4;
        let address: u32 = if case.offset() { 0x4088 } else { 0x4000 };
        built.rdram[at..at + 4].copy_from_slice(&address.to_be_bytes());
        built.rdram.resize(0x4200, 0xee);
        let offsets = if case.offset() {
            [0xd0, 0xd2, 0xd4, 0xd6, 0x110, 0x112, 0x114, 0x116]
        } else {
            [0, 2, 4, 6, 64, 66, 68, 70]
        };
        for (offset, word) in offsets.into_iter().zip(PACKED) {
            built.rdram[0x4000 + offset..0x4002 + offset].copy_from_slice(&word.to_be_bytes());
        }
        let result = crate::hle::interpret_rdram(&built.rdram, built.entry);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.dropped_runs, 0);
        assert_eq!(
            result
                .rdp
                .tmem_bank
                .sample_tile(&result.rdp.tiles[0], 0)
                .unwrap(),
            UNPACKED.concat()
        );
        let textured: Vec<_> = result
            .scene
            .materials
            .iter()
            .filter(|material| material.tex_w == 4 && material.tex_h == 2)
            .collect();
        assert!(!textured.is_empty());
        for material in textured {
            assert_eq!(material.texture, UNPACKED.concat());
        }
    }
}

#[test]
fn f0_guest_rgba16_bits_are_literal() {
    let packed =
        super::dl_builder::pack_texels(super::dl_builder::TexelFormat::Rgba16, 4, &PRODUCER);
    assert_eq!(
        packed.texels,
        [
            0x12, 0xa3, 0x44, 0x33, 0xf0, 0xd9, 0x6d, 0x8b, 0x22, 0x19, 0x85, 0x31, 0xdd, 0xe7,
            0x09, 0x97
        ]
    );
    for (word, expected) in PACKED.into_iter().zip(UNPACKED) {
        assert_eq!(crate::hle::texdec::decode_rgba16_entry(word), expected);
    }
}

#[test]
fn f0_loadtile_byte_mapping_and_snapshot() {
    use crate::hle::{rdp::TileDescriptor, tmem::Tmem};
    let mut source = [0xee; 512];
    let offsets = [0xd0, 0xd2, 0xd4, 0xd6, 0x110, 0x112, 0x114, 0x116];
    for (offset, word) in offsets.into_iter().zip(PACKED) {
        source[offset..offset + 2].copy_from_slice(&word.to_be_bytes());
    }
    assert_eq!((0x88 / 64, (0x88 % 64) / 2), (2, 4));
    assert_eq!((0x88 + 64 + 8, 0x88 + 2 * 64 + 8), (0xd0, 0x110));
    let mut tmem = Tmem::default();
    tmem.write_tile(&source[0xd0..0x118], 0x20, 2, 2, 1, 64, 2);
    let bank = tmem.profile_bank(false);
    assert_eq!(
        &bank.bytes[0x100..0x108],
        &[0x12, 0xa3, 0x44, 0x33, 0xf0, 0xd9, 0x6d, 0x8b]
    );
    assert_eq!(
        &bank.bytes[0x110..0x118],
        &[0xdd, 0xe7, 0x09, 0x97, 0x22, 0x19, 0x85, 0x31]
    );
    assert!(bank.bytes[..0x100]
        .iter()
        .chain(&bank.bytes[0x108..0x110])
        .chain(&bank.bytes[0x118..])
        .all(|&byte| byte == 0));
    let tile = TileDescriptor {
        fmt: 0,
        siz: 2,
        line: 2,
        tmem_addr: 0x20,
        width: 4,
        height: 2,
        cms: 2,
        cmt: 2,
        ..Default::default()
    };
    assert_eq!(tmem.sample_tile(&tile, 0).unwrap(), UNPACKED.concat());
    source.fill(0);
    assert_eq!(tmem.sample_tile(&tile, 0).unwrap(), UNPACKED.concat());
}

fn fixture(built: &Built, case: Case) -> Fixture {
    assert_boundary(built, case);
    Fixture {
        frame: Frame {
            serial: 0,
            dither_seed: 0,
            width: 320,
            height: 240,
            vi: None,
            dual_source_blending: false,
            config: RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                present_mode: wgpu::PresentMode::Fifo,
                clear_policy: crate::ClearPolicy::PerFrame,
                power_preference: wgpu::PowerPreference::None,
            },
        },
        tasks: vec![Task {
            entry: built.entry.into(),
            microcode: Microcode::F3dex2,
            data_format: DataFormat::Fixed,
            order: 0,
            source: SourceLayout { memory: MemoryLayout::IMAGE, segments: [0; 16] },
            spans: vec![MemorySpan { address: 0, bytes: built.rdram.clone() }],
        }],
        provenance: Provenance {
            source_symbols: "authored F0 framebuffer-load evidence".into(),
            command_vector: case.name().into(),
            synthetic_data: if case == Case::ShortcutRect {
                "Eight constant-shade RGBA8 quads into RGBA16; load-free copy-mode shortcut before and after producer overwrite. No rt64 load-semantics claim."
            } else {
                "Eight constant-shade RGBA8 quads into RGBA16; one LoadTile; two consumer groups separated by producer overwrite. No saved n64.toys source."
            }.into(),
            ..Default::default()
        },
    }
}

#[test]
#[ignore = "writes authored F0 IMAGE fixtures and independent expectations"]
fn write_rt64_framebuffer_load_evidence() {
    let output = std::path::PathBuf::from(
        std::env::var_os("FAST3D_WRITE_FIXTURES").expect("set FAST3D_WRITE_FIXTURES"),
    );
    std::fs::create_dir_all(&output).unwrap();
    for case in CASES {
        let built = scene(case);
        let fixture = fixture(&built, case);
        let bytes = fixture.to_bytes().unwrap();
        assert_eq!(Fixture::from_bytes(&bytes).unwrap(), fixture);
        let prefix = output.join(case.name());
        std::fs::write(prefix.with_extension("f3dcap"), bytes).unwrap();
        let fast3d: Vec<_> = (0..240)
            .flat_map(|y| (0..320).flat_map(move |x| semantics::fast3d_expected(case, x, y)))
            .collect();
        std::fs::write(prefix.with_extension("fast3d-expected.rgba8"), fast3d).unwrap();
        if case != Case::ShortcutRect {
            assert!(fixture.final_color_image().is_err());
            let mut rdram = built.rdram;
            rdram.resize(8 * 1024 * 1024, 0);
            std::fs::write(prefix.with_extension("rdram"), rdram).unwrap();
            std::fs::write(prefix.with_extension("json"), format!("{{\n  \"version\": 1, \"width\": 320, \"height\": 240,\n  \"tasks\": [{{\"entry\": {}, \"microcode\": \"f3dex2\", \"segments\": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}}],\n  \"color_image\": {{\"address\": 1048576, \"width\": 320, \"format\": 0, \"size\": 3}}\n}}\n", built.entry)).unwrap();
            let expected: Vec<_> = (0..240)
                .flat_map(|y| (0..320).flat_map(move |x| semantics::oracle_expected(x, y)))
                .collect();
            std::fs::write(prefix.with_extension("expected.rgba8"), expected).unwrap();
        }
    }
}
