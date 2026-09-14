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
