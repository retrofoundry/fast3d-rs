use super::*;
use crate::{
    profiling::Mode,
    replacement::{ReplacementRegistration, Representation, TextureKeyScheme},
};

fn request(byte: u8) -> EncodedTextureRequest {
    EncodedTextureRequest::from_owned_parts(
        EncodedInput::Tmem(Arc::new([byte; 4096])),
        DecodeRecipe {
            representation: Representation::Tile,
            output: [4, 2],
            logical: [4, 2],
            fmt: 2,
            siz: 1,
            base: 0,
            line: 1,
            palette: 0,
            tlut: 2,
            palette_lanes: false,
        },
    )
}
fn provider(
    request: &EncodedTextureRequest,
    scheme: TextureKeyScheme,
    status: ReplacementStatus,
) -> Arc<ReplacementProvider> {
    Arc::new(ReplacementProvider::new(
        1,
        [ReplacementRegistration {
            target: ReplacementTarget::for_request(request, scheme, &Recorder::default()).unwrap(),
            status,
        }],
    ))
}
fn ready(value: u8) -> ReplacementStatus {
    ReplacementStatus::Ready(ReplacementImage {
        extent: [4, 2],
        pixels: vec![value; 32].into(),
        format: wgpu::TextureFormat::Rgba8Unorm,
    })
}
fn resolve(
    cache: &mut Replacements<u8>,
    request: &EncodedTextureRequest,
    stats: &Recorder,
) -> Option<(u8, u32)> {
    cache.resolve_with(request, true, 4096, stats, |image| image.pixels[0])
}

#[test]
fn frame_freeze_revision_and_removal() {
    let a = request(1);
    let stats = Recorder::new(Mode::Counters);
    let mut frames = ProviderFrames::<u8>::default();
    frames.begin_frame();
    assert!(frames.active.is_none());
    assert!(stats.drain().counters.is_empty());
    frames.queue(Some(provider(&a, TextureKeyScheme::UltragfxV1, ready(17))));
    assert!(frames.active.is_none());
    frames.begin_frame();
    assert_eq!(
        resolve(frames.active.as_mut().unwrap(), &a, &stats),
        Some((17, 1))
    );
    frames.queue(Some(provider(&a, TextureKeyScheme::UltragfxV1, ready(91))));
    assert_eq!(
        resolve(frames.active.as_mut().unwrap(), &a, &stats),
        Some((17, 1))
    );
    frames.begin_frame();
    assert_eq!(
        resolve(frames.active.as_mut().unwrap(), &a, &stats),
        Some((91, 1))
    );
    assert_eq!(stats.drain().counters["replacement.provider_calls"], 2);
    frames.queue(None);
    assert!(frames.active.is_some());
    frames.begin_frame();
    assert!(frames.active.is_none());
}

#[test]
fn decisions_cache_hits_misses_pending_and_aliases_until_revision_change() {
    let a = request(1);
    let b = request(2);
    let stats = Recorder::new(Mode::Counters);
    for status in [ready(17), ReplacementStatus::Pending] {
        let mut cache = Replacements::new(provider(&a, TextureKeyScheme::Rt64V5, status.clone()));
        for query in [&b, &a, &b, &a] {
            resolve(&mut cache, query, &stats);
        }
        let counts = stats.drain().counters;
        assert_eq!(counts["replacement.provider_calls"], 4);
        assert_eq!(counts["replacement.v5_hashes"], 2);
        assert_eq!(counts["replacement.decision_hits"], 2);
        assert_eq!(counts["replacement.no_match"], 1);
        assert_eq!(
            counts.get("replacement.upload_calls").copied().unwrap_or(0),
            u64::from(matches!(status, ReplacementStatus::Ready(_)))
        );
    }
}

