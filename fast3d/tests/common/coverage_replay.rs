#[allow(dead_code)]
#[path = "coverage_semantics.rs"]
mod semantics;

const WARP_UV_TEXEL_TOLERANCE: f64 = 1.0 / 128.0;

fn warp_uv_policy(adapter: &wgpu::AdapterInfo, kind: semantics::Kind) -> bool {
    adapter.backend == wgpu::Backend::Dx12
        && adapter.device_type == wgpu::DeviceType::Cpu
        && adapter.name == "Microsoft Basic Render Driver"
        && matches!(
            kind,
            semantics::Kind::Perspective | semantics::Kind::PerspectiveNegative
        )
}

fn warp_uv_boundary_matches(case: &semantics::Case, x: u32, y: u32, got: &[u8; 4]) -> bool {
    if !(case.scissor[0]..case.scissor[2]).contains(&x)
        || !(case.scissor[1]..case.scissor[3]).contains(&y)
    {
        return false;
    }
    let Some(draw) = case.draws.iter().rev().find(|draw| {
        draw.survives() && semantics::covered(draw.triangle, x.into(), y.into(), semantics::Rule::A)
    }) else {
        return false;
    };
    let mut uv = semantics::perspective_uv(draw.triangle, [4 * i64::from(x), 4 * i64::from(y)]);
    if case.kind == semantics::Kind::PerspectiveNegative {
        uv = uv.map(|v| 31.0 - v);
    }
    let bands = uv.map(|v| {
        [-WARP_UV_TEXEL_TOLERANCE, WARP_UV_TEXEL_TOLERANCE].map(|offset| {
            let texel = (((v + offset) * 128.0).round() / 128.0).floor() as i64;
            texel.div_euclid(4)
        })
    });
    bands[0].into_iter().any(|s| {
        bands[1]
            .into_iter()
            .any(|t| *got == semantics::COLORS[(s + 2 * t).rem_euclid(3) as usize])
    })
}

