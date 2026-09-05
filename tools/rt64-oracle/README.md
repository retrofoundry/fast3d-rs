# rt64 capture oracle

This compares fast3d replay with rt64's native framebuffer written back to RDRAM.
It needs the separately built rt64 static library and an SDL/Metal session. The C++
project is outside the Cargo workspace; normal Cargo CI does not build it.

Run these commands from the fast3d worktree. The reference build on ci4 is
`/Volumes/DS Vault/hub/wt/rt64-reference`, revision
`43373749dac9bbc1b653e6a02aed40a9e1783bed`, configured with `RT64_STATIC=ON`.
Nothing here rebuilds or changes that checkout. Build output goes to `/tmp`.

```sh
cd '/Volumes/DS Vault/hub/wt/fast3d-rs-rt64-oracle'
export XDG_CACHE_HOME=/tmp/fast3d-oracle-cache

PATH="$HOME/hub/scratch/fast3d/xcrun-shim:$PATH" \
  devenv -O packages:pkgs "cmake ninja SDL2 zlib pkg-config" shell -- \
  bash -c 'cmake -S tools/rt64-oracle -B /tmp/rt64-oracle-build -G Ninja \
    -DRT64_ROOT="/Volumes/DS Vault/hub/wt/rt64-reference" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_COMPILER="$(xcrun -f clang)" \
    -DCMAKE_CXX_COMPILER="$(xcrun -f clang++)" \
    -DCMAKE_CXX_FLAGS="-include cstdlib"'

PATH="$HOME/hub/scratch/fast3d/xcrun-shim:$PATH" \
  devenv -O packages:pkgs "cmake ninja SDL2 zlib pkg-config" shell -- \
  cmake --build /tmp/rt64-oracle-build -j 2
```

`RT64_BUILD_DIR` defaults to `$RT64_ROOT/build`. CMake links rt64, re-spirv,
nativefiledialog, zstd and plume archives, SDL2, zlib, and the Apple frameworks.
It compiles the reference's `xxhash.c` to supply the external XXH symbols and uses
its `stb_image_write.h` for PNG output. There are no downloaded dependencies.
The temporary `XDG_CACHE_HOME` lets devenv run when the usual Nix cache is outside
the writable sandbox.

Generate both fixtures without a GPU. The tests are ignored by default and need
both `asm` (the existing in-crate test module gate) and `capture`.

```sh
mkdir -p /tmp/fast3d-oracle
FAST3D_WRITE_FIXTURES=/tmp/fast3d-oracle \
  devenv shell -- cargo test -p fast3d --features 'asm capture' --lib \
  write_rt64_ -- --ignored
```

The writers reuse the Metal Mario and mixed JRB scene builders. They append a
wrapper that sets the framebuffer, viewport, scissor, identity matrices and a
black clear, calls the scene, then performs FullSync. Both output 320x240 RGBA16
at `0x00100000`. Mario's original pairless test still renders at 256x256; the
capture uses 320x240 so fast3d's current fixed viewport fold does not introduce
an unrelated size discrepancy. Geometry, normals, textures and other payloads
are synthetic, as recorded in fixture provenance. These are isolated command
scenes, not captured game frames.

Run each fixture through both renderers, then compare. The rt64 command opens a
window. Its PNG comes from RDRAM readback, before VI filtering or presentation
scaling. Claude should run this part in the GPU session.

```sh
for scene in mario-metal-butt jrb-mixed-fog; do
  devenv shell -- cargo run -p fast3d --features capture \
    --example export_capture_rdram -- \
    "/tmp/fast3d-oracle/$scene.f3dcap" "/tmp/fast3d-oracle/$scene" || break

  /tmp/rt64-oracle-build/rt64-oracle \
    "/tmp/fast3d-oracle/$scene.rdram" "/tmp/fast3d-oracle/$scene.json" \
    "/tmp/fast3d-oracle/$scene-rt64" || break

  devenv shell -- cargo run -p fast3d --features capture \
    --example replay_capture -- \
    "/tmp/fast3d-oracle/$scene.f3dcap" "/tmp/fast3d-oracle/$scene-fast3d" || break

  devenv shell -- cargo run -p fast3d --example compare_rgba8 -- \
    "/tmp/fast3d-oracle/$scene-rt64.rgba8" "/tmp/fast3d-oracle/$scene-fast3d.rgba8" \
    320 240 --diff-mask "/tmp/fast3d-oracle/$scene-diff.png" || break
done
```

The comparison reports the largest absolute channel difference, per-channel
maxima, pixels with any RGBA channel above the threshold, and their inclusive
bounding box. White pixels in the mask exceed the threshold. The default is
8/255 to allow RGBA16 quantization; `--threshold N` changes it. Add
`--max-diff-pixels N` to fail when the count exceeds a reviewed budget. With no
budget the command reports differences and succeeds. No budget or golden is
established by this tool: current filtering, coverage and blending differences
can still produce a mask. Review that mask before using it as a gate for
`docs/design/sm64-fidelity.md` PRs 2 through 4.

