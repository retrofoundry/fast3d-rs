"""Check a coverage fixture against B (current renderer and rt64), or archived A, including dither statistics."""

import argparse
import csv
import hashlib
import json
from pathlib import Path


def check(directory, scene, actual, rt64, allow_rt64_f3d_quirk=False, parent=False):
    if parent and rt64:
        raise ValueError("rt64 uses B")
    if allow_rt64_f3d_quirk and (not rt64 or scene != "coverage-slopes-f3d"):
        raise ValueError("the 4337374 F3D quirk only applies to rt64 coverage-slopes-f3d")
    with (directory / "coverage-manifest.tsv").open() as manifest:
        rows = list(csv.DictReader(manifest, delimiter="\t"))
    row = next(r for r in rows if r["scene"] == scene)
    width, height = int(row["width"]), int(row["height"])
    assert len(actual) == width * height * 4
    if allow_rt64_f3d_quirk and (width, height, row["threshold"]) != (320, 240, "0"):
        raise ValueError("the 4337374 F3D quirk requires the exact 320x240 atlas")
    if rt64 and row["threshold"] == "statistical":
        covered, survivors = [], []
        unexpected = []
        for y in range(height):
            for x in range(width):
                i = (y * width + x) * 4
                rgb = actual[i:i + 3]
                inside = x >= 32 and y >= 32 and x + y < 224
                if inside:
                    covered.append((x, y))
                    if rgb == bytes([255, 0, 0]):
                        survivors.append((x, y))
                    elif rgb != bytes(3):
                        unexpected.append((x, y))
                elif rgb != bytes(3):
                    unexpected.append((x, y))
        fraction = len(survivors) / len(covered)
        b_only_survivors = sum(x + y == 223 for x, y in survivors)
        bands = []
        for axis in (0, 1):
            for value in range(max(width, height)):
                total = sum(p[axis] == value for p in covered)
                if total >= 32:
                    bands.append(1 - sum(p[axis] == value for p in survivors) / total)
        passed = not unexpected and b_only_survivors > 0 and abs(fraction - 128 / 255) <= 0.05 and max(bands) <= 0.8
        return {"passed": passed, "survivor_fraction": fraction, "b_only_edge_survivors": b_only_survivors, "maximum_discarded_band_fraction": max(bands), "unexpected_pixels": len(unexpected)}
    rule = "a" if parent else "b"
    expected = (directory / f"{scene}.expected-{rule}.rgba8").read_bytes()
    assert len(expected) == len(actual)
    channels = 3 if rt64 else 4
    scalar = row["threshold"] == "7"
    quirk_mask = bytearray(bytes([0, 0, 0, 255]) * (width * height))
    if allow_rt64_f3d_quirk:
        for y in range(8, 164):
            for x in range(8, 24):
                i = (y * width + x) * 4
                if expected[i:i + 3] != bytes(3):
                    quirk_mask[i:i + 4] = bytes([255] * 4)
        if hashlib.sha256(quirk_mask).hexdigest() != "c46a12b97bec102126536d8ad7a706739cf74bb026804a0ef364db7fa8e1c340":
            raise ValueError("the 4337374 F3D omission mask has changed")
    mismatches = []
    unclassified = []
    maximum = 0
    for i in range(0, len(actual), 4):
        want = expected[i:i + 4]
        tolerance = (7 if rt64 else 1) if scalar and want != bytes([0, 0, 0, 255]) else 0
        difference = max(abs(actual[i + c] - want[c]) for c in range(channels))
        maximum = max(maximum, difference)
        if difference > tolerance:
            mismatches.append([i // 4 % width, i // 4 // width])
        if quirk_mask[i]:
            if actual[i:i + 3] != bytes(3):
                unclassified.append([i // 4 % width, i // 4 // width])
        elif difference > tolerance:
            unclassified.append([i // 4 % width, i // 4 // width])
    result = {"passed": not unclassified, "maximum": maximum, "mismatched_pixels": len(mismatches), "first_mismatches": mismatches[:16]}
    if allow_rt64_f3d_quirk:
        result.update(quirk="rt64-4337374-f3d-left-column", quirk_pixels=256,
                      unclassified_pixels=len(unclassified), first_unclassified=unclassified[:16])
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expectations", type=Path)
    parser.add_argument("scene")
    parser.add_argument("rgba8", type=Path)
    parser.add_argument("--rt64", action="store_true")
    parser.add_argument("--parent", action="store_true", help="check archived fast3d output against A")
    parser.add_argument("--allow-rt64-f3d-quirk", action="store_true",
                        help="require the pinned 4337374 F3D omission and exact RGB everywhere else")
    args = parser.parse_args()
    result = check(args.expectations, args.scene, args.rgba8.read_bytes(), args.rt64,
                   args.allow_rt64_f3d_quirk, args.parent)
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result["passed"] else 1)
