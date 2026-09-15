#![cfg(all(feature = "profiling", feature = "capture"))]

use fast3d::profiling;

#[cfg(not(target_arch = "wasm32"))]
#[path = "../../tools/readbacks/export.rs"]
mod export;

#[path = "../src/render/texture_decode_vectors.rs"]
mod vectors;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[path = "../src/render/texture_decode_test_support.rs"]
mod test_support;

async fn decode_vectors() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("decode readback requires an adapter");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("texture-decode-witnesses"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .unwrap();
    let vectors = test_support::vectors();
    let requests: Vec<_> = vectors
        .iter()
        .map(|vector| vector.request.clone())
        .collect();
    let decoded = profiling::gpu_decode_readback(&device, &queue, &requests)
        .await
        .unwrap();
    test_support::verify(&device, &vectors, &decoded);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn texture_decode_gpu_component_and_layout_readbacks() {
    pollster::block_on(decode_vectors());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn texture_decode_gpu_component_and_layout_readbacks() {
    decode_vectors().await;
}
