# Texture replacements

`ultragfx::replacement` provides an immutable, in-memory registration provider.
Renderers start with replacements disabled. Install a snapshot with
`Renderer::set_replacement_provider(Some(Arc::new(provider)))`, then call
`begin_frame`. A queued update, including removal with `None`, takes effect at
that boundary. Calls between boundaries keep using the active snapshot.

`ReplacementProvider::new(revision, registrations)` owns its registrations.
The revision is a consumer-supplied label. Installing another snapshot invalidates
all previous decisions, even if its revision label is unchanged. Reconfiguration
and device recreation clear active and queued providers and all replacement GPU
objects. Reinstall a provider on the new renderer to upload its images again.

## Registering an image

Build a `ReplacementTarget` from a `DecodeRecipe`, owned `EncodedInput`, and
`TextureKeyScheme`. The constructor validates the recipe and constructs its exact
witness without decoding pixels. `EncodedInput::Tmem` owns an `Arc<[u8; 4096]>` in
physical N64 texture-byte order. Base and line are in eight-byte words; TLUT is
an extracted mode `0`–`3`. Modes 2/3 apply to every format. CI with modes 0/1
uses intensity, including the palette selector in CI4. A palette selector is
retained for CI4 or any four-bit TLUT recipe; other recipes use zero. Non-CI
recipes with TLUT disabled use mode zero.

UltragfxV1 uses the version-1 recipe and exact sampled address/value witness.
Set `palette_lanes` when a used TLUT slot has non-replicated halfwords; the
constructor verifies that flag against the owned bank. Such images contain four
stacked lane planes, with an allocation height four times `recipe.output[1]`.
The native witness includes the lane bytes and non-CI TLUT inputs. No load
provenance or retired LinearCompat stream participates in matching.

`ReplacementTarget::key()` exposes the versioned numeric key and `witness()`
exposes the exact scheme-specific bytes. The witness cannot be supplied separately
or replaced with an arbitrary hash. A registration pairs that target with
`ReplacementStatus::Ready(ReplacementImage { extent, pixels, format })` or
`ReplacementStatus::Pending`.

Ready pixels are owned `Arc<[u8]>` RGBA8, in linear UNORM colour space, with
straight alpha. `format` must be `wgpu::TextureFormat::Rgba8Unorm`. There is no
sRGB conversion, premultiplication, mip generation, guest-memory access or file
I/O during lookup. Consumers can load data asynchronously before installing a
new snapshot.

Single-plane tiles accept their native dimensions or a positive integer
scale shared by both axes. Both dimensions must fit the device's maximum 2D
texture dimension. Scale is at most 32,767 so a mirrored 15-bit mask period fits
signed shader arithmetic. Zero dimensions, fractional or unequal axis scales, overflow,
and a length other than `width * height * 4` are rejected. A lookup image accepts
only its native allocation (4096×4, or 4096×16 with palette lanes). Four-plane
tiles also require their native allocation size. `ReplacementTarget::extent()`
returns that allocation extent. Framebuffer views, dummy images and already-RGBA
inputs do not enter replacement lookup. Both exact and materialised
`TextureSource::Framebuffer` requests bypass the provider and v5 admission,
including when their CPU placeholder bytes would match a registered target.

## Lookup and lifetime

Lookup tries an exact UltragfxV1 target first. If absent, a provider containing v5
registrations can request an eligible exact Rt64V5 alias. If both schemes have an
exact registration, UltragfxV1 wins. Within a scheme, the last exact registration
wins. Numeric equality alone never authorizes a replacement.

An exact pending or invalid UltragfxV1 registration stops the search and uses the
native image. It does not fall through to v5. A missing registration also uses
native output. Invalid images increment `replacement.invalid` and emit a warning
with the provider revision and reason. Pending and no-match outcomes have their
own counters. All outcomes, including aliases, are cached until input changes,
eviction or the next installed provider snapshot.

Decision reuse checks the exact recipe and v5 eligibility. Equal owned inputs
reuse the decision directly. If input bytes differ, reuse requires an equal exact
native target and, when the lookup reached v5, equal bytes in every retained v5
input range. Those ranges include all eight bytes of every used palette slot. Native recipe
equality includes TLUT mode and palette-plane selection; native witness equality
checks every used lane when planes are required.
Unrelated TMEM writes do not force another provider lookup or upload. The native
target check preserves native-first precedence even for cached v5 misses. Native
decoded residency always uses its original UltragfxV1 key and witness, including
when replacements are enabled.

The decision cache admits at most 256 entries and 8 MiB of charged payload.
Charges include encoded input, native witness, v5 range storage and retained
RGBA bytes per entry, so at most 8 MiB of replacement RGBA stays cached; sharing
may make the actual allocation smaller.
Oversized entries bypass the cache. Eviction drops cache ownership; bound and
submitted images remain alive. Queue completion releases submission ownership.
These caps do not limit caller-owned registration snapshots or in-flight work.

Counters distinguish provider calls, witness/input comparisons, decisions,
pending/invalid/no-match results, v5 admission/hash bytes, uploads, evictions and
bypasses. Gauges report provider payload, charged cache payload, deduplicated live
RGBA CPU storage, live/pinned/transient/in-flight GPU storage and allocation
overhead. Provider-off rendering makes no provider calls, admission checks or v5
hashes. Existing native TMEM counters retain their meanings.

## Sampling scaled images

