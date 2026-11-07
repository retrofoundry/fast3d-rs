use super::{readback_requests, test_support};

#[test]
fn texture_decode_gpu_component_and_layout_readbacks() {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let vectors = test_support::vectors();
    let requests: Vec<_> = vectors
        .iter()
        .map(|vector| vector.request.clone())
        .collect();
    let decoded = pollster::block_on(readback_requests(&device, &queue, &requests)).unwrap();
    test_support::verify(&device, &vectors, &decoded);
}

#[test]
fn texture_decode_gpu_literal_load_readbacks() {
    use crate::hle::texture_request::{prepare, TextureSource};
    use crate::profiling::{Recorder, Request};

    let vectors: Vec<_> = crate::hle::tmem::load_tests::literal_load_vectors()
        .into_iter()
        .map(|vector| {
            let binding = prepare(&vector.rdp, &vector.tile, 0, &Recorder::default()).unwrap();
            let TextureSource::Encoded(encoded) = &binding.source else {
                panic!("{}: expected TMEM request", vector.name);
            };
            super::vectors::Vector {
                request: Request::capture(&vector.rdp, &vector.tile, 0, &vector.name, Ok(encoded)),
                name: vector.name,
                expected: vector.expected,
            }
        })
        .collect();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let requests: Vec<_> = vectors
        .iter()
        .map(|vector| vector.request.clone())
        .collect();
    let decoded = pollster::block_on(readback_requests(&device, &queue, &requests)).unwrap();
    test_support::assert_decoded(&vectors, &decoded);
}

#[test]
fn texture_decode_gpu_angrylion_fetch_readbacks() {
    let vectors: Vec<_> = crate::hle::angrylion_tests::decode_vectors()
        .into_iter()
        .map(|(name, request, expected)| super::vectors::Vector {
            name,
            request,
            expected,
        })
        .collect();
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let requests: Vec<_> = vectors.iter().map(|v| v.request.clone()).collect();
    let decoded = pollster::block_on(readback_requests(&device, &queue, &requests)).unwrap();
    test_support::assert_decoded(&vectors, &decoded);
}