pub async fn replay() {
    let captures: &[(&str, &[u8])] = &[
        (
            "coverage-slopes-f3d",
            include_bytes!("../fixtures/coverage-slopes-f3d.f3dcap"),
        ),
        (
            "coverage-slopes-f3dex2",
            include_bytes!("../fixtures/coverage-slopes-f3dex2.f3dcap"),
        ),
        (
            "coverage-ties-f3d",
            include_bytes!("../fixtures/coverage-ties-f3d.f3dcap"),
        ),
        (
            "coverage-ties-f3dex2",
            include_bytes!("../fixtures/coverage-ties-f3dex2.f3dcap"),
        ),
        (
            "coverage-ties-aa",
            include_bytes!("../fixtures/coverage-ties-aa.f3dcap"),
        ),
        (
            "coverage-subpixel-x",
            include_bytes!("../fixtures/coverage-subpixel-x.f3dcap"),
        ),
        (
            "coverage-subpixel-y",
            include_bytes!("../fixtures/coverage-subpixel-y.f3dcap"),
        ),
        (
            "coverage-subpixel-xy",
            include_bytes!("../fixtures/coverage-subpixel-xy.f3dcap"),
        ),
        (
            "coverage-subpixel-half",
            include_bytes!("../fixtures/coverage-subpixel-half.f3dcap"),
        ),
        (
            "coverage-subpixel-yx",
            include_bytes!("../fixtures/coverage-subpixel-yx.f3dcap"),
        ),
        (
            "coverage-subpixel-f3d",
            include_bytes!("../fixtures/coverage-subpixel-f3d.f3dcap"),
        ),
        (
            "coverage-extent-192x128",
            include_bytes!("../fixtures/coverage-extent-192x128.f3dcap"),
        ),
        (
            "coverage-extent-193x132",
            include_bytes!("../fixtures/coverage-extent-193x132.f3dcap"),
        ),
        (
            "coverage-retained-height",
            include_bytes!("../fixtures/coverage-retained-height.f3dcap"),
        ),
        (
            "coverage-cull-back",
            include_bytes!("../fixtures/coverage-cull-back.f3dcap"),
        ),
        (
            "coverage-cull-back-f3d",
            include_bytes!("../fixtures/coverage-cull-back-f3d.f3dcap"),
        ),
        (
            "coverage-cull-front",
            include_bytes!("../fixtures/coverage-cull-front.f3dcap"),
        ),
        (
            "coverage-cull-front-f3d",
            include_bytes!("../fixtures/coverage-cull-front-f3d.f3dcap"),
        ),
        (
            "coverage-shared-0-0",
            include_bytes!("../fixtures/coverage-shared-0-0.f3dcap"),
        ),
        (
            "coverage-shared-0-1",
            include_bytes!("../fixtures/coverage-shared-0-1.f3dcap"),
        ),
        (
            "coverage-shared-1-0",
            include_bytes!("../fixtures/coverage-shared-1-0.f3dcap"),
        ),
        (
            "coverage-shared-1-1",
            include_bytes!("../fixtures/coverage-shared-1-1.f3dcap"),
        ),
        (
            "coverage-clip-scissor",
            include_bytes!("../fixtures/coverage-clip-scissor.f3dcap"),
        ),
        (
            "coverage-near-plane",
            include_bytes!("../fixtures/coverage-near-plane.f3dcap"),
        ),
        (
            "coverage-uv-perspective",
            include_bytes!("../fixtures/coverage-uv-perspective.f3dcap"),
        ),
        (
            "coverage-uv-perspective-negative",
            include_bytes!("../fixtures/coverage-uv-perspective-negative.f3dcap"),
        ),
        (
            "coverage-shade",
            include_bytes!("../fixtures/coverage-shade.f3dcap"),
        ),
        (
            "coverage-rect-controls",
            include_bytes!("../fixtures/coverage-rect-controls.f3dcap"),
        ),
        (
            "coverage-lod-perspective",
            include_bytes!("../fixtures/coverage-lod-perspective.f3dcap"),
        ),
        (
            "coverage-alpha-threshold",
            include_bytes!("../fixtures/coverage-alpha-threshold.f3dcap"),
        ),
        (
            "coverage-fog",
            include_bytes!("../fixtures/coverage-fog.f3dcap"),
        ),
        (
            "coverage-dither",
            include_bytes!("../fixtures/coverage-dither.f3dcap"),
        ),
        (
            "coverage-decal-depth",
            include_bytes!("../fixtures/coverage-decal-depth.f3dcap"),
        ),
        (
            "coverage-shared-0-0-alpha",
            include_bytes!("../fixtures/coverage-shared-0-0-alpha.f3dcap"),
        ),
        (
            "coverage-shared-0-1-alpha",
            include_bytes!("../fixtures/coverage-shared-0-1-alpha.f3dcap"),
        ),
        (
            "coverage-shared-1-0-alpha",
            include_bytes!("../fixtures/coverage-shared-1-0-alpha.f3dcap"),
        ),
        (
            "coverage-shared-1-1-alpha",
            include_bytes!("../fixtures/coverage-shared-1-1-alpha.f3dcap"),
        ),
        (
            "coverage-shared-uv",
            include_bytes!("../fixtures/coverage-shared-uv.f3dcap"),
        ),
    ];
    let cases = semantics::cases();
    assert_eq!(captures.len(), cases.len());
    for (name, bytes) in captures {
        let case = cases.iter().find(|c| c.name == *name).unwrap();
        let fixture = fast3d::capture::Fixture::from_bytes(bytes).unwrap();
        assert!(!fixture.frame.dual_source_blending);
        let output = fixture
            .replay_headless()
            .await
            .expect("coverage requires a working WebGPU adapter");
        let adapter = output.adapter_info.as_ref().unwrap();
        #[cfg(target_arch = "wasm32")]
        {
            assert_eq!(adapter.backend, wgpu::Backend::BrowserWebGpu);
            wasm_bindgen_test::console_log!("coverage: {} {:?}", name, adapter);
        }
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!("coverage: {} {:?}", name, adapter);
        assert_eq!((output.width, output.height), (case.width, case.height));
        assert!(output.diagnostics.iter().all(Vec::is_empty));
        assert!(output
            .summaries
            .iter()
            .all(|s| s.renderable && s.errors == 0));
        let expected = case.expected(semantics::Rule::A);
        assert_eq!(output.rgba8.len(), expected.len());
        let warp_uv = warp_uv_policy(adapter, case.kind);
        let mut boundary_differences = 0;
        for (i, (got, want)) in output
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .enumerate()
        {
            let tolerance = u8::from(
                *want != semantics::BLACK
                    && matches!(
                        case.kind,
                        semantics::Kind::Shade
                            | semantics::Kind::Fog
                            | semantics::Kind::Alpha
                            | semantics::Kind::SharedAlpha
                    ),
            );
            if got
                .iter()
                .zip(want)
                .all(|(g, w)| g.abs_diff(*w) <= tolerance)
            {
                continue;
            }
            let x = i as u32 % case.width;
            let y = i as u32 / case.width;
            assert!(
                warp_uv && warp_uv_boundary_matches(case, x, y, got),
                "{} pixel ({},{}): {:?} != {:?}, tolerance {}, WARP UV boundary policy {}",
                name,
                x,
                y,
                got,
                want,
                tolerance,
                warp_uv
            );
            boundary_differences += 1;
        }
        if warp_uv {
            eprintln!(
                "coverage: {name} WARP UV boundary differences: {boundary_differences}; \
                 UV tolerance {WARP_UV_TEXEL_TOLERANCE} texel per axis; coverage and RGBA palette exact"
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use semantics::{cases, Kind, Rule, BLACK, COLORS};

    fn warp() -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "Microsoft Basic Render Driver".into(),
            vendor: 0,
            device: 0,
            device_type: wgpu::DeviceType::Cpu,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend: wgpu::Backend::Dx12,
            subgroup_min_size: 4,
            subgroup_max_size: 128,
            transient_saves_memory: false,
        }
    }

    #[test]
    fn coverage_warp_policy_requires_adapter_and_unequal_w() {
        let adapter = warp();
        for case in cases() {
            assert_eq!(
                warp_uv_policy(&adapter, case.kind),
                matches!(case.kind, Kind::Perspective | Kind::PerspectiveNegative)
            );
        }
        for backend in [
            wgpu::Backend::Metal,
            wgpu::Backend::BrowserWebGpu,
            wgpu::Backend::Vulkan,
        ] {
            assert!(!warp_uv_policy(
                &wgpu::AdapterInfo { backend, ..warp() },
                Kind::Perspective
            ));
        }
        assert!(!warp_uv_policy(
            &wgpu::AdapterInfo {
                device_type: wgpu::DeviceType::IntegratedGpu,
                ..warp()
            },
            Kind::Perspective
        ));
        assert!(!warp_uv_policy(
            &wgpu::AdapterInfo {
                name: "another CPU adapter".into(),
                ..warp()
            },
            Kind::Perspective
        ));
    }

    #[test]
    fn coverage_warp_boundary_preserves_coverage_alpha_and_other_bands() {
        let mut case = cases()
            .into_iter()
            .find(|c| c.kind == Kind::Perspective)
            .unwrap();
        let expected = case.expected(Rule::A);
        let i = ((48 * case.width + 174) * 4) as usize;
        assert_eq!(expected[i..i + 4], COLORS[1]);
        assert!(warp_uv_boundary_matches(&case, 174, 48, &COLORS[2]));
        for wrong in [BLACK, COLORS[0], [0, 0, 255, 254], [0, 1, 255, 255]] {
            assert!(!warp_uv_boundary_matches(&case, 174, 48, &wrong));
        }
        for color in COLORS {
            assert!(!warp_uv_boundary_matches(&case, 32, 192, &color));
        }
        assert!(warp_uv_boundary_matches(&case, 32, 32, &COLORS[0]));
        assert!(!warp_uv_boundary_matches(&case, 32, 32, &COLORS[1]));
        case.scissor[2] = 174;
        assert!(!warp_uv_boundary_matches(&case, 174, 48, &COLORS[2]));
    }

    #[test]
    fn coverage_warp_boundary_budget_rejects_b_and_wrong_sample_origin() {
        for (kind, budget) in [(Kind::Perspective, 97), (Kind::PerspectiveNegative, 124)] {
            let case = cases().into_iter().find(|c| c.kind == kind).unwrap();
            let a = case.expected(Rule::A);
            let b = case.expected(Rule::B);
            let mut ambiguous = 0;
            let mut rejected_b = 0;
            let mut rejected_center = 0;
            for (i, (want, other)) in a
                .as_chunks::<4>()
                .0
                .iter()
                .zip(b.as_chunks::<4>().0)
                .enumerate()
            {
                let x = i as u32 % case.width;
                let y = i as u32 / case.width;
                ambiguous +=
                    usize::from(COLORS.iter().any(|color| {
                        color != want && warp_uv_boundary_matches(&case, x, y, color)
                    }));
                rejected_b +=
                    usize::from(want != other && !warp_uv_boundary_matches(&case, x, y, other));
                if *want == BLACK {
                    continue;
                }
                assert!(warp_uv_boundary_matches(&case, x, y, want));
                let mut uv = semantics::perspective_uv(
                    case.draws[0].triangle,
                    [4 * i64::from(x) + 2, 4 * i64::from(y) + 2],
                );
                if kind == Kind::PerspectiveNegative {
                    uv = uv.map(|v| 31.0 - v);
                }
                let bands =
                    uv.map(|v| (((v * 128.0).round() / 128.0).floor() as i64).div_euclid(4));
                let center = COLORS[(bands[0] + 2 * bands[1]).rem_euclid(3) as usize];
                rejected_center +=
                    usize::from(*want != center && !warp_uv_boundary_matches(&case, x, y, &center));
            }
            assert_eq!(ambiguous, budget, "{kind:?}");
            assert_eq!(rejected_b, 160, "{kind:?}");
            assert!(rejected_center > 500, "{kind:?}: {rejected_center}");
            eprintln!("{kind:?}: {ambiguous} boundary pixels, {rejected_b} rejected B pixels, {rejected_center} rejected center-UV pixels");
        }
    }
}
