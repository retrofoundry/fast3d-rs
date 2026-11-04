use super::*;
use crate::profiling::Mode;

fn request(byte: u8, tlut: u8) -> EncodedTextureRequest {
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
            tlut,
            palette_lanes: false,
        },
    )
}

fn forced(request: &EncodedTextureRequest, scheme: TextureKeyScheme) -> ReplacementTarget {
    let mut target = ReplacementTarget::for_request(request, scheme, &Recorder::default()).unwrap();
    target.key.xxh3_64 = 0;
    target
}

#[test]
fn provider_collisions_b_first_recipe_and_tlut_stay_separate() {
    let a = request(1, 2);
    let b = request(2, 2);
    let c = request(1, 3);
    let mut recipe = a.recipe().clone();
    recipe.line = 2;
    let d = EncodedTextureRequest::from_owned_parts(a.input().clone(), recipe);
    for scheme in [TextureKeyScheme::Fast3dV1, TextureKeyScheme::Rt64V5] {
        let provider = ReplacementProvider::new(
            7,
            [ReplacementRegistration {
                target: forced(&a, scheme),
                status: ReplacementStatus::Pending,
            }],
        );
        for request in [&b, &c, &d, &a, &b, &a] {
            let (_, status) =
                provider.resolve_with(|s| Some(forced(request, s)), &Recorder::default());
            assert_eq!(status.is_some(), std::ptr::eq(request, &a));
        }
    }
}

#[test]
fn native_pending_and_invalid_stop_precedence_without_v5_work() {
    let a = request(1, 2);
    for status in [
        ReplacementStatus::Pending,
        ReplacementStatus::Ready(ReplacementImage {
            extent: [0, 0],
            pixels: Arc::from([]),
            format: wgpu::TextureFormat::Rgba8Unorm,
        }),
    ] {
        let provider = ReplacementProvider::new(
            11,
            [
                ReplacementRegistration {
                    target: ReplacementTarget::for_request(
                        &a,
                        TextureKeyScheme::Fast3dV1,
                        &Recorder::default(),
                    )
                    .unwrap(),
                    status,
                },
                ReplacementRegistration {
                    target: ReplacementTarget::for_request(
                        &a,
                        TextureKeyScheme::Rt64V5,
                        &Recorder::default(),
                    )
                    .unwrap(),
                    status: ReplacementStatus::Pending,
                },
            ],
        );
        let stats = Recorder::new(Mode::Counters);
        let (target, status) = provider.resolve(&a, true, &stats);
        assert!(status.is_some());
        assert_eq!(target.key.scheme, TextureKeyScheme::Fast3dV1);
        let snapshot = stats.drain();
        assert_eq!(
            snapshot.counters.get("replacement.provider_calls"),
            Some(&1)
        );
        assert_eq!(snapshot.counters.get("replacement.v5_hashes"), None);
    }
}

#[test]
fn v5_lookup_is_lazy_and_namespaced() {
    let a = request(1, 2);
    let target =
        ReplacementTarget::for_request(&a, TextureKeyScheme::Rt64V5, &Recorder::default()).unwrap();
    let provider = ReplacementProvider::new(
        1,
        [ReplacementRegistration {
            target: target.clone(),
            status: ReplacementStatus::Pending,
        }],
    );
    let stats = Recorder::new(Mode::Counters);
    let (resolved, status) = provider.resolve(&a, true, &stats);
    assert_eq!(resolved, target);
    assert!(status.is_some());
    let counts = stats.drain().counters;
    assert_eq!(counts["replacement.provider_calls"], 2);
    assert_eq!(counts["replacement.v5_hashes"], 1);
    assert_eq!(counts["replacement.v5_bytes_hashed"], 28);
    let mut fake = target;
    fake.key.scheme = TextureKeyScheme::Fast3dV1;
    assert!(provider.lookup(&fake, &Recorder::default()).is_none());
    let empty = ReplacementProvider::new(1, []);
    empty.resolve(&a, true, &stats);
    assert_eq!(stats.drain().counters.get("replacement.v5_hashes"), None);
}

#[test]
fn replacement_extent_length_colour_and_overflow_checks() {
    let a = request(1, 2);
    let mut target =
        ReplacementTarget::for_request(&a, TextureKeyScheme::Fast3dV1, &Recorder::default())
            .unwrap();
    let image = |extent, n, format| ReplacementImage {
        extent,
        pixels: vec![0; n].into(),
        format,
    };
    let rgba = wgpu::TextureFormat::Rgba8Unorm;
    for (extent, n, max, result) in [
        ([4, 2], 32, 4096, Ok(1)),
        ([8, 4], 128, 4096, Ok(2)),
        ([12, 6], 288, 4096, Ok(3)),
        ([8, 5], 160, 4096, Err(ReplacementError::Extent)),
        ([0, 2], 0, 4096, Err(ReplacementError::Extent)),
        ([8, 4], 128, 4, Err(ReplacementError::Extent)),
        ([4, 2], 31, 4096, Err(ReplacementError::Length)),
        (
            [u32::MAX, u32::MAX],
            0,
            u32::MAX,
            Err(ReplacementError::Extent),
        ),
    ] {
        assert_eq!(image(extent, n, rgba).scale(&target, max), result);
    }
    assert_eq!(
        image([4, 2], 32, wgpu::TextureFormat::Rgba8UnormSrgb).scale(&target, 4096),
        Err(ReplacementError::ColorSpace)
    );
    target.extent = [1, 1];
    assert_eq!(
        image([32768, 32768], 0, rgba).scale(&target, u32::MAX),
        Err(ReplacementError::Extent)
    );
    assert_eq!(
        image([32767, 32767], 0, rgba).scale(&target, u32::MAX),
        Err(ReplacementError::Length)
    );
    target.representation = Representation::Lookup4096x4;
    target.extent = [4096, 4];
    assert_eq!(image([4096, 4], 65536, rgba).scale(&target, 8192), Ok(1));
    assert_eq!(
        image([8192, 8], 262144, rgba).scale(&target, 8192),
        Err(ReplacementError::Extent)
    );
}

