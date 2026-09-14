use super::*;
use crate::hle::texture_request::TextureKeyScheme;

fn key(n: u64) -> TextureKey {
    TextureKey {
        scheme: TextureKeyScheme::Fast3dV1,
        xxh3_64: n,
    }
}

fn image(cache: &mut Residency<u8>, hash: u64, byte: u8, stats: &Recorder) -> Arc<Resident<u8>> {
    cache.resolve(key(hash), Arc::from([byte]), 4, stats, || byte)
}

#[test]
fn hits_skip_creation_and_compare_exact_witnesses_inside_collision_bucket() {
    let stats = Recorder::new(crate::profiling::Mode::Counters);
    let mut cache = Residency::new(Limits::default());
    let a = image(&mut cache, 0, 17, &stats);
    let b = image(&mut cache, 0, 91, &stats);
    let again = cache.resolve(key(0), Arc::from([17]), 4, &stats, || {
        panic!("a hit must not create an image")
    });
    assert!(Arc::ptr_eq(&a, &again));
    assert_eq!(b.value, 91);
    assert_eq!(cache.entries, 2);
}

#[test]
fn live_reader_forces_bypass_and_pending_bypass_is_reused() {
    let stats = Recorder::default();
    let mut cache = Residency::new(Limits {
        entries: 1,
        ..Limits::default()
    });
    let a = image(&mut cache, 1, 17, &stats);
    cache.submitted();
    let b = image(&mut cache, 2, 91, &stats);
    assert!(a.admitted);
    assert!(!b.admitted);
    let again = image(&mut cache, 2, 91, &stats);
    assert!(Arc::ptr_eq(&b, &again));
    cache.submitted();
    let incomplete_task = image(&mut cache, 2, 91, &stats);
    assert!(Arc::ptr_eq(&b, &incomplete_task));
    drop((b, again, incomplete_task));
    let separate_task = image(&mut cache, 2, 91, &stats);
    assert!(!separate_task.admitted);
    assert_eq!(a.value, 17);
}

#[test]
fn completed_transient_is_not_a_resident_hit_in_the_next_task() {
    let stats = Recorder::new(crate::profiling::Mode::Counters);
    let mut cache = Residency::new(Limits {
        entries: 1,
        ..Limits::default()
    });
    let a = image(&mut cache, 1, 17, &stats);
    drop(image(&mut cache, 2, 91, &stats));
    cache.submitted();
    drop(image(&mut cache, 2, 91, &stats));
    assert_eq!(stats.snapshot().counters["tmem.misses"], 3);
    assert_eq!(stats.snapshot().counters["tmem.bypasses"], 2);
    assert_eq!(a.value, 17);
}

#[test]
fn lru_changes_at_submission_and_evicts_only_unpinned_entries() {
    let stats = Recorder::new(crate::profiling::Mode::Counters);
    let mut cache = Residency::new(Limits {
        entries: 2,
        ..Limits::default()
    });
    drop(image(&mut cache, 1, 17, &stats));
    cache.submitted();
    drop(image(&mut cache, 2, 91, &stats));
    cache.submitted();
    drop(image(&mut cache, 1, 17, &stats));
    cache.submitted();
    let c = image(&mut cache, 3, 42, &stats);
    assert!(c.admitted);
    assert!(cache.buckets.contains_key(&key(1)));
    assert!(!cache.buckets.contains_key(&key(2)));
    assert_eq!(stats.snapshot().counters["tmem.evictions"], 1);
}

#[test]
fn oversized_inputs_bypass_without_flushing_residents() {
    let stats = Recorder::default();
    let mut cache = Residency::new(Limits {
        entries: 2,
        gpu_bytes: 8,
        cpu_bytes: 2,
    });
    drop(image(&mut cache, 1, 17, &stats));
    cache.submitted();
    let gpu = cache.resolve(key(2), Arc::from([91]), 12, &stats, || 91);
    let cpu = cache.resolve(key(3), Arc::from([1, 2, 3]), 4, &stats, || 42);
    assert!(!gpu.admitted && !cpu.admitted);
    assert!(cache.buckets.contains_key(&key(1)));
    assert_eq!((cache.entries, cache.gpu_bytes, cache.cpu_bytes), (1, 4, 1));
}

