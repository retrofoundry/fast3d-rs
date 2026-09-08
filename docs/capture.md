# Display-list capture and replay

Enable fast3d's opt-in `capture` feature. `CaptureFrame` records public renderer operations;
`RecordingHardware` intercepts all memory reads made by one task. The supplied `HostRam` and
`RdramImage` readers report their layout and initial segment bases. A reader without capture
metadata is rejected. Host-pointer recording retains `HostRam`'s unsafe lifetime contract: the
guest must remain blocked until `process_dl` returns. Encoding and writing need only the owned
snapshot and can happen after the guest resumes.

`Fixture::from_bytes` loads one frame. `Fixture::replay` accepts a wgpu device and queue;
`replay_headless` requests them. Both return task diagnostics, summaries, and packed RGBA8
pixels. These async operations work on native and wasm32; wasm readback awaits the mapping
callback. The device must match the recorded dual-source-blending feature. Missing memory
latches an error and rejects the output, including missing TexRect continuation commands.

A version-one fixture starts with default RDP registers, empty TMEM and no established color
image. Replay preserves register state between its tasks. Each replay and clear-policy probe
restarts the registers, so framebuffer initialization commands cannot affect the fixture's RDP
starting state. The fixture format does not store a prior register or TMEM snapshot.

`CaptureFrame::begin` preserves live RDP state. At completion, capture walks its owned task
snapshots from both the live starting registers and the version-one defaults. It rejects a
difference in prepared render inputs, diagnostics or summaries as dependence on prior RDP
state. GPU submission consumes those same prepared inputs, including target effects, uniforms,
textures and RSP buffers. Inspection-only metadata does not participate. Tasks that hit the
renderer’s no-work return still carry each walk’s registers forward separately, so a later
active use of inherited state is checked. The check does not reset the running guest.

Reset before `CaptureFrame::begin` when recording an independent scene. Renderer mutations
outside the capture wrapper, including either reset, frame boundaries, reconfiguration and
unrecorded submissions, invalidate the capture. Recording and completion report an error while
live rendering continues. Returning registers or configuration to their earlier values does
not make the capture valid again.

A version-one fixture must also be self-contained in framebuffer contents. Replay compares
`PerFrame` and `Persist` output after initializing used color targets with two contrasting
colors and depth targets with distinct near (`0x0000`) and far (`0xfffc`) packed depths through
display lists. Every color/depth combination starts with a fresh renderer. A mismatch rejects
the fixture as dependence on prior attachment contents. The initialization check rejects targets
wider or taller than 1023 pixels, the primer's fixed-coordinate range. GPU contents are never
reconstructed from guest RAM. Legacy color and depth attachments are also primed through GPU
load clears, including pairless workloads.

Alpha dither uses the recorded frame serial and seed on replay. `CaptureFrame::begin` calls
`Renderer::begin_frame` and records its count, starting at one; its legacy serial argument is
ignored. The counter survives renderer reconfiguration; `Renderer::reset` restarts it and
the dither seed at zero. The shader uses the serial's low 32 bits, XORed with the dither seed,
and the framebuffer pixel index.

## Capture from Helix

Build Helix with this branch's fast3d and the `capture` dependency feature enabled. Set
`FAST3D_CAPTURE_DIR` and `FAST3D_CAPTURE_FRAMES` when starting the game:

```sh
FAST3D_CAPTURE_DIR=../sm64-frames \
FAST3D_CAPTURE_FRAMES=120,240,360 \
FAST3D_CAPTURE_REVISION="$(git rev-parse HEAD)" \
./build-cmake/sm64-us
```

Helix's selection indices start at zero and count graphics-task consumes, including frames
before gameplay. Whitespace around comma-separated indices is accepted; invalid or absent indices disable
capture with a warning. A selected consume copies memory while the guest is blocked. After
presentation, Helix names the file from the recorded renderer serial: selection `120` writes
`frame-000121.f3dcap` when each consume begins one frame. Existing files are never overwritten.
Use a new directory for another run. `FAST3D_CAPTURE_REVISION` and
`FAST3D_CAPTURE_SYMBOLS` optionally supply provenance; omitted values are marked unknown.
The hook cannot discover decomp symbols from runtime pointers.

An already running process must be relaunched to pick up the environment variables and the
newly linked hook. sm64 statically links Helix, so rebuilding only the Rust library is
insufficient: relink the game too.