The exporter accepts only IMAGE layout: big-endian data and eight-byte commands.
It preserves physical addresses in an 8 MiB image, zeroes uncaptured gaps, walks
each task through the existing CPU interpreter, and writes the final colour
image plus ordered task entries, microcodes and initial segment tables to JSON.
Interpreter diagnostics, missing reads, out-of-range spans and conflicting bytes
between task snapshots are errors. HOST64 Helix captures cannot be fed to rt64.
The two renderers receive the same exported command bytes; there is no pointer
translation or reconstruction of a host capture.

A single image cannot represent changing task snapshots. Tasks must initialize
their own state and colour attachments; RAM feedback and dependence on earlier
GPU contents are outside this comparison. The final target must be RGBA16 or
RGBA32, have an eight-byte aligned address, fit RDRAM and match the output width.
A recorded fast3d VI must select that target. The harness currently supports the
macOS static build, widths 1..1022 and heights 4..512 divisible by four; RGBA16
rows need an even width. Use only trusted fixtures: rt64's HLE walker does not
bounds-check arbitrary display-list graphs.

`--gbi f3d|f3dex2` overrides the recorded microcode. Without it, each task selects
its recorded GBI. The harness initializes the common RDP table, the selected GBI
table, standard flags, RSP state and segments without loading microcode binaries.
`--scale N` accepts 1..16 and scales the SDL window only; the oracle stays native.
`<prefix>.log` receives rt64 stderr/stdout and setup, GBI, workload and presentation
diagnostics. Setup or output failures return nonzero.
Unknown opcodes are rejected by the harness. The supplied Release rt64 archive
compiles out debug logging and some assertions inside supported command handlers;
redirecting stderr cannot restore those checks. A clean log is not proof that
every possible subcommand was validated.

The reference source establishes the integration points below. Paths and lines
are relative to the pinned rt64 checkout.

- `src/hle/rt64_application.h:61` defines Core memory, register pointers and the
  interrupt callback. `src/hle/rt64_application.cpp:122` creates a window for an
  empty `core.window`; `:486` forms native display-list pointers and `:497` invokes
  HLE. `src/gbi/rt64_display_list.h:8` stores two native `uint32_t` words.
  `src/hle/rt64_rdp.cpp:382` reads RDRAM bytes with `address ^ 3`.
- `src/gbi/rt64_gbi.cpp:396` hashes microcode text and data. This checkout does
  contain SM64 F3D: the instance is at `:58`, with text and data hashes at `:172`
  and `:271`. Direct selection still removes the binary requirement.
  `:462` initializes the cache and `:509` assigns flags;
  `src/hle/rt64_interpreter.cpp:37` and `:46` apply RSP GBI and task reset.
- `src/hle/rt64_state.cpp:820` selects render-to-RAM; `:1160` fixes it to 1x.
  `:1471` waits for the graphics worker before `:1479` copies to RAM.
  `src/hle/rt64_framebuffer.cpp:149` reverses each word on writeback. The harness
  reverses each input word and accesses output bytes through `address ^ 3`.
- `src/hle/rt64_application.cpp:521` presents through `core.decodeVI()`;
  `:528` shows the workload/presentation ID and idle waits. The harness waits
  after FullSync and again after presenting once. `src/hle/rt64_vi.cpp:81`
  subtracts one row from VI origin, so the supplied origin is the colour address
  plus one row. `:115` accounts for the extra rows in its height estimate.

## First results (2026-09-05, rt64 `4337374`, fast3d at PR 5)

`jrb-mixed-fog`: max channel difference 7/255, no pixel over 8/255. rt64 writes RGBA16 back to
RAM, so a difference below 8 is quantisation; the two fog settings in one frame come out the same
in both renderers.

`mario-metal-butt`: 12000 pixels differ, up to 128/255, all inside the ten patches. fast3d still
samples with GPU bilinear at normalised coordinates, so an integer texel coordinate straddles two
texels, while rt64 addresses the texel exactly. The fixture's coordinate-coded texture turns that
half-texel disagreement into a large colour difference on purpose. This is the baseline the tile
addressing and filter work (design PRs 6 and 7) has to close; rerun after those land.

Two things the oracle caught in the fixtures themselves, both invisible to fast3d because it never
writes depth to memory: a Z-buffered scene needs `SETZIMG`, or rt64's RAM write-back puts depth
values at address 0 and overwrites the display list; and it needs the depth buffer cleared, or
every fragment fails the depth test. The wrapper in `fixture.rs` now does both, the way sm64 does.

On macOS rt64's own SDL window path hands plume an `SDL_Window*` where `CocoaWindow` expects an
`NSWindow*` and crashes in `objc_msgSend`; the harness creates the window itself and passes the
Cocoa window plus the Metal layer through `core.window`, as the recomp frontends do.
