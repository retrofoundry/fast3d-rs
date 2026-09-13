import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys


def json_hash(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def artifact_file(root, name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name:
        raise ValueError(f"unsafe file: {name}")
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError(f"unsafe file: {name}")
    if not resolved.is_file():
        raise ValueError(f"missing file: {name}")
    return resolved


def expected_rows(inventory):
    if inventory["schema"] != 1 or not inventory["rows"]:
        raise ValueError("invalid inventory")
    expected = {}
    for row in inventory["rows"]:
        for config in row["configurations"]:
            if config not in inventory["configurations"]:
                raise ValueError(f"unknown configuration: {config}")
            key = (config, row["id"])
            if key in expected:
                raise ValueError(f"duplicate inventory row: {key}")
            expected[key] = row
    return expected


def capture_failures(manifest):
    failures = []
    for config, result in manifest["configurations"].items():
        if result["exit_code"] != 0:
            details = result.get("failed_tests", []) + result.get("failed_targets", [])
            log = result.get("build_log" if result.get("build_exit_code") not in [None, 0] else "test_log")
            failures.append(f"failed configuration: {config} (exit {result['exit_code']})"
                            + (f"; {', '.join(details)}" if details else "")
                            + (f"; log: {log}" if log else ""))
    failures.extend(manifest.get("errors", []))
    return failures


def load_capture(root, inventory, source_sha, run_id, *, allow_incomplete=False):
    root = Path(root)
    manifest = json.loads(artifact_file(root, "manifest.json").read_text(encoding="utf-8"))
    if manifest["schema"] != 1:
        raise ValueError("unsupported manifest schema")
    for field in ["source_sha", "tested_sha", "base_sha", "harness_sha256", "inventory_sha256"]:
        length = 40 if field.endswith("_sha") else 64
        if not re.fullmatch(r"[0-9a-f]{" + str(length) + "}", manifest[field]):
            raise ValueError(f"invalid {field}")
    if not manifest["runtime"] or not manifest["workflow"] or not str(manifest["run_attempt"]).isdigit():
        raise ValueError("missing runtime/workflow provenance")
    windows = manifest["runtime"]["os"] == "Windows"
    if windows and any(manifest["runtime"].get(field) != value for field, value in
                       [("backend", "dx12"), ("compiler", "staticdxc"), ("test_threads", "1")]):
        raise ValueError("WARP requires dx12/staticdxc and one test thread")
    if manifest["runtime"].get("dependency_comparison") == "native-test-graph-v1":
        if file_hash(artifact_file(root, "dependencies.txt")) != manifest["full_dependency_graph_sha256"]:
            raise ValueError("full dependency graph hash mismatch")
        graph = json.loads(artifact_file(root, "test-dependencies.json").read_text(encoding="utf-8"))
        if json_hash(graph) != manifest["runtime"]["dependencies"]:
            raise ValueError("test dependency graph hash mismatch")
    if manifest["source_sha"] != source_sha or manifest["tested_sha"] != source_sha:
        raise ValueError("source SHA does not match exact requested revision")
    if str(manifest["run_id"]) != str(run_id):
        raise ValueError("run ID does not match requested artifact")
    if manifest["inventory_sha256"] != json_hash(inventory):
        raise ValueError("incompatible inventory hash")
    if set(manifest["configurations"]) != set(inventory["configurations"]):
        raise ValueError("configuration inventory differs")
    failures = capture_failures(manifest)
    if failures and not allow_incomplete:
        raise ValueError("; ".join(failures))
    expected = expected_rows(inventory)
    rows = {}
    files = set()
    for row in manifest["rows"]:
        key = (row["configuration"], row["id"])
        if key in rows:
            raise ValueError(f"duplicate row: {key}")
        if key not in expected:
            raise ValueError(f"unexpected row: {key}")
        for field in ["stage", "width", "height", "blend_path"]:
            if row[field] != expected[key][field]:
                raise ValueError(f"{key}: incompatible {field}")
        if row["channels"] != "RGBA":
            raise ValueError(f"{key}: channel policy must be RGBA")
        if not row.get("test_id") or row["role"] not in ["render", "cpu-oracle", "compute"]:
            raise ValueError(f"{key}: missing test or decode provenance")
        if row["stage"] == "final" or row["role"] == "compute":
            adapter = row.get("adapter")
            if not adapter or any(field not in adapter for field in
                                  ["name", "vendor", "device", "device_type", "driver", "driver_info", "backend"]):
                raise ValueError(f"{key}: missing adapter provenance")
            if windows and (adapter["backend"] != "Dx12" or adapter["device_type"] != "Cpu"):
                raise ValueError(f"{key}: WARP requires a CPU Dx12 adapter")
        if not re.fullmatch(r"[0-9a-f]{64}", row["input_sha256"]):
            raise ValueError(f"{key}: invalid input hash")
        if file_hash(artifact_file(root, row["input_file"])) != row["input_sha256"]:
            raise ValueError(f"{key}: input_sha256 mismatch")
        path = artifact_file(root, row["file"])
        if path in files:
            raise ValueError(f"overwritten output file: {row['file']}")
        files.add(path)
        pixels = path.read_bytes()
        if row["width"] <= 0 or row["height"] <= 0 or len(pixels) != row["width"] * row["height"] * 4:
            raise ValueError(f"{key}: wrong RGBA byte length")
        if file_hash(path) != row["sha256"]:
            raise ValueError(f"{key}: file hash mismatch")
        if row["stage"] == "decode":
            literal = artifact_file(root, row["expected_file"])
            if file_hash(literal) != row["expected_sha256"] or literal.read_bytes() != pixels:
                raise ValueError(f"{key}: independent literal differs from decode")
        rows[key] = row
    missing = set(expected) - set(rows)
    if missing and not allow_incomplete:
        raise ValueError(f"missing rows: {sorted(missing)}")
    return manifest, rows


def compare(base, candidate, inventory, base_sha, candidate_sha, base_run_id, candidate_run_id,
            *, allow_incomplete=False):
    parent, parent_rows = load_capture(base, inventory, base_sha, base_run_id, allow_incomplete=allow_incomplete)
    head, head_rows = load_capture(candidate, inventory, candidate_sha, candidate_run_id, allow_incomplete=allow_incomplete)
    if head["base_sha"] != base_sha:
        raise ValueError("candidate base SHA does not match requested base")
    for field in ["runtime", "harness_sha256"]:
        if parent[field] != head[field]:
            raise ValueError(f"incompatible {field}; same-run base capture required")
    missing = {label: sorted(set(expected_rows(inventory)) - set(rows))
               for label, rows in [("base", parent_rows), ("candidate", head_rows)]}
    failures = {"base": capture_failures(parent), "candidate": capture_failures(head)}
    complete = not any(missing.values()) and not any(failures.values())
    report = {"passed": complete, "complete": complete, "missing_rows": missing, "failures": failures,
              "threshold": 0, "base_sha": base_sha, "candidate_sha": candidate_sha,
              "base_run_id": str(base_run_id), "candidate_run_id": str(candidate_run_id), "rows": []}
    for key, row in head_rows.items():
        if key not in parent_rows:
            continue
        old = parent_rows[key]
        for field in ["input_sha256", "adapter", "test_id", "stage", "blend_path"]:
            if old.get(field) != row.get(field):
                raise ValueError(f"{key}: incompatible {field}; same-run base capture required")
        if row["stage"] == "decode" and old["expected_sha256"] != row["expected_sha256"]:
            raise ValueError(f"{key}: independent literals changed")
        left = artifact_file(Path(base), old["file"]).read_bytes()
        right = artifact_file(Path(candidate), row["file"]).read_bytes()
        deltas = [abs(a - b) for a, b in zip(left, right)]
        changed = sum(delta != 0 for delta in deltas)
        report["rows"].append({"configuration": key[0], "id": key[1], "changed_bytes": changed,
                               "changed_pixels": sum(any(deltas[i:i + 4]) for i in range(0, len(deltas), 4)),
                               "channel_maxima": [max(deltas[c::4], default=0) for c in range(4)]})
        report["passed"] &= changed == 0
    return report


def main():
    parser = argparse.ArgumentParser(description="Compare every frozen RGBA byte at threshold zero.")
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--base-run-id", required=True)
    parser.add_argument("--candidate-run-id", required=True)
    parser.add_argument("--inventory", type=Path, default=Path(__file__).with_name("inventory.json"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = compare(args.base, args.candidate, json.loads(args.inventory.read_text(encoding="utf-8")),
                         args.base_sha, args.candidate_sha, args.base_run_id, args.candidate_run_id)
    except (ValueError, KeyError, OSError) as error:
        report = {"passed": False, "error": str(error)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
