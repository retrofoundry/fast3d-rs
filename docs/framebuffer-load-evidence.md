# F0 framebuffer load evidence

F0 authors a real-load oracle probe and pins fast3d's current rejection. It adds
no framebuffer-load support, does not close item 11, and does not activate F1.
Native GPU, Chrome WebGPU and pinned rt64 results must be recorded separately.
The saved n64.toys `offscreen-then-sample.n64` and its `.bin` golden are unchanged.

`fast3d/src/tests/framebuffer_load_evidence.rs` builds four IMAGE fixtures:

| Fixture | Consumer and source |
| --- | --- |
| `fbload-offset-rect` | One-cycle TexRect; interior SETTIMG and nonzero LoadTile bounds |
| `fbload-offset-triangle` | The same load, consumed by pairs of textured triangles |
| `fbload-exact-rect` | One-cycle TexRect; producer-base SETTIMG and zero LoadTile origin |
| `fbload-shortcut-rect` | Existing load-free, exact-base copy-mode convenience; fast3d only |

The first three must be rejected by parent and candidate fast3d with exactly one
`UnsupportedFramebufferAccess { reason: TextureLoad }` diagnostic and 32 dropped
draw commands. A TRI2 drops once for its two triangles. Their consumers stay
black, while the unrelated blue witness remains visible. Rejection is a passing
boundary assertion. The shortcut has no diagnostic and executes its 16 TexRects.

## Independent construction

One F3DEX2 task clears a 320×240 RGBA32 output at `0x100000`, clears a 32×8 RGBA16
producer at `0x300000`, and draws eight constant-shade, one-pixel quads into the
producer. Each quad uses four identical vertex colors, identity modelview,
projection scale `(1/256,1/256,1/128)` and viewport scale `(256,256)` with center
`(160,120)`. Thus vertex `(x-160,120-y,0)` projects to exactly `(x,y)`.
There is no lighting, blending with another color, Z test, texture filtering,
color dither or alpha dither. `CVG_DST_FULL` forces stored coverage to seven;
the visible RGBA16 coverage bit is therefore one. Triangle consumers enable
perspective correction and have constant ST and W=1.