#[test]
fn palette_replica_bytes_are_part_of_v5_exact_witness() {
    let a = request(1, 2);
    let mut bank = [1; 4096];
    bank[2058] = 7;
    let b = EncodedTextureRequest::from_owned_parts(
        EncodedInput::Tmem(Arc::new(bank)),
        a.recipe().clone(),
    );
    assert_eq!(a.key(), b.key());
    assert_eq!(a.witness(), b.witness());
    let stats = Recorder::default();
    assert_ne!(
        ReplacementTarget::for_request(&a, TextureKeyScheme::Rt64V5, &stats),
        ReplacementTarget::for_request(&b, TextureKeyScheme::Rt64V5, &stats)
    );
}

#[test]
fn native_key_and_request_layout_preserve_s3_allocation_counts() {
    assert_eq!(
        std::mem::size_of::<crate::hle::texture_request::TextureKey>(),
        8
    );
    #[cfg(target_pointer_width = "64")]
    assert_eq!(std::mem::size_of::<EncodedTextureRequest>(), 72);
}

#[test]
fn s2b_native_targets_accept_every_decoded_format_and_palette_lanes() {
    use crate::hle::{
        rdp::Rdp,
        rdp::TileDescriptor,
        texture_request::{prepare, TextureSource},
    };
    for fmt in 0..=7 {
        for siz in 0..=3 {
            for tlut in 0..=3 {
                let mut rdp = Rdp::default();
                rdp.tmem_bank.write_tile(
                    &[0x10, 0xff, 0x30, 0xff, 0x50, 0xff, 0x70, 0xff],
                    256,
                    0,
                    1,
                    1,
                    8,
                    1,
                );
                let tile = TileDescriptor {
                    fmt,
                    siz,
                    width: 2,
                    height: 1,
                    line: 1,
                    ..Default::default()
                };
                let Ok(binding) = prepare(&rdp, &tile, tlut, &Recorder::default()) else {
                    continue;
                };
                let TextureSource::Encoded(request) = binding.source else {
                    panic!()
                };
                let target = ReplacementTarget::new(
                    request.recipe().clone(),
                    request.input().clone(),
                    TextureKeyScheme::Fast3dV1,
                )
                .unwrap();
                assert_eq!(target.witness(), request.witness());
                assert_eq!(target.key().xxh3_64, request.key().xxh3_64);
                assert_eq!(target.extent(), request.recipe().allocation_extent());
                if (tlut >= 2 && fmt != 2)
                    || (fmt == 2 && tlut < 2)
                    || request.recipe().palette_lanes
                {
                    assert!(ReplacementTarget::for_request(
                        &request,
                        TextureKeyScheme::Rt64V5,
                        &Recorder::default()
                    )
                    .is_none());
                }
                let image = ReplacementImage {
                    extent: target.extent(),
                    pixels: request.decode().into(),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                };
                assert_eq!(image.scale(&target, 4096), Ok(1));
                if request.recipe().palette_lanes {
                    let scaled = ReplacementImage {
                        extent: target.extent().map(|x| x * 2),
                        pixels: vec![0; image.pixels.len() * 4].into(),
                        ..image
                    };
                    assert_eq!(scaled.scale(&target, 4096), Err(ReplacementError::Extent));
                }
            }
        }
    }
}

#[test]
fn s2b_nonreplicated_palette_and_t_wrap_have_no_alias() {
    let a = request(1, 2);
    let mut recipe = a.recipe().clone();
    recipe.palette_lanes = true;
    let mut bank = [1; 4096];
    bank[2058] = 99;
    let lanes = EncodedTextureRequest::from_owned_parts(EncodedInput::Tmem(Arc::new(bank)), recipe);
    assert!(
        ReplacementTarget::for_request(&lanes, TextureKeyScheme::Rt64V5, &Recorder::default())
            .is_none()
    );
    let mut recipe = a.recipe().clone();
    recipe.fmt = 4;
    recipe.tlut = 0;
    recipe.output = [1, 257];
    recipe.logical = recipe.output;
    let tall = EncodedTextureRequest::from_owned_parts(a.input().clone(), recipe);
    assert!(
        ReplacementTarget::for_request(&tall, TextureKeyScheme::Rt64V5, &Recorder::default())
            .is_none()
    );
}
