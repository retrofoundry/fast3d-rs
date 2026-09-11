B1 provides diagnostic instrumentation and preliminary cache-cost measurements. B2 owns quiet performance evidence and optimization decisions. The driver is outside the workspace. Generated manifests, dependency locks, binaries, traces and results belong in scratch. The assembler remains a read-only path dependency at its existing revision and pin.

From the reviewed worktree, using the tools already on PATH:

```sh
export B1_BUILD='/Volumes/DS Vault/hub/scratch/fast3d/b1-build'
export B1_OUT='/Volumes/DS Vault/hub/scratch/fast3d/b1-evidence'
export B1_CAPTURES='/Volumes/DS Vault/hub/scratch/captures/seq'
export B1_ASM='/Users/ci/hub/repos/n64.toys'
export B1_SCENE="$B1_ASM/crates/asm/tests/scenes/chrome-icosphere.n64"
python3 tools/perf/configure.py --assembler "$B1_ASM" --out "$B1_BUILD"
cargo build --offline --release --manifest-path "$B1_BUILD/Cargo.toml"
export B1_BIN="$B1_BUILD/target/release/b1-perf"
```

`inspect` validates and interprets the complete reset prefix without acquiring a device. It records CPU diagnostic counts, not GPU execution or a performance baseline. `source-cpu` calls the real assembler at every sampled time and rejects diagnostics from the bridge. It saves the generated RGBA8 and assembled RGBA16 bytes and their hashes.

```sh
for name in demo1-dense demo1 pinned demo2; do
  "$B1_BIN" metadata "$B1_CAPTURES/$name.f3dcap" "$B1_OUT/$name"
  "$B1_BIN" inspect "$B1_CAPTURES/$name.f3dcap" "$B1_OUT/$name"
done
"$B1_BIN" source-cpu "$B1_SCENE" "$B1_OUT/source"
"$B1_BIN" preflight authored "$B1_OUT/authored-costs"
python3 tools/perf/report.py cost-report --input "$B1_OUT/authored-costs/costs.jsonl" --out "$B1_OUT/authored-costs/break-even.json"
for name in demo1-dense demo1 pinned demo2 source; do
  "$B1_BIN" preflight "$B1_OUT/$name/cases.json" "$B1_OUT/$name/costs"
  python3 tools/perf/report.py summarize --input "$B1_OUT/$name/frames.jsonl" --out "$B1_OUT/$name/summary.json"
  python3 tools/perf/report.py break-even --costs "$B1_OUT/$name/costs/costs.jsonl" --hits "$B1_OUT/$name/hits.jsonl" --out "$B1_OUT/$name/break-even.json"
done
python3 tools/perf/report.py manifest --build "$B1_BUILD" --assembler "$B1_ASM" --captures "$B1_CAPTURES" --source-run "$B1_OUT/source" --evidence "$B1_OUT" --out "$B1_OUT/manifest" --costs "$B1_OUT/authored-costs/costs.jsonl" "$B1_OUT/demo1-dense/costs/costs.jsonl" "$B1_OUT/demo2/costs/costs.jsonl" "$B1_OUT/source/costs/costs.jsonl" --artifacts "$B1_OUT/demo1-dense" "$B1_OUT/demo2" "$B1_OUT/source"
```

Each diagnostic directory contains every frame in `frames.jsonl`, ordered full encoded requests in `requests.jsonl`, finite-budget simulation records in `hits.jsonl`, and all distinct canonical inputs in `cases.json`. Raw traces can be several gigabytes. They contain captured texture content and belong in scratch. Native and browser preflight emit four rows per representation/format/TLUT/extent/input-size class in `costs.jsonl`: hot distinct-equal buffers and a rotating set for each of `fnv1a64-v1` and `xxh3-64-v1`. Hash order alternates between classes. Each row measures the selected hash in both the isolated probe and complete hit/miss paths. Authored requests sweep smaller/larger extents and all supported formats, including the 32×32 RGBA16 tile and 4096×4 lookup. Authored rotating sets use 16 byte variants. Captured rotating sets include every distinct observed canonical input without a sampling cap.

`metadata` writes `admission.json`, counting configurations and microcode/data/memory declarations across every frame and task. It can add metadata to an existing diagnostic directory without replacing frame records.

