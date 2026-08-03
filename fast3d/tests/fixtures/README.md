# fast3d display-list fixtures

Each version-1 fixture is a directory containing canonical `image.rdram` bytes, a strict
`fixture.toml` manifest, and a deterministic `image.dump` generated from those two files. The
checked Rust registry embeds all three, so missing files fail compilation; fixture lookup validates
lengths, hashes, the artifact identity, and the canonical dump before interpretation.

Verify internal integrity, live source/texture hashes, the generated registry, and literal recipe
reproduction with:

```sh
cargo run -p fast3d-fixtures -- verify
```

The compiler-origin pilot is captured only while the opt-in compiler exists:

```sh
cargo run -p fast3d-fixture-capture --features asm -- capture \
  --registry fast3d/tests/fixtures/capture-plan.toml \
  --out /tmp/fast3d-captures
cargo run -p fast3d-fixtures -- import \
  --from /tmp/fast3d-captures \
  --root fast3d/tests/fixtures
cargo run -p fast3d-fixture-capture --features asm -- verify
```

The last command recompiles every pilot tuple twice and byte-compares the checked image with the
current compiler. The same three comparisons run inside the existing morphcube test whenever the
`asm` feature is enabled, so `cargo test -p fast3d --features asm` also fails on compiler drift. The
ordinary verifier proves checked-file integrity and live source/input-hash binding while a source is
declared with `kind = "file"`. When that source moves to another repository, change only its source
kind to `external`; the repository, revision, path, and SHA-256 remain recorded, but verification no
longer requires a local copy. Source kind and path are provenance and are deliberately excluded from
the artifact identity, which remains bound to the recorded source content hash.

Compiler-origin fixtures are compatibility captures, not independent protocol evidence. Once the
compiler leaves this repository, source-to-compiler-to-renderer coverage belongs in n64-toys; the
external source record and fast3d's artifact-integrity checks continue to work here.

New fast3d-local fixtures are built from explicit addresses, packed records, and literal command
words. First add the sorted `[[fixture]]` entry to `index.toml`, then regenerate the worked example
with:

```sh
cargo run -p fast3d-fixtures -- build literal/colored-triangle/v1
```

`build` refuses unregistered IDs so it cannot leave a successful-looking orphan directory.

The fixture files intentionally ship in the fast3d crate archive: the generated test registry uses
`include_str!`/`include_bytes!`, and Step 9's extracted-archive tests must exercise those same bytes.
Review package size alongside the repository fixture budget as the corpus grows.
