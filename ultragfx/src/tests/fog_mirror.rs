use super::dl_builder::{Built, DlBuilder};
use crate::hle::mem::RdramImage;
use n64_gbi::{consts::*, encode::*};
use serde_json::Value;

fn word(value: &Value) -> u32 {
    value.as_u64().unwrap().try_into().unwrap()
}

fn fixture(manifest: &Value, case: &Value) -> Built {
    let mut b = DlBuilder::new();
    let projection = b.matrix(n64_gbi::gu::gu_scale(
        1.0 / 16384.0,
        1.0 / 16384.0,
        1.0 / 128.0,
    ));
    let model = b.matrix(n64_gbi::gu::gu_scale(1.0, 1.0, 1.0));
    let viewport = b.viewport(Vp {
        vscale: [512, 512, 511, 0],
        vtrans: [640, 480, 0, 0],
    });
    let mut dl = vec![
        gdp_set_depth_image(0x0020_0000),
        gdp_set_color_image(
            0,
            if case["format"] == "rgba16" { 2 } else { 3 },
            320,
            0x0010_0000,
        ),
        gdp_set_scissor(0, 0, 0, 1280, 960),
        (0xef30_00f0, 0),
        gdp_set_fill_color(0),
        gdp_fill_rectangle(0, 0, 1276, 956),
        gdp_pipe_sync(),
        gsp_clear_geometrymode(u32::MAX),
        gsp_set_geometrymode(G_SHADE | G_SHADING_SMOOTH),
        gsp_matrix(projection, true, true, false),
        gsp_matrix(model, false, true, false),
        gsp_viewport(viewport),
        gsp_texture(0xffff, 0xffff, 0, 0, case["group"] == "modes"),
    ];
    let common = manifest["common_ops"].as_array().unwrap();
    let ops = case["ops"].as_array().unwrap();
    for op in common
        .iter()
        .filter(|_| case["group"] == "modes")
        .chain(ops)
    {
        if let Some(command) = op.get("command") {
            let w0 = word(&command[0]);
            dl.push((w0, word(&command[1])));
        } else {
            for triangle in op["triangles"].as_array().unwrap() {
                let vertices: Vec<_> = triangle
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| VtxColored {
                        x: (v[0].as_i64().unwrap() - 160 * 128).try_into().unwrap(),
                        y: (120 * 128 - v[1].as_i64().unwrap()).try_into().unwrap(),
                        z: 0,
                        flag: 0,
                        s: 0,
                        t: 0,
                        r: word(&v[2]) as u8,
                        g: word(&v[3]) as u8,
                        b: word(&v[4]) as u8,
                        a: word(&v[5]) as u8,
                    })
                    .collect();
                let address = b.vertices(&vertices);
                dl.extend([gsp_vertex(0, 3, address), gsp_1triangle(0, 1, 2)]);
            }
        }
    }
    dl.push(gsp_enddl());
    b.list("main", &dl);
    let mut built = b.finish("main");
    if case["group"] == "modes" {
        built.rdram.resize(0x0030_c200, 0);
        let ci = case["ci"].as_bool().unwrap();
        for pixel in built.rdram[0x0030_b000..0x0030_b080].as_chunks_mut::<2>().0 {
            *pixel = if ci { [3, 3] } else { [0x7c, 0xbd] };
        }
        for pixel in built.rdram[0x0030_c000..0x0030_c200].as_chunks_mut::<2>().0 {
            *pixel = [0x7c, 0xbd];
        }
    }
    built
}

fn hardware_rgb(bytes: &[u8], packed: bool) -> [u8; 3] {
    if packed {
        let word = u16::from_be_bytes(bytes.try_into().unwrap());
        [11, 6, 1].map(|shift| ((word >> shift) & 31) as u8)
    } else {
        bytes[..3].try_into().unwrap()
    }
}

