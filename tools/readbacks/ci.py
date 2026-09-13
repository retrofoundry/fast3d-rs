import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import zipfile

from readbacks import artifact_file, compare, file_hash, json_hash, load_capture


HERE = Path(__file__).resolve().parent
INVENTORY = json.loads((HERE / "inventory.json").read_text(encoding="utf-8"))
COMMANDS = {
    "default": ["cargo", "test", "--locked", "--no-fail-fast", "--workspace"],
    "debug-ui": ["cargo", "test", "--locked", "--no-fail-fast", "-p", "fast3d", "--features", "debug-ui"],
    "all-features": ["cargo", "test", "--locked", "--no-fail-fast", "--workspace", "--all-features"],
}


def command_output(command, checkout):
    return subprocess.check_output(command, cwd=checkout, text=True, encoding="utf-8").strip()


def freeze_lock(checkout):
    if not (checkout / "Cargo.lock").is_file():
        subprocess.check_call(["cargo", "generate-lockfile"], cwd=checkout)
    if not (checkout / "Cargo.lock").is_file():
        raise ValueError("cargo did not generate Cargo.lock")


def harness_files(checkout):
    names = json.loads((HERE / "harness-files.json").read_text(encoding="utf-8"))
    files = {name: file_hash(artifact_file(checkout, name)) for name in names}
    files["Cargo.lock"] = file_hash(checkout / "Cargo.lock")
    files["fast3d/Cargo.toml#dev-dependencies"] = json_hash(dev_dependencies((checkout / "fast3d/Cargo.toml").read_text(encoding="utf-8")))
    return files


def dev_dependencies(toml):
    section = re.search(r"(?ms)^\[dev-dependencies\]\n.*?(?=^\[|\Z)", toml)
    if section is None:
        raise ValueError("missing dev-dependencies section")
    return section.group()


def replace_dev_dependencies(original, candidate):
    return original.replace(dev_dependencies(original), dev_dependencies(candidate), 1)


def collect_rows(output):
    output = output.resolve()
    rows = []
    errors = []
    for config in INVENTORY["configurations"]:
        for stage in ["final", "decode"]:
            directory = output / config / stage
            for metadata in sorted(directory.glob("*.json")):
                try:
                    row = json.loads(metadata.read_text(encoding="utf-8"))
                    stem = row["id"]
                    binary = artifact_file(directory, f"{stem}.bin")
                    source = artifact_file(directory, f"{stem}.input.bin")
                    row.update(configuration=config, file=binary.relative_to(output).as_posix(),
                               sha256=file_hash(binary), input_file=source.relative_to(output).as_posix(),
                               input_sha256=file_hash(source))
                    if stage == "decode":
                        expected = artifact_file(directory, f"{stem}.expected.bin")
                        row.update(expected_file=expected.relative_to(output).as_posix(),
                                   expected_sha256=file_hash(expected))
                    if row["stage"] != stage:
                        raise ValueError(f"wrong output stage: {metadata}")
                    rows.append(row)
                except (ValueError, KeyError, OSError) as error:
                    errors.append(f"{metadata}: {error}")
    return rows, errors


def save_manifest(output, manifest):
    manifest["rows"], manifest["errors"] = collect_rows(output)
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def test_dependency_graph(metadata, checkout):
    # Native tests compile normal and dev dependencies together; build edges stay distinct.
    normalized = json.dumps(metadata).replace(checkout.as_uri(), "file://<checkout>").replace(str(checkout), "<checkout>")
    nodes = json.loads(normalized)["resolve"]["nodes"]
    return sorted([{
        "id": node["id"], "features": sorted(node["features"]),
        "dependencies": sorted([{
            "name": dep["name"], "package": dep["pkg"],
            "uses": sorted({("build" if use["kind"] == "build" else "test", use["target"] or "")
                            for use in dep["dep_kinds"]}),
        } for dep in node["deps"]], key=lambda dep: (dep["name"], dep["package"])),
    } for node in nodes], key=lambda node: node["id"])


