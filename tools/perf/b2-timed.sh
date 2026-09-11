#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${B1_ATTESTATION:?}" "${B1_RESERVED_START:?}" "${B1_RESERVED_END:?}"
exec python3 tools/perf/timed.py \
  --out "/Volumes/DS Vault/hub/scratch/fast3d/b2-timed/${1:-attempt2}" \
  --candidate "${B2_CANDIDATE_BIN:-/Volumes/DS Vault/hub/scratch/fast3d/b2-candidate-build/target/release/b1-perf}" \
  --parent "${B2_PARENT_BIN:-/Volumes/DS Vault/hub/scratch/fast3d/b2-parent-build/target/release/b1-perf}" \
  --captures '/Volumes/DS Vault/hub/scratch/captures/seq' \
  --scene '/Users/ci/hub/repos/n64.toys/crates/asm/tests/scenes/chrome-icosphere.n64' \
  --settle "${B2_SETTLE:-90}" --attestation "$B1_ATTESTATION" \
  --scheduled-start "$B1_RESERVED_START" --scheduled-end "$B1_RESERVED_END"
