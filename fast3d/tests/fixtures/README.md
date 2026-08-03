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

Compiler-origin compatibility fixtures can be captured while the optional compiler remains:

```sh
cargo run -p fast3d-fixture-capture --features asm -- capture \
  --registry fast3d/tests/fixtures/capture-plan.toml \
  --out /tmp/fast3d-captures
cargo run -p fast3d-fixtures -- import \
  --from /tmp/fast3d-captures \
  --root fast3d/tests/fixtures
cargo run -p fast3d-fixture-capture --features asm -- verify
```

`fast3d-fixture-capture verify` compiles every tuple twice to check determinism, then byte-compares
it with the checked image. The morphcube test runs the same three parity checks under `asm`, so
`cargo test -p fast3d --features asm` also detects compiler drift. The repository verifier checks
file integrity and live source/input hashes while a source has `kind = "file"`. When the source
moves to another repository, change its kind to `external`; the repository, revision, path, and
SHA-256 remain recorded, but verification no longer requires a local copy. Source kind and path are
provenance rather than artifact identity; identity remains bound to the recorded source content
hash.

Compiler-origin fixtures record compatibility, not independent protocol evidence. Once the
compiler leaves this repository, source-to-compiler-to-renderer coverage belongs in n64-toys; the
external source record and fast3d's artifact-integrity checks continue to work here.

Fast3d-local fixtures use explicit addresses, packed records, and literal command words. Add the
sorted `[[fixture]]` entry to `index.toml` before building:

```sh
cargo run -p fast3d-fixtures -- build literal/colored-triangle/v1
```

`build` refuses unregistered IDs so it cannot leave a successful-looking orphan directory.

Publication policy (owner decision, 2026-08-03): the entire fast3d test tree is repository-only and
is deliberately excluded from the published crate archive, together with the test-only
`goldens/**` framebuffer outputs. This covers the in-crate `src/tests/**` modules and all of
`tests/**`, including integration tests, scene sources, fixture manifests, dumps, and RDRAM images.
Shipping the complete and growing corpus would add dead download weight for consumers. Local
testing is unchanged because Cargo's `exclude` setting affects only package archives.

The crate's build script enables the external `src/tests/**` module only when that repository tree
is present. Consequently, `cargo test --no-run` on an extracted archive compiles the inline unit
tests retained in production source files, but the archive cannot re-run the external modules or
integration tests that were excluded. The package gate compiles that reduced archive test target;
running GPU-backed tests remains in the macOS/Windows test job. Run the fixture verifier and full
test suite in the repository before packaging.
