use super::*;
use fast3d::replacement::{
    DecodeRecipe, EncodedInput, ReplacementImage, ReplacementProvider, ReplacementRegistration,
    ReplacementStatus, ReplacementTarget, Representation as DecodeRepresentation, TextureKeyScheme,
};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
#[path = "../../../tools/readbacks/export.rs"]
mod export;

fn target(
    request: &Request,
    scheme: TextureKeyScheme,
) -> Result<ReplacementTarget, fast3d::replacement::ReplacementError> {
    let t = &request.tile;
    let representation = match request.representation {
        Representation::Tile => DecodeRepresentation::Tile,
        Representation::Lookup => DecodeRepresentation::Lookup4096x4,
    };
    let recipe = DecodeRecipe {
        representation,
        output: request.extent,
        logical: [t.width.into(), t.height.into()],
        fmt: t.fmt,
        siz: t.siz,
        base: t.tmem_addr,
        line: t.line,
        palette: if (t.fmt == 2 || request.tlut >= 2) && t.siz == 0 {
            t.palette
        } else {
            0
        },
        tlut: if t.fmt == 2 || request.tlut >= 2 {
            request.tlut
        } else {
            0
        },
        palette_lanes: request.decode().unwrap().len()
            == request.extent[0] as usize * request.extent[1] as usize * 16,
    };
    let bank = Arc::new(request.bank.bytes.as_slice().try_into().unwrap());
    let input = EncodedInput::Tmem(bank);
    ReplacementTarget::new(recipe, input, scheme)
}

fn ready(target: ReplacementTarget, extent: [u32; 2], pixels: Vec<u8>) -> ReplacementRegistration {
    ReplacementRegistration {
        target,
        status: ReplacementStatus::Ready(ReplacementImage {
            extent,
            pixels: pixels.into(),
            format: wgpu::TextureFormat::Rgba8Unorm,
        }),
    }
}

fn identity_provider(fixture: &Fixture, scheme: TextureKeyScheme) -> Arc<ReplacementProvider> {
    Arc::new(ReplacementProvider::new(
        1,
        requests(fixture).iter().filter_map(|request| {
            target(request, scheme).ok().map(|target| {
                let extent = target.extent();
                ready(target, extent, request.decode().unwrap())
            })
        }),
    ))
}

