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
    println!(
        "T2_WORK {}",
        serde_json::json!({"case": case, "counters": snapshot.counters, "gauges": snapshot.gauges})
    );
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
fn angrylion_preserves_unaffected_frozen_streams_and_separates_changed_semantics() {
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
        if n("representation") == 2 {
            continue;
        }
        let recipe = DecodeRecipe {
            palette_lanes: false,
            representation: [Representation::Tile, Representation::Lookup4096x4]
                [n("representation") as usize],
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
        if fmt == 2 && recipe.tlut >= 2 {
            for slot in bank[2048..].as_chunks_mut::<8>().0 {
                let pair = [slot[0], slot[1]];
                for lane in slot[2..].as_chunks_mut::<2>().0 {
                    lane.copy_from_slice(&pair);
                }
            }
        }
        let bytes = serialize(&recipe, &bank);
        let frozen = unhex(row["canonical_preimage_hex"].as_str().unwrap());
        if fmt == 2 && recipe.tlut < 2 {
            assert_ne!(bytes, frozen, "{}", row["name"]);
        } else {
            assert_eq!(bytes, frozen, "{}", row["name"]);
            assert_eq!(
                format!("{:016x}", twox_hash::XxHash3_64::oneshot(&bytes)),
                row["xxh3_64"].as_str().unwrap()
            );
        }
        let rdp = Rdp {
            tmem_bank: super::super::tmem::Tmem::from_profile(&crate::profiling::Bank {
                bytes: bank.to_vec(),
            }),
            ..Default::default()
        };
        let mut tile = recipe.tile();
        tile.palette = n("palette") as u8;
        if recipe.representation == Representation::Lookup4096x4 {
            tile.masks = 10;
        }
        let binding = prepare(&rdp, &tile, n("tlut") as u8, &Recorder::default()).unwrap();
        if fmt != 2 && n("tlut") >= 2 {
            assert_ne!(request(&binding).witness(), bytes);
        } else {
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
    // Angrylion enables TLUT for every format in modes 2/3; mode 1 stays disabled.
    let binding = prepare(&rdp, &tile, 1, &profiler).unwrap();
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
    let witness = old.witness_profiled(&profiler);
    assert!(Arc::ptr_eq(&witness, &old.witness_profiled(&profiler)));
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
        kind: DiagKind::UnsupportedFramebufferAccess {
            address: 0x1000,
            reason: crate::FramebufferAccess::Reinterpretation,
        },
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
fn partial_overwrites_keep_retained_physical_requests() {
    let (mut rdp, mut tile, profiler) = state();
    tile.width = 3;
    tile.height = 3;
    rdp.tmem_bank.write_block(&[61; 16], 0, 0, 0, 2, 1);
    let first = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_eq!(
        request(&first).recipe().representation,
        Representation::Tile
    );
    assert_eq!(
        first.source.decode().as_ref(),
        [[61; 24].as_slice(), &[0; 12]].concat()
    );
    assert_work("narrow-first-3x3", &profiler.drain());
    prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_work("narrow-warm", &profiler.drain());
    let before = rdp.tmem_bank.encoded_bank().to_vec();
    rdp.tmem_bank.write_tile(&[61; 8], 0, 1, 1, 1, 8, 1);
    assert_eq!(before, rdp.tmem_bank.encoded_bank().as_slice());
    let overwritten = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_eq!(first, overwritten);
    assert_work("equal-byte-partial-overwrite", &profiler.drain());
    assert!(request(&first).identity.get().is_none());
    drop(rdp);
    assert_eq!(
        first.source.decode().as_ref(),
        [[61; 24].as_slice(), &[0; 12]].concat()
    );
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
    let EncodedInput::Tmem(bank) = request(&first).input();
    assert!(Arc::ptr_eq(bank, &rdp.tmem_bank.share_bank()));
    let mut memory = TextureMemory::default();
    for source in [&first.source, &first.source, &second.source] {
        memory.include(source);
    }
    assert_eq!(memory.payload_bytes, TMEM_BYTES);
    let before_identity_overhead = memory.overhead_bytes;
    assert!(request(&first).identity.get().is_none());
    assert!(request(&second).identity.get().is_none());
    request(&first).identity(&profiler);
    let mut memory = TextureMemory::default();
    for source in [&first.source, &first.source, &second.source] {
        memory.include(source);
    }
    assert_eq!(memory.payload_bytes, TMEM_BYTES + 59);
    assert_eq!(
        memory.overhead_bytes,
        before_identity_overhead + 2 * std::mem::size_of::<usize>()
    );
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
fn unset_physical_extents_decode_one_texel() {
    let (rdp, mut tile, profiler) = state();
    tile.width = 0;
    tile.height = 0;
    let binding = prepare(&rdp, &tile, 0, &profiler).unwrap();
    assert_eq!(request(&binding).recipe().output, [1, 1]);
    assert_eq!(binding.source.decode().as_ref(), [61; 4]);
    let mut trace =
        crate::profiling::Request::capture(&rdp, &tile, 0, "texture0", Ok(request(&binding)));
    trace.analyze().unwrap();
    assert_eq!(trace.output, [61; 4]);
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
fn equal_banks_with_different_load_history_keep_equal_lazy_identity() {
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
    assert!(first.matches(&second, &profiler));
    assert_eq!(first.source.decode().as_ref()[32..], [0; 4]);
    assert_eq!(second.source.decode().as_ref()[32..], [0; 4]);
    assert_eq!(request(&first).key(), request(&second).key());
    assert_eq!(request(&first).witness(), request(&second).witness());
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
    let construction = profiling.drain();
    assert_work("display-list-reloads-three-frames", &construction);
    drop(rdp);
    for pair in frames.windows(2) {
        for (a, b) in pair[0].materials.iter().zip(&pair[1].materials) {
            assert_eq!(a.texture.decode(), b.texture.decode());
        }
    }
    for source in frames
        .iter()
        .flat_map(|frame| &frame.materials)
        .flat_map(|m| m.texture_sources())
    {
        let TextureSource::Encoded(request) = source else {
            panic!("encoded material")
        };
        request.identity_profiled(&profiling);
    }
    let profile = profiling.drain();
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/tmem/dispatch-expectations.json"
    ))
    .unwrap();
    for name in [
        "hashes_computed",
        "bytes_hashed",
        "preimage_bytes_constructed",
        "footprint_span_bytes",
        "palette_index_reads",
    ] {
        assert_eq!(
            count(&profile, name) + count(&construction, name),
            contract["t4"]["display_list_reloads_three_frames"][name]
                .as_u64()
                .unwrap(),
            "{name}"
        );
    }
    println!(
        "T4_IDENTITY_WORK {}",
        serde_json::to_string(&profile.counters).unwrap()
    );
}

#[test]
fn construction_and_identity_memo_work_are_counted_separately() {
    let (rdp, tile, profiler) = state();
    let binding = prepare(&rdp, &tile, 0, &profiler).unwrap();
    let request = request(&binding);
    request.key_profiled(&profiler);
    let first = profiler.drain();
    assert_eq!(count(&first, "preimage_bytes_constructed"), 59);
    assert_eq!(count(&first, "bytes_hashed"), 59);
    assert_eq!(count(&first, "identity_memo_hits"), 0);
    request.key_profiled(&profiler);
    let repeat = profiler.drain();
    assert_eq!(count(&repeat, "preimage_bytes_constructed"), 0);
    assert_eq!(count(&repeat, "bytes_hashed"), 0);
    assert_eq!(count(&repeat, "identity_memo_hits"), 1);
}

#[test]
fn physical_footprints_keep_retained_bytes_and_ignore_unrelated_mutations_even_on_collision() {
    for (fmt, siz, address, changes) in [
        (4, 1, 16, true),
        (4, 1, 32, true),
        (4, 1, 40, false),
        (2, 1, 2048, true),
        (2, 1, 2056, false),
        (2, 0, 2048, true),
        (2, 0, 2056, false),
    ] {
        let mut rdp = Rdp::default();
        rdp.tmem_bank.write_block(&[0; 32], 0, 0, 0, 4, 1);
        rdp.tmem_bank.write_block(&[0; 8], 0, 0, 0, 1, 1);
        let tile = TileDescriptor {
            fmt,
            siz,
            width: 4,
            height: 5,
            line: 1,
            ..Default::default()
        };
        let first = prepare(
            &rdp,
            &tile,
            if fmt == 2 { 3 } else { 0 },
            &Recorder::default(),
        )
        .unwrap();
        let pixels = first.source.decode().into_owned();
        let witness = request(&first).witness().to_vec();
        let mut bank = rdp.tmem_bank.profile_bank();
        bank.bytes[address] = 0x91;
        rdp.tmem_bank = super::super::tmem::Tmem::from_profile(&bank);
        let mut second = prepare(
            &rdp,
            &tile,
            if fmt == 2 { 3 } else { 0 },
            &Recorder::default(),
        )
        .unwrap();
        drop(rdp);
        assert_eq!(first.source.decode().as_ref(), pixels);
        assert_eq!(request(&first).witness(), witness);
        assert_eq!(
            request(&first).key() != request(&second).key(),
            changes,
            "{fmt}/{siz}/{address}"
        );
        assert_eq!(first.source.decode() != second.source.decode(), changes);
        let TextureSource::Encoded(second) = &mut second.source else {
            unreachable!()
        };
        Arc::get_mut(second)
            .unwrap()
            .identity
            .get_mut()
            .unwrap()
            .key = request(&first).key();
        assert_eq!(request(&first).witness() != second.witness(), changes);
        assert_eq!(
            request(&first).matches(second, &Recorder::default()),
            !changes
        );
    }
}

#[test]
fn angrylion_tlut_state_lanes_and_full_bank_indices_have_mutation_witnesses() {
    use super::super::tmem::Tmem;
    let tile = TileDescriptor {
        fmt: 4,
        siz: 0,
        width: 1,
        height: 1,
        palette: 10,
        ..Default::default()
    };
    let mut bank = crate::profiling::Bank {
        bytes: vec![0; 4096],
    };
    bank.bytes[0] = 0x12;
    for slot in bank.bytes[2048..].as_chunks_mut::<8>().0 {
        slot.copy_from_slice(&[0x10, 255, 0x10, 255, 0x10, 255, 0x10, 255]);
    }
    let make = |bank: &crate::profiling::Bank, tile: &TileDescriptor, tlut| {
        let rdp = Rdp {
            tmem_bank: Tmem::from_profile(bank),
            ..Default::default()
        };
        prepare(&rdp, tile, tlut, &Recorder::default()).unwrap()
    };
    let original = make(&bank, &tile, 3);
    assert_eq!(original.sampling.allocation_extent(), [1, 1]);
    assert_ne!(
        request(&original).key(),
        request(&make(&bank, &tile, 0)).key()
    );
    assert_ne!(
        request(&original).key(),
        request(&make(&bank, &tile, 2)).key()
    );
    assert_ne!(
        request(&original).key(),
        request(&make(
            &bank,
            &TileDescriptor {
                palette: 9,
                ..tile.clone()
            },
            3
        ))
        .key()
    );
    let original_pixels = original.source.decode().into_owned();
    for address in 2048 + 0xa1 * 8..2056 + 0xa1 * 8 {
        let mut changed = bank.clone();
        changed.bytes[address] ^= 0x80;
        let next = make(&changed, &tile, 3);
        assert_ne!(
            request(&original).key(),
            request(&next).key(),
            "lane byte {address}"
        );
        assert_ne!(request(&original).witness(), request(&next).witness());
        assert!(!request(&original).matches(request(&next), &Recorder::default()));
    }
    bank.bytes[2048 + 0xa2 * 8 + 2] ^= 0x80;
    assert_eq!(
        request(&original).key(),
        request(&make(&bank, &tile, 3)).key()
    );
    assert_eq!(original.source.decode().as_ref(), original_pixels);
    let ci = TileDescriptor {
        fmt: 2,
        siz: 1,
        tmem_addr: 256,
        ..tile.clone()
    };
    let off = make(&bank, &ci, 0);
    bank.bytes[0] ^= 0x80;
    assert_eq!(request(&off).key(), request(&make(&bank, &ci, 0)).key());
    bank.bytes[2048] ^= 0x80;
    assert_ne!(request(&off).key(), request(&make(&bank, &ci, 0)).key());
}

#[test]
fn angrylion_tlut_halfword_indices_ignore_low_bytes_and_rgba32_high_bank() {
    use super::super::tmem::Tmem;
    for fmt in [0, 2, 3, 4, 5, 6, 7] {
        for siz in [2, 3] {
            let tile = TileDescriptor {
                fmt,
                siz,
                width: 1,
                height: 1,
                ..Default::default()
            };
            let mut bank = crate::profiling::Bank {
                bytes: vec![0; 4096],
            };
            bank.bytes[0] = 12;
            let first = prepare(
                &Rdp {
                    tmem_bank: Tmem::from_profile(&bank),
                    ..Default::default()
                },
                &tile,
                3,
                &Recorder::default(),
            )
            .unwrap();
            bank.bytes[1] = 255;
            bank.bytes[2048] = 255;
            let next = prepare(
                &Rdp {
                    tmem_bank: Tmem::from_profile(&bank),
                    ..Default::default()
                },
                &tile,
                3,
                &Recorder::default(),
            )
            .unwrap();
            assert_eq!(request(&first).key(), request(&next).key());
            assert_eq!(first.source.decode(), next.source.decode());
        }
    }
}

#[test]
fn angrylion_routing_checks_yuv_neighbors() {
    let diagnostic = crate::Diagnostic {
        at: 0x40,
        kind: DiagKind::UnsupportedFramebufferAccess {
            address: 0,
            reason: crate::FramebufferAccess::Reinterpretation,
        },
    };
    let mut rdp = Rdp::default();
    rdp.tmem_bank
        .reject_load(diagnostic, |tmem| tmem.write_tlut(&[], 1, 1));
    let yuv = TileDescriptor {
        fmt: 1,
        siz: 1,
        width: 8,
        height: 1,
        masks: 3,
        ..Default::default()
    };
    assert_eq!(
        prepare(&rdp, &yuv, 3, &Recorder::default()).unwrap_err(),
        diagnostic.kind
    );
}

#[test]
fn replicated_loadtlut_skips_index_probes_until_a_palette_write_invalidates_it() {
    let (mut rdp, mut tile, profiling) = state();
    tile.fmt = 2;
    rdp.tmem_bank.write_tile(&[0; 8], 0, 0, 1, 1, 8, 1);
    rdp.tmem_bank.write_tlut(&[0x10, 255], 1, 256);
    profiling.drain();
    let first = prepare(&rdp, &tile, 3, &profiling).unwrap();
    assert_eq!(first.sampling.allocation_extent(), [1, 1]);
    let counts = profiling.drain();
    assert_eq!(count(&counts, "palette_lane_slot_checks"), 256);
    assert_eq!(count(&counts, "palette_lane_index_reads"), 0);
    prepare(&rdp, &tile, 3, &profiling).unwrap();
    let counts = profiling.drain();
    assert_eq!(count(&counts, "palette_lane_slot_checks"), 0);
    assert_eq!(count(&counts, "palette_lane_index_reads"), 0);
    rdp.tmem_bank.write_tile(
        &[0x10, 255, 0x30, 255, 0x50, 255, 0x70, 255],
        256,
        0,
        1,
        1,
        8,
        1,
    );
    let changed = prepare(&rdp, &tile, 3, &profiling).unwrap();
    assert_eq!(changed.sampling.allocation_extent(), [1, 4]);
    let counts = profiling.drain();
    assert_eq!(count(&counts, "palette_lane_slot_checks"), 256);
    assert_eq!(count(&counts, "palette_lane_index_reads"), 1);
    assert_eq!(first.source.decode().as_ref(), [0x10, 0x10, 0x10, 255]);
}

#[test]
fn angrylion_encoding_flags_separate_old_meanings_of_the_same_bank() {
    let (rdp, mut tile, profiling) = state();
    tile.fmt = 2;
    let off = prepare(&rdp, &tile, 0, &profiling).unwrap();
    let witness = request(&off).witness();
    assert_eq!(witness[19], 1);
    let mut old = witness.to_vec();
    old[19] = 0;
    assert_ne!(
        request(&off).key().xxh3_64,
        twox_hash::XxHash3_64::oneshot(&old)
    );

    tile.fmt = 4;
    tile.height = 257;
    let tall = prepare(&rdp, &tile, 0, &profiling).unwrap();
    assert_eq!(request(&tall).witness()[19], 4);
    tile.line = 2;
    let already_wrapped = prepare(&rdp, &tile, 0, &profiling).unwrap();
    assert_eq!(request(&already_wrapped).witness()[19], 0);

    let original = request(&off);
    let mut recipe = original.recipe().clone();
    recipe.tlut = 3;
    let one = EncodedTextureRequest::from_test_parts(original.input().clone(), recipe.clone());
    recipe.palette_lanes = true;
    let four = EncodedTextureRequest::from_test_parts(original.input().clone(), recipe);
    assert_ne!(one.key(), four.key());
    assert_eq!(one.witness()[19], 0);
    assert_eq!(four.witness()[19], 2);
}
