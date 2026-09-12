#![cfg(feature = "capture")]

#[allow(dead_code)]
#[path = "common/tmem_semantics.rs"]
mod semantics;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn bytes(name: &str) -> &'static [u8] {
    match name {
        "tmem-layouts" => include_bytes!("fixtures/tmem-layouts.f3dcap"),
        "tmem-tlut-mutation" => include_bytes!("fixtures/tmem-tlut-mutation.f3dcap"),
        "tmem-roles-lifetime" => include_bytes!("fixtures/tmem-roles-lifetime.f3dcap"),
        _ => panic!("unregistered fixture {name}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn export(name: &str, input: &[u8], pixels: &[u8], info: &wgpu::AdapterInfo) {
    use std::io::Write;
    let Some(directory) = std::env::var_os("FAST3D_GOLDEN_OUTPUT") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let metadata = serde_json::json!({
        "id": name, "stage": "final", "role": "render", "blend_path": "forced-fallback",
        "test_id": format!("tmem_fixture_replay::{}", name.replace('-', "_")),
        "width": 320, "height": 240, "channels": "RGBA",
        "adapter": {"name": info.name, "vendor": info.vendor, "device": info.device,
            "device_type": format!("{:?}", info.device_type), "driver": info.driver,
            "driver_info": info.driver_info, "backend": format!("{:?}", info.backend),
            "features": "empty (fixture requests dual_source_blending=false)"}
    });
    for (suffix, contents) in [
        ("bin", pixels),
        ("input.bin", input),
        ("json", &serde_json::to_vec(&metadata).unwrap()),
    ] {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(format!("{name}.{suffix}")))
            .unwrap()
            .write_all(contents)
            .unwrap();
    }
}

async fn replay(name: &str) {
    let input = bytes(name);
    let fixture = fast3d::capture::Fixture::from_bytes(input).unwrap();
    assert!(!fixture.frame.dual_source_blending);
    let output = fixture
        .replay_headless()
        .await
        .expect("T1 fixture requires an adapter");
    assert!(
        output.diagnostics.iter().all(Vec::is_empty),
        "{name}: {:?}",
        output.diagnostics
    );
    assert_eq!((output.width, output.height), (320, 240));
    let info = output
        .adapter_info
        .as_ref()
        .expect("fixture must record adapter");
    #[cfg(target_arch = "wasm32")]
    {
        assert_eq!(info.backend, wgpu::Backend::BrowserWebGpu);
        wasm_bindgen_test::console_log!("T1 fixture {name}: {info:?}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    export(name, input, &output.rgba8, info);
    semantics::assert_pixels(name, &output.rgba8);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tmem_layouts() {
    pollster::block_on(replay("tmem-layouts"));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn tmem_layouts() {
    replay("tmem-layouts").await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tmem_tlut_mutation() {
    pollster::block_on(replay("tmem-tlut-mutation"));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn tmem_tlut_mutation() {
    replay("tmem-tlut-mutation").await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn tmem_roles_lifetime() {
    pollster::block_on(replay("tmem-roles-lifetime"));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn tmem_roles_lifetime() {
    replay("tmem-roles-lifetime").await;
}