def capture(checkout, output, base_sha, harness_checkout):
    checkout, output = checkout.resolve(), output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    if "UPDATE_GOLDENS" in os.environ:
        raise ValueError("UPDATE_GOLDENS must be unset")
    freeze_lock(checkout)
    source_sha = command_output(["git", "rev-parse", "HEAD"], checkout)
    goldens = json.loads((HERE / "goldens.json").read_text(encoding="utf-8"))
    actual_goldens = {path.relative_to(checkout).as_posix(): file_hash(path)
                      for path in sorted((checkout / "fast3d/goldens").glob("*.bin"))}
    if actual_goldens != goldens:
        raise ValueError("committed golden inventory or bytes changed")
    harness = harness_files(harness_checkout)
    shutil.copyfile(checkout / "Cargo.lock", output / "Cargo.lock")
    dependencies = command_output(["cargo", "tree", "--locked", "--workspace", "--all-features", "-e", "features"], checkout)
    dependencies = dependencies.replace(str(checkout), "<checkout>")
    (output / "dependencies.txt").write_text(dependencies, encoding="utf-8")
    rustc = command_output(["rustc", "-Vv"], checkout)
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    metadata = json.loads(command_output(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1", "--filter-platform", host], checkout))
    graph = test_dependency_graph(metadata, checkout)
    (output / "test-dependencies.json").write_text(json.dumps(graph, indent=2) + "\n", encoding="utf-8")
    manifest = {
        "schema": 1, "source_sha": source_sha, "tested_sha": source_sha, "base_sha": base_sha,
        "run_id": os.environ["GITHUB_RUN_ID"], "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "workflow": os.environ.get("GITHUB_WORKFLOW", "validate"),
        "event": os.environ.get("GITHUB_EVENT_NAME", "local"),
        "harness_source_sha": command_output(["git", "rev-parse", "HEAD"], harness_checkout),
        "harness_sha256": json_hash(harness), "harness_files": harness,
        "inventory_sha256": json_hash(INVENTORY), "checkout": str(checkout), "output": str(output),
        "goldens": goldens, "full_dependency_graph_sha256": file_hash(output / "dependencies.txt"),
        "runtime": {
            "os": platform.system(), "release": platform.release(), "version": platform.version(),
            "machine": platform.machine(), "image_os": os.environ.get("ImageOS", "local"),
            "image_version": os.environ.get("ImageVersion", "local"),
            "rustc": rustc, "dependency_comparison": "native-test-graph-v1",
            "dependencies": json_hash(graph), "lock_sha256": file_hash(checkout / "Cargo.lock"),
            "backend": os.environ.get("WGPU_BACKEND", ""),
            "compiler": os.environ.get("WGPU_DX12_COMPILER", ""),
            "test_threads": os.environ.get("RUST_TEST_THREADS", ""),
        },
        "configurations": {config: {"command": command, "exit_code": None}
                           for config, command in COMMANDS.items()}, "rows": [], "errors": [],
    }
    save_manifest(output, manifest)
    try:
        for config, command in COMMANDS.items():
            directory = output / config
            directory.mkdir()
            env = dict(os.environ, FAST3D_GOLDEN_OUTPUT=str(directory / "final"),
                       FAST3D_DECODE_OUTPUT=str(directory / "decode"))
            build = subprocess.run(command + ["--no-run", "--message-format=json"], cwd=checkout,
                                   env=env, text=True, encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (directory / "build.log").write_text(build.stdout, encoding="utf-8")
            binaries = {}
            for line in build.stdout.splitlines():
                try:
                    message = json.loads(line)
                    executable = message.get("executable")
                    if executable:
                        binaries[str(Path(executable).resolve())] = file_hash(executable)
                except json.JSONDecodeError:
                    continue
            with (directory / "test.log").open("w") as log:
                result = subprocess.run(command, cwd=checkout, env=env, stdout=log, stderr=subprocess.STDOUT) if build.returncode == 0 else build
            test_log = (directory / "test.log").read_text(encoding="utf-8")
            manifest["configurations"][config] = {
                "command": command, "exit_code": result.returncode, "build_exit_code": build.returncode,
                "failed_tests": sorted(set(re.findall(r"(?m)^test (\S+) \.\.\. FAILED\s*$", test_log))),
                "failed_targets": sorted(set(re.findall(r"(?m)^error: test failed, to rerun pass (.+)$", test_log))),
                "build_log": f"{config}/build.log", "test_log": f"{config}/test.log",
                "binaries": binaries, "environment": {key: env[key] for key in ["FAST3D_GOLDEN_OUTPUT", "FAST3D_DECODE_OUTPUT"]},
            }
            print(f"{checkout}: {config} exit {result.returncode}", flush=True)
    finally:
        save_manifest(output, manifest)
    return manifest


def overlay(base, candidate, source_sha):
    if command_output(["git", "rev-parse", "HEAD"], base) != source_sha:
        raise ValueError("bootstrap checkout is not the exact base SHA")
    if command_output(["git", "status", "--porcelain"], base):
        raise ValueError("bootstrap requires a clean base checkout")
    files = harness_files(candidate)
    shutil.copyfile(candidate / "Cargo.lock", base / "Cargo.lock")
    tree_command = ["cargo", "tree", "--workspace", "--edges", "normal"]
    original_dependencies = {
        name: command_output(tree_command + ["--offline"] + features, base).replace(str(base), "<checkout>")
        for name, features in [("default", []), ("all-features", ["--all-features"])]
    }
    original_lock = file_hash(base / "Cargo.lock")
    for name in files:
        if name in ["Cargo.lock", "fast3d/Cargo.toml#dev-dependencies"]:
            continue
        if not name.startswith(("fast3d/src/tests/", "fast3d/tests/", "tools/readbacks/")) and name != "tools/tmem/vectors.json":
            raise ValueError(f"production file forbidden in test harness overlay: {name}")
        destination = base / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(candidate / name, destination)
    cargo_toml = base / "fast3d/Cargo.toml"
    cargo_toml.write_text(replace_dev_dependencies(cargo_toml.read_text(encoding="utf-8"), (candidate / "fast3d/Cargo.toml").read_text(encoding="utf-8")))
    shutil.copyfile(candidate / "Cargo.lock", base / "Cargo.lock")
    overlaid_dependencies = {
        name: command_output(tree_command + ["--locked", "--offline"] + features, base).replace(str(base), "<checkout>")
        for name, features in [("default", []), ("all-features", ["--all-features"])]
    }
    if original_dependencies != overlaid_dependencies:
        raise ValueError("bootstrap changed the production dependency graph")
    if harness_files(base) != files:
        raise ValueError("bootstrap harness hash mismatch")
    return {"files": files, "production_dependencies_before": json_hash(original_dependencies),
            "production_dependencies_after": json_hash(overlaid_dependencies),
            "original_manifest_resolved_lock_sha256": original_lock,
            "frozen_capture_lock_sha256": file_hash(base / "Cargo.lock"),
            "production_dependency_graphs": original_dependencies}


def gh_json(endpoint):
    return json.loads(subprocess.check_output(["gh", "api", endpoint], text=True, encoding="utf-8"))


def download_artifact(repository, run_id, artifact_name, destination):
    artifacts = gh_json(f"repos/{repository}/actions/runs/{run_id}/artifacts?per_page=100")
    matches = [artifact for artifact in artifacts["artifacts"]
               if artifact["name"] == artifact_name and not artifact["expired"]]
    if len(matches) != 1:
        raise ValueError(f"missing or ambiguous artifact {artifact_name} in run {run_id}; bootstrap required")
    destination.mkdir(parents=True, exist_ok=False)
    archive = destination / "download.zip"
    with archive.open("wb") as output:
        subprocess.run(["gh", "api", f"repos/{repository}/actions/artifacts/{matches[0]['id']}/zip"], stdout=output, check=True)
    with zipfile.ZipFile(archive) as bundle:
        seen = set()
        for name in bundle.namelist():
            path = Path(name)
            if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name or name in seen:
                raise ValueError(f"unsafe or duplicate archive path: {name}")
            seen.add(name)
        bundle.extractall(destination)
    archive.unlink()


def fetch_base(repository, base_sha, artifact_name, destination):
    runs = gh_json(f"repos/{repository}/actions/workflows/validate.yml/runs?branch=main&event=push&status=success&head_sha={base_sha}&per_page=100")
    for run in runs["workflow_runs"]:
        if run["head_sha"] != base_sha or run["conclusion"] != "success" or run["event"] != "push":
            continue
        download_artifact(repository, str(run["id"]), artifact_name, destination)
        load_capture(destination, INVENTORY, base_sha, str(run["id"]))
        return str(run["id"])
    raise ValueError(f"no complete main artifact for exact base {base_sha}")


def gate(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    candidate = output / "candidate"
    manifest = capture(args.checkout, candidate, args.base_sha, args.checkout)
    if not args.base_checkout:
        load_capture(candidate, INVENTORY, manifest["source_sha"], manifest["run_id"])
        return
    history = []
    base = output / "downloaded-base"
    try:
        load_capture(candidate, INVENTORY, manifest["source_sha"], manifest["run_id"])
        run_id = fetch_base(args.repository, args.base_sha, args.artifact_name, base)
        report = compare(base, candidate, INVENTORY, args.base_sha, manifest["source_sha"], run_id, manifest["run_id"])
        if not report["passed"]:
            (output / "cross-run-comparison.json").write_text(json.dumps(report, indent=2) + "\n")
            raise ValueError("cross-run RGBA delta; recapture both revisions on this runner")
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        history.append(str(error))
        (output / "bootstrap-reason.json").write_text(json.dumps(history, indent=2) + "\n")
        overlay_provenance = overlay(args.base_checkout.resolve(), args.checkout.resolve(), args.base_sha)
        (output / "bootstrap-overlay.json").write_text(json.dumps(overlay_provenance, indent=2) + "\n")
        base = output / "bootstrap-base"
        base_manifest = capture(args.base_checkout, base, args.base_sha, args.checkout)
        try:
            report = compare(base, candidate, INVENTORY, args.base_sha, manifest["source_sha"],
                             base_manifest["run_id"], manifest["run_id"], allow_incomplete=True)
        except (ValueError, KeyError, OSError) as error:
            report = {"passed": False, "complete": False, "error": str(error)}
    (output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    if not report["passed"]:
        failures = [f"{label}: {failure}" for label, entries in report.get("failures", {}).items()
                    for failure in entries]
        raise ValueError("exact RGBA comparison failed; inspect comparison.json"
                         + ("; " + "; ".join(failures) if failures else "")
                         + ("; " + report["error"] if "error" in report else ""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--base-checkout", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"))
    parser.add_argument("--artifact-name", required=True)
    args = parser.parse_args()
    try:
        gate(args)
    except FileExistsError as error:
        print(error, file=sys.stderr)
        return 1
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        args.output.mkdir(parents=True, exist_ok=True)
        (args.output / "failure.json").write_text(json.dumps({"error": str(error)}, indent=2) + "\n")
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
