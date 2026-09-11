`ci4-dryrun.jsonl.gz` is a lossless gzip copy of the complete telemetry from
`/Volumes/DS Vault/hub/scratch/fast3d/b2-preflight-dryrun/telemetry.jsonl`, recorded
on ci4 on 2026-09-11. The decompressed SHA-256 is
`2df9a557ea9541339fb2c9380da349aa4162bc496fe83f145ac4592f750f9339`.

All 30 snapshots, process inventories, original derived fields and raw monitor
output are retained. Replay ignores the old derived CPU totals and rejection
lists, recomputes the 29 observed CPU intervals, and applies the current policy.
The first inventory still participates in prohibited-job checks. This fixture
contains no benchmark run or independent resting calibration, so retrospective
quiet admission cannot make it valid timing evidence.
