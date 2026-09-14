#![cfg(all(feature = "capture", feature = "profiling"))]
use fast3d::{
    capture::{Fixture, MeasurementOptions, Sequence},
    profiling::Mode,
    ClearPolicy,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn sequence(frames: u64, warmup: u32) -> Sequence {
    let fixture = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    Sequence {
        frames: (1..=frames)
            .map(|serial| {
                let mut fixture = fixture.clone();
                fixture.frame.serial = serial;
                fixture.frame.config.clear_policy = ClearPolicy::Persist;
                fixture
            })
            .collect(),
        warmup_frames: warmup,
        presentations: vec![frames],
    }
}

#[test]
fn sequence_measurement_preserves_prefix() {
    for (frames, warmup) in [
        (1519, 1399),
        (2719, 2599),
        (1600, 1199),
        (1419, 1399),
        (6, 2),
    ] {
        let mut full = sequence(frames, warmup);
        full.validate().unwrap();
        full.frames.remove(0);
        assert!(full.validate().is_err());
    }
}

async fn compare_modes() {
    let sequence = sequence(5, 3);
    let (device, queue, _) = sequence.measurement_device().await.unwrap();
    let mut ordinary = Vec::new();
    sequence
        .replay_streamed(device.clone(), queue.clone(), |serial, frame| {
            ordinary.push((serial, frame))
        })
        .await
        .unwrap();
    for mode in [Mode::Counters, Mode::Coarse, Mode::Detailed, Mode::Trace] {
        let mut index = 0;
        sequence
            .measure(
                device.clone(),
                queue.clone(),
                MeasurementOptions {
                    mode,
                    readback: true,
                    ..Default::default()
                },
                |frame| {
                    assert_eq!(frame.serial, ordinary[index].0);
                    assert_eq!(frame.observed, frame.serial > 3);
                    assert_eq!(frame.output.rgba8, ordinary[index].1.rgba8);
                    assert_eq!(frame.output.summaries, ordinary[index].1.summaries);
                    assert_eq!(frame.output.diagnostics, ordinary[index].1.diagnostics);
                    assert!(frame.output.commands.is_empty());
                    index += 1;
                },
            )
            .await
            .unwrap();
        assert_eq!(index, 5);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn timed_replay_matches_readback_replay() {
    pollster::block_on(compare_modes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn timed_replay_matches_readback_replay() {
    compare_modes().await;
}

async fn check_upload_accounting() {
    let sequence = sequence(1, 0);
    let (device, queue, _) = sequence.measurement_device().await.unwrap();
    let (first, second) = fast3d::profiling::gpu_accounting_probe(&device, &queue);
    assert_eq!(first.counters["buffer.source.creations"], 1);
    assert_eq!(first.counters["buffer.source.capacity_bytes"], 4);
    assert_eq!(first.counters["buffer.source.upload_bytes"], 3);
    let count = |snapshot: &fast3d::profiling::Snapshot, name: &str| {
        snapshot.counters.get(name).copied().unwrap_or(0)
    };
    for (name, cold, warm) in [
        ("texture.lod0.creations", 1, 0),
        ("tmem.misses", 1, 0),
        ("tmem.hits", 4, 5),
        ("tmem.hashes_computed", 1, 0),
        ("tmem.witness_comparisons", 4, 5),
        ("tmem.decode_dispatches", 1, 0),
        ("tmem.decode_compute_passes", 1, 0),
        ("tmem.decode_input_upload_calls", 1, 0),
        ("tmem.decode_input_upload_bytes", 4096, 0),
        ("tmem.decode_uniform_upload_calls", 1, 0),
        ("tmem.decode_uniform_upload_bytes", 64, 0),
        ("tmem.cpu_decode_executions", 0, 0),
        ("tmem.cpu_decode_output_bytes", 0, 0),
    ] {
        assert_eq!(count(&first, name), cold, "cold {name}");
        assert_eq!(count(&second, name), warm, "warm {name}");
    }
    for role in ["lod0", "lod1", "lod2", "texture1", "detail"] {
        assert_eq!(count(&first, &format!("texture.{role}.upload_bytes")), 0);
        assert_eq!(count(&second, &format!("texture.{role}.upload_bytes")), 0);
        if role != "lod0" {
            assert_eq!(count(&first, &format!("texture.{role}.creations")), 0);
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_texture_roles_decode_once_and_warm_hits_upload_nothing() {
    pollster::block_on(check_upload_accounting());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_texture_roles_decode_once_and_warm_hits_upload_nothing() {
    check_upload_accounting().await;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cache_preflight_matches_uncached_decode() {
    use fast3d::profiling::{authored_requests, Representation};
    let requests = authored_requests();
    for representation in [
        Representation::Tile,
        Representation::Linear,
        Representation::Lookup,
    ] {
        assert!(requests.iter().any(|r| r.representation == representation));
    }
    for request in requests {
        let executor = request.executor();
        let linear = executor.prepare().unwrap();
        assert_eq!(executor.decode(&linear).unwrap(), request.output);
        let key = request.canonical(&linear);
        let equal = request.clone().canonical(&linear);
        assert_eq!(key, equal);
        assert_ne!(key.as_ptr(), equal.as_ptr());
        if request.representation == Representation::Tile
            && request.tile.fmt == 0
            && request.tile.siz == 2
            && request.extent == [32, 32]
        {
            assert_eq!(request.reachable_bytes, 2048);
            assert_eq!(request.output.len(), 4096);
            assert_eq!(key.len(), 4141);
        }
        if request.representation == Representation::Lookup {
            assert_eq!(request.extent, [4096, 4]);
            assert_eq!(request.output.len(), 65536);
        }
    }
}