Replay a captured frame from the fast3d worktree:

```sh
devenv shell -- cargo run -p fast3d --features capture --example replay_capture -- \
  ../sm64-frames/frame-000121.f3dcap ../sm64-frame-121
```

The example reports adapter information, task diagnostics and summaries, and writes a PNG
and raw RGBA8 file. Failure to obtain an adapter, missing memory, or a clear-policy mismatch
is an error. Review live images and independent semantic assertions before pinning any
regression golden. `fast3d/tests/fixtures/host64-fill.f3dcap` is a synthetic full-frame red fill,
with literal F3D commands and addresses above 4 GiB; it contains no game assets.

## Reset-rooted sequences

`Sequence` wraps existing frame payloads in a version-two container. It requires every frame
from renderer reset, serials `1..N` in order, contiguous task order within each frame, `Persist`,
and one unchanged configuration, extent and dither seed. `warmup_frames` counts the retained
initial frames before observation. `presentations` contains sorted original serials after that
warm-up. Cropped, missing, duplicated or reordered frames are rejected; packing never renumbers
a serial. Payloads retain their source layouts and full 64-bit addresses.

For a live recording, call `CaptureSequence::begin(&mut renderer, seed)`. It resets the renderer
and selects normal depth persistence. For each frame, call `begin_frame`, record every task
through `frame_mut()?.process_dl(...)` or its existing unsafe `process_dl_host(...)`, then call
one of the sequence's `present`, `present_to`, `present_last` or `present_last_to` methods.
Finish with `finish(warmup_frames, presentation_serials)`. Task memory is copied synchronously
before the recording call returns. `CaptureSequence` owns the active frame so its completion
can retain inherited RDP/TMEM state without changing standalone `CaptureFrame` admission.
Unrecorded renderer changes invalidate recording. The sequence starts from reset; it cannot be
attached halfway through a running game.

`Sequence::replay(device, queue, DepthResetPolicy::Never)` and `replay_headless` execute all
frames and tasks through one renderer, including every warm-up presentation. They preserve
registers, TMEM, attachments, original serials and seed between frames. Selected presentations
return packed RGBA8; every frame returns summaries, diagnostics and fetched CIMG, ZIMG,
FILLRECT, FILLCOLOR and SCISSOR commands with full words, task order and command PC. The log
records fetch order, including repeated commands reached through control flow.

The `capture` feature also exposes `Renderer::set_depth_reset_policy`. `Never` is the normal
path. `ColorImageSwitch`, `TaskBoundary` and `FrameBoundary` discard only depth at their named
boundaries. Color, explicit fills, task order, seed and presentation stay fixed. These switches
are diagnostic controls, separate from `ClearPolicy`. `CaptureFrame` rejects recording with a
diagnostic reset policy because version-one metadata cannot preserve it.

Pack a complete, ordered set of frame payloads and replay all controls:

```sh
cargo run -p fast3d --features capture --example replay_capture -- \
  --pack startup.f3dcap 120 121,122,123 captures/frame-*.f3dcap
cargo run -p fast3d --features capture --example replay_capture -- \
  startup.f3dcap evidence/startup all
```

The output directory must already exist. The fourth packing argument lists selected renderer
serials, not Helix's zero-based consume indices. A frame payload within a sequence may inherit
prior state; replaying that payload alone still uses strict standalone admission. The old Helix
single-frame hook must be updated to use the sequence recorder for inherited RDP/TMEM workloads.
A directory of intermittently selected captures cannot be converted into a reset prefix.

The replay mode defaults to `persist`; alternatives are `cimg`, `task`, `frame`, or `all`.
Each selected serial gets a PNG and `.rgba8` file per variant. Each run gets a `.log` with
adapter, frame configuration, provenance, task metadata, diagnostics and command records.
`all` uses the same device and adapter for every variant and also writes exact changed-pixel counts, inclusive bounds, FNV-1a 64-bit image hashes and
white-on-transparent PNG masks against the persistent run. These hashes identify bytes for
comparison; use an external SHA-256 tool when archiving cryptographic evidence. File names keep
the original serial, for example `startup-task-000121.png`.

