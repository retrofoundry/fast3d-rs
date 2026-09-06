use super::*;
use crate::diag::{DiagKind, Diagnostic};
use crate::hle::rdp::{Rdp, TileDescriptor};
use crate::hle::rsp::{Rsp, Scene};
use n64_gbi::encode::{gdp_set_combine_lerp, CcPass};

const FLAT: [u32; 8] = [8, 8, 16, 3, 7, 7, 7, 6];

#[test]
fn filter_mode_is_captured_in_material_and_uniform() {
    for rect in [false, true] {
        let mut uniforms = Vec::new();
        let mut materials = Vec::new();
        for filter in [0, 2, 3, 0] {
            let mut rdp = state([FLAT; 2], 0);
            rdp.other_mode_h |= filter << 12;
            let (mat, diags) = material(&rdp, rect);
            assert!(diags.is_empty(), "{diags:?}");
            let mat = mat.unwrap();
            uniforms.push(
                crate::render::CombinerUniform::from_run(
                    &mat,
                    &crate::hle::blender::decode_render_mode(0, 0, 0),
                    [0; 4],
                )
                .inv_tex1_size[3],
            );
            materials.push(mat);
        }
        assert_eq!(uniforms, [0.0, 2.0, 3.0, 0.0]);
        assert_ne!(materials[0], materials[1]);
        assert_ne!(materials[1], materials[2]);
        assert_eq!(materials[0], materials[3]);
        assert_eq!(materials[0].texture, materials[1].texture);
    }
}

#[test]
fn filter_changes_split_material_snapshots() {
    let mut rdp = state([FLAT; 2], 0);
    let mut rsp = Rsp::default();
    let mut scene = Scene::default();
    let mut diags = Vec::new();
    let mut indices = Vec::new();
    for mode in [0, 2, 3, 0] {
        rsp.set_other_mode_h_raw(12, 2, mode << 12, &mut rdp);
        rsp.material_dirty = true;
        indices.push(
            crate::hle::rsp::snapshot_run(&mut rsp, &rdp, &mut diags, &mut scene, 0)
                .unwrap()
                .0,
        );
    }
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(indices, [0, 1, 2, 3]);
}
const SLOTS: [&str; 8] = ["CA", "CB", "CC", "CD", "AA", "AB", "AC", "AD"];
const SUPPORT: [&[bool]; 8] = [
    &[
        true, true, true, true, true, true, true, false, true, true, true, true, true, true, true,
        true,
    ],
    &[
        true, true, true, true, true, true, false, false, true, true, true, true, true, true, true,
        true,
    ],
    &[
        true, true, true, true, true, true, false, false, true, true, true, true, true, true, true,
        false, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true, true,
    ],
    &[true; 8],
    &[true; 8],
    &[true; 8],
    &[true; 8],
    &[true; 8],
];

fn state(cycles: [[u32; 8]; 2], cycle_type: u32) -> Rdp {
    let pass = |s: &[u32]| CcPass {
        a: s[0],
        b: s[1],
        c: s[2],
        d: s[3],
    };
    let (combine_l, combine_h) = gdp_set_combine_lerp(
        pass(&cycles[0][..4]),
        pass(&cycles[0][4..]),
        pass(&cycles[1][..4]),
        pass(&cycles[1][4..]),
    );
    let mut rdp = Rdp {
        combine_l,
        combine_h,
        other_mode_h: cycle_type << 20,
        tmem: vec![48; 16],
        prim: [64, 96, 128, 80],
        env: [160, 192, 224, 208],
        prim_lod_frac: 112.0 / 256.0,
        ..Default::default()
    };
    rdp.tmem_bank
        .write_block(&[32, 48, 64, 112, 32, 48, 64, 112], 0, 0, 0, 1, 3);
    rdp.tmem_bank
        .write_block(&[176, 192, 208, 240, 176, 192, 208, 240], 1, 0, 0, 1, 3);
    for i in 0..2 {
        rdp.tiles[i] = TileDescriptor {
            fmt: 0,
            siz: 3,
            width: 2,
            height: 1,
            line: 1,
            tmem_addr: i as u16,
            cms: 2,
            cmt: 2,
            ..Default::default()
        };
    }
    rdp
}

