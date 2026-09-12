# Exact readback gate

`validate.yml` exports default, debug-ui and all-features captures on macOS and
Windows. `inventory.json` freezes 171 configuration rows: 29 golden test variants
and 27 decode witnesses in each configuration, plus three IMAGE fixtures under
all-features. `goldens.json` freezes the SHA-256 ledger of all 27 committed golden
files. Neither inventory is discovered from whatever output a test happens to write.

Every comparison uses all four RGBA channels, threshold zero and zero differing
pixels. The existing tolerance-two golden assertions still run. A missing adapter,
row, configuration, input, literal or output file fails the gate. Windows captures
must report a CPU DX12 adapter, static DXC and one test thread.

Golden filenames use the Rust test name. In particular, the three multi-material
tests have distinct files. `FAST3D_GOLDEN_OUTPUT` and `FAST3D_DECODE_OUTPUT` point to
separate directories inside each feature configuration. Writers refuse to replace
an existing row. Final rows record the actual adapter and requested device features;
T1 decode rows record `cpu-oracle` and retain independently derived expected bytes.
The IMAGE inputs are the complete capture files. Golden input witnesses retain
generated RDRAM and entry addresses, together with the test and common-helper source
that constructs inline scenes and sampling parameters.

The artifact manifest records the actual tested source SHA, base SHA, workflow run
and attempt, resolved checkout/output paths, harness file hashes, input/output hashes,
dimensions, stage, role, blend path, command, binary hashes, exit codes, adapter,
compiler, runner image and dependency graph. The generated `Cargo.lock`, build logs
and test logs travel with the artifact.

For PRs, `ci.py` searches successful main push runs with the exact PR base SHA and
downloads `readbacks-macos-14` or `readbacks-windows-latest`. It verifies the manifest
source SHA and run ID before comparing. Missing, expired or incompatible artifacts
trigger a capture of the exact base checkout in the same job. A cross-run byte delta
also triggers this capture and retains the original comparison report. A nonzero
same-run delta fails.

A failed candidate configuration also triggers the same-run base capture. Cargo
uses `--no-fail-fast` so later test targets and configurations still export their
rows. `comparison.json` compares the available authenticated rows and lists missing
rows and failed configurations on each side. Such a report has `passed: false`
and `complete: false`, even if every available byte agrees. Integrity and provenance
checks remain mandatory. `failure.json` names failing tests/targets and their build
or test logs; a partial report cannot satisfy the gate or supply a successful base
artifact. The standalone comparator continues to require complete captures.

The base receives only the files listed in `harness-files.json`, the candidate's
`[dev-dependencies]` section, and the frozen dependency lock. `bootstrap-overlay.json`
records the overlay hashes separately from the base production SHA. The bootstrap
compares the production dependency graph before and after the overlay under default
and all-features. Its initial offline resolution may prune the candidate lock's new
dev dependencies for the original base manifest; captures restore and use the exact
candidate lock with `--locked`. A production dependency graph change fails.

Both CI artifacts upload even when tests or comparison fail, with 90-day retention.
`readbacks-<runner>` contains the candidate capture. `readback-evidence-<runner>` also
contains any downloaded/base capture, bootstrap provenance, reports and failure
details. Expired evidence must be regenerated.

Run the CPU checks from the repository's devenv shell:

```sh
python3 -m unittest discover -s tools/readbacks -v
cargo test -p fast3d --lib readback_export
```

To download two explicitly selected runs and compare them, use the authenticated
GitHub CLI through `fetch.py`. The base run must be a successful main push for
`BASE_SHA`. `CANDIDATE_SHA` is the actual tested SHA recorded in the PR artifact,
which may be the merge SHA and differ from the API's PR head SHA.

```sh
python3 '/Volumes/DS Vault/hub/wt/fast3d-rs-tmem/tools/readbacks/fetch.py' \
  --repo "$REPOSITORY" --base-sha "$BASE_SHA" --candidate-sha "$CANDIDATE_SHA" \
  --base-run-id "$BASE_RUN_ID" --candidate-run-id "$CANDIDATE_RUN_ID" \
  --artifact-name readbacks-windows-latest \
  --output '/Volumes/DS Vault/hub/scratch/fast3d/tmem-checks/T1/fetched-warp'
```

This command requires a fresh output directory. Missing or incompatible evidence
exits 1 and requests the workflow's same-run bootstrap; it never selects another
revision. For an initial T1 bootstrap, download the run's `readback-evidence-*`
artifact and compare its `bootstrap-base` and `candidate` directories below.

To compare downloaded captures, pass the two source SHAs and run IDs explicitly.
`BASE_ARTIFACT` and `CANDIDATE_ARTIFACT` must name physical directories containing
`manifest.json`; take each run ID from the selected workflow run, then verify it
against its manifest. For a same-run bootstrap the two run IDs are equal.

```sh
python3 '/Volumes/DS Vault/hub/wt/fast3d-rs-tmem/tools/readbacks/readbacks.py' \
  --base "$BASE_ARTIFACT" --candidate "$CANDIDATE_ARTIFACT" \
  --base-sha "$BASE_SHA" --candidate-sha "$CANDIDATE_SHA" \
  --base-run-id "$BASE_RUN_ID" --candidate-run-id "$CANDIDATE_RUN_ID" \
  --output '/Volumes/DS Vault/hub/scratch/fast3d/tmem-checks/T1/readback-comparison.json'
```

The comparator exits 1 for validation failures or changed bytes and writes the
reason or per-row changed-byte/pixel counts and channel maxima. Its unit tests
exercise an alpha-only mutation, truncated and missing rows, overwritten
configurations, incorrect SHAs/run IDs, input corruption, independent decode
literals and unchanged captures. These CPU checks do not establish the Metal or
WARP hardware cells; the workflow must complete against the actual base.