This harness supports steps 2 and 3 of the live sm64 depth experiment. Authored sequence tests
exercise persistence and distinguish multiple tasks in one frame from separate frames. They do
not close the live gate. That requires a continuous startup-to-gameplay capture covering the
specified route, scene annotations, reviewed visible differences with writer/read PCs and shared
Z addresses, or complete zero-diff results plus command evidence that prior writes are cleared
or unused before observation. The current Helix hook puts one task in each frame, so task and
frame controls coincide for those recordings. The harness emits color images and command logs;
it does not yet emit per-operation depth readback probes or identify writer/read dependencies
automatically. HOST64 replay remains semantic evidence, not an rt64 input. No live route or
absence result is claimed by these tests.

## Version-two sequence byte layout

The version-two header uses `F3DCAP\0\0` and the same little-endian marker as version one.
All integer fields are little-endian and reserved fields must be zero.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | Magic `F3DCAP\0\0` |
| 8 | 4 | Version, `2` |
| 12 | 4 | Endian marker, `0x04030201` |
| 16 | 8 | Total file length |
| 24 | 4 | Frame count |
| 28 | 4 | Warm-up frame count |
| 32 | 4 | Selected presentation count |
| 36 | 4 | Reserved |

The header is followed by selected presentation serials as u64 values. Each frame then has a
u64 payload length and a complete version-one fixture, including its header and provenance.
Lengths must fit the container and trailing bytes are rejected. The version-two contract implies
a reset origin; there is no optional missing-prefix flag. Version-one decoding is unchanged.

## Authored corpus and browser tests

The in-crate `sm64_corpus::fixtures` function lists the ten sm64 cases. Their payloads
are synthetic and their provenance identifies the modelled decomp symbols. Water uses
`dl_waterbox_rgba16_begin`'s `MODULATERGBA`: texture alpha times vertex alpha from
`movtex_make_quad_vertex`. Environment alpha does not control this source path.

The browser suite embeds five `.f3dcap` files: the high-address fill, an environment-alpha
combiner selector, power-meter point filtering, castle TRILERP and transparent-Mario dither.
Native and browser runs share semantic assertions; the browser dither check allows 450–575
survivors among 1024 pixels with alpha 128/255. The native transparent-Mario test also checks
the exact deterministic mask. No pixel goldens are generated here.

Regenerate the checked-in files from the workspace root. The ignored writer and byte-drift
test need no GPU; the writer records the synthetic command reads through the CPU interpreter.
Cargo runs tests from the crate directory, so use an absolute output path:

```sh
FAST3D_WRITE_FIXTURES="$PWD/fast3d/tests/fixtures" \
  cargo test -p fast3d --features capture --lib \
  write_browser_sm64_fixtures -- --ignored
cargo test -p fast3d --features capture --lib browser_fixture_bytes_match_builders
cargo test -p fast3d --features capture --lib sm64_corpus_roundtrips_without_diagnostics
```

The GPU gate round-trips every sm64 fixture through the public headless facade and compares
its output with explicit-device replay on the same adapter:

```sh
cargo test -p fast3d --features capture --lib sm64_corpus_public_facade -- --nocapture
```

