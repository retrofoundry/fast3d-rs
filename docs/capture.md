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

A version-one fixture must be self-contained. Replay compares `PerFrame` and `Persist` output
after initializing the used color targets with two contrasting colors through display lists.
This catches missing clears that two fresh renderers would conceal. A mismatch rejects the
fixture. This is a test of the current renderer's color-persistence behavior; it does not add
persistent depth or reconstruct GPU contents from RAM. The initialization check rejects paired
framebuffers wider or taller than 1023 pixels, the primer's fixed-coordinate range.

Alpha dither uses the recorded frame serial and seed on replay. `CaptureFrame::begin` calls
`Renderer::begin_frame` and records its count, starting at one; its legacy serial argument is
ignored. The counter survives renderer reconfiguration. The shader uses the serial's low
32 bits, XORed with the dither seed, and the framebuffer pixel index.

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
  cargo test -p fast3d --features "asm capture" --lib \
  write_browser_sm64_fixtures -- --ignored
cargo test -p fast3d --features "asm capture" --lib browser_fixture_bytes_match_builders
cargo test -p fast3d --features "asm capture" --lib sm64_corpus_roundtrips_without_diagnostics
```

The GPU gate round-trips every sm64 fixture through the public headless facade and compares
its output with explicit-device replay on the same adapter:

```sh
cargo test -p fast3d --features "asm capture" --lib sm64_corpus_public_facade -- --nocapture
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