Preflight compares FNV-1a 64 (`fnv1a64-v1`) with default-seed XXH3-64 (`xxh3-64-v1`, `twox-hash` 2.1.2, one-shot API, `std` and `xxhash3_64` features). The dependency belongs only to the standalone measurement driver. Native aarch64 dispatch can use NEON; wasm uses the crate’s scalar implementation. The ordered diagnostic simulator still uses FNV-1a. `hash-inputs.json` (browser: `hash_inputs`) checks every distinct canonical input across all classes for collisions under both hashes and equal-key output agreement. With no collisions, canonical equality, request order and payload sizes give both algorithms the same simulated hit/miss and eviction history. Existing trace files can then be reused with their original provenance; they are not new rendered or timed executions. Canonical bytes start with `fast3d-b1-bank-v1\0`, a one-byte representation, eight little-endian u16 descriptor fields, ten one-byte descriptor/TLUT fields, all 4096 bank bytes, and any reconstructed linear bytes. This is 45 metadata bytes before bank/compatibility input. It is a conservative measurement identity, not a replacement-pack identifier. Diagnostic read visitors measure distinct bytes and read operations at the existing decoder's reads; they do not implement a cache footprint. The linear compatibility path retains source mapping for reconstruction. Rejected requests never become simulated hits.

The cost unit is ns/request. `D` is decoder execution including its existing output allocation. `owned_copy` and `allocation_only` are separate probes; the model claims zero avoided ownership copies (`copy_multiplier_in_d=0`). Common validation and reconstruction are measured separately and together, included in both end-to-end paths, and excluded from incremental `K`. `K` sums snapshot/metadata construction, hashing and map lookup. `snapshot` includes the unchanged canonical builder’s allocation, vector growth, metadata encoding and bank/linear copies. `canonical_copy_control` allocates and copies an already-built key of the same size; the difference estimates construction overhead beyond that bulk copy. `snapshot_and_hash` directly measures their combined path. Bank-only and metadata-only hash probes are controls, not terms added to `K`. `H` sums distinct-buffer equality and Arc handoff. `M` measures insertion, retained ownership and eviction with owned inputs prepared outside its batch. Full hit and miss paths execute independently, and `baseline_total` calls the existing material validity scan followed by the unchanged full decode dispatch. That scan is common work already paid before production decoding. The baseline/common decomposition residual is recorded too; it catches extra validation or dispatch work omitted by a component model. Their residual against the component model is reported; an unexplained residual beyond uncertainty leaves the boundary unresolved.

The miss probe churns a class-local cache holding half the rotating set. A one-input hot miss probe explicitly evicts its prior entry. Its budget is recorded in each cost row. Those costs do not establish eviction cost for a mixed 16 MiB working set. The ordered simulator uses a default 16 MiB payload budget (`B1_CACHE_BUDGET` overrides it), LRU, canonical equality on digest matches, and two subsequent frames of pinning. Payload accounting includes key/decoded bytes and reports retained/pinned high water, evictions and bypasses. It does not observe actual scene reference lifetimes or allocator overhead, so its hit fractions and predicted savings are explicitly upper bounds. B2 must resolve these limitations before using a cache forecast for acceptance.

`h_min=(K+M)/(D-H+M)` is only defined for a positive denominator. Perfect hits require `D>K+H`; `h_min>=1` cannot produce a strict gain. Reports enumerate measured viable sizes rather than assuming monotonicity. Both cost and workload reports use `by_hash`; batch spread identities include the hash version. Workload windows list zero-request cost classes separately and exclude them from weighted forecasts. Authored Lookup/Linear results carry no observed frequency and cannot offset observed Tile costs. Component residual gates and direct full-path predictions remain separate, including a direct full-path `h_min`. Each boundary carries measured spread, clock precision and end-to-end model residuals. No throughput or break-even threshold is a unit-test constant.

For actual native measurement, use `sequence` or `source`. Every invocation creates a fresh renderer. Counter-only, coarse and detailed runs are separate configurations; `trace` is diagnostic. Device/pipeline/output setup, replay adaptation, result serialization, readback and completion waits are separate from the CPU spans.

```sh
"$B1_BIN" sequence "$B1_CAPTURES/demo1-dense.f3dcap" "$B1_OUT/native-dense-coarse" coarse
"$B1_BIN" sequence "$B1_CAPTURES/demo2.f3dcap" "$B1_OUT/native-demo2-detailed" detailed
"$B1_BIN" source "$B1_SCENE" "$B1_OUT/native-source-coarse" coarse
```

Use all four sequence inputs, plus the source on Metal and Chrome. The observed windows come from the capture warm-up field: 1400–1519, 1200–1600, 1400–1419 and 2600–2719 respectively. Every earlier frame executes and presents. The source samples `f32(i)/60` for 0–719, observing 120–719, with F3DEX2/Fixed, pairless logical 320×240, output 800×600, PerFrame and seed zero. Capture configuration, Persist policy, device declaration and output extent remain recorded values. Add `--one-frame` for the pacing control. Native waits can retire both outstanding frames together; the bound is two, not a promise that exactly two remain outstanding. Browser completion uses queue notifications rather than browser polling. All waits remain outside renderer spans.

