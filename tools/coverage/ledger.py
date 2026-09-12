"""Freeze golden hashes and derive the accepted eight masks without rendered candidate input."""

import argparse
import hashlib
import json
from pathlib import Path

from predict import mask_file


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ledger(repo, predictions, output):
    output.mkdir(parents=True, exist_ok=True)
    rows = []
    for path in sorted((repo / "fast3d/goldens").glob("*.bin")):
        name = path.stem
        size = 96 if path.stat().st_size == 96 * 96 * 4 else 64
        assert path.stat().st_size == size * size * 4
        mask = []
        for y in range(size):
            for x in range(size):
                changed = False
                if name == "high-poly":
                    changed = x + y == 21
                elif name in ("tron", "tron-fallback", "multi-material"):
                    changed = x == 64
                elif name == "fogworld":
                    changed = y == 44 and (18 <= x <= 51 or 55 <= x <= 84) or x in (55, 85) and 45 <= y <= 77
                elif name == "alpha-threshold":
                    changed = x >= 32
                elif name == "2d-hud-over-3d":
                    changed = 20 <= x < 44 and 20 <= y < 44
                mask.append(changed)
        if name == "pairless-chrome-icosphere":
            source = (predictions / f"{name}.byte-delta.rgba8").read_bytes()
            assert len(source) == size * size * 4
            mask = [source[i] != 0 for i in range(0, len(source), 4)]
            support = (predictions / f"{name}.support-a.rgba8").read_bytes()
            before = path.read_bytes()
            assert all((support[i] != 0) == (before[i:i + 4] != bytes([13, 13, 20, 255])) for i in range(0, len(before), 4))
        stats = mask_file(output / f"{name}.predicted-byte-delta.rgba8", mask, size)
        rows.append({"file": str(path.relative_to(repo)), "width": size, "height": size,
                     "before_sha256": sha(path), "prediction": stats, "candidate": None})
    assert len(rows) == 27
    assert sum(r["prediction"]["pixels"] > 0 for r in rows) == 8
    source_paths = ["fast3d/src/tests/scene_builders.rs", "fast3d/src/tests/fixtures.rs", "fast3d/src/tests/goldens.rs", "fast3d/src/tests/dl_builder.rs"]
    sources = {name: sha(repo / name) for name in source_paths}
    (output / "golden-ledger.json").write_text(json.dumps({"input_builders": sources, "goldens": rows}, indent=2) + "\n")
    return rows


def archived_quad(archive, output):
    names = ["f3dex2-quad-winding-fast3d", "f3dex2-quad-winding-rt64", "quad-as-tri2-rt64"]
    a, b, tri2 = [(archive / f"{name}.rgba8").read_bytes() for name in names]
    assert len(a) == len(b) == len(tri2) == 320 * 240 * 4
    assert b == tri2
    predicted = {(left + (191 - 4 * y) // 3, top + y) for left, top in [(40, 32), (176, 32), (176, 104), (40, 176)] for y in range(48)}
    actual = {(i // 4 % 320, i // 4 // 320) for i in range(0, len(a), 4) if a[i:i + 4] != b[i:i + 4]}
    assert actual == predicted
    assert len(actual) == 192 and len({y for _, y in actual}) == 144
    stats = mask_file(output / "quad-192.rgba8", [(x, y) in predicted for y in range(240) for x in range(320)], 320)
    stats["inputs"] = {name: sha(archive / f"{name}.rgba8") for name in names}
    (output / "quad-192.json").write_text(json.dumps(stats, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repo", type=Path)
    parser.add_argument("predictions", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--quad-archive", type=Path)
    args = parser.parse_args()
    rows = ledger(args.repo.resolve(), args.predictions, args.output)
    if args.quad_archive:
        archived_quad(args.quad_archive, args.output)
    print([(Path(r["file"]).name, r["prediction"]["pixels"]) for r in rows])