fn material(rdp: &Rdp, rect: bool) -> (Option<Material>, Vec<Diagnostic>) {
    let mut rsp = Rsp::default();
    rsp.texture_state.on = !rect;
    let mut diags = Vec::new();
    let result = if rect {
        let mut scene = Scene::default();
        let _ = crate::hle::rsp::snapshot_rect_run(&rsp, rdp, 0, &mut diags, &mut scene, 0x1234);
        scene.materials.pop()
    } else {
        build_material(rdp, &rsp, &mut diags, 0x1234)
    };
    (result, diags)
}

#[test]
fn combiner_support_matrix_all_slots() {
    let mut failures = Vec::new();
    for cycle_type in 0..=1 {
        for cycle in 0..2 {
            for (slot, encodings) in SUPPORT.iter().enumerate() {
                for (encoding, &supported) in encodings.iter().enumerate() {
                    let mut cycles = [FLAT; 2];
                    cycles[cycle][slot] = encoding as u32;
                    let rdp = state(cycles, cycle_type);
                    let texel1 = encoding == 2 || (slot == 2 && encoding == 9);
                    let rejected = (cycle_type == 1 || cycle == 1)
                        && (!supported || (cycle_type == 0 && texel1));
                    let expected = if rejected {
                        let kind = match (slot, encoding) {
                            (1, 6) => DiagKind::UnsupportedKeyInput {
                                selector: crate::diag::KeyInput::Center,
                            },
                            (2, 6) => DiagKind::UnsupportedKeyInput {
                                selector: crate::diag::KeyInput::Scale,
                            },
                            (1, 7) => DiagKind::UnsupportedConvertInput {
                                selector: crate::diag::ConvertInput::K4,
                            },
                            (2, 15) => DiagKind::UnsupportedConvertInput {
                                selector: crate::diag::ConvertInput::K5,
                            },
                            _ => DiagKind::UnwiredSelector {
                                slots: 1 << (slot + if cycle == 0 { 8 } else { 0 }),
                            },
                        };
                        vec![Diagnostic { at: 0x1234, kind }]
                    } else {
                        Vec::new()
                    };
                    for rect in [false, true] {
                        let (mat, diags) = material(&rdp, rect);
                        if diags != expected || mat.is_some() == rejected {
                            failures.push(format!("type {cycle_type} cycle {cycle} {}={encoding} rect={rect}: {diags:?}, expected {expected:?}, material={}", SLOTS[slot], mat.is_some()));
                        }
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "support matrix mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn combiner_rejects_unwired_cycle0() {
    let mut cycles = [FLAT; 2];
    cycles[0][0] = 7;
    cycles[0][2] = 7;
    cycles[1][0] = 7;
    for loaded in [false, true] {
        let mut rdp = state(cycles, 1);
        if !loaded {
            rdp.tmem.clear();
        }
        let (mat, diags) = material(&rdp, false);
        assert!(mat.is_none());
        assert_eq!(
            diags,
            vec![Diagnostic {
                at: 0x1234,
                kind: DiagKind::UnwiredSelector { slots: 0x0501 }
            }],
            "both active cycles must contribute diagnostic slots before TMEM checks"
        );
        let display = diags[0].kind.to_string();
        for name in ["cycle 0 CA", "cycle 0 CC", "cycle 1 CA"] {
            assert!(
                display.contains(name),
                "diagnostic must name {name}: {display}"
            );
        }
    }
}

#[test]
fn combiner_ignores_inactive_selectors() {
    let poison = [7, 6, 7, 2, 2, 2, 2, 2];
    for cycle_type in [0, 2, 3] {
        let active = if cycle_type == 0 {
            [6, 8, 10, 7, 7, 7, 7, 6]
        } else {
            poison
        };
        for rect in [false, true] {
            let (mat, diags) = material(&state([poison, active], cycle_type), rect);
            assert!(
                mat.is_some() && diags.is_empty(),
                "inactive selectors must be ignored for type {cycle_type}, rect={rect}: {diags:?}"
            );
        }
    }
}

#[test]
fn combiner_texrect_uses_shared_validation() {
    {
        let (rdram, entry_addr) = crate::tests::fixtures::fixture("texrect--invalid-combiner");
        let result = crate::hle::interpret_rdram(rdram, entry_addr as u32);
        assert_eq!(
            result.dropped_runs, 1,
            "invalid TexRect must be dropped while the following rectangle survives"
        );
        assert_eq!(result.scene.materials.len(), 1);
        assert_eq!(
            result
                .scene
                .framebuffer_pairs
                .iter()
                .flat_map(|p| &p.ops)
                .filter(|op| matches!(op, crate::hle::SceneOp::TexRect { .. }))
                .count(),
            1
        );
        assert_eq!(result.diags.len(), 1);
        assert_eq!(
            result.diags[0].kind,
            DiagKind::UnwiredSelector { slots: 0x0100 }
        );
    }
    for cycle_type in 0..=1 {
        let mut cycles = [FLAT; 2];
        cycles[cycle_type as usize ^ 1][0] = 7;
        let mut rdp = state(cycles, cycle_type);
        rdp.tmem.clear();
        let triangle = material(&rdp, false);
        let rect = material(&rdp, true);
        assert!(
            rect.0.is_none(),
            "TexRect must refuse an active NOISE selector before decoding"
        );
        assert_eq!(
            rect.1, triangle.1,
            "triangle and TexRect validation must agree"
        );
    }
}

#[test]
fn combiner_cycle1_texel1_reads_only_physical0() {
    for slot in [0, 1, 2, 3, 4, 5, 6, 7] {
        for encoding in if slot == 2 { &[2, 9][..] } else { &[2][..] } {
            let mut cycles = [FLAT; 2];
            cycles[1][slot] = *encoding;
            for rect in [false, true] {
                let (mat, diags) = material(&state(cycles, 1), rect);
                assert!(diags.is_empty(), "{diags:?}");
                let mat = mat.unwrap();
                assert!(
                    mat.tex_enable,
                    "cycle 1 {}={encoding} must enable physical 0, rect={rect}",
                    SLOTS[slot]
                );
                assert_eq!(mat.tile_count, 1);
                assert!(mat.tex1.is_none());
                assert_eq!(mat.texture, [32, 48, 64, 112].repeat(2));
            }
        }
    }
    for cycle in 0..2 {
        for token in [1, 2] {
            let mut cycles = [FLAT; 2];
            cycles[cycle][3] = token;
            for rect in [false, true] {
                let (mat, diags) = material(&state(cycles, 1), rect);
                assert!(diags.is_empty(), "{diags:?}");
                let mat = mat.unwrap();
                let physical0 = [(0, 1), (1, 2)].contains(&(cycle, token));
                assert_eq!(mat.tex_enable, physical0);
                if physical0 {
                    assert_eq!(mat.texture, [32, 48, 64, 112].repeat(2));
                } else {
                    assert_eq!(
                        mat.texture,
                        vec![0; 4],
                        "unused physical 0 must not be decoded"
                    );
                    assert_eq!(mat.tex1.unwrap().texture, [176, 192, 208, 240].repeat(2));
                }
            }
        }
    }
}

#[test]
fn combiner_missing_texture_checks_both_cycles() {
    for cycle in 0..2 {
        for slot in 0..8 {
            for token in if slot == 2 {
                &[1, 2, 8, 9][..]
            } else {
                &[1, 2][..]
            } {
                let mut cycles = [FLAT; 2];
                cycles[cycle][slot] = *token;
                let mut rdp = state(cycles, 1);
                rdp.tmem.clear();
                for rect in [false, true] {
                    let (mat, diags) = material(&rdp, rect);
                    assert!(
                        mat.is_none(),
                        "missing TMEM must reject cycle {cycle} {}={token}, rect={rect}",
                        SLOTS[slot]
                    );
                    assert_eq!(
                        diags,
                        vec![Diagnostic {
                            at: 0x1234,
                            kind: DiagKind::NoTextureLoaded
                        }]
                    );
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod pixels {
    use super::*;
    use crate::hle::{AlphaCompare, BlendClass, RenderMode};
    use crate::render::{headless_device, headless_device_forced_fallback, SceneRenderer};
    use crate::tests::common::{pixel, render_to_pixels};

    fn scene() -> Scene {
        let (rdram, entry_addr) = crate::tests::fixtures::fixture("flat-color--vertex-colors");
        let result = crate::hle::interpret_rdram(rdram, entry_addr as u32);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        result.scene
    }

    fn input_value(cycle: usize, slot: usize, encoding: usize) -> [f32; 3] {
        let combined = if cycle == 0 {
            [0.0; 3]
        } else {
            [
                64.0 * 160.0 / 255.0,
                96.0 * 192.0 / 255.0,
                128.0 * 224.0 / 255.0,
            ]
        };
        let combined_alpha = if cycle == 0 {
            0.0
        } else {
            80.0 * 208.0 / 255.0
        };
        let (tex0, tex1) = if cycle == 0 {
            ([32.0, 48.0, 64.0, 112.0], [176.0, 192.0, 208.0, 240.0])
        } else {
            ([176.0, 192.0, 208.0, 240.0], [32.0, 48.0, 64.0, 112.0])
        };
        if slot >= 4 {
            return [match encoding {
                0 if slot == 6 => 255.0,
                0 => combined_alpha,
                1 => tex0[3],
                2 => tex1[3],
                3 => 80.0,
                4 => 144.0,
                5 => 208.0,
                6 if slot == 6 => 112.0 * 255.0 / 256.0,
                6 => 255.0,
                _ => 0.0,
            }; 3];
        }
        match encoding {
            0 => combined,
            1 => [tex0[0], tex0[1], tex0[2]],
            2 => [tex1[0], tex1[1], tex1[2]],
            3 => [64.0, 96.0, 128.0],
            4 => [96.0, 128.0, 160.0],
            5 => [160.0, 192.0, 224.0],
            6 if slot == 0 || slot == 3 => [255.0; 3],
            8 if slot == 2 => [tex0[3]; 3],
            9 if slot == 2 => [tex1[3]; 3],
            10 if slot == 2 => [80.0; 3],
            11 if slot == 2 => [144.0; 3],
            12 if slot == 2 => [208.0; 3],
            13 if slot == 2 => [255.0; 3],
            14 if slot == 2 => [112.0 * 255.0 / 256.0; 3],
            _ => [0.0; 3],
        }
    }

    enum AlphaProbe {
        Threshold(f32),
        Blend(f32),
    }

    fn check_cases(cases: Vec<(String, Rdp, [f32; 3], Option<AlphaProbe>)>) {
        let (device, queue, dual) = headless_device();
        assert!(dual, "selector acceptance requires a dual-source adapter");
        let (fallback, fallback_queue) = headless_device_forced_fallback();
        assert!(!fallback
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING));
        let mut failures = Vec::new();
        for (device, queue, dual) in [(device, queue, true), (fallback, fallback_queue, false)] {
            let mut renderer =
                SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
            let mut scene = scene();
            for (name, rdp, expected_rgb, alpha) in &cases {
                let (mat, diags) = material(rdp, false);
                let Some(mat) = mat else {
                    failures.push(format!("dual={dual} {name}: material rejected: {diags:?}"));
                    continue;
                };
                assert!(diags.is_empty(), "{diags:?}");
                scene.materials = vec![mat];
                scene.render_modes = vec![if matches!(alpha, Some(AlphaProbe::Blend(_))) {
                    RenderMode {
                        blender_mux: 0x0050,
                        force_blend: true,
                        blend_class: BlendClass::DualSrc,
                        fallback_class: BlendClass::AlphaOver,
                        ..Default::default()
                    }
                } else {
                    RenderMode {
                        blender_mux: 0x0a5f,
                        blend_class: BlendClass::DualSrc,
                        fallback_class: BlendClass::Replace,
                        ..Default::default()
                    }
                }];
                let thresholds = match alpha {
                    Some(AlphaProbe::Threshold(a)) => vec![
                        (a.floor() as u8).saturating_sub(1),
                        (a.ceil() as u8).saturating_add(1),
                    ],
                    _ => vec![0],
                };
                for threshold in thresholds {
                    if matches!(alpha, Some(AlphaProbe::Threshold(_))) {
                        scene.render_modes[0].alpha_compare = AlphaCompare::Threshold;
                        scene.materials[0].blend_color[3] = threshold;
                    }
                    let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 64, 64);
                    let got = pixel(&pixels, 64, 32, 32);
                    let clear = [13.0, 13.0, 20.0];
                    let expected = match alpha {
                        Some(AlphaProbe::Threshold(a)) if *a < threshold as f32 => clear,
                        Some(AlphaProbe::Blend(a)) => std::array::from_fn(|i| {
                            clear[i] + (expected_rgb[i] - clear[i]) * a / 255.0
                        }),
                        _ => *expected_rgb,
                    };
                    if got[..3]
                        .iter()
                        .zip(expected)
                        .any(|(&g, e)| (g as f32 - e).abs() > 2.0)
                    {
                        failures.push(format!("dual={dual} {name} threshold={threshold}: pixel {got:?}, expected RGB {expected:?}"));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "selector pixel mismatches:\n{}",
            failures.join("\n")
        );
    }

    fn check_rect_roles() {
        let (rdram, entry_addr) = crate::tests::fixtures::fixture("texrect--combiner-roles");
        let result = crate::hle::interpret_rdram(rdram, entry_addr as u32);
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        let mut scene = result.scene;
        let (device, queue, dual) = headless_device();
        assert!(dual);
        let (fallback, fallback_queue) = headless_device_forced_fallback();
        for (device, queue, dual) in [(device, queue, true), (fallback, fallback_queue, false)] {
            let mut renderer =
                SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
            for (token, expected) in [(1, [176, 192, 208]), (2, [32, 48, 64])] {
                let mut cycle0 = FLAT;
                cycle0[3] = 1;
                let mut cycle1 = FLAT;
                cycle1[3] = token;
                let mut rdp = state([cycle0, cycle1], 1);
                rdp.tmem_bank
                    .write_block(&[8, 16, 24, 255, 176, 192, 208, 240], 1, 0, 0, 1, 3);
                let (mat, diags) = material(&rdp, true);
                assert!(diags.is_empty(), "{diags:?}");
                scene.materials = vec![mat.unwrap()];
                let pixels = render_to_pixels(&device, &queue, &mut renderer, &scene, 64, 64);
                let got = pixel(&pixels, 64, 32, 32);
                assert_eq!(
                    &got[..3],
                    &expected,
                    "TexRect cycle 1 D={token} at texel S=1.5, dual={dual}"
                );
            }
        }
    }

    #[test]
    fn combiner_alpha_color_inputs_render() {
        check_cases(
            [(10, 80.0), (11, 144.0), (12, 208.0)]
                .into_iter()
                .map(|(encoding, expected)| {
                    let cycle = [6, 8, encoding, 7, 7, 7, 7, 6];
                    (
                        format!("one-cycle CC={encoding}"),
                        state([FLAT, cycle], 0),
                        [expected; 3],
                        None,
                    )
                })
                .collect(),
        );
    }

    #[test]
    fn combiner_selector_pixels_dualsrc_and_fallback() {
        let mut cases = Vec::new();
        for cycle in 0..2 {
            for (slot, encodings) in SUPPORT.iter().enumerate() {
                for (encoding, &supported) in encodings.iter().enumerate() {
                    if !supported {
                        continue;
                    }
                    let mut cycles = [[3, 8, 5, 7, 3, 7, 5, 7], [8, 8, 16, 0, 7, 7, 7, 0]];
                    cycles[cycle] = [8, 8, 16, 6, 7, 7, 7, 6];
                    match slot {
                        0 => {
                            cycles[cycle][2] = 13;
                            cycles[cycle][3] = 7;
                        }
                        1 => {
                            cycles[cycle][0] = 6;
                            cycles[cycle][2] = 13;
                            cycles[cycle][3] = 7;
                        }
                        2 => {
                            cycles[cycle][0] = 6;
                            cycles[cycle][3] = 7;
                        }
                        4 => {
                            cycles[cycle][6] = 0;
                            cycles[cycle][7] = 7;
                        }
                        5 => {
                            cycles[cycle][4] = 6;
                            cycles[cycle][6] = 0;
                            cycles[cycle][7] = 7;
                        }
                        6 => {
                            cycles[cycle][4] = 6;
                            cycles[cycle][7] = 7;
                        }
                        _ => {}
                    }
                    cycles[cycle][slot] = encoding as u32;
                    let value = input_value(cycle, slot, encoding).map(|v| {
                        if slot == 1 || slot == 5 {
                            255.0 - v
                        } else {
                            v
                        }
                    });
                    let (rgb, alpha) = if slot < 4 {
                        (value, None)
                    } else {
                        let alpha = if cycle == 1 {
                            AlphaProbe::Blend(value[0])
                        } else {
                            AlphaProbe::Threshold(value[0])
                        };
                        ([255.0; 3], Some(alpha))
                    };
                    cases.push((
                        format!("cycle {cycle} {}={encoding}", SLOTS[slot]),
                        state(cycles, 1),
                        rgb,
                        alpha,
                    ));
                }
            }
        }
        check_cases(cases);
        check_rect_roles();
    }
}
