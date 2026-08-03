#!/usr/bin/env bash
set -euo pipefail

if (( $# != 1 )); then
  echo "usage: $0 PATH/TO/fast3d-VERSION.crate" >&2
  exit 2
fi

archive="$1"
if [[ ! -f "$archive" ]]; then
  echo "::error::fast3d archive does not exist: $archive" >&2
  exit 1
fi

archive_root="$(basename "$archive" .crate)"
if [[ "$archive_root" != fast3d-* ]]; then
  echo "::error::unexpected fast3d archive name: $archive" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
allowlist="$script_dir/../fast3d-package-files.txt"
actual="$(mktemp)"
expected="$(mktemp)"
trap 'rm -f -- "$actual" "$expected"' EXIT

while IFS= read -r entry; do
  if [[ "$entry" != "$archive_root/"* ]]; then
    echo "::error::archive entry is outside $archive_root/: $entry" >&2
    exit 1
  fi
  printf '%s\n' "${entry#"$archive_root/"}"
done < <(tar -tzf "$archive") > "$actual"

duplicates="$(LC_ALL=C sort "$actual" | uniq -d)"
if [[ -n "$duplicates" ]]; then
  echo "::error::fast3d archive contains duplicate paths" >&2
  printf '%s\n' "$duplicates" >&2
  exit 1
fi

grep -Ev '^[[:space:]]*(#|$)' "$allowlist" | LC_ALL=C sort -u > "$expected"
LC_ALL=C sort -u -o "$actual" "$actual"

if ! diff -u --label permitted-fast3d-package-paths --label actual-fast3d-package-paths \
  "$expected" "$actual"; then
  echo "::error::fast3d archive differs from its exact path allowlist" >&2
  exit 1
fi
