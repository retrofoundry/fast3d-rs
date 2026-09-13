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
    let mut rdp = Rdp::default();
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
        Recorder::new(Mode::Counters),
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
fn unchanged_frames_second_recipe_and_equal_reload_have_exact_work_counts() {
    let (mut rdp, mut tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let cold = profiler.drain();
    assert_work("first-i8-1x1", &cold);
    assert_eq!(count(&cold, "snapshot_allocations"), 1);
    assert_eq!(count(&cold, "snapshot_bytes"), 4096);
    assert_eq!(count(&cold, "hashes_computed"), 1);
    assert_eq!(count(&cold, "bytes_hashed"), 59);
    for _ in 0..3 {
        rdp = rdp.clone();
        let next = prepare(&rdp, &tile, 0, &profiler).unwrap();
        assert!(Arc::ptr_eq(request(&first), request(&next)));
        assert!(first.matches(&next, &profiler));
        let warm = profiler.drain();
        assert_work("unchanged-frame", &warm);
        assert_eq!(count(&warm, "memo_hits"), 1);
        for name in [
            "snapshot_allocations",
            "snapshot_bytes",
            "hashes_computed",
            "bytes_hashed",
            "linear_reconstruction_bytes",
        ] {
            assert_eq!(count(&warm, name), 0, "{name}");
        }
        assert_eq!(count(&warm, "witness_comparisons"), 1);
        assert_eq!(count(&warm, "bytes_compared"), 59);
    }
    tile.width = 2;
    let second = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_ne!(request(&first).key(), request(&second).key());
    let counters = profiler.drain();
    assert_work("second-recipe-i8-2x1", &counters);
    assert_eq!(count(&counters, "snapshot_allocations"), 0);
    assert_eq!(count(&counters, "hashes_computed"), 1);
    assert_eq!(count(&counters, "bytes_hashed"), 62);
    tile.width = 1;
    tile.shifts = 2;
    tile.cms = 2;
    tile.palette = 15;
    let binding = prepare(&rdp, &tile, 3, &profiler).unwrap();
    assert!(Arc::ptr_eq(request(&first), request(&binding)));
    assert_ne!(first.sampling, binding.sampling);
    let descriptor = profiler.drain();
    assert_work("sampling-only", &descriptor);
    assert_eq!(count(&descriptor, "memo_hits"), 1);
    assert_eq!(count(&descriptor, "hashes_computed"), 0);
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    let reload = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(!Arc::ptr_eq(request(&first), request(&reload)));
    assert_eq!(request(&first).key(), request(&reload).key());
    assert!(request(&first).matches(request(&reload), &profiler));
    let equal = profiler.drain();
    assert_work("equal-byte-reload", &equal);
    assert_eq!(count(&equal, "snapshot_allocations"), 1);
    assert_eq!(count(&equal, "snapshot_bytes"), 4096);
    assert_eq!(count(&equal, "hashes_computed"), 1);
    assert_eq!(count(&equal, "bytes_hashed"), 59);
    drop(rdp);
    assert_eq!(first.source.decode().as_ref(), [61; 4]);
}

#[test]
fn rejected_equal_bytes_and_bad_formats_never_reach_the_memo() {
    let (mut rdp, mut tile, profiler) = state();
    let retained = prepare(&rdp, &tile, 0, &profiler).unwrap();
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
fn linear_provenance_is_validated_before_memo_and_retained_requests_own_the_stream() {
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
    drop(rdp);
    assert_eq!(first.source.decode().as_ref(), [61; 36]);
}

#[test]
fn forced_collisions_compare_witnesses_even_for_the_same_request() {
    let (mut rdp, tile, profiler) = state();
    let a = prepare(&rdp, &tile, 0, &profiler).unwrap();
    rdp.tmem_bank.write_tile(&[91; 8], 0, 1, 1, 1, 8, 1);
    let mut b = prepare(&rdp, &tile, 0, &profiler).unwrap();
    rdp.tmem_bank.requests.borrow_mut().entries.clear();
    let TextureSource::Encoded(b) = &mut b.source else {
        unreachable!()
    };
    Arc::get_mut(b).unwrap().key = request(&a).key();
    assert!(!request(&a).matches(b, &profiler));
    assert!(request(&a).matches(request(&a), &profiler));
    let measured = profiler.drain();
    assert_eq!(count(&measured, "witness_comparisons"), 2);
    assert_eq!(count(&measured, "bytes_compared"), 118);
}

#[test]
fn memo_caps_payload_and_entries_and_counts_shared_bank_once() {
    let (rdp, mut tile, profiler) = state();
    for line in 0..300 {
        tile.line = line;
        prepare(&rdp, &tile, 0, &profiler).unwrap();
    }
    assert_eq!(rdp.tmem_bank.requests.borrow().entries.len(), 256);
    assert_eq!(
        rdp.tmem_bank.requests.borrow().payload_bytes(),
        4096 + 256 * 59
    );
    assert_eq!(count(&profiler.drain(), "snapshot_allocations"), 1);
    tile.masks = 10;
    for line in 0..300 {
        tile.line = line;
        prepare(&rdp, &tile, 0, &profiler).unwrap();
        assert!(rdp.tmem_bank.requests.borrow().payload_bytes() <= MEMO_BYTES);
    }
    let memo = rdp.tmem_bank.requests.borrow();
    assert_eq!(memo.entries.len(), (MEMO_BYTES - 4096) / 12344);
    let mut memory = TextureMemory::default();
    for entry in &memo.entries {
        let source = TextureSource::Encoded(entry.request.clone());
        memory.include(&source);
        memory.include(&source);
    }
    assert_eq!(memory.payload_bytes, memo.payload_bytes());
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
fn oversized_requests_bypass_without_evicting_valid_entries() {
    let (rdp, tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let original = request(&first);
    let huge = Arc::new(EncodedTextureRequest {
        input: original.input.clone(),
        recipe: original.recipe.clone(),
        key: original.key,
        witness: vec![0; MEMO_BYTES].into_boxed_slice(),
    });
    let mut memo = rdp.tmem_bank.requests.borrow_mut();
    memo.admit(huge, &profiler);
    assert_eq!(memo.entries.len(), 1);
    assert_eq!(memo.payload_bytes(), 4096 + 59);
    assert!(Arc::ptr_eq(&memo.entries[0].request, original));
    assert_eq!(count(&profiler.drain(), "memo_bypasses"), 1);
}

#[test]
fn clone_mutation_and_provenance_wrap_cannot_alias_a_retained_version() {
    let (rdp, tile, profiler) = state();
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let mut branch = rdp.clone();
    branch.tmem_bank.requests.borrow_mut().provenance_revision = u64::MAX;
    branch.tmem_bank.write_tile(&[91; 8], 0, 1, 1, 1, 8, 1);
    let second = prepare(&branch, &tile, 0, &profiler).unwrap();
    assert_eq!(branch.tmem_bank.requests.borrow().provenance_revision, 0);
    assert_eq!(first.source.decode().as_ref(), [61; 4]);
    assert_eq!(second.source.decode().as_ref(), [91; 4]);
    let original = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert!(Arc::ptr_eq(request(&first), request(&original)));
    let recreated = Rdp::default();
    let reset = prepare(&recreated, &tile, 0, &profiler).unwrap();
    assert!(!Arc::ptr_eq(request(&first), request(&reset)));
    assert_eq!(reset.source.decode().as_ref(), [0; 4]);
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
fn image_roles_share_four_requests_and_one_bank_with_level_zero_aliases() {
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
    assert_eq!(distinct.len(), 4);
    assert_eq!(memory.payload_bytes, 4096 + 407);
    assert_work("roles-fixture", &profiling.drain());
}