The producer uses shade output, not packed fill words. Its upper row contains
colors whose low RGB bits matter. The lower row contains exactly representable
RGBA16 colors. With dithering disabled, each five-bit channel is `c8 >> 3`;
the guest word is `(r5 << 11) | (g5 << 6) | (b5 << 1) | 1`. TMEM texture decoding
expands each channel as `(c5 << 3) | (c5 >> 2)` and the low bit to alpha 255.
The SDK documents framebuffer format, coverage and dither controls in
[Programming Manual §15.5](https://ultra64.ca/files/documentation/online-manuals/man/pro-man/pro15/15-05.html).

| Texel (column,row) | Shaded RGB8 | Guest RGBA16, big endian | Loaded RGB8 |
| --- | --- | --- | --- |
| (0,0) | 19,85,141 | `12 a3` | 16,82,140 |
| (1,0) | 67,133,201 | `44 33` | 66,132,206 |
| (2,0) | 245,29,99 | `f0 d9` | 247,24,99 |
| (3,0) | 111,177,43 | `6d 8b` | 107,181,41 |
| (0,1) | 33,66,99 | `22 19` | 33,66,99 |
| (1,1) | 132,165,198 | `85 31` | 132,165,198 |
| (2,1) | 222,189,156 | `dd e7` | 222,189,156 |
| (3,1) | 8,49,90 | `09 97` | 8,49,90 |

Offset SETTIMG is `fd10001f / 00300088`: width 32, RGBA16, base byte offset 136.
The producer row stride is 64 bytes, so that base is row 2, column 4. LoadTile
`f4010004 / 0701c008` specifies tile 7 and 10.2 bounds `(16,4)..(28,8)`, or texels
`(4,1)..(7,2)` inclusive. Its first byte is
`0x300088 + 1*64 + 4*2 = 0x3000d0`, producer row 3, column 8. The second row
starts at `0x300110`, producer row 4, column 8. Each row copies one 64-bit word;
the strided source span is 72 bytes, with 16 bytes actually loaded. All bytes
belong to the same 512-byte producer. No captured RAM spans contain producer
pixels; reading its absent stale RAM would fail the CPU test.

The exact-base load is `fd10001f / 00300000` followed by
`f4000000 / 0700c004`, selecting producer rows 0–1, columns 0–3. All eight
colors and the later overwrite are otherwise the same.

The load descriptor is `f5100420 / 07080200`: RGBA16, TMEM word `0x20`, line 2.
The render descriptor uses tile 0 with the same TMEM base and line, clamps both
axes, and has size 4×2 at origin zero. Line 2 is a 16-byte destination stride,
independent of the 64-byte producer stride and the eight bytes loaded per row.
The odd load row swaps the two 32-bit halves:

| TMEM physical byte range | Literal bytes |
| --- | --- |
| `100..108` | `12 a3 44 33 f0 d9 6d 8b` |
| `108..110` | untouched |
| `110..118` | `dd e7 09 97 22 19 85 31` |

The read-side swap recovers texels (0,1)..(3,1) from halfword addresses
`114,116,110,112`. CPU tests check physical bytes, untouched padding, decoded
pixels and the same command stream redirected to independently packed RAM.
The [SDK GBI header](https://ultra64.ca/files/documentation/online-manuals/man/header/gbi.htm)
specifies coordinate and TMEM descriptor fields and LoadSync → LoadTile →
PipeSync. The fixture also uses TileSync before descriptor changes and PipeSync
before color-image and render-state changes.

## Pixels and order

The real fixtures execute: producer shade → PipeSync → switch to output →
LoadTile → first consumer group → PipeSync → overwrite the whole producer with
shaded `(203,53,107,255)` → PipeSync → second consumer group. There is exactly
one texture load and no later TMEM write. Both groups must retain the old eight
texels. A pointer to the live producer would fail this test.

Every consumer patch is 16×16. First-group columns start at `x=40,56,72,88`;
second-group columns at `x=136,152,168,184`. Rows start at `y=40,56`. The loaded
RGB8 column above gives the required color of every pixel. The triangle variant
draws each patch as two triangles with identical ST at all vertices; there are
no sloped outer edges. TexRects use constant `s=column+0.5,t=row+0.5`; vertex ST
doubles that value before the RSP's exact one-half texture scale. Point sampling
addresses the same eight texels.

Panels `[40,104)×[88,120)` and `[136,200)×[88,120)` use
`(1-0)*TEXEL0_ALPHA+0` for RGB. Every pixel must be white. This checks the loaded
RGBA16 low bit independently of final-target coverage alpha. A blue witness
fills `[232,264)×[40,72)`. Everything else is black. Bounds are half-open.

`*.expected.rgba8` contains these independently authored RGB values. Its alpha
bytes are 255 placeholders, excluded from the rt64 gate: rt64 writes internal
coverage into final RGBA32 alpha. The RGB alpha panels remain exact. The RGBA32
output avoids another RGB16 quantization that could hide an unquantized copy.
No threshold above zero is allowed for the grid-exact rows and the alpha panels; the non-grid upper rows are a recorded reference disagreement, see the rt64 result below.

The shortcut uses copy mode because today's one-cycle load-free path rejects
`NoTextureLoaded`. Its first group sees the shaded RGB8 column; its second sees
`203,53,107` everywhere. It has no alpha panels. Only its first group's lower row,
`[40,104)×[56,72)`, has the same RGB prediction as the exact-base real load:
those colors already lie on the RGBA16 grid. Neither the first upper row nor the
post-overwrite group is an equality claim. The saved load-free source acquires
no hardware-load semantics from this comparison.

## Run the gates

Use tools already on PATH from this worktree. The writer requires no GPU:

```sh
export FAST3D_WRITE_FIXTURES=/tmp/fast3d-f0
cargo test -p fast3d --features capture --lib f0_
cargo test -p fast3d --features capture --lib write_rt64_framebuffer_load_evidence -- --ignored
cargo test -p fast3d --features capture --test framebuffer_load_evidence -- --nocapture
cargo test -p fast3d --lib exact_base_fb_alias_keeps_compatibility_pixels
```

The writer directly emits `.f3dcap`, `.rdram`, `.json`, `.expected.rgba8` and
`.fast3d-expected.rgba8` for each real load. It first asserts exact rejection,
normal termination and final target. It preserves the same big-endian commands
in both exports, with zero-filled gaps in the 8 MiB RDRAM. The ordinary
`export_capture_rdram` remains strict and rejects these diagnostic fixtures.
The shortcut gets only a capture and fast3d expectation, never an rt64 export.
Checked-in captures must match regenerated bytes exactly.

Run pinned rt64 `43373749dac9bbc1b653e6a02aed40a9e1783bed` in the Metal session
using the harness built from `tools/rt64-oracle/README.md`:

```sh
set -e
for scene in fbload-offset-rect fbload-offset-triangle fbload-exact-rect; do
  /tmp/rt64-oracle-build/rt64-oracle \
    "$FAST3D_WRITE_FIXTURES/$scene.rdram" "$FAST3D_WRITE_FIXTURES/$scene.json" \
    "$FAST3D_WRITE_FIXTURES/$scene-rt64"
  cargo run -p fast3d --example compare_rgba8 -- \
    "$FAST3D_WRITE_FIXTURES/$scene.expected.rgba8" \
    "$FAST3D_WRITE_FIXTURES/$scene-rt64.rgba8" \
    320 240 --ignore-alpha --threshold 0 --max-diff-pixels 0 \
    --diff-mask "$FAST3D_WRITE_FIXTURES/$scene-rt64-diff.png"
done
```

For browser execution, set `CHROMEDRIVER` to the installed matching driver:

```sh
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT=300
export WASM_BINDGEN_TEST_WEBDRIVER_JSON="$PWD/fast3d/webdriver.json"
cargo test -p fast3d --features capture,profiling --target wasm32-unknown-unknown \
  --test browser_sm64_fixture_replay --test capture_facade \
  --test capture_memory_wasm --test profiling_capture \
  --test framebuffer_load_evidence -- --nocapture
```

The native/browser test asserts current rejection, exact surviving fast3d pixels
and shortcut pixels under both clear policies. It does not require fast3d to
render the real loads. Preserve the existing Metal/DX12 and fallback matrix.

## Refutation and remaining limits

Any real-load RGB pixel outside its literal expectation, any nonwhite alpha
panel, incorrect offset, or new producer color in the second group fails the
oracle claim. A changed rejection, stale guest read or changed shortcut pixel
fails compatibility. Do not bless renderer output as the expectation, increase
tolerance, or update a `.bin` golden to make these gates pass.

Pinned rt64 source raises a specific unresolved concern: framebuffer tile copies
use `TextureCopyPS.hlsl`'s unquantized `gInput.Load`, and `TextureSampler.hlsli`
decodes copied coverage alpha without an evident same-format RGB16 repack.
If hardware produces the shaded RGB8 upper row instead of the loaded RGB8 row,
that refutes this oracle agreement claim. Resolve the oracle path or revise the
proposed semantics before F1; this is not permission to sample an unquantized
fast3d attachment as RGBA16.

These probes make the low bit representable by forcing full coverage and opaque
shade. They do not establish recoverable guest bits for partial coverage,
blending or dither in fast3d's current RGBA8 attachment model. A broader packing
requirement needs explicit representation or a tighter supported boundary.
Even oracle agreement only establishes feasibility: F1 still requires a named
consumer trace that actually issues a framebuffer load.


## rt64 result (recorded 2026-09-12, ci4 Apple M1 Metal, rt64 `4337374`)

The three real-load fixtures were exported and run through the pinned rt64 oracle and compared
against `*.expected.rgba8`, RGB only, threshold zero. **rt64 disagrees on 2048 pixels in every
fixture**, max channel difference 5, bounds (40,40)..(199,55) inclusive: exactly the upper row of
every consumer patch in both groups. The lower rows and both alpha panels match to the pixel, and
the second group still shows the pre-overwrite texels, so snapshot-at-load ordering agrees.

Per texel rt64 returns the unquantized shaded RGB8 — (19,85,141), (67,133,201), (245,29,99),
(111,177,43) — where the derivation above gives the RGBA16 round trip (16,82,140), (66,132,206),
(247,24,99), (107,181,41). rt64's LoadTile from a rendered target copies its full-precision RGBA8
render target; it does not represent the 16-bit guest bits the framebuffer memory would hold.
fast3d's existing exact-base shortcut behaves the same way, so fast3d and rt64 agree with each
other and both differ from the derived hardware bits.

This is the refutation the acceptance section anticipates. The derived expectation stands as the
hardware model; the rt64 comparison is a recorded reference disagreement, not a passing gate, for
non-grid texels. Which convention a future framebuffer-load implementation should follow is an
accuracy decision outside this fixture's scope, and F1 remains deferred until it is made.

### Decision (2026-09-12)

Framebuffer loads follow rt64: a load from a rendered target reads the full-precision render
target, not a quantised RGBA16 copy. fast3d renders on modern hardware and, like rt64, upscales
and enhances rather than reproducing 5-bit dithered banding. The derived RGBA16 expectations above
remain the hardware model and the record of the difference; they are not the target. Any future
framebuffer-load support (F1) implements the rt64 convention, and the existing exact-base shortcut
already matches it.