`process_dl`, `begin_frame` and `presentation` are coarse inclusive spans; their sum is the end-to-end library metric. Detailed `interpretation`, `render_inputs`, `encoding`, `resources` and `submission` nest under `process_dl`; `decode` nests under interpretation and resource API spans can nest within encoding/resources. Sum exclusive spans when reconciling phases. `process_dl.exclusive_ms` is remaining library bookkeeping. Never add nested inclusive spans to the coarse total. Times are host monotonic elapsed time, not GPU time or OS thread CPU consumption. Gauge maxima over all frame records describe known application resources; creation capacity totals are not a driver memory high-water estimate.

Correctness runs retain every readback hash, summary and diagnostic. They are separate from timing:

```sh
"$B1_BIN" ordinary "$B1_CAPTURES/demo2.f3dcap" "$B1_OUT/demo2-ordinary"
"$B1_BIN" sequence "$B1_CAPTURES/demo2.f3dcap" "$B1_OUT/demo2-measured-correctness" trace --readback
python3 tools/perf/report.py compare --input "$B1_OUT/demo2-ordinary/frames.jsonl" --other "$B1_OUT/demo2-measured-correctness/frames.jsonl" --out "$B1_OUT/demo2-mode-comparison.json"
```

Repeat on all four captures. Missing frames, changed hashes/diagnostics/summaries, an admission failure or any dropped task refutes equivalence. A command-trace vector is intentionally absent in measurement mode. Original replay remains the library default.

The parent-compatible adapter uses public APIs already present at `509cf9b`. It streams all readbacks and supports these declared seed-zero inputs. Build it separately; never patch the assembler's pin:

```sh
export B1_PARENT='/Volumes/DS Vault/hub/scratch/fast3d/b1-parent-build'
python3 tools/perf/configure.py --baseline --library /Users/ci/hub/repos/fast3d-rs/fast3d --assembler "$B1_ASM" --out "$B1_PARENT"
cargo build --offline --release --manifest-path "$B1_PARENT/Cargo.toml"
"$B1_PARENT/target/release/b1-perf" sequence "$B1_CAPTURES/demo2.f3dcap" "$B1_OUT/demo2-parent"
python3 tools/perf/report.py compare --input "$B1_OUT/demo2-parent/frames.jsonl" --other "$B1_OUT/demo2-measured-correctness/frames.jsonl" --out "$B1_OUT/demo2-parent-comparison.json"
"$B1_PARENT/target/release/b1-perf" source "$B1_SCENE" "$B1_OUT/source-parent"
"$B1_BIN" source "$B1_SCENE" "$B1_OUT/source-correctness" coarse --readback
python3 tools/perf/report.py compare --input "$B1_OUT/source-parent/frames.jsonl" --other "$B1_OUT/source-correctness/frames.jsonl" --out "$B1_OUT/source-parent-comparison.json"
```

The comparison must exit zero with every frame accounted for. Review samples and mismatch PNG/masks can be produced with the existing replay example; hashes are streamed rather than keeping all readback images resident. Parent/candidate IMAGE oracle checks retain their existing fixture-specific thresholds, including the exact workload-interleaving RGB gate. This driver does not establish rt64 semantics for HOST64 captures.

Build browser artifacts outside a timing reservation. Use the installed matching wasm-bindgen and ChromeDriver:

```sh
cargo build --offline --release --manifest-path "$B1_BUILD/Cargo.toml" --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir "$B1_BUILD/pkg" "$B1_BUILD/target/wasm32-unknown-unknown/release/b1_perf.wasm"
python3 tools/perf/browser_run.py --build "$B1_BUILD" --scene "$B1_SCENE" --out "$B1_OUT/chrome-source" --chromedriver "$CHROMEDRIVER" --mode coarse
python3 tools/perf/browser_run.py --build "$B1_BUILD" --scene "$B1_SCENE" --out "$B1_OUT/chrome-source-correctness" --chromedriver "$CHROMEDRIVER" --readback
python3 tools/perf/browser_run.py --build "$B1_BUILD" --scene "$B1_SCENE" --out "$B1_OUT/chrome-authored-costs" --chromedriver "$CHROMEDRIVER" --kind preflight
python3 tools/perf/browser_run.py --build "$B1_BUILD" --scene "$B1_SCENE" --out "$B1_OUT/chrome-source-costs" --chromedriver "$CHROMEDRIVER" --kind preflight --cases "$B1_OUT/source/cases.json"
```

