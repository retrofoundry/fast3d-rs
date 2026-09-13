use super::*;
use crate::hle::{
    combiner,
    rdp::{Rdp, TileDescriptor},
    rsp::Rsp,
};

#[test]
fn decode_counters_distinguish_tile_linear_lookup() {
    let cases = authored_requests();
    let toy = cases
        .iter()
        .find(|r| {
            r.representation == Representation::Tile
                && r.tile.fmt == 0
                && r.tile.siz == 2
                && r.extent == [32, 32]
        })
        .unwrap();
    assert_eq!(
        (toy.reachable_bytes, toy.output.len(), toy.bank.bytes.len()),
        (2048, 4096, 4096)
    );
    assert_eq!(toy.canonical(&[]).len(), 4096 + 45);
    let lookup = cases
        .iter()
        .find(|r| r.representation == Representation::Lookup && r.tile.fmt == 0 && r.tile.siz == 2)
        .unwrap();
    assert_eq!(
        (lookup.reachable_bytes, lookup.extent, lookup.output.len()),
        (4096, [4096, 4], 65536)
    );
    let ci = cases
        .iter()
        .find(|r| {
            r.representation == Representation::Tile
                && r.tile.fmt == 2
                && r.tile.siz == 1
                && r.extent == [1, 1]
        })
        .unwrap();
    assert_eq!((ci.reachable_bytes, ci.palette_bytes), (3, 2));
    let linear = cases
        .iter()
        .find(|r| r.representation == Representation::Linear && r.tile.fmt == 0 && r.tile.siz == 2)
        .unwrap();
    assert_eq!(
        linear.reachable_bytes,
        usize::from(linear.tile.width) * usize::from(linear.tile.height) * 2
    );
    assert_ne!(linear.bank.sources.len(), 0);
}

#[test]
fn cache_preflight_matches_uncached_decode() {
    for request in authored_requests() {
        let executor = request.executor();
        let prepared = executor.prepare().unwrap();
        assert_eq!(
            executor.decode(&prepared).unwrap(),
            request.output,
            "{}",
            request.class()
        );
        let equal = request.clone();
        assert_eq!(request.canonical(&prepared), equal.canonical(&prepared));
        let mut changed = request.clone();
        changed.bank.bytes[0] ^= 0xff;
        assert_ne!(request.canonical(&prepared), changed.canonical(&prepared));
        if request.representation == Representation::Linear {
            let mut lost = request.clone();
            lost.bank.sources.fill([0, 0]);
            assert!(lost.executor().prepare().is_err());
        }
        let mut rejected = request;
        rejected.rejection = Some("lost source".into());
        assert!(rejected.executor().prepare().is_err());
    }
}

#[test]
fn profiling_does_not_change_summaries() {
    let mut rdp = Rdp {
        texture_loaded: true,
        combine_l: 0xFC12_7E24,
        combine_h: 0xFFFF_F9FC,
        ..Default::default()
    };
    rdp.tiles[0] = TileDescriptor {
        fmt: 0,
        siz: 2,
        width: 4,
        height: 4,
        line: 1,
        ..Default::default()
    };
    rdp.tmem_bank.write_tile(&[255; 32], 0, 1, 4, 1, 8, 2);
    let mut rsp = Rsp::default();
    rsp.texture_state.on = true;
    let mut plain_diags = Vec::new();
    let plain = combiner::build_material(&rdp, &rsp, &mut plain_diags, 0);
    for mode in [Mode::Counters, Mode::Coarse, Mode::Detailed, Mode::Trace] {
        rsp.profiling = Recorder::new(mode);
        let mut diags = Vec::new();
        assert_eq!(combiner::build_material(&rdp, &rsp, &mut diags, 0), plain);
        assert_eq!(diags, plain_diags);
        let snapshot = rsp.profiling.drain();
        assert_eq!(snapshot.counters["material.builds"], 1);
        assert_eq!(snapshot.counters["tmem.requests"], 1);
        assert_eq!(snapshot.requests.len(), usize::from(mode == Mode::Trace));
    }
    rdp.tiles[0].fmt = 1;
    rsp.profiling = Recorder::new(Mode::Trace);
    assert!(combiner::build_material(&rdp, &rsp, &mut Vec::new(), 0).is_none());
    let snapshot = rsp.profiling.drain();
    assert_eq!(snapshot.counters["tmem.rejected_requests"], 1);
    assert!(snapshot.requests[0].rejection.is_some());
}

#[cfg(feature = "capture")]
#[test]
fn profiling_preserves_interpreter_summary_and_capture_bytes() {
    use crate::Hardware;
    let bytes = include_bytes!("../../tests/fixtures/host64-fill.f3dcap");
    let fixture = crate::capture::Fixture::from_bytes(bytes).unwrap();
    let task = &fixture.tasks[0];
    let hardware = crate::capture::ReplayHardware::new(task, fixture.frame.vi).unwrap();
    let mut plain = CpuInterpreter::default();
    let expected = plain.process(
        hardware.rdram(),
        task.entry,
        task.microcode,
        task.data_format,
    );
    for mode in [Mode::Counters, Mode::Coarse, Mode::Detailed, Mode::Trace] {
        let mut measured = CpuInterpreter {
            recorder: Recorder::new(mode),
            ..Default::default()
        };
        assert_eq!(
            measured.process(
                hardware.rdram(),
                task.entry,
                task.microcode,
                task.data_format
            ),
            expected
        );
        assert_eq!(fixture.to_bytes().unwrap(), bytes);
    }
}

#[test]
fn recorder_inactive_and_nested_accounting() {
    let inactive = Recorder::default();
    inactive.count("unused", 1);
    assert!(inactive.drain().counters.is_empty());
    let recorder = Recorder::new(Mode::Detailed);
    {
        let _outer = recorder.span("process_dl");
        let _inner = recorder.span("interpretation");
        recorder.count("tasks", 1);
    }
    let snapshot = recorder.drain();
    let process = &snapshot.timings["process_dl"];
    let interpretation = &snapshot.timings["interpretation"];
    assert!(
        (process.inclusive_ms - process.exclusive_ms - interpretation.inclusive_ms).abs() < 1e-6
    );
    assert_eq!(snapshot.counters["tasks"], 1);
    assert!(recorder.drain().counters.is_empty());
}
