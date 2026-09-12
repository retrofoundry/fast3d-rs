"""Freeze golden hashes and derive the accepted eight masks without rendered candidate input."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil

from compare_three_way import differences
from predict import mask_file


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


VALUE_RULES = {
    "high-poly": "Gain x+y=21: constant red marker [255,0,0,255]; flat blue mesh unchanged.",
    "tron": "Gain cyan under magenta at x=64: blend each layer with alpha 128/255, rounding each RGBA8 store; dual-source stores alpha 255.",
    "tron-fallback": "Same cyan/magenta overlap as tron; fallback stores source alpha 128.",
    "multi-material": "Column x=64 belongs to the middle quad: PRIM blue, SHADE alpha 1, hence [0,0,255,255].",
    "fogworld": "Far shade [200,50,50] mixes toward [128,128,128] with fog alpha (110/128)*256/255; near fog alpha is zero. Lost coverage becomes [13,13,20,255].",
    "alpha-threshold": "At x=32 alpha x/64 fails 128/255; x>=33 retains RGB (UV identity) and stores round(255*x/64).",
    "2d-hud-over-3d": "On [20,44)^2, u=(x-20)/24, v=(y-20)/24: RGB=(1-u,abs(u+v-1),min(u,1-v)), alpha=1; rectangle HUD unchanged.",
    "pairless-chrome-icosphere": "Projected B support selects constant RGBA16 texture [206,99,49,255], otherwise clear [13,13,20,255]; owner changes have identical values.",
}


def predicted_values(name, before, mask, width, predictions):
    result = bytearray(before)
    clear = [13, 13, 20, 255]
    support = (predictions / f"{name}.support-b.rgba8").read_bytes() if "icosphere" in name else None
    for i, changed in enumerate(mask):
        if not changed:
            continue
        x, y = i % width, i // width
        pixel = list(before[i * 4:i * 4 + 4])
        if name == "high-poly":
            pixel = [255, 0, 0, 255]
        elif name in ("tron", "tron-fallback"):
            rgb = clear[:3]
            for layer in ([0, 220, 255], [255, 0, 220]):
                rgb = [round((s * 128 + d * 127) / 255) for s, d in zip(layer, rgb)]
            pixel = [*rgb, 128 if name == "tron-fallback" else 255]
        elif name == "multi-material":
            pixel = [0, 0, 255, 255]
        elif name == "fogworld":
            if y == 44 and x <= 51:
                pixel = [round(c + (128 - c) * 220 / 255) for c in (200, 50, 50)] + [220]
            elif x == 85:
                pixel = [200, 50, 50, 0]
            else:
                pixel = clear
        elif name == "alpha-threshold":
            pixel = clear if x == 32 else [*pixel[:3], round(255 * x / 64)]
        elif name == "2d-hud-over-3d":
            u, v = (x - 20) / 24, (y - 20) / 24
            pixel = [round(c * 255) for c in (1 - u, abs(u + v - 1), min(u, 1 - v))] + [255]
        elif name == "pairless-chrome-icosphere":
            pixel = [206, 99, 49, 255] if support[i * 4] else clear
        else:
            raise ValueError(f"missing value derivation for {name}")
        result[i * 4:i * 4 + 4] = bytes(pixel)
    return result


def ledger(repo, predictions, output):
    output.mkdir(parents=True, exist_ok=True)
    (output / "before").mkdir(exist_ok=True)
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
        stats["file"] = f"{name}.predicted-byte-delta.rgba8"
        values = output / f"{name}.predicted-values.rgba8"
        values.write_bytes(predicted_values(name, path.read_bytes(), mask, size, predictions))
        shutil.copyfile(path, output / "before" / path.name)
        semantic = "TriangleAttributeSampleOrigin" if name in ("alpha-threshold", "2d-hud-over-3d") else "TriangleRasterSampleOrigin"
        rows.append({"file": str(path.relative_to(repo)), "width": size, "height": size,
                     "before_sha256": sha(path), "before_file": f"before/{path.name}",
                     "semantic_fix": semantic if any(mask) else "no change",
                     "value_rule": VALUE_RULES.get(name, "Stable coverage and constant attributes, or rectangle-only; byte identity required."),
                     "prediction": stats, "predicted_values": {"file": values.name, "sha256": sha(values), "channel_tolerance": 2},
                     "candidate": None})
        if name == "alpha-threshold":
            admission = output / "alpha-threshold.predicted-admission.rgba8"
            rows[-1]["admission_prediction"] = {"file": admission.name, **mask_file(admission, [i % size == 32 for i in range(size * size)], size)}
    assert len(rows) == 27
    assert sum(r["prediction"]["pixels"] > 0 for r in rows) == 8
    source_paths = ["fast3d/src/tests/scene_builders.rs", "fast3d/src/tests/fixtures.rs", "fast3d/src/tests/goldens.rs", "fast3d/src/tests/dl_builder.rs",
                    "fast3d/src/tests/scene_builders/framebuffers.rs", "fast3d/src/tests/scene_builders/lighting.rs"]
    sources = {name: sha(repo / name) for name in source_paths}
    (output / "golden-ledger.json").write_text(json.dumps({"input_builders": sources, "goldens": rows}, indent=2) + "\n")
    return rows


def compare_ledger(frozen, rendered, output):
    report = json.loads(frozen.read_text())
    root = frozen.parent
    output.mkdir(parents=True, exist_ok=True)
    failures = []
    for row in report["goldens"]:
        name = Path(row["file"]).stem
        paths = [root / row["before_file"], root / row["prediction"]["file"], root / row["predicted_values"]["file"]]
        for path, digest in zip(paths, [row["before_sha256"], row["prediction"]["sha256"], row["predicted_values"]["sha256"]]):
            assert sha(path) == digest, f"frozen artifact changed: {path}"
        before, mask, values = [p.read_bytes() for p in paths]
        candidate_path = rendered / Path(row["file"]).name
        after = candidate_path.read_bytes()
        assert len(before) == len(after) == len(mask) == len(values) == row["width"] * row["height"] * 4
        maximum, changed = differences(before, after, 4)
        predicted = [mask[i] != 0 for i in range(0, len(mask), 4)]
        missing = [want and not got for want, got in zip(predicted, changed)]
        outside = [got and not want for want, got in zip(predicted, changed)]
        wrong_values = differences(values, after, 4, row["predicted_values"]["channel_tolerance"])[1]
        masks = {"byte-delta": changed, "over-two": differences(before, after, 4, 2)[1],
                 "missing": missing, "outside": outside, "wrong-values": wrong_values}
        passed = not any(missing) and not any(outside) and not any(wrong_values)
        measurement = {"file": str(candidate_path.resolve()), "sha256": sha(candidate_path),
                       "maximum_channel_delta": maximum, "passed": passed}
        for kind, bits in masks.items():
            path = output / f"{name}.{kind}.rgba8"
            measurement[kind] = {"file": path.name, **mask_file(path, bits, row["width"])}
        if "admission_prediction" in row:
            admission = root / row["admission_prediction"]["file"]
            assert sha(admission) == row["admission_prediction"]["sha256"]
            expected = [v != 0 for v in admission.read_bytes()[::4]]
            actual = [before[i:i + 4] != bytes([13, 13, 20, 255]) and after[i:i + 4] == bytes([13, 13, 20, 255]) for i in range(0, len(after), 4)]
            measurement["admission"] = mask_file(output / f"{name}.admission.rgba8", actual, row["width"])
            passed &= actual == expected
            measurement["passed"] = passed
        row["candidate"] = measurement
        if not passed:
            failures.append(name)
    report["passed"] = not failures
    report["held"] = failures
    (output / "golden-ledger-after.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


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
    import sys

    if sys.argv[1:2] == ["compare"]:
        parser = argparse.ArgumentParser(description="Compare readbacks with frozen C2 masks and values; never update goldens.")
        for name in ("ledger", "rendered", "output"):
            parser.add_argument(name, type=Path)
        args = parser.parse_args(sys.argv[2:])
        report = compare_ledger(args.ledger, args.rendered, args.output)
        print(json.dumps({"goldens": len(report["goldens"]), "held": report["held"]}))
        raise SystemExit(0 if report["passed"] else 1)
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