CI runs Chrome on `macos-14`, using Metal WebGPU, with the runner image's own Google Chrome
and the ChromeDriver at `$CHROMEWEBDRIVER`. A Chrome for Testing download does not work
there: its child processes cannot reach the browser's Mach rendezvous port and the network
service crash-loops before the page loads. Locally, put a matching Chrome and ChromeDriver on
PATH (or set `CHROMEDRIVER` to the driver's absolute path); set `goog:chromeOptions.binary`
in `fast3d/webdriver.json` only if Chrome is installed outside its usual location.
The crate's [WebDriver configuration](https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/browsers.html#configuring-headless-browser-capabilities)
enables headless Chrome and WebGPU with Metal. The checked-in arguments omit
`--no-sandbox`; wasm-bindgen-test-runner appends it internally.

`fast3d/Cargo.toml` pins `wasm-bindgen-test` exactly, which fixes the `wasm-bindgen` version
the runner must match (the repository does not track `Cargo.lock`). Install that runner, then
use the same test command and environment as CI:

```sh
version=$(cargo tree -p fast3d --target wasm32-unknown-unknown --features capture -e normal,dev --prefix none \
  | awk '$1 == "wasm-bindgen" {print substr($2, 2); exit}')
cargo install --locked wasm-bindgen-cli --version "$version"
export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
export WASM_BINDGEN_TEST_TIMEOUT=300
export CHROMEDRIVER="${CHROMEDRIVER:-$(command -v chromedriver)}"
export WASM_BINDGEN_TEST_WEBDRIVER_JSON="$PWD/fast3d/webdriver.json"
cargo test -p fast3d --features capture --target wasm32-unknown-unknown \
  --test browser_sm64_fixture_replay --test capture_facade \
  --test capture_memory_wasm -- --nocapture
```

Replay requests `Features::empty()` and default limits, exercising the fallback blender.
The suite prints `sm64 fixture adapter: <backend> <name> (<fixture>)` for every replay
and fails if WebGPU has no adapter. `--nocapture` keeps adapter lines in successful CI logs.

## Version-one byte layout

All container integers are unsigned little-endian. Payload bytes retain the source byte order;
there is no pointer rewriting, struct transcode, compression, or implicit alignment. Every
reserved field must be zero, lengths must fit the file, and no trailing bytes are accepted.

The 32-byte header is:

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | Magic `F3DCAP\0\0` |
| 8 | 4 | Version, `1` |
| 12 | 4 | Endian marker, `0x04030201` (bytes `01 02 03 04`) |
| 16 | 8 | Total file length |
| 24 | 4 | Task count |
| 28 | 4 | Reserved |

The 84-byte frame record follows:

| Relative offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | Frame serial |
| 8 | 4 | Dither seed |
| 12, 16 | 4 each | Output width, height |
| 20, 24 | 4 each | Resolution multiplier, sample count |
| 28 | 4 | Present mode: AutoVsync=0, AutoNoVsync=1, Fifo=2, FifoRelaxed=3, Immediate=4, Mailbox=5 |
| 32 | 4 | Output format: unspecified=0, RGBA8=1, BGRA8=2, RGBA8 sRGB=3, BGRA8 sRGB=4 |
| 36 | 4 | Clear policy: PerFrame=0, Persist=1 |
| 40 | 4 | Power preference: none=0, low=1, high=2 |
| 44 | 4 | Dual-source blending enabled: 0 or 1 |
| 48 | 4 | VI present: 0 or 1 |
| 52 | 32 | Eight VI u32s: status, origin, width, x_scale, y_scale, h_start, v_start, v_current |

VI words are zero when absent. The capture facade stores the effective output format, including
when the renderer originally selected it automatically. Four provenance strings follow at file
offset 116: decomp revision, source symbols, command-vector identity, synthetic-data description.
Each is a u32 UTF-8 byte length followed immediately by its bytes.

Each task then has a 176-byte header, a span directory, and a byte payload:

| Relative offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | Task order, contiguous from zero |
| 4 | 4 | Microcode: F3DEX2=0, F3D=1 |
| 8 | 4 | Data format: fixed=0, float=1 |
| 12 | 4 | Reserved |
| 16 | 8 | Entry virtual address |
| 24 | 8 | Memory layout record, below |
| 32 | 128 | Sixteen initial segment bases, u64 each |
| 160 | 4 | Span count |
| 164 | 4 | Reserved |
| 168 | 8 | Payload byte length |

The memory layout's eight bytes are: address space (image=0, host=1), byte order
(big=0, little=1), command word width, command stride, fixed-matrix packing
(split halfwords=0, packed words=1), and three reserved bytes. Image layout is
`00 00 04 08 00 00 00 00`. Host64 little-endian is `01 01 08 10 01 00 00 00`;
big-endian host64 changes only byte-order byte to zero. No other layouts are supported.
Image tasks require fixed data and 32-bit segment bases.

Each span-directory entry is three u64s: virtual address, length, and offset relative to this
task's payload start. Entries are sorted by virtual address, nonempty and nonoverlapping.
Payload offsets are contiguous from zero and account for the entire payload. Adjacent spans
can satisfy one read; any gap is an error. Address arithmetic stays in u64 until an offset
has been checked against owned payload bytes, so host64 fixtures replay on wasm32.

Every task owns its own snapshot. Overlapping reads within a task must agree byte for byte;
two successive tasks may contain different bytes at the same virtual address. Vertex flags
and trailing float-vertex padding are not read or captured. Fixed vertices use 16-byte strides,
float vertices 24; matrix records use 64 bytes. Host fixed matrices contain native-endian
packed u32s, while image matrices contain big-endian split halfwords. Textures remain verbatim.
