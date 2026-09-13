use super::*;
use crate::profiling::{Mode, Snapshot};

fn count(snapshot: &Snapshot, name: &str) -> u64 {
    snapshot
        .counters
        .get(&format!("tmem.{name}"))
        .copied()
        .unwrap_or(0)
}

fn assert_work(case: &str, snapshot: &Snapshot) {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/tmem/dispatch-expectations.json"
    ))
    .unwrap();
    for (name, expected) in contract["t2"]["cases"][case].as_object().unwrap() {
        assert_eq!(
            count(snapshot, name),
            expected.as_u64().unwrap(),
            "{case}: {name}"
        );
    }
    println!(
        "T2_WORK {}",
        serde_json::json!({"case": case, "counters": snapshot.counters, "gauges": snapshot.gauges})
    );
}

fn state() -> (Rdp, TileDescriptor, Recorder) {
    let profiler = Recorder::new(Mode::Counters);
    let mut rdp = Rdp::default();
    rdp.tmem_bank.profiling = profiler.clone();
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    (
        rdp,
        TileDescriptor {
            fmt: 4,
            siz: 1,
            width: 1,
            height: 1,
            line: 1,
            ..Default::default()
        },
        profiler,
    )
}

fn request(binding: &TextureBindingInput) -> &Arc<EncodedTextureRequest> {
    let TextureSource::Encoded(request) = &binding.source else {
        panic!("expected encoded request")
    };
    request
}

fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn production_preimages_match_every_frozen_vector() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../../tools/tmem/vectors.json")).unwrap();
    for row in vectors["vectors"].as_array().unwrap() {
        let r = &row["recipe"];
        let n = |name: &str| r[name].as_u64().unwrap();
        let extent = |name: &str| {
            [
                r[name][0].as_u64().unwrap() as u32,
                r[name][1].as_u64().unwrap() as u32,
            ]
        };
        let fmt = n("fmt") as u8;
        let siz = n("siz") as u8;
        let recipe = DecodeRecipe {
            representation: [
                Representation::Tile,
                Representation::Lookup4096x4,
                Representation::LinearCompat,
            ][n("representation") as usize],
            output: extent("output"),
            logical: extent("logical"),
            fmt,
            siz,
            base: n("base") as u16,
            line: n("line") as u16,
            palette: if fmt == 2 && siz == 0 {
                n("palette") as u8
            } else {
                0
            },
            tlut: if fmt == 2 { n("tlut") as u8 } else { 0 },
        };
        let mut bank = [0; TMEM_BYTES];
        for field in ["physical_pairs_hex", "palette_pairs_hex"] {
            for pair in unhex(row[field].as_str().unwrap()).as_chunks::<3>().0 {
                bank[usize::from(u16::from_le_bytes([pair[0], pair[1]]))] = pair[2];
            }
        }
        let linear = unhex(row["linear_hex"].as_str().unwrap());
        let bytes = serialize(&recipe, &bank, &linear);
        assert_eq!(
            bytes,
            unhex(row["canonical_preimage_hex"].as_str().unwrap()),
            "{}",
            row["name"]
        );
        assert_eq!(
            format!("{:016x}", twox_hash::XxHash3_64::oneshot(&bytes)),
            row["xxh3_64"].as_str().unwrap()
        );
        if recipe.representation != Representation::LinearCompat {
            let rdp = Rdp {
                tmem_bank: super::super::tmem::Tmem::from_profile(&crate::profiling::Bank {
                    bytes: bank.to_vec(),
                    sources: Vec::new(),
                    blocks: Vec::new(),
                }),
                ..Default::default()
            };
            let mut tile = recipe.tile();
            tile.palette = n("palette") as u8;
            if recipe.representation == Representation::Lookup4096x4 {
                tile.masks = 10;
            }
            let binding = prepare(&rdp, &tile, n("tlut") as u8, &Recorder::default()).unwrap();
            assert_eq!(request(&binding).witness(), bytes);
        }
    }
}