fn expected_color(color: [u8; 3], width: usize, height: usize) -> Vec<u8> {
    let mut pixels = [0, 0, 0, 255].repeat(320 * 240);
    for y in 24..24 + height {
        for x in 16..16 + width {
            pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4]
                .copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
    pixels
}

fn export_case(
    id: &str,
    fixture: &Fixture,
    harness: &Harness,
    pixels: &[u8],
    expected: &[u8],
    before: &[u8],
) {
    assert_eq!(pixels, expected, "{id}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        export::write_row(
            "FAST3D_REPLACEMENT_OUTPUT",
            id,
            "final",
            "replacement",
            "none",
            pixels,
            320,
            240,
            &fixture.to_bytes().unwrap(),
            Some(harness.renderer.device()),
        );
        if let Some(directory) = std::env::var_os("FAST3D_REPLACEMENT_OUTPUT") {
            let directory = std::path::PathBuf::from(directory);
            for (suffix, bytes) in [("expected.bin", expected), ("before.bin", before)] {
                std::fs::write(directory.join(format!("{id}.{suffix}")), bytes).unwrap();
            }
            let mask: Vec<_> = expected
                .as_chunks::<4>()
                .0
                .iter()
                .zip(before.as_chunks::<4>().0)
                .map(|(a, b)| u8::from(a != b))
                .collect();
            std::fs::write(directory.join(format!("{id}.mask.bin")), mask).unwrap();
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (fixture, harness, before);
}

async fn lifecycle() {
    let fixture = recipe_fixture(RecipeCase::NarrowI8, false);
    let native = expected_recipe_pixels(RecipeCase::NarrowI8, false);
    let blue = expected_color([8, 40, 232], 3, 3);
    let red = expected_color([224, 16, 32], 3, 3);
    let request = requests(&fixture).remove(0);
    let target = target(&request, TextureKeyScheme::Fast3dV1).unwrap();
    let mut harness = Harness::new(&fixture).await;
    let scope = harness
        .renderer
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let providers = [
        ("replacement-off", None, &native),
        (
            "replacement-empty",
            Some(Arc::new(ReplacementProvider::new(1, []))),
            &native,
        ),
        (
            "replacement-pending",
            Some(Arc::new(ReplacementProvider::new(
                2,
                [ReplacementRegistration {
                    target: target.clone(),
                    status: ReplacementStatus::Pending,
                }],
            ))),
            &native,
        ),
        (
            "replacement-identity",
            Some(identity_provider(&fixture, TextureKeyScheme::Fast3dV1)),
            &native,
        ),
        (
            "replacement-blue",
            Some(Arc::new(ReplacementProvider::new(
                3,
                [ready(target.clone(), [3, 3], [8, 40, 232, 71].repeat(9))],
            ))),
            &blue,
        ),
        (
            "replacement-red-same-revision",
            Some(Arc::new(ReplacementProvider::new(
                3,
                [ready(target.clone(), [3, 3], [224, 16, 32, 137].repeat(9))],
            ))),
            &red,
        ),
        ("replacement-removed", None, &native),
        (
            "replacement-invalid-extent",
            Some(Arc::new(ReplacementProvider::new(
                4,
                [ready(target.clone(), [6, 5], [0, 0, 0, 0].repeat(30))],
            ))),
            &native,
        ),
        (
            "replacement-invalid-format",
            Some(Arc::new(ReplacementProvider::new(
                5,
                [ReplacementRegistration {
                    target: target.clone(),
                    status: ReplacementStatus::Ready(ReplacementImage {
                        extent: [3, 3],
                        pixels: [0, 0, 0, 0].repeat(9).into(),
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    }),
                }],
            ))),
            &native,
        ),
    ];
    for (id, provider, expected) in providers {
        harness.renderer.set_replacement_provider(provider);
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let counts = harness.recorder.drain();
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(id, &counts);
        assert_eq!(count(&counts, "replacement.v5_hashes"), 0);
        let bytes = if matches!(
            id,
            "replacement-identity" | "replacement-blue" | "replacement-red-same-revision"
        ) {
            36
        } else {
            0
        };
        for (gauge, value) in [
            ("replacement.cached_gpu_bytes", bytes),
            ("replacement.live_gpu_bytes", bytes),
            ("replacement.pinned_gpu_bytes", bytes),
            ("replacement.transient_gpu_bytes", 0),
            ("replacement.in_flight_gpu_bytes", bytes),
            ("replacement.in_flight_entries", bytes / 36),
        ] {
            assert_eq!(
                counts.gauges.get(gauge).copied().unwrap_or(0),
                value,
                "{id}: {gauge}"
            );
        }
        let pixels = harness.pixels().await;
        export_case(id, &fixture, &harness, &pixels, expected, &native);
        harness.recorder.drain();
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let warm = harness.recorder.drain();
        assert_eq!(count(&warm, "replacement.provider_calls"), 0, "{id}");
        assert_eq!(count(&warm, "replacement.upload_calls"), 0, "{id}");
        assert_eq!(count(&warm, "replacement.v5_hashes"), 0, "{id}");
        assert_eq!(harness.pixels().await, *expected);
        harness.recorder.drain();
    }
    let provider = Arc::new(ReplacementProvider::new(
        6,
        [ready(target, [3, 3], [8, 40, 232, 71].repeat(9))],
    ));
    harness
        .renderer
        .set_replacement_provider(Some(provider.clone()));
    harness.renderer.begin_frame();
    harness.process(&fixture, None);
    harness.renderer.set_replacement_provider(None);
    harness.process(&fixture, None);
    let pixels = harness.pixels().await;
    export_case(
        "replacement-frame-frozen",
        &fixture,
        &harness,
        &pixels,
        &blue,
        &native,
    );
    harness.renderer.set_replacement_provider(Some(provider));
    harness.renderer.reconfigure(fixture.frame.config);
    harness.renderer.begin_frame();
    harness.process(&fixture, None);
    let pixels = harness.pixels().await;
    export_case(
        "replacement-new-epoch",
        &fixture,
        &harness,
        &pixels,
        &native,
        &native,
    );
    assert!(scope.pop().await.is_none());
}

async fn identity_roles_and_aliases() {
    for (name, scheme, id) in [
        (
            "tmem-layouts",
            TextureKeyScheme::Fast3dV1,
            "replacement-formats",
        ),
        (
            "tmem-layouts",
            TextureKeyScheme::Rt64V5,
            "replacement-v5-formats",
        ),
        (
            "tmem-roles-lifetime",
            TextureKeyScheme::Fast3dV1,
            "replacement-roles",
        ),
        (
            "tmem-tlut-mutation",
            TextureKeyScheme::Fast3dV1,
            "replacement-palette-banks",
        ),
    ] {
        let fixture = fixture(name);
        let mut harness = Harness::new(&fixture).await;
        harness
            .renderer
            .set_replacement_provider(Some(identity_provider(&fixture, scheme)));
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let counts = harness.recorder.drain();
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(id, &counts);
        assert!(count(&counts, "replacement.matches") > 0);
        assert_eq!(
            count(&counts, "tmem.decode_dispatches"),
            0,
            "{id}: {:?}",
            counts.counters
        );
        assert_eq!(
            count(&counts, "replacement.v5_hashes") > 0,
            scheme == TextureKeyScheme::Rt64V5
        );
        let expected = semantics::expected_pixels(name);
        let pixels = harness.pixels().await;
        export_case(id, &fixture, &harness, &pixels, &expected, &expected);
        harness.recorder.drain();
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let counts = harness.recorder.drain();
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(&format!("{id}-warm"), &counts);
        assert_eq!(count(&counts, "replacement.provider_calls"), 0, "{id}");
        assert_eq!(count(&counts, "replacement.v5_hashes"), 0, "{id}");
        assert_eq!(count(&counts, "replacement.upload_calls"), 0, "{id}");
    }
    for recipe in RecipeCase::ALL {
        let fixture = recipe_fixture(recipe, false);
        let mut harness = Harness::new(&fixture).await;
        harness
            .renderer
            .set_replacement_provider(Some(identity_provider(
                &fixture,
                TextureKeyScheme::Fast3dV1,
            )));
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let expected = expected_recipe_pixels(recipe, false);
        let pixels = harness.pixels().await;
        export_case(
            match recipe {
                RecipeCase::LookupI8 => "replacement-lookup",
                RecipeCase::NarrowI8 => "replacement-narrow-i8",
                RecipeCase::NarrowCi8 => "replacement-narrow-ci8",
            },
            &fixture,
            &harness,
            &pixels,
            &expected,
            &expected,
        );
    }
}

fn artwork() -> Vec<u8> {
    (0..6)
        .flat_map(|y| {
            (0..6).flat_map(move |x| {
                [
                    16 + x * 28,
                    24 + y * 32,
                    8 + x * 12 + y * 8,
                    48 + x * 16 + y * 12,
                ]
            })
        })
        .collect()
}

fn scaled_fixture(recipe: RecipeCase, filter: u32, mirror: bool) -> Fixture {
    let mut fixture = recipe_fixture(recipe, false);
    for span in &mut fixture.tasks[0].spans {
        if span.address == 0x0030_0000 {
            span.bytes = vec![61; 32];
        }
    }
    let mut list = commands(&fixture);
    for command in &mut list {
        if command.0 >> 24 == 0xf3 {
            *command = n64_gbi::encode::gdp_load_block(7, 0, 0, 31, 0);
        }
    }
    let rect = list.iter().position(|(w, _)| w >> 24 == 0xe4).unwrap();
    list[rect].0 = 0xe400_0000 | ((16 + 6) * 4) << 12 | ((24 + 6) * 4);
    list[rect + 1].1 = if filter == 0 { 0 } else { 0x0004_0004 };
    list[rect + 2].1 = 0x0200_0200;
    list.insert(rect, gsp_setothermode_h_f3d(12, 2, filter << 12));
    if mirror {
        // A two-texel mirror period is contained in the three-texel logical image.
        for (w0, w1) in &mut list {
            if *w0 >> 24 == 0xf5 && *w1 >> 24 == 0 {
                *w1 = (*w1 & !((3 << 8) | (15 << 4) | (3 << 18) | (15 << 14)))
                    | (1 << 8)
                    | (1 << 4)
                    | (1 << 18)
                    | (1 << 14);
            }
        }
    }
    let pass = n64_gbi::encode::CcPass {
        a: n64_gbi::encode::ZERO_C,
        b: n64_gbi::encode::ZERO_C,
        c: n64_gbi::encode::ZERO_C,
        d: 1,
    };
    let alpha = n64_gbi::encode::CcPass {
        a: n64_gbi::encode::ZERO_A,
        b: n64_gbi::encode::ZERO_A,
        c: n64_gbi::encode::ZERO_A,
        d: 1,
    };
    for command in &mut list {
        if command.0 >> 24 == 0xfc {
            *command = n64_gbi::encode::gdp_set_combine_lerp(pass, alpha, pass, alpha);
        }
        if command.0 == 0xb900_031d {
            command.1 = 0;
        }
    }
    replace_commands(&mut fixture, list);
    fixture
}

fn scale_native() -> Vec<u8> {
    let mut pixels = expected_color([61; 3], 6, 6);
    for y in 24..30 {
        for x in 16..22 {
            pixels[(y * 320 + x) * 4 + 3] = 61;
        }
    }
    pixels
}

fn scaled_expected(filter: u32, mirror: bool) -> Vec<u8> {
    let art = artwork();
    let mut expected = [0, 0, 0, 255].repeat(320 * 240);
    let axis = |i: usize| {
        if mirror {
            [0, 1, 2, 3, 3, 2, 1][i]
        } else {
            i.min(5)
        }
    };
    for y in 0..6 {
        for x in 0..6 {
            let sample = |dx, dy, c| u32::from(art[(axis(y + dy) * 6 + axis(x + dx)) * 4 + c]);
            for c in 0..4 {
                let value = if filter == 0 {
                    sample(0, 0, c)
                } else {
                    // Pixel centres give 2 * (1/8 + (i + 1/2)/2) = i + 3/4.
                    (sample(1, 1, c) * 2 + sample(1, 0, c) + sample(0, 1, c)) / 4
                };
                expected[((24 + y) * 320 + 16 + x) * 4 + c] = value as u8;
            }
        }
    }
    expected
}

#[test]
fn replacement_scaled_expectations_include_pixel_centers() {
    for (mirror, samples) in [
        (
            false,
            vec![
                (0, 0, [37, 48, 23, 69]),
                (5, 0, [156, 48, 74, 137]),
                (0, 5, [37, 184, 57, 120]),
                (5, 5, [156, 184, 108, 188]),
            ],
        ),
        (
            true,
            vec![
                (0, 0, [37, 48, 23, 69]),
                (3, 3, [100, 120, 68, 132]),
                (4, 0, [79, 48, 41, 93]),
                (5, 5, [51, 64, 33, 83]),
            ],
        ),
    ] {
        let pixels = scaled_expected(2, mirror);
        for (x, y, expected) in samples {
            let offset = ((24 + y) * 320 + 16 + x) * 4;
            assert_eq!(
                &pixels[offset..offset + 4],
                &expected,
                "mirror={mirror}, ({x},{y})"
            );
        }
    }
}

async fn scale_mapping() {
    for (filter, mirror, id) in [
        (0, false, "replacement-2x-point"),
        (2, false, "replacement-2x-three-point"),
        (0, true, "replacement-2x-mirror-point"),
        (2, true, "replacement-2x-mirror-three-point"),
    ] {
        let fixture = scaled_fixture(RecipeCase::NarrowI8, filter, mirror);
        let request = requests(&fixture).remove(0);
        let target = target(&request, TextureKeyScheme::Fast3dV1).unwrap();
        let mut harness = Harness::new(&fixture).await;
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let before = harness.pixels().await;
        assert_eq!(before, scale_native());
        harness.recorder.drain();
        harness
            .renderer
            .set_replacement_provider(Some(Arc::new(ReplacementProvider::new(
                1,
                [ready(target, [6, 6], artwork())],
            ))));
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let counts = harness.recorder.drain();
        assert_eq!(count(&counts, "replacement.upload_calls"), 1);
        assert_eq!(count(&counts, "replacement.upload_bytes"), 144);
        assert_eq!(count(&counts, "tmem.decode_dispatches"), 0);
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(id, &counts);
        let pixels = harness.pixels().await;
        export_case(
            id,
            &fixture,
            &harness,
            &pixels,
            &scaled_expected(filter, mirror),
            &before,
        );
        harness.recorder.drain();
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let warm = harness.recorder.drain();
        assert_eq!(count(&warm, "replacement.provider_calls"), 0);
        assert_eq!(count(&warm, "replacement.upload_calls"), 0);
        assert_eq!(count(&warm, "replacement.v5_hashes"), 0);
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(&format!("{id}-warm"), &warm);
        assert_eq!(harness.pixels().await, pixels);
    }
}

async fn scaled_tlut_point() {
    let mut fixture = scaled_fixture(RecipeCase::NarrowCi8, 0, false);
    let mut list = commands(&fixture);
    let rect = list.iter().position(|(w0, _)| w0 >> 24 == 0xe4).unwrap();
    list.insert(rect, gsp_setothermode_h_f3d(9, 3, 6 << 9));
    replace_commands(&mut fixture, list);
    let request = requests(&fixture).remove(0);
    assert_eq!((request.tile.fmt, request.tlut), (2, 2));
    let target = target(&request, TextureKeyScheme::Fast3dV1).unwrap();
    let mut harness = Harness::new(&fixture).await;
    harness
        .renderer
        .set_replacement_provider(Some(Arc::new(ReplacementProvider::new(
            1,
            [ready(target, [6, 6], artwork())],
        ))));
    harness.renderer.begin_frame();
    harness.process(&fixture, None);
    assert_eq!(
        count(&harness.recorder.drain(), "replacement.upload_calls"),
        1
    );
    assert_eq!(harness.pixels().await, scaled_expected(0, false));
}

gpu_test!(replacement_revision_pending_removal_and_epoch, lifecycle);
gpu_test!(
    replacement_identity_lookup_banks_roles_and_v5,
    identity_roles_and_aliases
);
gpu_test!(
    replacement_asymmetric_scale_point_three_point_mirror_clamp,
    scale_mapping
);
gpu_test!(replacement_scaled_point_tlut_floors, scaled_tlut_point);

#[test]
fn replacement_fixture_targets_are_proved_before_rendering() {
    let layout = fixture("tmem-layouts");
    for request in requests(&layout) {
        assert!(
            target(&request, TextureKeyScheme::Rt64V5).is_ok(),
            "{:?}",
            request.tile
        );
    }
    for (filter, mirror) in [(0, false), (2, false), (0, true), (2, true)] {
        let fixture = scaled_fixture(RecipeCase::NarrowI8, filter, mirror);
        let request = requests(&fixture).remove(0);
        assert_eq!(request.extent, [3, 3]);
        assert_eq!(request.decode().unwrap(), vec![61; 36]);
        assert_eq!(request.representation, Representation::Tile);
        assert!(target(&request, TextureKeyScheme::Fast3dV1).is_ok());
        assert!(target(&request, TextureKeyScheme::Rt64V5).is_ok());
        let expected = scaled_expected(filter, mirror);
        let before = scale_native();
        let changes: Vec<_> = expected
            .as_chunks::<4>()
            .0
            .iter()
            .zip(before.as_chunks::<4>().0)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect();
        assert_eq!(changes.len(), 36);
        assert_eq!(changes[0], 24 * 320 + 16);
        assert_eq!(changes[35], 29 * 320 + 21);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn replacement_literal_inventory() {
    let native = expected_recipe_pixels(RecipeCase::NarrowI8, false);
    let blue = expected_color([8, 40, 232], 3, 3);
    let red = expected_color([224, 16, 32], 3, 3);
    let mut rows = Vec::new();
    for (id, expected) in [
        ("replacement-off", native.clone()),
        ("replacement-empty", native.clone()),
        ("replacement-pending", native.clone()),
        ("replacement-identity", native.clone()),
        ("replacement-blue", blue.clone()),
        ("replacement-red-same-revision", red),
        ("replacement-removed", native.clone()),
        ("replacement-invalid-extent", native.clone()),
        ("replacement-invalid-format", native.clone()),
        ("replacement-frame-frozen", blue),
        ("replacement-new-epoch", native.clone()),
    ] {
        rows.push((id, expected, native.clone()));
    }
    for (id, fixture) in [
        ("replacement-formats", "tmem-layouts"),
        ("replacement-v5-formats", "tmem-layouts"),
        ("replacement-roles", "tmem-roles-lifetime"),
        ("replacement-palette-banks", "tmem-tlut-mutation"),
    ] {
        let expected = semantics::expected_pixels(fixture);
        rows.push((id, expected.clone(), expected));
    }
    for (id, recipe) in [
        ("replacement-lookup", RecipeCase::LookupI8),
        ("replacement-narrow-i8", RecipeCase::NarrowI8),
        ("replacement-narrow-ci8", RecipeCase::NarrowCi8),
    ] {
        let expected = expected_recipe_pixels(recipe, false);
        rows.push((id, expected.clone(), expected));
    }
    for (filter, mirror, id) in [
        (0, false, "replacement-2x-point"),
        (2, false, "replacement-2x-three-point"),
        (0, true, "replacement-2x-mirror-point"),
        (2, true, "replacement-2x-mirror-three-point"),
    ] {
        rows.push((id, scaled_expected(filter, mirror), scale_native()));
    }
    for (id, draw, before, after) in [
        ("replacement-target-tex1", 0, 91, 239),
        ("replacement-target-lod1", 0, 37, 37),
        ("replacement-target-detail", 1, 37, 239),
    ] {
        rows.push((id, role_expected(draw, after), role_expected(draw, before)));
    }
    assert_eq!(rows.len(), 25);
    if let Some(directory) = std::env::var_os("FAST3D_REPLACEMENT_EXPECTATIONS") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (id, expected, before) in rows {
            let mask: Vec<_> = expected
                .as_chunks::<4>()
                .0
                .iter()
                .zip(before.as_chunks::<4>().0)
                .map(|(a, b)| u8::from(a != b))
                .collect();
            std::fs::write(directory.join(format!("{id}.expected.bin")), expected).unwrap();
            std::fs::write(directory.join(format!("{id}.before.bin")), before).unwrap();
            std::fs::write(directory.join(format!("{id}.mask.bin")), mask).unwrap();
        }
    }
}

fn role_expected(draw: usize, value: u8) -> Vec<u8> {
    let mut expected = [0, 0, 0, 255].repeat(320 * 240);
    for y in 112..128 {
        for x in 136 + draw * 16..152 + draw * 16 {
            expected[(y * 320 + x) * 4..(y * 320 + x + 1) * 4]
                .copy_from_slice(&[value, value, value, 255]);
        }
    }
    expected
}

async fn targeted_roles() {
    for (role, before_value, after_value, draw, id) in [
        ("texture1", 91, 239, 0, "replacement-target-tex1"),
        ("lod1", 37, 37, 0, "replacement-target-lod1"),
        ("detail", 37, 239, 1, "replacement-target-detail"),
    ] {
        let fixture = one_role(role);
        let request = requests(&fixture)
            .into_iter()
            .find(|request| request.role == role)
            .unwrap();
        let target = target(&request, TextureKeyScheme::Fast3dV1).unwrap();
        let extent = target.extent();
        let provider = Arc::new(ReplacementProvider::new(
            1,
            [ready(
                target,
                extent,
                vec![239; extent[0] as usize * extent[1] as usize * 4],
            )],
        ));
        let mut harness = Harness::new(&fixture).await;
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let before = harness.pixels().await;
        assert_eq!(before, role_expected(draw, before_value));
        harness.recorder.drain();
        harness.renderer.set_replacement_provider(Some(provider));
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let counts = harness.recorder.drain();
        #[cfg(not(target_arch = "wasm32"))]
        export_snapshot(id, &counts);
        assert_eq!(count(&counts, "replacement.matches"), 1, "{id}");
        assert_eq!(count(&counts, "replacement.upload_calls"), 1, "{id}");
        assert_eq!(count(&counts, "tmem.decode_dispatches"), 0, "{id}");
        let pixels = harness.pixels().await;
        export_case(
            id,
            &fixture,
            &harness,
            &pixels,
            &role_expected(draw, after_value),
            &before,
        );
    }
}

gpu_test!(
    replacement_changes_only_the_exact_independent_role,
    targeted_roles
);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn replacement_shared_fixture_v5_inputs() {
    let mut rows = Vec::new();
    for name in ["tmem-layouts", "tmem-roles-lifetime", "tmem-tlut-mutation"] {
        for (index, request) in requests(&fixture(name)).iter().enumerate() {
            let Ok(target) = target(request, TextureKeyScheme::Rt64V5) else {
                continue;
            };
            if request.tile.fmt != 2 && request.tlut != 0 {
                continue;
            }
            let hex = |bytes: &[u8]| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            };
            rows.push(serde_json::json!({
                "name":format!("{name}-{}-{index}",request.role),
                "width":request.extent[0],"height":request.extent[1],"fmt":request.tile.fmt,"siz":request.tile.siz,
                "base":request.tile.tmem_addr,"line":request.tile.line,
                "palette":if request.tile.fmt==2 && request.tile.siz==0 {request.tile.palette} else {0},
                "tlut":request.tlut,"bank":hex(&request.bank.bytes),
                "xxh3_64":format!("{:016x}",target.key().xxh3_64),"stream":hex(target.witness()),
            }));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = std::env::var_os("FAST3D_V5_FIXTURE_INPUTS") {
        std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
    }
    let frozen: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/tmem/rt64-v5-fixtures.json")).unwrap();
    let expected = frozen["rows"].as_array().unwrap();
    assert_eq!(rows.len(), expected.len());
    for (row, expected) in rows.iter().zip(expected) {
        assert_eq!(row["name"], expected["name"]);
        assert_eq!(row["xxh3_64"], expected["xxh3_64"]);
        assert_eq!(row["stream"], expected["stream"]);
    }
}

#[test]
fn replacement_narrow_requests_include_stale_and_initial_bytes() {
    for recipe in [RecipeCase::NarrowI8, RecipeCase::NarrowCi8] {
        let request = requests(&recipe_fixture(recipe, false)).remove(0);
        assert_eq!(request.representation, Representation::Tile);
        assert_eq!(
            request.decode().unwrap(),
            expected_recipe_decode(recipe, false)
        );
        for scheme in [TextureKeyScheme::Fast3dV1, TextureKeyScheme::Rt64V5] {
            assert!(target(&request, scheme).is_ok());
        }
    }
}

fn framebuffer_fixture(materialized: bool, shortcut: bool) -> Fixture {
    let mut fixture = Fixture::from_bytes(if shortcut {
        include_bytes!("../fixtures/fbload-shortcut-rect.f3dcap")
    } else {
        include_bytes!("../fixtures/fbload-exact-rect.f3dcap")
    })
    .unwrap();
    if materialized {
        let mut list = commands(&fixture);
        for (w0, w1) in &mut list {
            if *w0 >> 24 == 0xf5 && *w1 >> 24 == 0 {
                *w0 = (*w0 & !(0x1f << 19)) | (4 << 21) | (1 << 19);
            }
        }
        replace_commands(&mut fixture, list);
    }
    fixture
}

fn framebuffer_attempts(fixture: &Fixture) -> Vec<fast3d::profiling::CensusAttempt> {
    let mut interpreter = CpuInterpreter::default();
    interpreter.recorder = Recorder::new(Mode::Census);
    for task in &fixture.tasks {
        let hardware = ReplayHardware::new(task, fixture.frame.vi).unwrap();
        let (_, diagnostics) = interpreter.process(
            hardware.rdram(),
            task.entry,
            task.microcode,
            task.data_format,
        );
        hardware.check().unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    interpreter.recorder.drain().census_attempts
}

#[test]
fn replacement_framebuffer_sources_have_no_cpu_candidates() {
    for (materialized, shortcut, mode) in [
        (false, false, "exact"),
        (true, false, "materialized"),
        (false, true, "shortcut"),
    ] {
        let attempts = framebuffer_attempts(&framebuffer_fixture(materialized, shortcut));
        assert!(!attempts.is_empty());
        for attempt in attempts {
            assert!(!attempt.replacement_native && !attempt.replacement_rt64_v5);
            assert!(attempt.witness.is_none());
            if shortcut {
                assert!(attempt.outcome.starts_with("FramebufferShortcut"));
            } else {
                assert_eq!(attempt.framebuffer_decode.as_deref(), Some(mode));
            }
        }
    }
}

async fn framebuffer_exclusion() {
    for (materialized, shortcut) in [(false, false), (true, false), (false, true)] {
        let fixture = framebuffer_fixture(materialized, shortcut);
        let attempts = framebuffer_attempts(&fixture);
        let mut registrations = Vec::new();
        for attempt in attempts {
            let t = attempt.tile;
            let recipe = DecodeRecipe {
                representation: DecodeRepresentation::Tile,
                output: [t.width.into(), t.height.into()],
                logical: [t.width.into(), t.height.into()],
                fmt: t.fmt,
                siz: t.siz,
                base: t.tmem_addr,
                line: t.line,
                palette: 0,
                tlut: 0,
                palette_lanes: false,
            };
            for scheme in [TextureKeyScheme::Fast3dV1, TextureKeyScheme::Rt64V5] {
                if let Ok(target) = ReplacementTarget::new(
                    recipe.clone(),
                    EncodedInput::Tmem(Arc::new(attempt.bank.as_slice().try_into().unwrap())),
                    scheme,
                ) {
                    let extent = target.extent();
                    registrations.push(ready(
                        target,
                        extent,
                        [0, 255, 0, 255].repeat((extent[0] * extent[1]) as usize),
                    ));
                }
            }
        }
        let mut harness = Harness::new(&fixture).await;
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let before = harness.pixels().await;
        harness.recorder.drain();
        harness
            .renderer
            .set_replacement_provider(Some(Arc::new(ReplacementProvider::new(1, registrations))));
        for _ in 0..2 {
            harness.renderer.begin_frame();
            harness.process(&fixture, None);
            let counts = harness.recorder.drain();
            for (name, value) in &counts.counters {
                if name.starts_with("replacement.") {
                    assert_eq!(*value, 0, "{name}");
                }
            }
            if shortcut {
                for name in ["tmem.misses", "tmem.hits", "tmem.decode_dispatches"] {
                    assert_eq!(count(&counts, name), 0, "{name}");
                }
            } else {
                assert!(
                    count(
                        &counts,
                        if materialized {
                            "framebuffer.materialized_requests"
                        } else {
                            "framebuffer.exact_requests"
                        }
                    ) > 0
                );
            }
            assert_eq!(harness.pixels().await, before);
        }
    }
}

gpu_test!(
    replacement_framebuffer_exact_materialized_and_shortcut_excluded,
    framebuffer_exclusion
);

fn tlut_fixture(lanes: bool) -> Fixture {
    use n64_gbi::encode::*;
    let mut fixture = recipe_fixture(RecipeCase::NarrowCi8, false);
    let mut list = commands(&fixture);
    let render_tile = list
        .iter_mut()
        .find(|(w0, w1)| *w0 >> 24 == 0xf5 && *w1 >> 24 == 0)
        .unwrap();
    render_tile.0 = (render_tile.0 & !(7 << 21)) | (4 << 21);
    if lanes {
        fixture.tasks[0].spans.push(MemorySpan {
            address: 0x0030_0200,
            bytes: vec![0xf8, 1, 0x07, 0xc1, 0, 0x3f, 0xff, 0xff],
        });
        let start = list
            .iter()
            .position(|(w0, w1)| *w0 >> 24 == 0xfd && *w1 == 0x0030_0000)
            .unwrap();
        list.splice(
            start..start,
            [
                gdp_set_texture_image(4, 1, 8, 0x0030_0200),
                gdp_set_tile(4, 1, 1, 256, 7, 0, 2, 0, 0, 2, 0, 0),
                (0xf400_0000, (7 << 24) | (28 << 12)),
            ],
        );
    }
    let rect = list.iter().position(|(w0, _)| *w0 >> 24 == 0xe4).unwrap();
    list.insert(rect, gsp_setothermode_h_f3d(12, 2, 2 << 12));
    replace_commands(&mut fixture, list);
    fixture
}

#[test]
fn replacement_s2b_tlut_fixture_decodes_literal_planes() {
    for lanes in [false, true] {
        let request = requests(&tlut_fixture(lanes)).remove(0);
        assert_eq!((request.tile.fmt, request.tlut), (4, 2));
        let pixels = request.decode().unwrap();
        assert_eq!(&pixels[..4], [255, 0, 0, 255]);
        if lanes {
            for (lane, expected) in [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255; 4],
            ]
            .into_iter()
            .enumerate()
            {
                assert_eq!(&pixels[lane * 36..lane * 36 + 4], expected);
            }
        }
        assert_eq!(
            target(&request, TextureKeyScheme::Fast3dV1)
                .unwrap()
                .extent(),
            [3, if lanes { 12 } else { 3 }]
        );
        assert!(target(&request, TextureKeyScheme::Rt64V5).is_err());
    }
}

async fn s2b_tlut_identity() {
    for lanes in [false, true] {
        let fixture = tlut_fixture(lanes);
        let mut harness = Harness::new(&fixture).await;
        harness.renderer.begin_frame();
        harness.process(&fixture, None);
        let before = harness.pixels().await;
        harness.recorder.drain();
        harness
            .renderer
            .set_replacement_provider(Some(identity_provider(
                &fixture,
                TextureKeyScheme::Fast3dV1,
            )));
        for warm in [false, true] {
            harness.renderer.begin_frame();
            harness.process(&fixture, None);
            let counts = harness.recorder.drain();
            assert_eq!(count(&counts, "replacement.upload_calls"), u64::from(!warm));
            assert_eq!(count(&counts, "replacement.v5_hashes"), 0);
            assert_eq!(count(&counts, "tmem.decode_dispatches"), 0);
            assert_eq!(harness.pixels().await, before);
        }
    }
}

gpu_test!(
    replacement_non_ci_tlut_and_palette_planes_keep_sampling,
    s2b_tlut_identity
);