#[test]
fn eviction_reinsertion_and_full_palette_slot_invalidation() {
    let a = request(1);
    let b = request(2);
    let stats = Recorder::new(Mode::Counters);
    let mut cache = Replacements::new(provider(&a, TextureKeyScheme::Rt64V5, ready(17)));
    cache.entry_limit = 1;
    assert_eq!(resolve(&mut cache, &b, &stats), None);
    assert_eq!(resolve(&mut cache, &a, &stats), Some((17, 1)));
    assert_eq!(resolve(&mut cache, &b, &stats), None);
    assert_eq!(resolve(&mut cache, &a, &stats), Some((17, 1)));
    assert_eq!(stats.drain().counters["replacement.evictions"], 3);
    let mut bank = [1; 4096];
    bank[2058] = 99;
    let c = EncodedTextureRequest::from_owned_parts(
        EncodedInput::Tmem(Arc::new(bank)),
        a.recipe().clone(),
    );
    assert_eq!(a.key(), c.key());
    assert_eq!(resolve(&mut cache, &c, &stats), None);
    assert_eq!(stats.drain().counters.get("replacement.v5_hashes"), None);
}

#[test]
fn oversized_and_zero_entry_decisions_bypass_without_losing_owned_pixels() {
    let a = request(1);
    let stats = Recorder::new(Mode::Counters);
    let mut cache = Replacements::new(provider(&a, TextureKeyScheme::UltragfxV1, ready(17)));
    cache.entry_limit = 0;
    for _ in 0..2 {
        assert_eq!(resolve(&mut cache, &a, &stats), Some((17, 1)));
    }
    let counts = stats.drain().counters;
    assert_eq!(counts["replacement.bypasses"], 2);
    assert_eq!(counts["replacement.upload_calls"], 2);
    assert!(cache.decisions.is_empty());
    let mut recipe = a.recipe().clone();
    recipe.output = [2048, 1024];
    recipe.logical = recipe.output;
    let large = EncodedTextureRequest::from_owned_parts(a.input().clone(), recipe);
    let mut cache = Replacements::new(provider(
        &large,
        TextureKeyScheme::UltragfxV1,
        ReplacementStatus::Ready(ReplacementImage {
            extent: [2048, 1024],
            pixels: vec![71; 8 * 1024 * 1024].into(),
            format: wgpu::TextureFormat::Rgba8Unorm,
        }),
    ));
    assert_eq!(resolve(&mut cache, &large, &stats), Some((71, 1)));
    assert_eq!(stats.drain().counters["replacement.bypasses"], 1);
    assert!(cache.decisions.is_empty());
}

#[test]
fn raw_tlut_on_non_ci_blocks_alias_even_with_a_warm_allowed_decision() {
    let r = DecodeRecipe {
        representation: Representation::Tile,
        output: [4, 2],
        logical: [4, 2],
        fmt: 4,
        siz: 1,
        base: 0,
        line: 1,
        palette: 0,
        tlut: 0,
        palette_lanes: false,
    };
    let request =
        EncodedTextureRequest::from_owned_parts(EncodedInput::Tmem(Arc::new([17; 4096])), r);
    let mut cache = Replacements::new(provider(&request, TextureKeyScheme::Rt64V5, ready(71)));
    let stats = Recorder::new(Mode::Counters);
    assert_eq!(
        cache.resolve_with(&request, true, 4096, &stats, |image| image.pixels[0]),
        Some((71, 1))
    );
    stats.drain();
    assert_eq!(
        cache.resolve_with(&request, false, 4096, &stats, |image| image.pixels[0]),
        None
    );
    let counts = stats.drain().counters;
    assert_eq!(counts["replacement.provider_calls"], 1);
    assert_eq!(counts.get("replacement.v5_hashes"), None);
}