#[test]
fn unused_identity_stays_lazy_across_frames_recipes_and_equal_reloads() {
    let (mut rdp, mut tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_work("first-i8-1x1", &profiler.drain());
    for _ in 0..3 {
        rdp = rdp.clone();
        let next = prepare(&rdp, &tile, 0, &profiler).unwrap();
        assert!(first.matches(&next, &profiler));
        assert_work("unchanged-frame", &profiler.drain());
    }
    tile.width = 2;
    let second = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(!first.matches(&second, &profiler));
    assert_work("second-recipe-i8-2x1", &profiler.drain());
    tile.width = 1;
    tile.shifts = 2;
    tile.cms = 2;
    tile.palette = 15;
    let binding = prepare(&rdp, &tile, 3, &profiler).unwrap();
    assert_eq!(request(&first).recipe(), request(&binding).recipe());
    assert_ne!(first.sampling, binding.sampling);
    assert_work("sampling-only", &profiler.drain());
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    let reload = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(request(&first).matches(request(&reload), &profiler));
    assert_work("equal-byte-reload", &profiler.drain());
    drop(rdp);
    for binding in [&first, &second, &binding, &reload] {
        assert!(request(binding).identity.get().is_none());
    }
    assert_eq!(first.source.decode().as_ref(), [61; 4]);
}

#[test]
fn identity_is_computed_once_from_retained_inputs_after_bank_mutation_and_drop() {
    let (mut rdp, tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    rdp.tmem_bank.write_tile(&[91; 8], 0, 1, 1, 1, 8, 1);
    let second = prepare(&rdp, &tile, 0, &profiler).unwrap();
    drop(rdp);
    profiler.drain();
    let old = request(&first);
    assert!(old.identity.get().is_none());
    let key = old.key_profiled(&profiler);
    let identity = old.identity(&profiler);
    assert_eq!(key, identity.key);
    assert_eq!(old.decode(), [61; 4]);
    assert_eq!(old.key(), identity.key);
    assert_eq!(old.witness(), identity.witness.as_ref());
    assert!(std::ptr::eq(identity, old.identity(&profiler)));
    assert_work("identity-first-i8-1x1", &profiler.drain());
    assert_ne!(old.key(), request(&second).identity(&profiler).key);
    assert_eq!(request(&second).decode(), [91; 4]);
    assert_work("identity-first-i8-1x1", &profiler.drain());
    old.identity(&profiler);
    request(&second).identity(&profiler);
    assert_work("identity-repeat", &profiler.drain());
}

#[test]
fn rejected_equal_bytes_and_bad_formats_never_construct_requests() {
    let (mut rdp, mut tile, profiler) = state();
    let retained = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let key = request(&retained).key();
    profiler.drain();
    let diagnostic = crate::Diagnostic {
        at: 0x40,
        kind: DiagKind::TextureBytesUnavailable { tmem_addr: 0 },
    };
    rdp.tmem_bank.reject_load(diagnostic, |bank| {
        bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1)
    });
    assert_eq!(
        prepare(&rdp, &tile, 0, &profiler).unwrap_err(),
        diagnostic.kind
    );
    tile.fmt = 1;
    assert!(prepare(&rdp, &tile, 0, &profiler).is_err());
    assert_eq!(request(&retained).key(), key);
    let rejected = profiler.drain();
    assert_work("rejected-load-or-format", &rejected);
    for name in [
        "memo_hits",
        "memo_misses",
        "snapshot_allocations",
        "hashes_computed",
    ] {
        assert_eq!(count(&rejected, name), 0);
    }
    assert_eq!(retained.source.decode().as_ref(), [61; 4]);
}

#[test]
fn linear_provenance_is_validated_and_retained_requests_own_the_stream() {
    let (mut rdp, mut tile, profiler) = state();
    tile.width = 3;
    tile.height = 3;
    rdp.tmem_bank.write_block(&[61; 16], 0, 0, 0, 2, 1);
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_eq!(
        request(&first).recipe().representation,
        Representation::LinearCompat
    );
    assert_eq!(first.source.decode().as_ref(), [61; 36]);
    assert_work("linear-first-3x3", &profiler.drain());
    prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_work("linear-warm", &profiler.drain());
    let before = rdp.tmem_bank.encoded_bank().to_vec();
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    assert_eq!(before, rdp.tmem_bank.encoded_bank().as_slice());
    assert!(prepare(&rdp, &tile, 0, &profiler).is_err());
    assert_work("provenance-only-rejection", &profiler.drain());
    assert!(request(&first).identity.get().is_none());
    drop(rdp);
    assert_eq!(first.source.decode().as_ref(), [61; 36]);
}

#[test]
fn forced_collisions_cannot_hide_different_owned_inputs() {
    let (mut rdp, tile, profiler) = state();
    let a = prepare(&rdp, &tile, 0, &profiler).unwrap();
    rdp.tmem_bank.write_tile(&[91; 8], 0, 1, 1, 1, 8, 1);
    let mut b = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let TextureSource::Encoded(b) = &mut b.source else {
        unreachable!()
    };
    b.identity(&profiler);
    Arc::get_mut(b).unwrap().identity.get_mut().unwrap().key = request(&a).key();
    assert_ne!(request(&a).witness(), b.witness());
    assert!(!request(&a).matches(b, &profiler));
    assert!(request(&a).matches(request(&a), &profiler));
}

#[test]
fn memory_accounting_does_not_force_identity_and_counts_shared_bank_once() {
    let (rdp, mut tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    tile.width = 2;
    let second = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let EncodedInput::Tmem(bank) = request(&first).input() else {
        panic!("physical bank")
    };
    assert!(Arc::ptr_eq(bank, &rdp.tmem_bank.share_bank()));
    let mut memory = TextureMemory::default();
    for source in [&first.source, &first.source, &second.source] {
        memory.include(source);
    }
    assert_eq!(memory.payload_bytes, TMEM_BYTES);
    assert!(request(&first).identity.get().is_none());
    assert!(request(&second).identity.get().is_none());
    request(&first).identity(&profiler);
    let mut memory = TextureMemory::default();
    for source in [&first.source, &first.source, &second.source] {
        memory.include(source);
    }
    assert_eq!(memory.payload_bytes, TMEM_BYTES + 59);
    assert!(request(&second).identity.get().is_none());
}

#[test]
fn footprints_match_existing_read_visitors_without_using_pixels_to_build_keys() {
    for trace in crate::profiling::authored_requests() {
        if trace.rejection.is_some() {
            continue;
        }
        let rdp = Rdp {
            tmem_bank: super::super::tmem::Tmem::from_profile(&trace.bank),
            ..Default::default()
        };
        let binding = prepare(&rdp, &trace.tile, trace.tlut, &Recorder::default()).unwrap();
        let request = request(&binding);
        assert_eq!(request.decode(), trace.output, "{}", trace.class());
        let witness = request.witness();
        let physical = u32::from_le_bytes(witness[44..48].try_into().unwrap()) as usize;
        let linear_offset = 48 + physical * 3;
        let linear = u32::from_le_bytes(
            witness[linear_offset..linear_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let palette_offset = linear_offset + 4 + linear;
        let palette = u32::from_le_bytes(
            witness[palette_offset..palette_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        assert_eq!(
            physical + linear + palette,
            trace.reachable_bytes,
            "{}",
            trace.class()
        );
    }
}

#[test]
fn clone_mutation_and_recreation_cannot_alias_a_retained_bank() {
    let (rdp, tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let mut branch = rdp.clone();
    branch.tmem_bank.write_tile(&[91; 8], 0, 1, 1, 1, 8, 1);
    let second = prepare(&branch, &tile, 0, &profiler).unwrap();
    assert_eq!(first.source.decode().as_ref(), [61; 4]);
    assert_eq!(second.source.decode().as_ref(), [91; 4]);
    let original = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(request(&first).matches(request(&original), &profiler));
    let recreated = Rdp::default();
    let reset = prepare(&recreated, &tile, 0, &profiler).unwrap();
    drop((rdp, branch, recreated));
    assert_ne!(request(&first).key(), request(&second).key());
    assert_eq!(request(&first).key(), request(&original).key());
    assert_ne!(request(&first).key(), request(&reset).key());
    assert_eq!(reset.source.decode().as_ref(), [0; 4]);
}

#[test]
fn unretained_bank_writes_do_not_copy_storage() {
    let (mut rdp, tile, profiler) = state();
    for byte in 0..10 {
        rdp.tmem_bank.write_tile(&[byte; 8], 0, 1, 1, 1, 8, 1);
        let binding = prepare(&rdp, &tile, 0, &profiler).unwrap();
        assert_eq!(binding.source.decode().as_ref(), [byte; 4]);
    }
    assert_work("unretained-reloads", &profiler.drain());
}

#[test]
fn unset_physical_extents_preserve_the_empty_cpu_payload() {
    let (rdp, mut tile, profiler) = state();
    tile.width = 0;
    let binding = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(binding.source.decode().is_empty());
    assert_eq!(
        binding.source.decode(),
        rdp.tmem_bank.sample_tile(&tile, 0).unwrap()
    );
}

#[cfg(feature = "capture")]
#[test]
fn image_roles_share_one_bank_with_level_zero_aliases_and_no_identity() {
    use crate::Hardware;
    let fixture = crate::capture::Fixture::from_bytes(include_bytes!(
        "../../tests/fixtures/tmem-roles-lifetime.f3dcap"
    ))
    .unwrap();
    let task = &fixture.tasks[0];
    let hardware = crate::capture::ReplayHardware::new(task, fixture.frame.vi).unwrap();
    let profiling = Recorder::new(Mode::Counters);
    let result = crate::hle::interp::interpret_profiled(
        hardware.rdram(),
        task.entry,
        task.microcode.into(),
        task.data_format,
        Default::default(),
        Default::default(),
        None,
        profiling.clone(),
    );
    assert!(result.diags.is_empty());
    assert_eq!(result.scene.materials.len(), 3);
    let mut distinct = std::collections::HashSet::new();
    let mut memory = TextureMemory::default();
    for material in &result.scene.materials {
        let TextureSource::Encoded(base) = &material.texture else {
            panic!("encoded material")
        };
        let TextureSource::Encoded(level_zero) = &material.mip_levels[0].texture else {
            panic!("encoded level zero")
        };
        assert!(Arc::ptr_eq(base, level_zero));
        for source in material.texture_sources() {
            let TextureSource::Encoded(request) = source else {
                panic!("encoded role")
            };
            distinct.insert(Arc::as_ptr(request));
            memory.include(source);
        }
    }
    assert_eq!(distinct.len(), 6);
    assert_eq!(memory.payload_bytes, 4096);
    assert_work("roles-fixture", &profiling.drain());
}

#[test]
fn equal_banks_with_different_linear_provenance_keep_distinct_lazy_identity() {
    let (mut rdp, mut tile, profiler) = state();
    tile.width = 3;
    tile.height = 3;
    rdp.tmem_bank
        .write_block(&(0..16).collect::<Vec<_>>(), 0, 0, 0, 2, 1);
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let before = rdp.tmem_bank.encoded_bank().to_vec();
    rdp.tmem_bank.write_block(
        &[0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15, 8, 9, 10, 11],
        0,
        0,
        2048,
        2,
        1,
    );
    assert_eq!(rdp.tmem_bank.encoded_bank().as_slice(), before);
    let second = prepare(&rdp, &tile, 0, &profiler).unwrap();
    drop(rdp);
    assert!(request(&first).identity.get().is_none());
    assert!(request(&second).identity.get().is_none());
    assert!(!first.matches(&second, &profiler));
    assert_eq!(first.source.decode().as_ref()[32..], [8; 4]);
    assert_eq!(second.source.decode().as_ref()[32..], [12; 4]);
    assert_ne!(request(&first).key(), request(&second).key());
    assert_ne!(request(&first).witness(), request(&second).witness());
}

#[test]
fn late_identity_retains_the_palette_from_its_draw() {
    let (mut rdp, mut tile, profiler) = state();
    tile.fmt = 2;
    rdp.tmem_bank.write_tile(&[0; 8], 0, 1, 1, 1, 8, 1);
    rdp.tmem_bank.write_tlut(&[0xf8, 1], 1, 256);
    let red = prepare(&rdp, &tile, 2, &profiler).unwrap();
    rdp.tmem_bank.write_tlut(&[7, 0xc1], 1, 256);
    let green = prepare(&rdp, &tile, 2, &profiler).unwrap();
    drop(rdp);
    assert!(request(&red).identity.get().is_none());
    assert!(request(&green).identity.get().is_none());
    assert_eq!(red.source.decode().as_ref(), [255, 0, 0, 255]);
    assert_eq!(green.source.decode().as_ref(), [0, 255, 0, 255]);
    assert_ne!(request(&red).key(), request(&green).key());
    assert_ne!(request(&red).witness(), request(&green).witness());
}

#[cfg(feature = "capture")]
#[test]
fn display_list_reloads_banks_between_draws_and_frames_without_identity_work() {
    use crate::Hardware;
    let fixture = crate::capture::Fixture::from_bytes(include_bytes!(
        "../../tests/fixtures/tmem-layouts.f3dcap"
    ))
    .unwrap();
    let task = &fixture.tasks[0];
    let hardware = crate::capture::ReplayHardware::new(task, fixture.frame.vi).unwrap();
    let profiling = Recorder::new(Mode::Counters);
    let mut rdp = Rdp::default();
    let mut frames = Vec::new();
    for _ in 0..3 {
        let result = crate::hle::interp::interpret_profiled(
            hardware.rdram(),
            task.entry,
            task.microcode.into(),
            task.data_format,
            rdp,
            Default::default(),
            None,
            profiling.clone(),
        );
        assert!(result.diags.is_empty());
        assert!(!result.scene.materials.is_empty());
        let mut memory = TextureMemory::default();
        for source in result
            .scene
            .materials
            .iter()
            .flat_map(|m| m.texture_sources())
        {
            memory.include(source);
            let TextureSource::Encoded(request) = source else {
                panic!("encoded material")
            };
            assert!(request.identity.get().is_none());
        }
        rdp = result.rdp.clone();
        frames.push(result.scene);
    }
    assert_work("display-list-reloads-three-frames", &profiling.drain());
    drop(rdp);
    for pair in frames.windows(2) {
        for (a, b) in pair[0].materials.iter().zip(&pair[1].materials) {
            assert_eq!(a.texture.decode(), b.texture.decode());
        }
    }
}
