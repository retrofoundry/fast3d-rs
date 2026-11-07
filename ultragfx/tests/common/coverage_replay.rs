#[allow(dead_code)]
#[path = "coverage_semantics.rs"]
mod semantics;

pub async fn replay() {
    let captures: &[(&str, &[u8])] = &[
        (
            "coverage-slopes-f3d",
            include_bytes!("../fixtures/coverage-slopes-f3d.ugfxcap"),
        ),
        (
            "coverage-slopes-f3dex2",
            include_bytes!("../fixtures/coverage-slopes-f3dex2.ugfxcap"),
        ),
        (
            "coverage-ties-f3d",
            include_bytes!("../fixtures/coverage-ties-f3d.ugfxcap"),
        ),
        (
            "coverage-ties-f3dex2",
            include_bytes!("../fixtures/coverage-ties-f3dex2.ugfxcap"),
        ),
        (
            "coverage-ties-aa",
            include_bytes!("../fixtures/coverage-ties-aa.ugfxcap"),
        ),
        (
            "coverage-subpixel-x",
            include_bytes!("../fixtures/coverage-subpixel-x.ugfxcap"),
        ),
        (
            "coverage-subpixel-y",
            include_bytes!("../fixtures/coverage-subpixel-y.ugfxcap"),
        ),
        (
            "coverage-subpixel-xy",
            include_bytes!("../fixtures/coverage-subpixel-xy.ugfxcap"),
        ),
        (
            "coverage-subpixel-half",
            include_bytes!("../fixtures/coverage-subpixel-half.ugfxcap"),
        ),
        (
            "coverage-subpixel-yx",
            include_bytes!("../fixtures/coverage-subpixel-yx.ugfxcap"),
        ),
        (
            "coverage-subpixel-f3d",
            include_bytes!("../fixtures/coverage-subpixel-f3d.ugfxcap"),
        ),
        (
            "coverage-extent-192x128",
            include_bytes!("../fixtures/coverage-extent-192x128.ugfxcap"),
        ),
        (
            "coverage-extent-193x132",
            include_bytes!("../fixtures/coverage-extent-193x132.ugfxcap"),
        ),
        (
            "coverage-retained-height",
            include_bytes!("../fixtures/coverage-retained-height.ugfxcap"),
        ),
        (
            "coverage-cull-back",
            include_bytes!("../fixtures/coverage-cull-back.ugfxcap"),
        ),
        (
            "coverage-cull-back-f3d",
            include_bytes!("../fixtures/coverage-cull-back-f3d.ugfxcap"),
        ),
        (
            "coverage-cull-front",
            include_bytes!("../fixtures/coverage-cull-front.ugfxcap"),
        ),
        (
            "coverage-cull-front-f3d",
            include_bytes!("../fixtures/coverage-cull-front-f3d.ugfxcap"),
        ),
        (
            "coverage-shared-0-0",
            include_bytes!("../fixtures/coverage-shared-0-0.ugfxcap"),
        ),
        (
            "coverage-shared-0-1",
            include_bytes!("../fixtures/coverage-shared-0-1.ugfxcap"),
        ),
        (
            "coverage-shared-1-0",
            include_bytes!("../fixtures/coverage-shared-1-0.ugfxcap"),
        ),
        (
            "coverage-shared-1-1",
            include_bytes!("../fixtures/coverage-shared-1-1.ugfxcap"),
        ),
        (
            "coverage-clip-scissor",
            include_bytes!("../fixtures/coverage-clip-scissor.ugfxcap"),
        ),
        (
            "coverage-near-plane",
            include_bytes!("../fixtures/coverage-near-plane.ugfxcap"),
        ),
        (
            "coverage-uv-perspective",
            include_bytes!("../fixtures/coverage-uv-perspective.ugfxcap"),
        ),
        (
            "coverage-uv-perspective-negative",
            include_bytes!("../fixtures/coverage-uv-perspective-negative.ugfxcap"),
        ),
        (
            "coverage-shade",
            include_bytes!("../fixtures/coverage-shade.ugfxcap"),
        ),
        (
            "coverage-rect-controls",
            include_bytes!("../fixtures/coverage-rect-controls.ugfxcap"),
        ),
        (
            "coverage-lod-perspective",
            include_bytes!("../fixtures/coverage-lod-perspective.ugfxcap"),
        ),
        (
            "coverage-alpha-threshold",
            include_bytes!("../fixtures/coverage-alpha-threshold.ugfxcap"),
        ),
        (
            "coverage-fog",
            include_bytes!("../fixtures/coverage-fog.ugfxcap"),
        ),
        (
            "coverage-dither",
            include_bytes!("../fixtures/coverage-dither.ugfxcap"),
        ),
        (
            "coverage-decal-depth",
            include_bytes!("../fixtures/coverage-decal-depth.ugfxcap"),
        ),
        (
            "coverage-shared-0-0-alpha",
            include_bytes!("../fixtures/coverage-shared-0-0-alpha.ugfxcap"),
        ),
        (
            "coverage-shared-0-1-alpha",
            include_bytes!("../fixtures/coverage-shared-0-1-alpha.ugfxcap"),
        ),
        (
            "coverage-shared-1-0-alpha",
            include_bytes!("../fixtures/coverage-shared-1-0-alpha.ugfxcap"),
        ),
        (
            "coverage-shared-1-1-alpha",
            include_bytes!("../fixtures/coverage-shared-1-1-alpha.ugfxcap"),
        ),
        (
            "coverage-shared-uv",
            include_bytes!("../fixtures/coverage-shared-uv.ugfxcap"),
        ),
    ];
    let cases = semantics::cases();
    assert_eq!(captures.len(), cases.len());
    for (name, bytes) in captures {
        let case = cases.iter().find(|c| c.name == *name).unwrap();
        let fixture = ultragfx::capture::Fixture::from_bytes(bytes).unwrap();
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
        let expected = case.expected(semantics::Rule::B);
        assert_eq!(output.rgba8.len(), expected.len());
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
            assert!(
                got.iter()
                    .zip(want)
                    .all(|(g, w)| g.abs_diff(*w) <= tolerance),
                "{} pixel ({},{}): {:?} != {:?}, tolerance {}",
                name,
                i % case.width as usize,
                i / case.width as usize,
                got,
                want,
                tolerance
            );
        }
    }
}
