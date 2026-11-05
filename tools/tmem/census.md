# TMEM item 16 census

Run from the repository root with one or more capture sequences or fixtures:

```sh
cargo run -p fast3d --release --features capture,profiling \
  --example tmem16_census -- \
  /Users/ci/hub/scratch/fast3d/tmem16-census/runs/run-name \
  /path/to/capture.f3dcap /path/to/fixture.f3dcap
```

Each input gets a fresh interpreter. A sequence uses that interpreter for every
frame and task from its reset prefix, including warmup frames and frames without
a selected presentation. `begin_frame` preserves TMEM. Single-frame fixtures start
from default state. This command submits no GPU work.

The output contains a numbered directory for each input and a combined
`summary.json`. Existing summaries and workload directories are refused to protect earlier
evidence. Other inputs continue if one fails; the process then exits with an error.

Each workload directory contains:

- `input.json`: capture path and XXH3-64 checksum, reset-prefix scope, frame counts,
  presentation selection and evidence limits.
- `attempts.jsonl`: every observed role attempt, including rejections, with frame,
  task, draw PC, current outcome, class, sampled count/hash, writer ranges and
  links to its bank, tags, identity and descriptor. Each writer range is
  `[start_address, byte_count, load_id]`; load ID zero means unwritten.
- `loads.jsonl`: load command PC, command words, image state, load tile, parameters, source
  bytes and load ID. Source bytes live in `sources/`.
- `tasks.jsonl`: every task's command count, termination, cap status, diagnostics
  and replay errors.
- `banks/` and `tags/`: deduplicated complete bank and framebuffer-tag snapshots.
  A tag entry is `[bank_address, copy_id, pointer]`.
- `identities/`: exact sampled witnesses. `sampled_ranges` stores each contiguous
  span as `[start_address, [bytes...]]`. Hashes choose file names; existing contents
  are compared byte for byte and collisions get a numeric suffix.
- `witnesses/`: the encoded request's Fast3dV1 witness.
  Identities include this file reference, whose contents are deduplicated by exact
  equality with the same collision handling.
- `textures/`: one descriptor per distinct witness, linked to its first attempt
  and load IDs. Ordinary textures have physical decode PNGs. Framebuffer
  requests and failed decodes retain descriptors without PNGs. Paths are relative to the JSON file containing them.
- `summary.json`: attempt and distinct counts by class, roles, current outcomes,
  frames, tasks, errors, caps and terminations.

`prefix_complete` becomes false at the first non-End task or missing capture read.
That task and every later record carry the flag; the summary identifies the first
incomplete task. Replay continues with the interpreter's normal rollback rules.
Counts after that point do not establish what the original recording rendered.

Ordinary texture identity includes the descriptor, representations, sampled
address/value pairs. It excludes unrelated bank
bytes, draw PC and load history. Framebuffer identities describe a candidate
footprint only: placeholder bytes cannot establish the palette indices or texels
that the GPU will read. The load-free framebuffer shortcut is also unverified and
has no CPU image. Full bank/tag snapshots remain available for every attempt,
and tagged-byte placeholder decodes are never written as PNGs.

Schema 2 records the current `outcome` and `witness`; it does not reconstruct
retired linear streams. Classes are `unaffected`, `intentional_exception` or
`unverified`. Compare saved runs by workload, frame/task, draw PC, role,
descriptor, sampled bytes and exact witnesses to detect routing or content
regressions. A formerly refused request becoming Tile/Lookup is newly admitted.
`framebuffer_decode` is `exact` or `materialized` for accepted framebuffer requests.
Both remain `unverified`, with `GPU framebuffer copies unavailable on CPU` as the
decode error, an empty physical image and no Fast3dV1 witness. This is an unavailable
CPU observation, not a renderer refusal. Profiling counts `framebuffer.exact_requests`
and `framebuffer.materialized_requests`; neither enters the ordinary CPU trace.

All four oracle axes stay `unverified`: this tool has no independent load/decode
oracle, GPU readback or final sampling observation. These results do not close
hardware or GPU evidence gates.

JSONL records are flushed after each task. Bank, source and witness deduplication
compares files on disk, so the tool retains the decoded capture and the current
task's observation batch rather than accumulating every attempt in memory.