fn check(case: &Value, pixels: &[u8], dual: bool) -> Option<String> {
    let name = case["id"].as_str().unwrap();
    let packed = case["format"] == "rgba16";
    let size = if packed { 2 } else { 4 };
    let modern = case["modern"].as_bool().unwrap();
    let record_size = size + if modern { 3 } else { 0 };
    let mut palette = Vec::new();
    let mut lookup = Vec::new();
    if let Some(value) = case["hardware_word"].as_u64() {
        palette.extend_from_slice(&(value as u32).to_be_bytes()[4 - size..]);
        if modern {
            palette.extend(
                case["modern_rgb"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| word(v) as u8),
            );
        }
    } else {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fog-mirror")
            .join(case["data"].as_str().unwrap());
        let bytes = std::fs::read(path).unwrap();
        let count = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as usize;
        let end = 2 + count * record_size;
        palette.extend_from_slice(&bytes[2..end]);
        lookup.extend_from_slice(&bytes[end..]);
        assert_eq!(lookup.len(), word(&case["samples"]) as usize * 2, "{name}");
    }
    let tolerance = word(&case["hardware_tolerance"]) as u8;
    let modern_tolerance = word(&case["modern_tolerance"]) as u8;
    let mut samples = 0;
    let mut differing = 0;
    let mut maximum = 0;
    let mut covered = 0;
    let mut failure = None;
    for span in case["spans"].as_array().unwrap() {
        let start = word(&span[0]) as usize;
        let length = word(&span[1]) as usize;
        assert!(length > 0);
        for i in start..start + length {
            let index = if lookup.is_empty() {
                0
            } else {
                u16::from_le_bytes(lookup[samples * 2..samples * 2 + 2].try_into().unwrap())
                    as usize
            };
            let record = &palette[index * record_size..(index + 1) * record_size];
            let (x, y) = (
                i % word(&case["width"]) as usize,
                i / word(&case["width"]) as usize,
            );
            let actual: [u8; 3] = pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3]
                .try_into()
                .unwrap();
            let reduced = actual.map(|v| if packed { v >> 3 } else { v });
            let hardware = hardware_rgb(&record[..size], packed);
            let delta = reduced
                .into_iter()
                .zip(hardware)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap();
            maximum = maximum.max(delta);
            differing += usize::from(delta != 0);
            covered += usize::from(actual != [17, 23, 31]);
            let prediction = &record[size..];
            if failure.is_none()
                && (delta > tolerance
                    || (modern
                        && actual
                            .iter()
                            .zip(prediction)
                            .any(|(a, b)| a.abs_diff(*b) > modern_tolerance)))
            {
                failure = Some(format!("{name} ({x},{y}): GPU {actual:?}, hardware {hardware:?}, modern {prediction:?}"));
            }
            samples += 1;
        }
    }
    assert_eq!(samples, word(&case["samples"]) as usize, "{name}");
    assert!(samples > 0, "{name}");
    if case["region"] == "edge" {
        assert!(covered > 0, "{name}");
    }
    eprintln!(
        "mirror {name} {}: {differing}/{samples} differ, max {maximum}, {}",
        if dual { "dual" } else { "fallback" },
        if modern { "precision" } else { "hardware" }
    );
    failure
}

fn verify(dual: bool, bug: Option<&str>) -> Vec<String> {
    let manifest: Value =
        serde_json::from_str(include_str!("../../tests/fog-mirror/cases.json")).unwrap();
    let (device, queue) = if dual {
        let (d, q, supported) = crate::render::headless_device();
        assert!(supported, "dual-source adapter required");
        (d, q)
    } else {
        crate::render::headless_device_forced_fallback()
    };
    let mut renderer =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 320, 240, dual);
    let mut previous_format = None;
    let mut failures = Vec::new();
    let mut cases = 0;
    for case in manifest["cases"].as_array().unwrap() {
        if case["bug"].as_str() != bug {
            continue;
        }
        cases += 1;
        let packed = case["format"] == "rgba16";
        if previous_format.is_some_and(|format| format != packed) {
            renderer = crate::render::SceneRenderer::new(
                &device,
                wgpu::TextureFormat::Rgba8Unorm,
                320,
                240,
                dual,
            );
        }
        previous_format = Some(packed);
        let built = fixture(&manifest, case);
        let result = crate::hle::interpret(
            RdramImage::new(&built.rdram),
            built.entry.into(),
            crate::hle::gbi::GbiUcode::F3dex2,
            crate::hle::mem::GbiDataFormat::Fixed,
            None,
        );
        assert!(
            result.diags.iter().all(|d| matches!(
                d.kind,
                crate::DiagKind::NonCanonicalBlend | crate::DiagKind::RenderModeNeverSet
            )),
            "{}: {:?}",
            case["id"],
            result.diags
        );
        let pixels = super::common::render_to_pixels(
            &device,
            &queue,
            &mut renderer,
            &result.scene,
            320,
            240,
        );
        assert!(
            renderer.diagnostics.is_empty(),
            "{}: {:?}",
            case["id"],
            renderer.diagnostics
        );
        if let Some(output) = std::env::var_os("ULTRAGFX_FOG_OUTPUT") {
            let dir = std::path::PathBuf::from(output).join(if dual { "dual" } else { "fallback" });
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{}.rgba8", case["id"].as_str().unwrap())),
                &pixels,
            )
            .unwrap();
        }
        if let Some(failure) = check(case, &pixels, dual) {
            failures.push(failure);
        }
    }
    assert!(cases > 0);
    failures
}

#[test]
fn hardware_mirror_dual() {
    let failures = verify(true, None);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn hardware_mirror_fallback() {
    let failures = verify(false, None);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

macro_rules! semantic_test {
    ($name:ident, $bug:literal, $reason:literal) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            let failures: Vec<_> = [true, false]
                .into_iter()
                .flat_map(|dual| verify(dual, Some($bug)))
                .collect();
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        }
    };
}

semantic_test!(
    wrong_memory_alpha_and_coverage,
    "memory_alpha_and_coverage",
    "presentation omits stored coverage weights and the AA divider (16–18, coverage gradient)"
);
semantic_test!(
    missing_color_on_coverage,
    "color_on_coverage",
    "COLOR_ON_CVG must select M when coverage does not overflow (20)"
);
semantic_test!(
    missing_partial_reject,
    "partial_reject",
    "full alpha must bypass blender arithmetic under partial reject (22)"
);
semantic_test!(
    wrong_decal_depth_tolerance,
    "decal_depth_tolerance",
    "four OoT decal modes reject Z=0 over stored Z=64 accepted by hardware"
);
semantic_test!(
    missing_first_cycle_memory_rgb,
    "first_cycle_memory_rgb",
    "two-cycle first blender MEM RGB input is omitted (memory gradient)"
);
