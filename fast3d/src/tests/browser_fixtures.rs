use super::sm64_semantics::{Case, CASES};
use crate::capture::{Fixture, Frame, MemoryLayout, MemorySpan, Provenance, SourceLayout, Task};
use crate::{ClearPolicy, DataFormat, Microcode, RendererConfig};
use n64_gbi::encode::*;

fn host64_fill() -> Fixture {
    let commands: [(u64, u64); 6] = [
        (0xed00_0000, 0x0010_00c0),
        (0xff10_003f, 0x0000_0002_3456_7000),
        (0xba00_1402, 0x0030_0000),
        (0xf700_0000, 0xf801_f801),
        (0xf60f_c0bc, 0),
        (0xb800_0000, 0),
    ];
    Fixture {
        frame: Frame {
            serial: 42,
            dither_seed: 17,
            config: RendererConfig {
                resolution_multiplier: 1,
                sample_count: 1,
                present_mode: wgpu::PresentMode::Fifo,
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                clear_policy: ClearPolicy::Persist,
                power_preference: wgpu::PowerPreference::LowPower,
            },
            width: 64,
            height: 48,
            vi: None,
            dual_source_blending: false,
        },
        tasks: vec![Task {
            entry: 0x0000_0001_2345_6000,
            microcode: Microcode::F3d,
            data_format: DataFormat::Fixed,
            order: 0,
            source: SourceLayout {
                memory: MemoryLayout::HOST64_LE,
                segments: [0; 16],
            },
            spans: vec![MemorySpan {
                address: 0x0000_0001_2345_6000,
                bytes: commands
                    .into_iter()
                    .flat_map(|(w0, w1)| w0.to_le_bytes().into_iter().chain(w1.to_le_bytes()))
                    .collect(),
            }],
        }],
        provenance: Provenance {
            command_vector: "host64-fill-v1".into(),
            synthetic_data:
                "synthetic red full-frame fill; literal F3D words; virtual addresses above 4 GiB"
                    .into(),
            ..Default::default()
        },
    }
}

fn combiner_selector() -> Fixture {
    let mut bytes = vec![0; 0x400];
    bytes[..64].copy_from_slice(&mtx_to_bytes([
        [1.0 / 128.0, 0.0, 0.0, 0.0],
        [0.0, 1.0 / 128.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    bytes[64..80].copy_from_slice(
        &Vp {
            vscale: [512, 512, 511, 0],
            vtrans: [640, 480, 511, 0],
        }
        .to_bytes(),
    );
    for (i, (x, y)) in [(-16, -16), (16, -16), (16, 16), (-16, 16)]
        .into_iter()
        .enumerate()
    {
        bytes[0x100 + i * 16..0x110 + i * 16].copy_from_slice(
            &VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 192,
            }
            .to_bytes(),
        );
    }
    let color = CcPass {
        a: 3,
        b: ZERO_C,
        c: 12,
        d: ZERO_C,
    };
    let alpha = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 6,
    };
    let words = [
        gsp_matrix_f3d(0, true, true, false),
        gsp_viewport_f3d(64),
        (0xba00_1402, 0),
        (0xb900_031d, 0x0f0a_4000),
        (0xb700_0000, 4),
        gdp_set_prim_color(0, 0, 0xc864_2880),
        gdp_set_env_color(0x1020_3040),
        gdp_set_combine_lerp(color, alpha, color, alpha),
        gsp_vertex_f3d(0, 4, 0x100),
        gsp_1triangle_f3d(0, 1, 2),
        gsp_1triangle_f3d(0, 2, 3),
        gsp_enddl_f3d(),
    ];
    for (i, (w0, w1)) in words.into_iter().enumerate() {
        bytes[0x200 + i * 8..0x204 + i * 8].copy_from_slice(&w0.to_be_bytes());
        bytes[0x204 + i * 8..0x208 + i * 8].copy_from_slice(&w1.to_be_bytes());
    }
    super::capture_fixture::make(bytes, 0x200, 320, 240, Provenance {
        source_symbols: "include/PR/gbi.h: gsDPSetCombineLERP, G_CCMUX_ENV_ALPHA".into(),
        command_vector: "F3D one-cycle (PRIMITIVE - 0) * ENV_ALPHA + 0; alpha ONE; OPA_SURF".into(),
        synthetic_data: "All commands, addresses, matrices and geometry authored for this selector probe; 32x32 square at (144,104); primitive RGB [200,100,40], env alpha 64; independent RGB product [50,25,10] after rounding; primitive and shade alpha 128 and 192 distinguish selectors; cleared black 320x240 framebuffer. No ROM bytes".into(),
        ..Default::default()
    })
}

fn fixtures() -> Vec<(Case, Fixture)> {
    let corpus = super::sm64_corpus::fixtures();
    CASES
        .into_iter()
        .map(|case| {
            let fixture = match case {
                Case::Host64Fill => host64_fill(),
                Case::CombinerSelector => combiner_selector(),
                _ => corpus
                    .iter()
                    .find(|(name, _)| case.filename().strip_suffix(".f3dcap") == Some(name))
                    .unwrap()
                    .1
                    .clone(),
            };
            (case, fixture)
        })
        .collect()
}

#[test]
fn browser_fixture_bytes_match_builders() {
    for (case, fixture) in fixtures() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(case.filename());
        assert_eq!(
            std::fs::read(path).unwrap(),
            fixture.to_bytes().unwrap(),
            "{:?}: regenerate with write_browser_sm64_fixtures",
            case
        );
        for task in &fixture.tasks {
            task.final_color_image().unwrap();
        }
    }
}

#[test]
#[ignore = "writes browser fixtures to FAST3D_WRITE_FIXTURES"]
fn write_browser_sm64_fixtures() {
    let Some(directory) = std::env::var_os("FAST3D_WRITE_FIXTURES") else {
        panic!("set FAST3D_WRITE_FIXTURES to fast3d/tests/fixtures");
    };
    std::fs::create_dir_all(&directory).unwrap();
    for (case, fixture) in fixtures() {
        for task in &fixture.tasks {
            task.final_color_image().unwrap();
        }
        std::fs::write(
            std::path::Path::new(&directory).join(case.filename()),
            fixture.to_bytes().unwrap(),
        )
        .unwrap();
    }
}
