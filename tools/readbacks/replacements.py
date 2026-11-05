import json
from pathlib import Path

from readbacks import artifact_file, authenticate_rows, file_hash, json_hash

SOURCE = "fast3d/tests/texture_residency/replacements.rs"
INVENTORY = "tools/tmem/replacement-inventory.json"


def inventory():
    return json.loads((Path(__file__).resolve().parents[2] / INVENTORY).read_text(encoding="utf-8"))


def requirement(checkout):
    source = checkout / SOURCE
    return {"version": 1, "source_test_sha256": file_hash(source) if source.is_file() else None,
            "required_configurations": ["all-features"] if source.is_file() else []}


def save(output, manifest, frozen=None):
    output = output.resolve()
    declaration = manifest.get("replacements")
    if declaration is None:
        return
    frozen = inventory() if frozen is None else frozen
    rows, errors = [], []
    for config in manifest["configurations"]:
        directory = output / config / "replacements"
        for metadata in sorted(directory.glob("*.json")):
            try:
                row = json.loads(metadata.read_text(encoding="utf-8"))
                stem = row["id"]
                for field, suffix, digest in [("file", ".bin", "sha256"),
                                              ("input_file", ".input.bin", "input_sha256"),
                                              ("expected_file", ".expected.bin", "expected_sha256"),
                                              ("before_file", ".before.bin", "before_sha256"),
                                              ("mask_file", ".mask.bin", "mask_sha256")]:
                    path = artifact_file(directory, stem + suffix)
                    row[field] = path.relative_to(output).as_posix()
                    row[digest] = file_hash(path)
                row.update(configuration=config, metadata_file=metadata.relative_to(output).as_posix(),
                           metadata_sha256=file_hash(metadata))
                rows.append(row)
            except (ValueError, KeyError, OSError) as error:
                errors.append(f"{metadata}: {error}")
    declaration["inventory_sha256"] = json_hash(frozen)
    supplement = {field: declaration[field] for field in
                  ["version", "source_test_sha256", "required_configurations", "inventory_sha256"]}
    supplement.update({field: manifest[field] for field in
                       ["source_sha", "tested_sha", "base_sha", "run_id", "run_attempt"]})
    supplement.update(rows=rows, errors=errors)
    path = output / "replacement-manifest.json"
    path.write_text(json.dumps(supplement, indent=2) + "\n", encoding="utf-8")
    declaration.update(manifest_file=path.name, manifest_sha256=file_hash(path))


def load(root, manifest, *, allow_incomplete=False, require=None, frozen=None):
    declaration = manifest.get("replacements")
    empty = {"required_configurations": [], "rows": 0, "missing_rows": [], "errors": []}
    if declaration is None:
        if require or INVENTORY in manifest.get("harness_files", {}):
            raise ValueError("missing replacement declaration")
        return empty
    source = declaration.get("source_test_sha256")
    required = ["all-features"] if source is not None else []
    if declaration.get("version") != 1 or declaration.get("required_configurations") != required:
        raise ValueError("replacement requirement differs from tested source")
    if require is not None and require != (source is not None):
        raise ValueError("replacement requirement differs from requested checkout")
    if source is not None and file_hash(artifact_file(root, "replacement-source.rs")) != source:
        raise ValueError("replacement source hash mismatch")
    frozen = inventory() if frozen is None else frozen
    if declaration["inventory_sha256"] != json_hash(frozen):
        raise ValueError("replacement inventory hash mismatch")
    path = artifact_file(root, declaration["manifest_file"])
    if file_hash(path) != declaration["manifest_sha256"]:
        raise ValueError("replacement manifest hash mismatch")
    supplement = json.loads(path.read_text(encoding="utf-8"))
    for field in ["version", "source_test_sha256", "required_configurations", "inventory_sha256"]:
        if supplement.get(field) != declaration[field]:
            raise ValueError(f"replacement manifest {field} mismatch")
    for field in ["source_sha", "tested_sha", "base_sha", "run_id", "run_attempt"]:
        if supplement.get(field) != manifest[field]:
            raise ValueError(f"replacement manifest {field} mismatch")
    expected = {(config, row["id"]): dict(row, stage="final", blend_path="none")
                for config in required for row in frozen["rows"]}
    if len(expected) != len(required) * len(frozen["rows"]):
        raise ValueError("duplicate replacement inventory row")
    rows = authenticate_rows(root, supplement["rows"], expected, manifest["runtime"]["os"] == "Windows",
                             roles=("replacement",), metadata=True)
    regions = []
    for key, row in rows.items():
        blobs = {}
        for field in ["expected", "before", "mask"]:
            path = artifact_file(root, row[field + "_file"])
            if file_hash(path) != row[field + "_sha256"] or row[field + "_sha256"] != expected[key][field + "_sha256"]:
                raise ValueError(f"{key}: frozen replacement {field} mismatch")
            blobs[field] = path.read_bytes()
        pixels = artifact_file(root, row["file"]).read_bytes()
        if pixels != blobs["expected"]:
            raise ValueError(f"{key}: replacement RGBA differs from literal")
        before, mask = blobs["before"], blobs["mask"]
        if len(before) != len(pixels) or len(mask) != len(pixels) // 4:
            raise ValueError(f"{key}: replacement region length mismatch")
        changed = [i for i in range(len(mask)) if before[i*4:i*4+4] != pixels[i*4:i*4+4]]
        changed_set = set(changed)
        if bytes(int(i in changed_set) for i in range(len(mask))) != mask:
            raise ValueError(f"{key}: replacement change mask differs")
        regions.append({"configuration": key[0], "id": key[1], "changed_pixels": len(changed),
                        "bounds": [min(i % row["width"] for i in changed), min(i // row["width"] for i in changed),
                                   max(i % row["width"] for i in changed), max(i // row["width"] for i in changed)] if changed else None})
    missing = sorted(set(expected) - set(rows))
    errors = supplement["errors"]
    if (missing or errors) and not allow_incomplete:
        raise ValueError(f"missing replacement rows: {missing}; errors: {errors}")
    return {"required_configurations": required, "rows": len(rows), "missing_rows": missing, "errors": errors, "regions": regions}
