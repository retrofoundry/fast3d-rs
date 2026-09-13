# TMEM validation fixtures

Production rendering decodes owned TMEM requests on the CPU at upload. These tools
freeze the byte contract and test the request memo and validation boundaries.
Fast3dV1 is specified in [fast3d-v1.md](fast3d-v1.md); the internal T2 boundary
and counter definitions are in [owned-requests.md](owned-requests.md).

## Frozen baselines

`goldens-bb0fea3.json` contains all 27 committed golden filenames, dimensions and
SHA-256 values. Each hash was checked against `bb0fea3` and the final C2 default,
all-features and debug-ui readbacks. The validated C2 commit `db04908` has the same
tree as `bb0fea3`. No golden was regenerated.

`oracle-bb0fea3.json` records 60 existing Metal/rt64 rows, including all 38 coverage
captures. It retains the per-row channel policy, channel maxima, exact residual
and mask hashes, input hashes and both output hashes. The compressed files in
`residuals/` contain signed fast3d-minus-rt64 channel differences, as little-endian
i16 values in pixel order. RGB rows omit alpha only from the reference residual;
fast3d parent/candidate comparison always checks RGBA and authenticates the full
parent against the frozen hash. Transparent Mario retains its statistical policy.
The three rejected real framebuffer-load rows and the compatibility shortcut have
separate F0 policies; they are not generic IMAGE agreement rows.

The QUAD winding row is exact RGBA. Its former 192-pixel coverage residual is
closed. The TRI2 replacement control is byte-identical to the QUAD reference.
A baseline discrepancy is a failing check, including a missing row, a new
residual position or changed residual value.

```sh
python3 -m unittest discover -s tools/tmem -p 'test_*.py'
python3 tools/tmem/compare_oracle.py SCENE PARENT.rgba8 CANDIDATE.rgba8 RT64.rgba8
```

These are the frozen Metal baselines. Backend-specific parent comparisons,
including WARP, use the [exact readback gate](../readbacks/README.md).

## Discriminating rows

Expected channel values and addresses in `tmem_witnesses.rs` are literals or
factorized component tables. Tests call the existing decoder only for the actual
result. Read visitors cross-check the explicitly listed addresses. No expectation
calls a decoder or derives a footprint by decoding pixels.

| Rows | Incorrect behavior caught |
| --- | --- |
| `format-rgba16` | c5/31 rounding, shifted/swapped channels, endian reversal and lost alpha; all 32 channel values and both alpha bits |
| `format-i4`, `format-i8`, `format-ia4`, `format-ia8`, `format-ia16` | wrong nibble order, intensity/alpha expansion or coupled IA16 channels; complete small domains and factorized 256-value IA16 axes |
| `format-rgba32` | missing or swapped RG/BA banks and lost alpha |
| `format-ci{4,8}-mode{0,1,2,3}`; bank sweep | wrong TLUT interpretation, wrong bank selector, lost endpoint indices; CI8's palette selector must be ignored |
| `layout-odd-5x3`, `alias-odd-swap`, `alias-final-nibble` | tight stride, missing odd swap or final odd-width byte; padding and unrelated-bank controls remain unchanged |
| `layout-rgba32-wrap`, `alias-high-bank` | wrong 2 KiB mask, omitted BA bytes or bank wrap |
| `layout-one-zero-line`, `layout-zero-line-nonci-tlut` | 1×1 bounds; zero-line odd-row addressing and erroneous TLUT masking on a non-CI format |
| `layout-lookup-wrap`, `alias-wrapped-lookup` | nominal-extent lookup, missing wrap or any odd/nibble plane; all 16,384 texels are compared |
| `layout-loadtile-padded`, `layout-block-dxt0`, `layout-block-dxt683` | conflated LoadTile/LoadBlock rows, padded source stride or DXT parity drift |
| equal-bank linear provenance | bank/descriptor equality used as a substitute for resolved linear input; equal banks produce two explicitly different valid streams |
| poison/reload and real GPU-source load sequence | validation after reuse, lost diagnostic PC, missing draw rejection or failed recovery after a valid guest reload; masked taps outside the nominal output remain part of rejection reach |
| TLUT ordered snapshots | used/unused/duplicate palette colors, destination wrap and RGBA32-over-TLUT writes cannot change earlier draws |
| role snapshots | changing tex1, detail or one non-halving LOD cannot alter other role bytes; level zero aliases its image; owned pixels survive source destruction |
| preimage/collision tests | host layout, missing fields and hash-only equality, including B-first lookup through a forced constant digest |

Existing `unsupported_formats`, `load_provenance`, `framebuffer_aliases`, role,
LOD, capture, retained-scene and prefix tests remain part of validation. They cover
all 23 unsupported format pairs, unsupported narrow RGBA32 compatibility decode,
missing/mixed/overwritten provenance and cross-task source lifetime.

`dispatch-expectations.json` contains explicit CPU-side count expectations tied to
the role fixture. Its tests check request inventories and alias arithmetic.
Those counts are not measurements of the current renderer: cache execution,
replacement execution and in-flight eviction witnesses are not implemented here.

## IMAGE writers and replay

`tests::tmem_fixtures::write_tmem_fixtures` uses the existing IMAGE capture writer.
The three captures set an RGBA32 target, initialize it, retain explicit VI registers
and execute FullSync. They use authored F3D commands and ordinary guest memory:

- `tmem-layouts`: all nine formats, odd rows, separate opaque RGB and alpha-to-RGB
  panels. Alpha errors appear as RGB differences in the reference comparison.
- `tmem-tlut-mutation`: five ordered rectangles retain red/green/green/green/blue
  snapshots while used, unused, duplicate and wrapped palette writes occur.
- `tmem-roles-lifetime`: overlapping request roles with independent 8×4, 3×5, 5×2
  and 2×2 levels. Three draws select adjacent pairs; four distinct images supply
  all six level references and three level-zero aliases.

The shared `tmem_semantics.rs` derives the complete expected RGBA frame, including
background. Native and browser replay use the same independent expectations.
The writer and CPU draw-snapshot tests do not need an adapter. Replay requires one.

```sh
FAST3D_WRITE_FIXTURES=/absolute/fresh/fixtures \
  cargo test -p fast3d --features capture --lib \
  tests::tmem_fixtures::write_tmem_fixtures -- --ignored --exact
cargo test -p fast3d --features capture --lib tests::tmem_
cargo test -p fast3d --features capture --test tmem_fixture_replay
cargo test -p fast3d --features capture,profiling --target wasm32-unknown-unknown \
  --test tmem_fixture_replay --test tmem_preimage
```

Set the wasm-bindgen browser runner and ChromeDriver as in `validate.yml` for the
last command. Wasm compilation alone does not establish browser replay. Regenerated
captures must match the committed files before any parent/candidate/oracle run.
Readback artifacts retain the CPU decoder's independent RGBA buffers in every
feature configuration and the three final IMAGE rows under all-features.