#[test]
fn replacing_device_epoch_cannot_find_old_images_or_release_old_readers() {
    let stats = Recorder::default();
    let mut cache = Residency::new(Limits::default());
    let old = image(&mut cache, 1, 17, &stats);
    cache = Residency::new(Limits::default());
    let new = image(&mut cache, 1, 17, &stats);
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(old.value, 17);
    assert_eq!(cache.entries, 1);
}

#[test]
fn byte_caps_evict_enough_entries_independently_of_entry_count() {
    for limits in [
        Limits {
            entries: 8,
            gpu_bytes: 8,
            cpu_bytes: 8,
        },
        Limits {
            entries: 8,
            gpu_bytes: 64,
            cpu_bytes: 2,
        },
    ] {
        let stats = Recorder::new(crate::profiling::Mode::Counters);
        let mut cache = Residency::new(limits);
        drop(image(&mut cache, 1, 17, &stats));
        drop(image(&mut cache, 2, 91, &stats));
        cache.submitted();
        let c = cache.resolve(key(3), Arc::from([42, 43]), 8, &stats, || 42);
        assert!(c.admitted);
        assert_eq!(cache.entries, 1);
        assert_eq!(stats.snapshot().counters["tmem.evictions"], 2);
        assert_eq!(c.value, 42);
    }
}

#[test]
fn collision_bucket_survives_eviction_and_reinsertion_in_both_orders() {
    for order in [[17, 91, 17], [91, 17, 91]] {
        let stats = Recorder::new(crate::profiling::Mode::Counters);
        let mut cache = Residency::new(Limits {
            entries: 1,
            ..Limits::default()
        });
        for byte in order {
            let result = image(&mut cache, 0, byte, &stats);
            assert_eq!(result.value, byte);
            cache.submitted();
        }
        assert_eq!(stats.snapshot().counters["tmem.evictions"], 2);
        assert_eq!(stats.snapshot().counters["tmem.misses"], 3);
        assert_eq!(stats.snapshot().counters["tmem.witness_comparisons"], 2);
    }
}

#[test]
fn poisoned_equal_bank_cannot_reach_a_previously_cached_image() {
    use crate::hle::{
        rdp::{Rdp, TileDescriptor},
        texture_request::{prepare, TextureSource},
    };
    let stats = Recorder::new(crate::profiling::Mode::Counters);
    let mut rdp = Rdp::default();
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    let tile = TileDescriptor {
        fmt: 4,
        siz: 1,
        width: 1,
        height: 1,
        line: 1,
        ..Default::default()
    };
    let mut cache = Residency::new(Limits::default());
    let first = prepare(&rdp, &tile, 0, &stats).unwrap();
    let TextureSource::Encoded(request) = &first.source else {
        panic!("encoded source")
    };
    let retained = cache.resolve(
        request.key_profiled(&stats),
        request.witness_profiled(&stats),
        4,
        &stats,
        || request.decode(),
    );
    let rejection = crate::Diagnostic {
        at: 0x40,
        kind: crate::DiagKind::TextureBytesUnavailable { tmem_addr: 0 },
    };
    rdp.tmem_bank.reject_load(rejection, |bank| {
        bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1)
    });
    assert_eq!(prepare(&rdp, &tile, 0, &stats).unwrap_err(), rejection.kind);
    assert_eq!(stats.snapshot().counters["tmem.misses"], 1);
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    let reloaded = prepare(&rdp, &tile, 0, &stats).unwrap();
    let TextureSource::Encoded(request) = &reloaded.source else {
        panic!("encoded source")
    };
    let again = cache.resolve(
        request.key_profiled(&stats),
        request.witness_profiled(&stats),
        4,
        &stats,
        || panic!("successful reload must reuse exact content"),
    );
    assert!(Arc::ptr_eq(&retained, &again));
    assert_eq!(again.value, [61; 4]);
}