Run observed-case preflight for both routes too. `--kind sequence --capture PATH --readback` exercises complete sequences in Chrome without changing their admission declaration. A rejected dual-source requirement or browser memory failure is blocked validation. It cannot be replaced with a truncated prefix or silently forced fallback. Browser results include capabilities, user agent, UTC interval, clock probe, adapter/features/limits and `result.jsonl`. For browser parent/candidate correctness, also build/bindgen the parent manifest and use `browser_run.py --build "$B1_PARENT" --readback`; compare its `result.jsonl` to the candidate. `serve.py` provides the same entry points interactively.

The following are hardware tests, not passing skips:

```sh
cargo test --offline -p fast3d --features capture,profiling --test profiling_capture -- --nocapture
cargo test --offline -p fast3d --features profiling --lib upload_counts_include_all_texture_roles -- --nocapture
cargo test --offline -p fast3d --features profiling --lib buffer_init_counts_bytes -- --nocapture
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT=300
export WASM_BINDGEN_TEST_WEBDRIVER_JSON="$PWD/fast3d/webdriver.json"
cargo test --offline -p fast3d --features capture,profiling --target wasm32-unknown-unknown --test browser_sm64_fixture_replay --test capture_facade --test capture_memory_wasm --test profiling_capture -- --nocapture
```

The authored upload test requires exactly one upload for each of lod0/lod1/lod2/texture1/detail, four bytes each, no extra level-zero alias upload, and zero further uploads for an unchanged material. The three-byte buffer initializer must count one creation, three uploaded bytes and four requested capacity bytes. Any mismatch refutes operation accounting. Keep the existing Metal, Windows DX12/static-DXC, dual-source/fallback and pinned IMAGE oracle matrix from the direction; WARP is correctness evidence only.

Reserve the machine before timing. `protocol.py` records 30 one-second preflight samples, the complete child benchmark/Chrome process tree during the command, and ten postflight samples. It uses the exact commands and fields embedded in `POLICY`/`COMMANDS`: ps cumulative CPU deltas normalized to occupied cores, top aggregate idle, vm_stat swap-out count and paging state, and pmset power/thermal reports. Monitoring errors or missing data invalidate the attempt. It never kills unrelated processes. Preflight requires 95% idle on every sample. During the run, unrelated CPU must average at most 0.10 core and cannot exceed 0.25 core in two consecutive samples. Swap-out growth, throttling, prohibited jobs, power changes or gaps over 1.5 seconds invalidate it. GPU utilization is unavailable in this portable monitor; process inventory and the operator's reservation attestation remain necessary.

Set `B1_ATTESTATION`, `B1_RESERVED_START` and `B1_RESERVED_END` to the operator's actual statement and reserved UTC interval. For example, wrap a prebuilt native command as follows; use a new directory for every attempt:

```sh
python3 tools/perf/protocol.py --out "$B1_OUT/quiet/demo2-coarse-pair1-candidate" --attestation "${B1_ATTESTATION:?}" --scheduled-start "${B1_RESERVED_START:?}" --scheduled-end "${B1_RESERVED_END:?}" --pair demo2-coarse-1-attempt1 --revision candidate --workload demo2 --configuration coarse --allowed-idle WindowServer -- "$B1_BIN" sequence "$B1_CAPTURES/demo2.f3dcap" "$B1_OUT/timed/demo2-coarse-1-candidate" coarse
```

Wrap `browser_run.py` the same way; ChromeDriver, Chrome and the local server then belong to the recorded benchmark tree. Do not wrap cargo, bindgen or other build commands. Run counter/coarse controls with and without the monitor to inspect observer cost. Preserve every attempted run, including slow valid ones and invalid pairs. Alternate parent/candidate order across five complete pairs for each workload/configuration. If either member fails admission, rerun the whole pair after a fresh preflight using a new attempt ID.

`report.py batch --input attempts.json --out batch.json` accepts `{"attempts":[{"quiet_run":".../run.json","summary":".../summary.json"}]}`; use `"costs":".../costs.jsonl"` for a microbenchmark attempt. A costs-only batch may set `"comparison":false` because each attempt already measures both baseline and candidate paths; five repetitions and every component spread still apply. It preserves attempts, invalidates both members of a failed pair, and requires exactly five valid totals per revision/workload/configuration/window/phase/component with `(max-min)/median <= 0.05`. It never selects the fastest five. Supply `--batch batch.json` to manifest generation to record the computed spreads and actual pair order. Missing telemetry, missing repetitions or excess spread remains an explicit rejection. B1 smoke manifests are expected to reject quiet admission.
