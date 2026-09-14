#![cfg(all(feature = "capture", feature = "profiling"))]

use fast3d::profiling::Snapshot;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn count(snapshot: &Snapshot, name: &str) -> u64 {
    snapshot.counters.get(name).copied().unwrap_or(0)
}

async fn pressure() {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("residency pressure requires an adapter");
    #[cfg(target_arch = "wasm32")]
    assert_eq!(adapter.get_info().backend, wgpu::Backend::BrowserWebGpu);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("residency-pressure"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        })
        .await
        .unwrap();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let snapshots = fast3d::profiling::gpu_residency_pressure_probe(&device, &queue)
        .await
        .unwrap();
    assert!(scope.pop().await.is_none());
    for (name, misses, hits, pending, bypasses, evictions, reuses) in [
        ("cold_pressure", 2, 1, 1, 1, 0, 0),
        ("pending_across_submission", 0, 1, 1, 0, 0, 0),
        ("completed", 0, 0, 0, 0, 0, 0),
        ("repeat_bypass", 1, 0, 0, 1, 0, 1),
        ("repeat_completed", 0, 0, 0, 0, 0, 0),
        ("evict_a", 1, 0, 0, 0, 1, 1),
        ("evict_b", 1, 0, 0, 0, 1, 1),
        ("new_epoch", 1, 0, 0, 0, 0, 0),
    ] {
        let snapshot = &snapshots[name];
        for (counter, expected) in [
            ("tmem.misses", misses),
            ("tmem.hits", hits),
            ("tmem.pending_hits", pending),
            ("tmem.bypasses", bypasses),
            ("tmem.evictions", evictions),
            ("tmem.staging_reuses", reuses),
            ("tmem.decode_dispatches", misses),
            ("tmem.decode_compute_passes", u64::from(misses != 0)),
            ("tmem.decode_input_upload_calls", misses),
            ("tmem.decode_input_upload_bytes", misses * 4096),
            ("tmem.decode_uniform_upload_calls", misses),
            ("tmem.decode_uniform_upload_bytes", misses * 64),
            ("tmem.cpu_decode_executions", 0),
            ("tmem.cpu_decode_output_bytes", 0),
            (
                "submissions",
                u64::from(!matches!(name, "completed" | "repeat_completed")),
            ),
        ] {
            assert_eq!(count(snapshot, counter), expected, "{name}: {counter}");
        }
        assert_eq!(snapshot.gauges["tmem.resident_entries"], 1, "{name}");
        assert_eq!(snapshot.gauges["tmem.resident_gpu_bytes"], 4, "{name}");
        assert!(snapshot.timings.is_empty(), "{name}");
    }
    for name in ["cold_pressure", "pending_across_submission"] {
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_entries"], 2,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_gpu_bytes"], 8,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_cpu_bytes"], 118,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.staging_pending_bytes"], 8320,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.staging_free_bytes"], 0,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.transient_entries"], 1,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.transient_gpu_bytes"], 4,
            "{name}"
        );
    }
    assert_eq!(
        snapshots["repeat_bypass"].gauges["tmem.staging_pending_bytes"],
        4160
    );
    assert_eq!(
        snapshots["repeat_bypass"].gauges["tmem.staging_free_bytes"],
        4160
    );
    for name in [
        "completed",
        "repeat_completed",
        "evict_a",
        "evict_b",
        "new_epoch",
    ] {
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_entries"], 0,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_gpu_bytes"], 0,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.in_flight_cpu_bytes"], 0,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.staging_pending_bytes"], 0,
            "{name}"
        );
        assert_eq!(
            snapshots[name].gauges["tmem.staging_free_bytes"],
            if name == "new_epoch" { 4160 } else { 8320 },
            "{name}"
        );
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(directory) = std::env::var_os("FAST3D_RESIDENCY_OUTPUT") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("pressure.json"),
            serde_json::to_vec_pretty(&snapshots).unwrap(),
        )
        .unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_images_pressure_pending_staging_and_epoch() {
    pollster::block_on(pressure());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_images_pressure_pending_staging_and_epoch() {
    pressure().await;
}
