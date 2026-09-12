# Triangle coverage witnesses

The current renderer tests triangle coverage at `(x + 0.5, y + 0.5)` (A), while
its UV correction reconstructs the owning triangle's UV at `(x, y)` using
fragment derivatives. The arithmetic A expectation evaluates that UV exactly;
the reconstruction can differ across backends. These tests also
calculate B, whose coverage and attributes use `(x, y)`. Both expectations stay
in the arithmetic module. The renderer conformance test currently requires A
on 37 fixtures. The separate retained-height replay is ignored because scanout
presents the retained framebuffer height rather than the VI height.
There is no renderer setting for selecting B.

`fast3d/tests/common/coverage_semantics.rs` uses literal quarter-pixel vertices,
signed integer edge equations and full-frame expectations. It imports no
production raster, viewport or sampling helpers. A separate fixed-point span
walker encodes RDP edge words and calculates all eight coverage bits on the
exact subset. The tests compare that walker with the edge equations, including
all integer ties, both windings, reflections and fractional XY coordinates.

The span arithmetic is frozen to angrylion-rdp-plus
`9c8b9ed3e7d7f00dff8bc872ccdd3fba1a3673fc`:
[rasterizer](https://github.com/ata4/angrylion-rdp-plus/blob/9c8b9ed3e7d7f00dff8bc872ccdd3fba1a3673fc/src/core/n64video/rdp/rasterizer.c#L2175),
[coverage](https://github.com/ata4/angrylion-rdp-plus/blob/9c8b9ed3e7d7f00dff8bc872ccdd3fba1a3673fc/src/core/n64video/rdp/coverage.c#L15),
[blender](https://github.com/ata4/angrylion-rdp-plus/blob/9c8b9ed3e7d7f00dff8bc872ccdd3fba1a3673fc/src/core/n64video/rdp/blender.c#L272).

At a quarter-row, x advances by `(dxdy >> 2) & ~1`; the minor edge resets at
`ym`. The y interval is `[yh, yl)`. The endpoint's eighth-pixel coordinate keeps
a sticky low bit from the discarded fraction. With `q=((endpoint&7)+1)>>1`,
left coverage uses `0xf>>q`, right uses `0xf0>>q`. At integer equality `q=0`, so
the left includes the sample and the right excludes it. Top y is valid; bottom
y is invalid. At a vertex all boundaries apply together. This derives the
same top-left rule as the edge-equation path for these exact setups.

The 256×256 ties fixtures use binary-exact slopes and quarter-steps. Their RDP
mask's high bit is the non-AA sample at `(0,0)`; its population count is AA
coverage. The ideal GPU row table also includes a nonbinary slope, 16×12. That
case is not a claim of equality with quantized RDP setup. The walker does not
model arbitrary overflow, scissor clipping, interpolation, Z correction or VI.
AA render modes still have binary coverage in these native-resolution HLE tests.

The IMAGE builders initialize color/depth memory, set viewport/scissor and end
with FullSync. They use F3D and F3DEX2, including direct XY modifications. An
encoder test checks literal matrix, viewport and command words and independently
recovers screen coordinates from the stored vertex bytes. Capture bytes are
checked against regeneration. The shared native/browser replay requires a real
adapter and compares each selected full frame with A. Primary-color pixels are exact
except for the WARP UV boundary policy below;
interpolated scalar output permits one UNORM rounding step at covered pixels.

The DX12 CPU adapter named `Microsoft Basic Render Driver` has a separate
band-boundary comparison for `coverage-uv-perspective` and
`coverage-uv-perspective-negative`. It accepts only exact RGBA palette colors
reachable by perturbing each arithmetic corner-UV component by at most
`1/128` texel before the existing 1/128-texel rounding and four-texel band
selection. This permits another band at 97 and 124 A-covered pixels respectively.
Background, coverage, scissor and alpha remain exact, and the test reports the
number of accepted differences per fixture. Every other adapter and fixture,
including `coverage-shared-uv` and `coverage-lod-perspective`, keeps its existing
comparison. The A/B images and saved-output checker remain unchanged.

At `(174,48)`, the positive-gradient fixture's exact V is `1231/308`, only
`13/19712` texel above the band transition at `4 - 1/256`. WARP selects blue
there while arithmetic A, Metal and Chrome select green. The UV allowance is
a comparison cap of one coordinate-quantization step, not a proven backend
error bound. Fragment
[`position.w`](https://www.w3.org/TR/WGSL/#position-builtin-value) interpolates
reciprocal clip W, so `(uv * position.w, position.w)` is affine in real
arithmetic. Subtracting its half-pixel derivatives is exact before division;
there is no rational-function truncation term. Backend interpolation and
floating-point evaluation remain relevant, and WGSL supplies
[no finite derivative accuracy bound](https://www.w3.org/TR/WGSL/#floating-point-accuracy).

Run CPU checks without an adapter:

```sh
cargo test -p fast3d --test coverage_arithmetic
cargo test -p fast3d --features capture --lib coverage_encoded_coordinates_and_commands
cargo test -p fast3d --features capture --lib coverage_fixture_bytes_match_builders
cargo test -p fast3d --features capture --test browser_sm64_fixture_replay coverage_warp_
python3 -m unittest discover -s tools/coverage -p 'test_*.py'
```

Generate inputs and both expectations into a scratch directory:

```sh
FAST3D_WRITE_FIXTURES=/tmp/coverage-fixtures cargo test -p fast3d --features capture \
  --lib write_rt64_coverage_fixtures -- --ignored
```

The writer produces 38 captures, A/B RGBA8 images, exact byte-XOR masks and a
manifest. Ties fixtures additionally produce every primitive's eight-bit RDP
sample mask, edge words and a table of all integer ties. No rendered output or
existing binary golden is written by these commands.

With a working GPU, run `coverage_parent_matches_a` in
`browser_sm64_fixture_replay`; on wasm it requires BrowserWebGpu. It is also
registered as a native test. The captures disable dual-source blending.
`coverage_retained_height_matches_a -- --ignored --exact --nocapture` runs the
held 256-row attachment / 128-row VI regression separately with the same exact
A expectation; the wasm runner needs `--include-ignored` instead of `--ignored`. Its capture, arithmetic and regeneration checks remain active.
The odd-width RGBA32 fixture uses a 193×132 VI. The near-plane fixture's clipped
edge is `x+y=144.25`, which misses both A and B sample lattices.
The same captures export through `export_capture_rdram` for the pinned rt64
oracle. `tools/coverage/check_fixture.py` checks saved parent output against A
or, with `--rt64`, saved oracle output against B. It keeps exact background and
primary-color gates, explicit RGB quantization bounds for scalar interactions,
and a statistical oracle check for the dither fixture. Native dither remains
an exact destination-index expectation.

The strict rt64 check reports the F3D slope atlas's missing first column. With
`--rt64 --allow-rt64-f3d-quirk`, the checker requires the pinned `4337374`
omission: exactly 256 B-covered pixels must be black, and all remaining RGB
pixels must match B. The mask's SHA-256 is fixed; the output still reports the
raw 256-pixel discrepancy. Restored pixels also fail this quirk check. The
manifest names this exception and the held parent scanout cell. Neither option
changes A/B expectations or the native/browser comparison.

`tools/coverage/predict.py` decodes the three frozen IMAGE inputs for sphere,
metal-butt and shadow-decal. It writes separate silhouette and owner-change
masks. It rejects unknown input hashes because its command and value-identity
assumptions have only been derived for those inputs. `ledger.py` emits the eight
specified golden-change masks and nineteen empty control masks, and can verify
the archived QUAD/TRI2 evidence. `compare_three_way.py` requires every parent to
candidate byte change, including alpha, to fall in a supplied explanation mask;
the candidate's oracle maximum cannot exceed the smaller of the measured parent
maximum and its frozen gate. Those masks and value explanations require review;
the tool does not infer a cause from a rendered difference.