N64 tile bounds, shifts, masks, clamp/mirror flags and logical extents remain in
N64 texel units. LOD and role selection use those original dimensions. After the
ordinary tile-coordinate calculation, a replacement multiplies the coordinate
by its integer scale. Addressing uses correspondingly scaled clamp extents and
mask periods. Point filtering floors that coordinate; three-point filtering uses
its fractional part and the same triangular interpolation rule. Alpha follows
the same interpolation as RGB. No half-texel shift or mip bias is added. Same-size
and lookup replacements use the native sampling path, including TLUT tap
selection and lane planes. Scaled single-plane images use only the
replacement-space rule above, so point-sampled TLUT draws also floor. Scale is
carried separately from the lane-plane height and filter bits.

For example, a scale of two maps a native coordinate of 0.125 to replacement
coordinate 0.25. Three-point filtering at `(0.25, 0.25)` weights taps `(0,0)`,
`(1,0)`, `(0,1)` by `1/2`, `1/4`, `1/4`. A two-texel native mirror period becomes
four replacement texels, addressed `0,1,2,3,3,2,1,0`.

## Rt64V5 compatibility

The alias matches the version-5 `TMEMHasher` at rt64 commit
`43373749dac9bbc1b653e6a02aed40a9e1783bed`. It admits supported physical Tile
requests with equal logical/output extents, nonzero line, drawn row bytes no
larger than stride, and no bank crossing by a row or swapped tail. Four-bit
widths must be even and height must not exceed 256. CI requires RGBA16 or IA16
TLUT with replicated used palette slots; non-CI requires raw TLUT off.
Because rt64 hashes each axis at the smaller of the clamped tile extent and the
mask period, a masked axis needs a period equal to the tile extent, or larger on
a clamped axis. Both checks use the retained sampling state before lookup.
Lookup requests have no v5 alias.

| TMEM semantics | Alias eligibility |
|---|---|
| Non-CI TLUT | Excluded. The frozen vectors cover non-CI formats only with TLUT off, and the hasher treats every texel byte as a palette index where the native decoder indexes larger texels by their high byte. |
| CI with TLUT off | Excluded. The frozen vectors cover CI only with an RGBA16 or IA16 TLUT, although CI4/CI8 intensity agrees with rt64's decoder. |
| Four TLUT lanes | Non-replicated used slots are excluded. The v5 stream hashes all eight bytes, but rt64's `TextureDecoder.hlsli` reads the first halfword; the native decoder can sample all four. Non-replicated unused slots do not exclude a request. |
| Eight-bit T wrap | Heights above 256 are excluded: rt64's hasher advances rows without the native eight-bit T wrap. Existing stride/bank bounds already exclude most such requests. |
| Load-tile destination layout | Eligible when the resulting physical request passes the same proof. The hasher consumes the bank at draw time and has no load-history dependency. |
| Mixed, stale or initial-zero contents | Eligible under the same proof. Narrow blocks and the crystal switch qualify without any reconstruction. |

Odd four-bit widths, zero line, rows wider than stride, bank crossings (including
swapped tails), mask periods that change rt64's hashed extent, and Lookup are
excluded. No other physical format is admitted.

The stream contains physical row spans, the low bank followed by the high bank
for RGBA32, and used palette slots in increasing index order. Each palette slot
contributes all eight bytes. Its little-endian tail is width u16, height u16,
shifted TLUT u32 (`0`, `0x8000`, `0xc000`), line u16, size u8, format u8.
Base and palette select bytes but are not appended. Relocation can match only
when the resolved stream and proved interpretation are exactly equal.

`rt64-v5-vectors.json` freezes 82 streams, span lists and hashes.
The C++ verifier checks all 82. The Rust production check matches the 52 eligible
streams and asserts exclusion of the 30 with non-replicated used palette slots. Run the probe
against an unchanged reference checkout:

```sh
python3 tools/tmem/verify_rt64_v5.py --reference /path/to/rt64-reference --output /new/evidence/directory
cargo test -p ultragfx --test rt64_v5
```

`rt64-v5-fixtures.json` also freezes IDs and streams for admitted requests in
the three committed TMEM fixtures; the residency tests check them on native and
wasm. All 32 streams are checked against the pinned C++ probe. The verifier
authenticates the pinned headers and probe source before compiling. It never
regenerates expected vectors. Hash-only pack records, older rt64 versions,
filesystem/archive importers and HD sampling parity with rt64 are not implemented.

## Verification

CPU checks (the residency filters do not obtain a device):

```sh
cargo test -p ultragfx --features capture,profiling --lib replacement
cargo test -p ultragfx --test rt64_v5 -- --nocapture
cargo test -p ultragfx --features capture,profiling --test texture_residency -- replacement_fixture_targets replacement_literal_inventory replacement_scaled_expectations replacement_shared_fixture replacement_narrow_requests replacement_framebuffer_sources replacement_s2b_tlut_fixture
python3 -m unittest discover -s tools/readbacks -p 'test_*.py'
python3 -m unittest discover -s tools/tmem -p 'test_*.py'
```

For current fixture inputs, set `ULTRAGFX_V5_FIXTURE_INPUTS` while running
`replacement_shared_fixture_v5_inputs`, then pass that JSON to the executable
produced by `verify_rt64_v5.py`. Compare `name`, `stream`, and `xxh3_64` for every
row. The original 82-vector verifier checks span lists as well.

On a GPU host, run the full `texture_residency` target with capture and profiling.
It includes provider-off controls, lifecycle/precedence, independent roles,
scaled point/three-point mirror/clamp, exact/materialised/shortcut framebuffer
exclusions, and non-CI TLUT/four-plane identity replacements. The same target
compiles for wasm and runs in Chrome WebGPU. Windows WARP uses DX12/staticdxc and
the existing WARP adapter selection. These runtime checks and provider-off
cold/steady timing need their respective hosts; CPU results do not establish GPU
execution, final sampling, or performance parity.
