# Triangle coverage witnesses

Triangle coverage and interpolated attributes use the integer framebuffer sample
`(x, y)` (B). The shared raster vertex shader translates triangle positions by
`(w/W, -w/H)` in clip space, using each draw's raster viewport dimensions. UVs
use ordinary interpolation at that same sample. Shade, alpha, fog and depth
follow the translated geometry; LOD uses its UV derivatives. Dither and decal
depth reads retain their integer destination pixel addresses.

Every draw carries a rectangle flag. Fills, textured rectangles and COPY keep
their existing position and sampling rules. Compute RSP positions, including
MODIFYVTX overrides, retain guest coordinates. The native/browser conformance
test requires B on all 38 fixtures. The arithmetic module also retains A's
half-integer coverage expectation as a counterexample.

Scanout uses the VI row count from origin when an IMAGE VI selects a stored
framebuffer. Without VI, it uses the selected target's retained logical height,
so a final pair with a smaller scissor does not crop the display.

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
with FullSync. Captures record the final color-image origin and width, vertical
endpoints `0..2*height` and unit X/Y scales (`1024`). The decoded VI height equals
the authored output height. They use F3D and F3DEX2, including direct XY
modifications. An encoder test checks literal matrix, viewport and command words and independently
recovers screen coordinates from the stored vertex bytes. Capture bytes are
checked against regeneration. The shared native/browser replay requires a real
adapter and compares each selected full frame with B. Primary-color pixels are exact;
interpolated scalar output permits one UNORM rounding step at covered pixels.

Run CPU checks without an adapter:

```sh
cargo test -p fast3d --test coverage_arithmetic
cargo test -p fast3d --features capture --lib coverage_encoded_coordinates_and_commands
cargo test -p fast3d --features capture --lib coverage_fixture_bytes_match_builders
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

With a working GPU, run `coverage_matches_b` in
`browser_sm64_fixture_replay`; on wasm it requires BrowserWebGpu. It is also
registered as a native test. The captures disable dual-source blending.
The gate includes the retained-height capture: a 256-row allocation followed by
a 128-row pair at the same address presents the first 128 rows through its
recorded VI. All 38 captures exercise VI scanout. No ignored-test flag is needed
on native or wasm.
`coverage_rect_sampling_origin` supplements the flat rectangle controls with a
16×16 two-axis ramp, ST origin `(0.75, 0.75)`, one texel per pixel, and both flip
states in one-cycle, two-cycle and COPY. Every pixel of the 64×64 output is
asserted. The first rectangle sample is `(1.25, 1.25)`; applying the triangle
translation would sample `(0.75, 0.75)` and select the wrong texel.

The odd-width RGBA32 fixture uses a 193×132 VI. The near-plane fixture's clipped
edge is `x+y=144.25`, which misses both A and B sample lattices.
The clip-scissor fixture uses fractional intercepts on its four boundary-crossing
triangles so vertex snapping after clipping cannot decide an exact edge tie.
Arithmetic regressions keep those edges off both sample lattices and their
target/scissor intersections fractional. The dedicated unclipped tie and shared-edge
witnesses retain their exact ownership checks.
The same captures export through `export_capture_rdram` for the pinned rt64
oracle. `tools/coverage/check_fixture.py` checks saved renderer output against B.
`--parent` selects archived A output; `--rt64` checks B with the oracle channel
policy. It keeps exact background and
primary-color gates, explicit RGB quantization bounds for scalar interactions,
and a statistical oracle check for the dither fixture. Native dither remains
an exact destination-index expectation.

The strict rt64 check reports the F3D slope atlas's missing first column. With
`--rt64 --allow-rt64-f3d-quirk`, the checker requires the pinned `4337374`
omission: exactly 256 B-covered pixels must be black, and all remaining RGB
pixels must match B. The mask's SHA-256 is fixed; the output still reports the
raw 256-pixel discrepancy. Restored pixels also fail this quirk check. The
manifest names this exception and requires every fixture. The quirk
option changes neither A/B expectations nor the native/browser comparison.

`tools/coverage/predict.py` decodes the three frozen IMAGE inputs for sphere,
metal-butt and shadow-decal. It writes separate silhouette and owner-change
masks. It rejects unknown input hashes because its command and value-identity
assumptions have only been derived for those inputs. `ledger.py` emits the eight
specified golden-change masks and nineteen empty control masks, freezes before
bytes and predicted values, and can verify the archived QUAD/TRI2 evidence. Its
`compare` command records exact and tolerance-two deltas, rejects missing or
out-of-mask changes, and checks independently predicted values. It never writes
repository goldens. `compare_three_way.py` requires every parent to
candidate byte change, including alpha, to fall in a supplied explanation mask;
the candidate's oracle maximum cannot exceed the smaller of the measured parent
maximum and its frozen gate. Those masks and value explanations require review;
the tool does not infer a cause from a rendered difference.
