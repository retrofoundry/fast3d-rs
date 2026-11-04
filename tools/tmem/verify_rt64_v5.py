"""Compile the pinned C++ hasher probe and compare its stream to frozen vectors."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(reference, output, compiler):
    root = Path(__file__).resolve().parents[2]
    frozen = json.loads((root / "tools/tmem/rt64-v5-vectors.json").read_text())
    provenance = frozen["provenance"]
    pin = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=reference, text=True).strip()
    if pin != provenance["pin"]:
        raise ValueError("rt64 reference pin differs")
    for name, digest in provenance["reference_sha256"].items():
        if sha(reference / name) != digest:
            raise ValueError(f"rt64 reference file changed: {name}")
    driver = root / "tools/tmem/rt64_hash_probe.cpp"
    if sha(driver) != provenance["driver_sha256"]:
        raise ValueError("hash probe changed without separate provenance review")
    output.mkdir(parents=True, exist_ok=False)
    executable = output / "rt64_hash_probe"
    command = [compiler, "-std=c++17", "-O2", "-I", str(reference / "src"), "-I", str(reference / "src/contrib"),
               str(driver), "-o", str(executable)]
    subprocess.run(command, check=True)
    inputs = []
    for row in frozen["vectors"]:
        bank = bytearray((i * 37 + (i // 256) * 11 + 19) & 255 for i in range(4096))
        for address, byte in row["overrides"]:
            bank[address] = byte
        inputs.append(dict({name: row[name] for name in ["name", "width", "height", "fmt", "siz", "base", "line", "palette", "tlut"]}, bank=bank.hex()))
    input_path = output / "inputs.json"
    input_path.write_text(json.dumps(inputs, indent=2) + "\n")
    run = [str(executable), str(input_path)]
    actual = subprocess.check_output(run)
    (output / "cpp-output.json").write_bytes(actual)
    expected = [{name: row[name] for name in ["name", "xxh3_64", "stream", "spans"]} for row in frozen["vectors"]]
    passed = json.loads(actual) == expected
    report = dict(passed=passed, vectors=len(expected), reference=str(reference), pin=pin,
                  compiler=subprocess.check_output([compiler, "--version"], text=True).strip(),
                  compile_command=command, probe_command=run, provenance=provenance,
                  files={path.name: sha(path) for path in [driver, executable, input_path, output / "cpp-output.json", root / "tools/tmem/rt64-v5-vectors.json"]})
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    if not passed:
        raise ValueError("pinned C++ stream, span or hash mismatch")
    print(f"{len(expected)} pinned C++ streams, spans and hashes agree")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--compiler", default=os.environ.get("CXX", "clang++"))
    args = parser.parse_args()
    verify(args.reference.resolve(), args.output.resolve(), args.compiler)