#[test]
fn decisions_ignore_unreferenced_bytes_but_track_used_palette_slots() {
    let a = request(1);
    let mut bank = [1; 4096];
    bank[1000] = 93;
    bank[2066] = 27;
    let unrelated = EncodedTextureRequest::from_owned_parts(
        EncodedInput::Tmem(Arc::new(bank)),
        a.recipe().clone(),
    );
    bank[2058] = 99;
    let used_slot = EncodedTextureRequest::from_owned_parts(
        EncodedInput::Tmem(Arc::new(bank)),
        a.recipe().clone(),
    );
    for scheme in [TextureKeyScheme::UltragfxV1, TextureKeyScheme::Rt64V5] {
        for status in [ready(71), ReplacementStatus::Pending] {
            let mut cache = Replacements::new(provider(&a, scheme, status.clone()));
            let stats = Recorder::new(Mode::Counters);
            let expected = matches!(status, ReplacementStatus::Ready(_)).then_some((71, 1));
            assert_eq!(resolve(&mut cache, &a, &stats), expected);
            stats.drain();
            assert_eq!(resolve(&mut cache, &unrelated, &stats), expected);
            let counts = stats.drain().counters;
            assert_eq!(counts["replacement.decision_hits"], 1);
            assert!(!counts.contains_key("replacement.provider_calls"));
            assert!(!counts.contains_key("replacement.v5_hashes"));
            assert_eq!(
                resolve(&mut cache, &used_slot, &stats),
                if scheme == TextureKeyScheme::UltragfxV1 {
                    expected
                } else {
                    None
                }
            );
            let counts = stats.drain().counters;
            assert_eq!(counts.get("replacement.v5_hashes").copied().unwrap_or(0), 0);
        }
    }
    let stats = Recorder::new(Mode::Counters);
    let mut cache = Replacements::<u8>::new(Arc::new(ReplacementProvider::new(1, [])));
    assert_eq!(resolve(&mut cache, &a, &stats), None);
    stats.drain();
    assert_eq!(resolve(&mut cache, &unrelated, &stats), None);
    assert_eq!(stats.drain().counters["replacement.decision_hits"], 1);
}

#[test]
fn changed_input_checks_native_priority_after_a_cached_alias() {
    let a = request(1);
    let b = request(2);
    let stats = Recorder::new(Mode::Counters);
    let provider = Arc::new(ReplacementProvider::new(
        1,
        [
            ReplacementRegistration {
                target: ReplacementTarget::for_request(&a, TextureKeyScheme::Rt64V5, &stats)
                    .unwrap(),
                status: ready(71),
            },
            ReplacementRegistration {
                target: ReplacementTarget::for_request(&b, TextureKeyScheme::UltragfxV1, &stats)
                    .unwrap(),
                status: ready(99),
            },
        ],
    ));
    let mut cache = Replacements::new(provider);
    assert_eq!(resolve(&mut cache, &a, &stats), Some((71, 1)));
    stats.drain();
    assert_eq!(resolve(&mut cache, &b, &stats), Some((99, 1)));
    let counts = stats.drain().counters;
    assert_eq!(counts["replacement.provider_calls"], 1);
    assert!(!counts.contains_key("replacement.v5_hashes"));
    assert_eq!(resolve(&mut cache, &a, &stats), Some((71, 1)));
    assert_eq!(resolve(&mut cache, &b, &stats), Some((99, 1)));
    assert_eq!(stats.drain().counters["replacement.decision_hits"], 2);
}

