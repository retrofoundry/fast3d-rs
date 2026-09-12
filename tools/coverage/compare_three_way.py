"""Gate each oracle scene against its measured parent and predicted change classes."""

import argparse
import json
from pathlib import Path

from predict import mask_file


def differences(a, b, channels, threshold=0):
    values = [max(abs(a[i + c] - b[i + c]) for c in range(channels)) for i in range(0, len(a), 4)]
    return max(values, default=0), [v > threshold for v in values]


def compare(parent, candidate, oracle, channels, gate, masks):
    assert len(parent) == len(candidate) == len(oracle)
    assert len(parent) % 4 == 0
    comparisons = {name: differences(a, b, 4 if name == "parent-candidate" else channels) for name, a, b in [
        ("parent-candidate", parent, candidate),
        ("parent-rt64", parent, oracle),
        ("candidate-rt64", candidate, oracle),
    ]}
    explained = [False] * (len(parent) // 4)
    for mask in masks:
        assert len(mask) == len(parent)
        explained = [old or mask[i * 4] != 0 for i, old in enumerate(explained)]
    unclassified = [changed and not allowed for changed, allowed in zip(comparisons["parent-candidate"][1], explained)]
    limit = min(gate, comparisons["parent-rt64"][0])
    passed = comparisons["candidate-rt64"][0] <= limit and not any(unclassified)
    return comparisons, unclassified, limit, passed


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("parent", "candidate", "rt64", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--width", type=int, required=True)
    parser.add_argument("--height", type=int, required=True)
    parser.add_argument("--maximum", type=int, required=True)
    parser.add_argument("--rgba", action="store_true")
    for name in ("coverage", "ownership", "attribute"):
        parser.add_argument(f"--{name}-mask", type=Path)
    parser.add_argument("--exact-change-mask", type=Path)
    args = parser.parse_args()
    parent, candidate, oracle = [p.read_bytes() for p in (args.parent, args.candidate, args.rt64)]
    assert len(parent) == args.width * args.height * 4
    masks = [p.read_bytes() for p in (args.coverage_mask, args.ownership_mask, args.attribute_mask) if p]
    results, unknown, limit, passed = compare(parent, candidate, oracle, 4 if args.rgba else 3, args.maximum, masks)
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"limit": limit}
    for name, (maximum, mask) in results.items():
        channels = 4 if name == "parent-candidate" or args.rgba else 3
        a, b = {"parent-candidate": (parent, candidate), "parent-rt64": (parent, oracle), "candidate-rt64": (candidate, oracle)}[name]
        over = differences(a, b, channels, limit)[1]
        report[name] = {"channels": channels, "maximum": maximum, **mask_file(args.output / f"{name}.rgba8", mask, args.width),
                        "over-limit": mask_file(args.output / f"{name}.over-limit.rgba8", over, args.width)}
    report["unclassified"] = mask_file(args.output / "unclassified.rgba8", unknown, args.width)
    if args.exact_change_mask:
        expected = args.exact_change_mask.read_bytes()
        assert len(expected) == len(parent)
        wrong = [bit != (expected[i * 4] != 0) for i, bit in enumerate(results["parent-candidate"][1])]
        report["prediction-disagreement"] = mask_file(args.output / "prediction-disagreement.rgba8", wrong, args.width)
        passed &= not any(wrong)
    report["passed"] = passed
    (args.output / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    raise SystemExit(0 if passed else 1)
