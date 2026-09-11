#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--assembler', type=Path, required=True)
parser.add_argument('--out', type=Path, required=True)
parser.add_argument('--library', type=Path)
parser.add_argument('--baseline', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
out = args.out.resolve()
library = args.library.resolve() if args.library else root / 'fast3d'
lib_file = 'baseline.rs' if args.baseline else 'driver.rs'
main_file = 'baseline_main.rs' if args.baseline else 'main.rs'
features = '["capture"]' if args.baseline else '["capture", "profiling"]'
hash_dependency = '' if args.baseline else 'twox-hash = { version = "=2.1.2", default-features = false, features = ["std", "xxhash3_64"] }'
out.mkdir(parents=True, exist_ok=True)
quote = lambda p: json.dumps(str(p))
(out / 'Cargo.toml').write_text(f'''[package]
name = "b1-perf"
version = "0.1.0"
edition = "2021"
[workspace]
[lib]
path = {quote(root / 'tools/perf' / lib_file)}
crate-type = ["rlib", "cdylib"]
[[bin]]
name = "b1-perf"
path = {quote(root / 'tools/perf' / main_file)}
[dependencies]
fast3d = {{ path = {quote(library)}, features = {features} }}
n64-toys-asm = {{ path = {quote(args.assembler.resolve() / 'crates/asm')} }}
wgpu = "29.0"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
sha2 = "0.10"
futures-channel = "0.3"
{hash_dependency}
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
pollster = "0.3"
[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "=0.2.128"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
web-sys = {{ version = "0.3", features = ["Window", "Performance"] }}
[profile.release]
debug = 1
''')
print(out / 'Cargo.toml')