#[cfg(feature = "capture")]
#[test]
fn display_list_reloads_reuse_replacement_decisions() {
    use crate::{
        capture::{Fixture, ReplayHardware},
        profiling::CpuInterpreter,
        Hardware,
    };

    for (name, bytes, expected_requests) in [
        (
            "layouts",
            include_bytes!("../../../tests/fixtures/tmem-layouts.ugfxcap").as_slice(),
            18,
        ),
        (
            "roles",
            include_bytes!("../../../tests/fixtures/tmem-roles-lifetime.ugfxcap").as_slice(),
            9,
        ),
        (
            "palette",
            include_bytes!("../../../tests/fixtures/tmem-tlut-mutation.ugfxcap").as_slice(),
            5,
        ),
    ] {
        let fixture = Fixture::from_bytes(bytes).unwrap();
        let mut interpreter = CpuInterpreter::default();
        interpreter.recorder = Recorder::new(Mode::Trace);
        let mut frames = Vec::new();
        for _ in 0..3 {
            for task in &fixture.tasks {
                let hardware = ReplayHardware::new(task, None).unwrap();
                let (_, diagnostics) = interpreter.process(
                    hardware.rdram(),
                    task.entry,
                    task.microcode,
                    task.data_format,
                );
                hardware.check().unwrap();
                assert!(diagnostics.is_empty());
            }
            frames.push(
                interpreter
                    .recorder
                    .drain()
                    .requests
                    .into_iter()
                    .map(|request| EncodedTextureRequest::from_diagnostic(&request).unwrap())
                    .collect::<Vec<_>>(),
            );
        }
        for scheme in [TextureKeyScheme::UltragfxV1, TextureKeyScheme::Rt64V5] {
            let provider = Arc::new(ReplacementProvider::new(
                1,
                frames[0].iter().map(|request| ReplacementRegistration {
                    target: ReplacementTarget::for_request(request, scheme, &Recorder::default())
                        .unwrap(),
                    status: ReplacementStatus::Ready(ReplacementImage {
                        extent: request.recipe().output,
                        pixels: vec![
                            71;
                            (request.recipe().output[0] * request.recipe().output[1] * 4)
                                as usize
                        ]
                        .into(),
                        format: wgpu::TextureFormat::Rgba8Unorm,
                    }),
                }),
            ));
            let mut cache = Replacements::new(provider);
            let stats = Recorder::new(Mode::Counters);
            for (frame, requests) in frames.iter().enumerate() {
                assert_eq!(requests.len(), expected_requests);
                for request in requests {
                    assert_eq!(resolve(&mut cache, request, &stats), Some((71, 1)));
                }
                let counts = stats.drain().counters;
                eprintln!("{name} {scheme:?} frame {frame}: {counts:?}");
                if frame > 0 {
                    assert_eq!(
                        counts["replacement.decision_hits"],
                        expected_requests as u64
                    );
                    for name in [
                        "replacement.provider_calls",
                        "replacement.v5_admission_checks",
                        "replacement.v5_hashes",
                        "replacement.v5_bytes_hashed",
                        "replacement.upload_calls",
                        "replacement.upload_bytes",
                        "replacement.decision_misses",
                    ] {
                        assert_eq!(
                            counts.get(name).copied().unwrap_or(0),
                            0,
                            "{scheme:?} frame {frame}: {name}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn s2b_decisions_track_non_ci_tlut_modes_and_all_used_lanes() {
    use crate::hle::{
        rdp::{Rdp, TileDescriptor},
        texture_request::{prepare, TextureSource},
    };
    for fmt in [0, 2, 3, 4] {
        let mut rdp = Rdp::default();
        let tile = TileDescriptor {
            fmt,
            siz: if fmt == 0 { 2 } else { 1 },
            width: 4,
            height: 2,
            line: 1,
            ..Default::default()
        };
        let binding = |rdp: &Rdp, tlut| {
            let TextureSource::Encoded(request) = prepare(rdp, &tile, tlut, &Recorder::default())
                .unwrap()
                .source
            else {
                panic!()
            };
            request
        };
        rdp.tmem_bank.write_tlut(&[0x10, 255], 1, 256);
        rdp.tmem_bank.write_tile(
            &[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255],
            256,
            0,
            1,
            1,
            8,
            1,
        );
        let a = binding(&rdp, 3);
        let target = ReplacementTarget::new(
            a.recipe().clone(),
            a.input().clone(),
            TextureKeyScheme::UltragfxV1,
        )
        .unwrap();
        assert_eq!(target.extent(), [4, 8]);
        assert_eq!(a.decode()[0..4], [16, 16, 16, 255]);
        let status = ReplacementStatus::Ready(ReplacementImage {
            extent: [4, 8],
            pixels: vec![71; 128].into(),
            format: wgpu::TextureFormat::Rgba8Unorm,
        });
        let mut cache = Replacements::new(Arc::new(ReplacementProvider::new(
            1,
            [ReplacementRegistration { target, status }],
        )));
        let stats = Recorder::new(Mode::Counters);
        assert_eq!(resolve(&mut cache, &a, &stats), Some((71, 1)));
        stats.drain();
        rdp.tmem_bank.write_tile(&[9; 8], 257, 0, 1, 1, 8, 1);
        assert_eq!(
            resolve(&mut cache, &binding(&rdp, 3), &stats),
            Some((71, 1))
        );
        assert_eq!(stats.drain().counters["replacement.decision_hits"], 1);
        for lane in 0..4 {
            let mut bytes = [0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255];
            bytes[lane * 2] += 1;
            rdp.tmem_bank.write_tile(&bytes, 256, 0, 1, 1, 8, 1);
            assert_eq!(resolve(&mut cache, &binding(&rdp, 3), &stats), None);
            assert_eq!(stats.drain().counters["replacement.provider_calls"], 1);
        }
        assert_eq!(resolve(&mut cache, &binding(&rdp, 2), &stats), None);
        assert_eq!(stats.drain().counters["replacement.provider_calls"], 1);
        assert_eq!(resolve(&mut cache, &a, &stats), Some((71, 1)));
    }
}
