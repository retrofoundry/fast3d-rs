# Library contract and F3DEX2 completion

Design for roadmap item 2, revised after Claude's challenge of `ed42e50` on
2026-09-06. Scope and delivery order are settled here; the consumer rollout and
evidence decisions in section 6 remain with David. No renderer changes, builds,
or tests belong to this round. The document check is `git diff --check`.

Correct TLUT vocabulary, diagnostics and the listed F3DEX2 controls first. Take
the memory API break when David selects the consumer update, with both companions
ready. Then put draws through one ordered workload and store depth by address.
Decode the remaining RDP registers and reject draws that require unwired inputs.
Shader support waits for a game that uses them; optimization waits for measured
workloads. General framebuffer emulation and the next microcode remain deferred.

## 1. Evidence and scope

The inspected fast3d code revision is `a36682ccf9a71a43eee388ad600a61aeece7bf4c`;
`ed42e50` added this document on `contract-local`. References below use these
read-only checkouts:

| Prefix | Root | Inspected revision |
|---|---|---|
| `helix:` | `/Users/ci/hub/repos/helix/` | `7c6a9bb25c92ed7b1ef3f78f226c3aa1116a9412` |
| `helix-capture:` | `/Users/ci/hub/wt/helix-capture/` | `81e9a70` (`helix-capture`, helix PR #33) |
| `toys:` | `/Users/ci/hub/repos/n64.toys/` | `d082774588e150c4217fe2c7b3964438f2d8ca3b` |
| `sm64:` | `/Users/ci/hub/repos/sm64/` | `4a9dcf0d0a82a637b19b401f969639c9f4e0c83a` |
| `rt64:` | `/Users/ci/hub/wt/rt64-reference/` | `43373749dac9bbc1b653e6a02aed40a9e1783bed` |

Unprefixed paths are relative to fast3d. Citations are `file:line` at those
revisions; proposed file names and tests are identified in section 4. The rt64
file named `rt64_f3dex2.cpp` in the task is actually
`rt64:src/gbi/rt64_gbi_f3dex2.cpp:5`.

### What changed since the roadmap

The crate has `capture` and `debug-ui`, with no `asm` feature
(`fast3d/Cargo.toml:32`). The README assigns command production to the independent
`n64-gbi` leaf (`README.md:46`). Fixtures now use an aligned Rust `DlBuilder`,
big-endian command pairs, and Rust scene builders
(`fast3d/src/tests/dl_builder.rs:31`, `fast3d/src/tests/dl_builder.rs:103`,
`fast3d/src/tests/scene_builders.rs:6`). The `.n64` paths in fixture metadata are
provenance from commit `696a67d`, not files to edit
(`fast3d/src/tests/fixtures.rs:1`, `fast3d/src/tests/fixtures.rs:37`).

Thus item 7's assembler migration and item 13's compiled source programs are work
in n64.toys. This crate owes it correct encoders, a stable BE memory contract,
structured diagnostics, and the item 13 measurements before texture-reuse work.
Do not restore an assembler or add `.n64` fixtures here. Literal libultra vectors
belong in `n64-gbi/tests/conformance.rs`; interpreter round trips complement them
(`n64-gbi/tests/conformance.rs:1`, `fast3d/src/tests/gbi_roundtrip.rs:3`).

The earlier design's settled decisions are historical. Its claim that sm64 clears
every target is a fixture admission rule, not evidence of persistent depth:
`docs/design/sm64-fidelity.md:585`. Its references to assembler scenes and wasm
`asm` builds are superseded by the feature list and fixture authorship above
(`docs/design/sm64-fidelity.md:566`, `docs/design/sm64-fidelity.md:590`). Its golden
rule remains binding (`docs/design/sm64-fidelity.md:516`).

### Consumers today

| Consumer | Observed dependence | Consequence for this design |
|---|---|---|
| helix native ports | `HelixHardware` constructs `HostRam::new(&[])`; the guest is blocked during consumption (`helix:src/render.rs:9`). Each task calls `begin_frame`, `process_dl`, then releases the guest before `present` (`helix:src/render.rs:214`, `helix:src/render.rs:260`). | Copy all guest inputs before consumption returns. No guest pointer reads during presentation or delayed uploads. Preserve task ordering and the native word layout. |
| helix capture branch | Selects tasks with `FAST3D_CAPTURE_DIR` / `FAST3D_CAPTURE_FRAMES`, calls `CaptureFrame::process_dl` during `consume_dl`, then presents/writes the fixture after releasing the guest (`helix-capture:src/render.rs:266`, `helix-capture:src/render.rs:284`, `helix-capture:src/render.rs:328`, `helix-capture:src/render.rs:378`). | Migrate both the ordinary and recording calls in the PR 1 companion. Recording must finish all guest reads before `consume_dl` returns. |
| helix scanout | Uses `ClearPolicy::Persist`, no VI, and last-rendered scanout (`helix:src/render.rs:198`, `helix:src/render.rs:14`). `HLXViSwapBuffer` only records a pointer (`helix:src/ultra/vi.rs:381`). | GPU attachments are renderer-owned; color/depth addresses are identities, not permission to read or write CPU memory. Do not introduce RAM write-back into this path. |
| helix vocabulary and diagnostics | Microcode IDs are 0=F3DEX2, 1=F3D; data format is separate (`helix:src/render.rs:92`, `helix:src/render.rs:109`, `helix:include/helix/runtime.h:26`). Its sink deduplicates `Display` strings (`helix:src/render.rs:24`). | Preserve these IDs. New diagnostics reach its sink; stable enum values, rather than strings, should become its dedup key. |
| oxideports sm64 | US defaults to `f3d_old`; build options include F3DEX/F3DEX2, while the host declaration chooses only F3D_OLD versus F3DEX2 (`sm64:Makefile:48`, `sm64:Makefile:80`, `sm64:src/game/game_init.c:261`). | F3DEX is a real build vocabulary but is not correctly declared to helix by this switch. Do not count that option as a supported consumer or silently map it to F3DEX2. |
| sm64 images | One Z allocation and three color buffers; `init_z_buffer` fills Z through CIMG=ZIMG, then `select_framebuffer` switches CIMG (`sm64:src/buffers/zbuffer.c:6`, `sm64:src/game/game_init.c:138`, `sm64:src/game/game_init.c:155`, `sm64:src/game/game_init.c:639`). | Correct explicit clears and depth shared across color switches belong now. Cross-frame depth dependence still needs a capture. |
| n64.toys wasm | Owns assembled bytes in `WebHardware`; uses F3DEX2, default Fixed format, no VI and `PerFrame`; assembles at each render call (`toys:crates/web/src/lib.rs:153`, `toys:crates/web/src/lib.rs:176`, `toys:crates/web/src/lib.rs:218`, `toys:crates/web/src/lib.rs:241`). | Keep the safe `Hardware` frame loop. Reject invalid images through diagnostics; avoid reparsing in this crate. |
| n64.toys saved content | Persists source text, sends the current source/time/texture snapshot to rendering, and maps command addresses to source lines in UI diagnostics (`toys:server/src/db/schema.ts:120`, `toys:web-app/src/lib/playground.svelte.ts:340`, `toys:crates/web/src/lib.rs:66`). | Correct generated bytes by recompiling source in n64.toys. Preserve the initiating command PC for diagnostics. A survey of saved sources is needed before promising no source migration. |

n64.toys now owns `n64-toys-asm` and pins both fast3d and `n64-gbi` to `35c0ccf`.
`default-features = false` is on fast3d's dependency, not on `n64-gbi`'s
(`toys:Cargo.toml:6`, `toys:Cargo.toml:7`); neither requests the removed `asm`
feature. `WebHardware` still returns `RdramImage`
(`toys:crates/web/src/lib.rs:161`). An unchecked out-of-range image read that
panics today is a wasm panic surfaced by `console_error_panic_hook`, not a hang
(`toys:crates/web/src/lib.rs:144`, `fast3d/src/hle/mem.rs:138`). This supports P2's
structured failure contract without making PR 1 a dependency of TLUT or control
work. Its relocated compiler emits BE words and prepends an
old-layout TLUT without a destination tile setup for CI textures
(`toys:crates/asm/src/asm.rs:315`, `toys:crates/asm/src/asm.rs:1506`). The checked-in
CI source uses high-level texture loading, and the compatibility corpus freezes
RDRAM and source-map hashes (`toys:crates/asm/tests/scenes/ci4-canary.n64:24`,
`toys:tools/asm-compat/tests/compat.rs:7`,
`toys:tools/asm-compat/tests/compat.rs:122`,
`toys:crates/asm/tests/compat/expected.json:28`). Correcting the compiler therefore
requires reviewed corpus changes even if rendered pixels stay the same. These
sources are examples and compatibility cases, not a survey of production toys.

Helix main consumes directly; the hook is in `helix-capture` at `81e9a70`.
David identifies helix PR #33 as unmerged and waiting on the fast3d revision
bump. The checked-out hook's selection and recording sites are
`helix-capture:src/render.rs:266` and `helix-capture:src/render.rs:305`; the
challenge's `:169` / `:267` point to validation/configuration, not consumption.
Both branches are design inputs. Branch identification is settled; deployment
pins and the API-break date still need David's choice.

An `rg` survey at the sm64 revision above, covering `src`, `actors`, `levels`
and `bin`, finds no calls to `gSPModifyVertex`, `gSPCullDisplayList`,
`gSPBranchLessZ`, `gDPSetPrimDepth` / `G_ZS_PRIM`, `gDPSetConvert`, `gDPSetKeyR/GB`
or `gSPLoadUcode`, including their `gs` forms. The only name hit is the
LOAD_UCODE comment at `sm64:src/game/game_init.c:248`. sm64 explicitly selects
`G_ZS_PIXEL` (`sm64:src/game/game_init.c:141`). No committed game establishes a
need for primitive-depth, convert or key shader wiring.

n64.toys is the only current consumer for exercising these additions through
authored source, rather than a game trace. Refine "can exercise them today": at
`d082774` its `Stmt` inventory has no MODIFYVTX, CULLDL, BRANCH_Z, primitive-depth,
convert/key or LOAD_UCODE macros (`toys:crates/asm/src/parser.rs:370`); unmatched
statements diagnose at `toys:crates/asm/src/parser.rs:2492`. Numeric othermode and
combiner operands can already select the unwired modes/inputs
(`toys:crates/asm/src/parser.rs:2235`, `toys:crates/asm/src/parser.rs:677`). New
command syntax belongs in its compiler companion, using this crate's encoders.
Keep the named control work for authored use and explicit diagnostics; PRs 9–10
stop at register snapshots and draw rejection. No saved-source survey has been
performed.

### Item 7: TLUT

Both sides encode the same wrong convention: `gdp_load_tlut(tile, lrt)` puts
`(entries-1)<<2` in bits 0..11; HLE reads those bits and forces destination word
`0x100`, ignoring the tile (`n64-gbi/src/encode.rs:59`,
`fast3d/src/hle/rdp.rs:278`). The Rust CI fixture builder preserves that convention
and omits the palette `SetTile` (`fast3d/src/tests/scene_builders.rs:261`). A normal
libultra command would therefore load only one entry and ignore any non-default
destination.

Existing tests defend packed BE source bytes, but also defend the wrong command
layout and zero padding in the other three TMEM halfwords
(`fast3d/src/hle/rdp.rs:500`, `fast3d/src/hle/rdp.rs:531`). `write_tlut` writes only
two bytes of each eight-byte slot (`fast3d/src/hle/tmem.rs:192`). CI8/CI4 decode
tests cover palette interpretation, including IA16 and bank selection, not legal
command encoding (`fast3d/src/hle/texdec.rs:397`,
`fast3d/src/hle/texdec.rs:409`, `fast3d/src/hle/texdec.rs:438`).

### Item 8: F3DEX2 and RDP state

The F3DEX2 table wires VTX, TRI1/2, matrices, geometry, texture, othermode and
move commands, but none of the requested extra opcodes
(`fast3d/src/hle/rsp_f3dex2.rs:11`). Structural control is hard-coded to DL/ENDDL;
handlers cannot change the PC (`fast3d/src/hle/interp.rs:137`,
`fast3d/src/hle/interp.rs:336`). Standalone RDPHALF_1 is diagnosed as stray, which
conflicts with its use as BRANCH_Z's address latch (`fast3d/src/hle/rdp.rs:118`,
`sm64:include/PR/gbi.h:2433`).

The gap is wider than missing top-level opcodes: MOVEMEM matrix and MOVEWORD
matrix insertion, force-matrix and light-color subcommands are absent
(`fast3d/src/hle/rsp_f3dex2.rs:108`,
`fast3d/src/hle/rsp_f3dex2.rs:166`). rt64 handles the MOVEWORD cases
(`rt64:src/gbi/rt64_gbi_f3dex2.cpp:63`). They remain diagnosed, unsupported state
operations here; implement them when a committed game supplies a task using
them. "Completion" in this design covers the roadmap's named operations, not
every F3DEX2 subcommand or variant.

There is reusable implementation: `Rsp::modify_vertex` handles RGBA/ST/XY/Z and
copies used vertices, but invalid slots/attributes disappear silently
(`fast3d/src/hle/rsp.rs:395`). F3D reaches it through MOVEWORD POINTS and has QUAD
support (`fast3d/src/hle/rsp_f3d.rs:98`, `fast3d/src/hle/rsp_f3d.rs:194`). Existing
F3D tests exercise dispatch and modifying an unloaded cache
(`fast3d/src/hle/rsp_f3d.rs:875`, `fast3d/src/hle/rsp_f3d.rs:897`). TRI2 ordering,
DL call/return and the dispatch cap already have tests; the culling tests cover
face winding, not CULLDL (`fast3d/src/tests/decode.rs:6`,
`fast3d/src/tests/dl_plumbing.rs:11`, `fast3d/src/tests/dl_plumbing.rs:122`,
`fast3d/src/tests/culling.rs:74`).

The RDP table lacks SETPRIMDEPTH, SETCONVERT and SETKEYR/GB
(`fast3d/src/hle/rdp.rs:98`). Material state has no primitive depth, conversion or
key values, and key/K selectors are rejected or shader placeholders
(`fast3d/src/hle/combiner.rs:365`, `fast3d/src/hle/combiner.rs:54`,
`fast3d/src/render/combiner_prelude.wgsl:119`). Existing selector tests deliberately
expect KEY_CENTER to be unwired (`fast3d/src/hle/combiner.rs:871`). Setters plus
named errors for draws that consume these inputs close the reduced scope here;
they do not establish rendering support for this part of item 8.

### Item 9: public memory and diagnostics

`Rdram` exposes `Command` and `RawVertex` through a private module; neither is
re-exported by `hardware` or the crate root. Its matrix return type aliases an
array, but the documented path is internal (`fast3d/src/hle/mem.rs:23`,
`fast3d/src/hle/mem.rs:93`, `fast3d/src/hle/math.rs:4`,
`fast3d/src/hardware.rs:4`, `fast3d/src/lib.rs:7`,
`fast3d/src/lib.rs:28`). Unit tests inside the crate cannot establish external
implementability; current memory equivalence tests use those internal paths
(`fast3d/src/tests/dlmemory_equivalence.rs:533`).

Only command fetches and rectangle continuations check bounds. Vertex, matrix,
light, viewport and texture reads use infallible methods
(`fast3d/src/hle/interp.rs:119`, `fast3d/src/hle/interp.rs:164`,
`fast3d/src/hle/rsp_f3dex2.rs:26`, `fast3d/src/hle/rsp_f3dex2.rs:99`,
`fast3d/src/hle/rsp_f3dex2.rs:108`, `fast3d/src/hle/rdp.rs:214`). Image scalar reads
index slices; byte reads silently shorten at the end and panic if the start is
past it; `in_bounds` can overflow its addition
(`fast3d/src/hle/mem.rs:138`, `fast3d/src/hle/mem.rs:214`,
`fast3d/src/hle/mem.rs:231`). The facade's invalid-entry test proves only the
ordinary command-entry case (`fast3d/src/tests/renderer_process_dl.rs:67`).

Host reads are native-endian, unaligned, 16-byte commands; `w1_addr` retains all
pointer bits and Fixed/Float vertices use 16/24-byte strides
(`fast3d/src/hle/host_mem.rs:104`, `fast3d/src/hle/host_mem.rs:135`,
`fast3d/src/hle/host_mem.rs:164`). `HostRam::new` is unsafe, but its safe `Rdram`
methods accept arbitrary addresses, and `in_bounds` always returns true
(`fast3d/src/hle/host_mem.rs:45`, `fast3d/src/hle/host_mem.rs:117`). A lifetime
witness and a command-count cap cannot establish validity for such reads.

Capture wraps typed reads through byte decoding, latches errors and substitutes
placeholders because the trait is infallible
(`fast3d/src/hle/capture.rs:393`, `fast3d/src/hle/capture.rs:416`,
`fast3d/src/hle/capture.rs:520`). Missing spans, high addresses, byte orders and
source destruction have useful tests (`fast3d/src/hle/capture/tests.rs:12`,
`fast3d/src/hle/capture/tests.rs:132`, `fast3d/src/hle/capture/tests.rs:646`,
`fast3d/src/hle/capture/tests.rs:664`).

`DiagKind` is already `Copy` and non-exhaustive; severity is derived from kind
(`fast3d/src/diag.rs:12`, `fast3d/src/diag.rs:57`). Unsupported formats still print
to stderr and decode as RGBA16, including a print inside texel decoding
(`fast3d/src/hle/texdec.rs:61`, `fast3d/src/hle/tmem.rs:410`). This bypasses the
consumer sink and can manufacture plausible pixels. Severity/formatting tests
cover only the current variants (`fast3d/src/diag.rs:190`).

#### RGBA16 fallback inventory and playground impact

The encoded domain is `fmt=0..7`, `siz=0..3` (4/8/16/32 bits), from
`fast3d/src/hle/rdp.rs:178`. These are the exact fallback combinations in that
domain; the internal decoders also reject any out-of-domain values after PR 3.

| Format | `(fmt, siz)` falling back in both decoders | Additional fallback in `texdec.rs` only |
|---|---|---|
| RGBA | `(0,0)`, `(0,1)` | `(0,3)` |
| YUV | `(1,0)`, `(1,1)`, `(1,2)`, `(1,3)` | None |
| CI | `(2,2)`, `(2,3)` | None |
| IA | `(3,3)` | None |
| I | `(4,2)`, `(4,3)` | None |
| Reserved 5 | `(5,0)`, `(5,1)`, `(5,2)`, `(5,3)` | None |
| Reserved 6 | `(6,0)`, `(6,1)`, `(6,2)`, `(6,3)` | None |
| Reserved 7 | `(7,0)`, `(7,1)`, `(7,2)`, `(7,3)` | None |

That is 24 combinations in `fast3d/src/hle/texdec.rs:52` and 23 in
`fast3d/src/hle/tmem.rs:340`. The comment at `fast3d/src/hle/tmem.rs:410` is stale:
RGBA32 has a real arm at `fast3d/src/hle/tmem.rs:351`. The material path gates
`sample_tile` to its nine supported pairs, so the 23 unsupported pairs reach the
linear fallback in normal rendering, not the per-texel fallback
(`fast3d/src/hle/combiner.rs:561`, `:588`). RGBA32 reaches
the linear fallback when a multi-row LoadBlock has non-word-aligned rows and
fails that predicate. Preserve its supported TMEM route; diagnose this unsupported
load/layout case rather than disabling RGBA32 wholesale in PR 3.

n64.toys can emit every pair in the table from authored `gsDPSetTile`, using
mnemonics for formats 0..4 and numeric values for 5..7; sizes can also be numeric
(`toys:crates/asm/src/parser.rs:629`, `:641`, `:1381`,
`toys:crates/asm/src/asm.rs:1736`). Its high-level `Texture` declarations allow
only RGBA16, I4/I8, IA4/IA8/IA16 and CI4/CI8, all supported
(`toys:crates/asm/src/asm.rs:137`). A low-level render tile can reinterpret those
bytes using any table entry, even without a matching high-level texture format.
Those 23 unsupported pairs and the RGBA32 layout case are the playground-impact
list for David. Capability to emit them is not evidence that saved toys use them.
The inspected sm64 source has no YUV/32-bit texture-size tokens; its ordinary
RGBA16 setup is supported (for example `sm64:src/game/print.c:371`). Helix supplies
no authoring restriction, but the inspected game gives no reproducer for this
fallback. Recommend named errors now and a n64.toys saved-source survey before
its production pin moves.

### Items 10 and 11: workload, depth, framebuffer ownership

The facade interprets and submits each task immediately, accumulating scenes for
the frame; presentation selects an existing GPU color target without reading
guest RAM (`fast3d/src/lib.rs:399`, `fast3d/src/lib.rs:427`,
`fast3d/src/lib.rs:527`). Color persists in an address-keyed map, with replacement
on size change. Format/size-in-bytes and overlapping ranges are not part of that
store's identity (`fast3d/src/render/mod.rs:1954`,
`fast3d/src/render/mod.rs:2366`).

Depth is transient in all paired paths. A depth-clear pair clears a new texture,
then discards it; subsequent geometry allocates another texture and clears it to
1.0 (`fast3d/src/render/mod.rs:3851`, `fast3d/src/render/mod.rs:3933`,
`fast3d/src/render/mod.rs:3958`). The decal helper also allocates per pair
(`fast3d/src/render/mod.rs:3009`). The recorder calls any CIMG=ZIMG pair a depth
clear without inspecting its operations, and treats address zero as no depth
(`fast3d/src/hle/rsp.rs:780`). Rectangle bounds, scissor and fill value never reach
the temporary clear pass (`fast3d/src/render/mod.rs:3848`).

Paired decal rendering now segments operations in order, despite its stale
three-pass comment (`fast3d/src/render/mod.rs:2966`,
`fast3d/src/render/mod.rs:3043`). Pairless rendering still groups all non-decals
before decals and always clears color/depth regardless of `ClearPolicy`
(`fast3d/src/render/mod.rs:1422`, `fast3d/src/render/mod.rs:2633`). Pairless
triangles also lack the ordered scissor recording used after CIMG
(`fast3d/src/hle/rsp.rs:504`, `fast3d/src/hle/rsp.rs:799`).

The existing parity test uses one flat, untextured, depthless run; the persistence
test checks color using fill rectangles (`fast3d/src/tests/facade.rs:148`,
`fast3d/src/tests/fb_store.rs:242`). Extent tests and the authored shadow fixture
cover other useful subsets, but cannot prove shared depth between tasks or
pairs (`fast3d/src/tests/framebuffer.rs:64`,
`fast3d/src/tests/sm64_surface_fixtures.rs:406`). Single-frame capture replay
compares clear policies and primes color targets; it has no multi-frame
container or initial depth payload (`fast3d/src/hle/capture/replay.rs:245`,
`fast3d/src/hle/capture/format.rs:16`).

Framebuffer texturing is a TexRect shortcut: find a prior pair containing TIMG,
retain only its base address, then bind its entire color view. Offsets, producing
write version, and triangle reads are absent
(`fast3d/src/hle/interp.rs:231`, `fast3d/src/scene.rs:61`,
`fast3d/src/render/mod.rs:2262`). Revisiting the same color address can trigger an
assertion even if the producer was a prior pair
(`fast3d/src/render/mod.rs:2271`). The authored offscreen fixture omits a TMEM load
altogether, so it proves this shortcut, not libultra framebuffer loading
(`fast3d/src/tests/scene_builders/framebuffers.rs:85`). FullSync is a no-op and
neither memory trait has write methods (`fast3d/src/hle/rdp.rs:110`,
`fast3d/src/hle/rdp.rs:126`, `fast3d/src/hle/mem.rs:23`,
`fast3d/src/hardware.rs:27`).

### Items 12 and 13: microcodes and CPU work

Only F3D and F3DEX2 are public. Detection exposes invented fixture hashes as if
they were supported identifiers, with no specified hash algorithm
(`fast3d/src/microcode.rs:5`, `fast3d/src/microcode.rs:34`,
`fast3d/src/hle/gbi/detect.rs:15`). The detection tests assert those invented
values (`fast3d/src/microcode.rs:54`). Neither supplied consumer calls detection:
their selection sites pass explicit families (`helix:src/render.rs:217`,
`toys:crates/web/src/lib.rs:241`).

Prim/env setters dirty the whole material; rebuilding decodes textures before
comparing with the preceding material, and stores cloned pixel vectors
(`fast3d/src/hle/rdp.rs:148`, `fast3d/src/hle/rdp.rs:158`,
`fast3d/src/hle/rsp.rs:700`, `fast3d/src/hle/combiner.rs:680`). GPU cache equality
already excludes prim/env, but the cache is indexed by material position,
truncated to each scene's length, and compares entire pixel vectors
(`fast3d/src/render/mod.rs:1776`, `fast3d/src/render/mod.rs:2507`). Calling it a
content-addressed cache overstates what it does. The existing content-change
test proves changed texels rebuild, not that repeated content is reused across
material reorderings (`fast3d/src/tests/facade.rs:113`). Source/state/index/output
and uniform buffers are allocated per scene
(`fast3d/src/render/mod.rs:1642`, `fast3d/src/render/mod.rs:2606`).

## 2. Positions

Each position has a one-line reason and can be challenged independently.

- P1 — Keep `n64-gbi` the command vocabulary and n64.toys the source compiler; reason: their dependency boundary already separates production from consumption.
- P2 — Make safe `Rdram` reads fallible and exact-length; reason: an external reader must report a bad operand without panicking or inventing bytes.
- P3 — Put unregistered host-pointer walking behind an unsafe per-task entry point; reason: construction alone cannot justify every later safe read at an arbitrary address.
- P4 — Keep `Hardware` borrowed, generic and read-only, with no `Send`/`Sync` requirement; reason: both current consumers fit it and wasm gains nothing from a machine-emulator interface.
- P5 — Correct TLUT words without old-layout autodetection; reason: valid and legacy words overlap, so guessing makes legal N64 commands ambiguous.
- P6 — Implement MODIFYVTX, QUAD, CULLDL and BRANCH_Z, and diagnose the named stubs; reason: known unsupported behavior must be distinguishable from unknown opcodes.
- P7 — Use libultra CULLDL and inclusive BRANCH_Z comparison; reason: rt64 leaves the former empty and its strict comparison contradicts the SDK contract.
- P8 — Decode and snapshot primitive-depth, convert and key registers; reject draws using unwired inputs; reason: no committed game needs shader wiring, and zero placeholders would hide wrong output.
- P9 — Normalize paired and pairless input into one ordered workload now; reason: separate paths already disagree about clearing and decal order.
- P10 — Persist depth by address now, including between tasks and frames under `Persist`; reason: CIMG switches must not destroy a still-selected ZIMG.
- P11 — Ship only bounded framebuffer alias handling now; reason: sm64 evidence supports independent targets and explicit clears, not general RAM/GPU coherence.
- P12 — F3DEX is the next family, with implementation gated on the next committed game; reason: it reuses F3D state and the F3DEX2 operations completed here without introducing a sprite engine.
- P13 — Remove synthetic hashes from production detection and wait for a ROM-task consumer; reason: a port's declared family is not a microcode fingerprint.
- P14 — Measure item 13 separately, then consider shared texture content and upload reuse; reason: optimization needs a consumer baseline and must preserve every pixel.
- P15 — Preserve native-resolution semantics and the existing pairless logical extent; reason: upscaling, widescreen and extended GBI remain separate roadmap decisions.

## 3. Target contracts

### Safe memory and the native escape hatch

Re-export `Command`, `RawVertex` and `Matrix` from both `fast3d` and
`fast3d::hardware`. `Matrix` is a public alias for `[[f32; 4]; 4]`, not a new math
dependency. `Command` has public `w0: u32`, `w1: u32`, `w1_addr: u64`, with
`Clone + Copy + Debug + PartialEq + Eq`. `RawVertex` has public `pos: [f32; 3]`,
`st: [i16; 2]`, `rgba: [u8; 4]`, with `Clone + Copy + Debug + PartialEq`.
These are decoded values, not `repr(C)` layouts for casting guest memory.
`w1` always holds numeric low bits; only address operands use `w1_addr`.

Keep the `Rdram` method vocabulary, but return `Result<T, MemoryError>` from all
reads and both address-resolution methods. `read_bytes` returns
`Result<Cow<'_, [u8]>, MemoryError>` and succeeds only with exactly the requested
length. Typed readers return decoded host values using the backend's documented
layout. `vertex_stride` becomes fallible for unsupported formats; default vertex
decoding rejects Float instead of relying on a debug assertion. Use `u64`
addresses without an intermediate `usize` cast until conversion is checked.

Expose `MemoryError { address: u64, length: u64, kind: MemoryErrorKind }` with
`Clone + Copy + Debug + PartialEq + Eq`;
`MemoryErrorKind` is `Copy`, non-exhaustive, with `OutOfBounds`, `AddressOverflow`,
`UnsupportedFormat`, `Unavailable`. It carries no command PC and performs no
logging. `in_bounds` remains an advisory, overflow-safe range query; successful
queries do not make later reads infallible. Segment writes retain the existing
raw-value convention. IMAGE resolution retains intentional 32-bit segmented
semantics, rejecting inputs above `u32::MAX`; host/replay resolution preserves
full-width bases and checks addition. Document exact masking and stride rules.

The walk turns a read error into
`DiagKind::MemoryRead { access: MemoryAccess, error: MemoryError }`, at the
initiating command's PC. `MemoryAccess` distinguishes command, continuation,
vertex, matrix, viewport, light, look-at and texture/TLUT. On the first memory
failure, abort the current task and discard its recorded operations before GPU
submission. Previously completed tasks remain intact. Return `errors > 0`,
`renderable = false`, `tris = 0`; count discarded recorded draw runs in
`dropped_runs`. A faulting command is counted as attempted once; continuation
words do not become separate dispatches. Retain the existing coarse diagnostic
variants for source compatibility, but migrate memory failures to the richer
variant and update their tests.

Read compound inputs into temporary decoded values before committing state.
Check command-PC increments, source address arithmetic, count times stride,
image ranges and allocation sizes. No partial vertex load, shortened texture,
zero-padded palette, or identity matrix can turn a failed task into a successful
one. A setter of CIMG/ZIMG alone does not read memory: GPU target addresses need
not lie in the source image. This matters for authored fixtures too. Backend
implementors remain responsible for their own safe Rust behavior; the crate
does not catch panics from a consumer implementation.

`Hardware::rdram(&self) -> impl Rdram + '_` and `vi()` keep their shape. Readers
are created and consumed only inside safe `process_dl`. The renderer owns all
retained bytes before returning. `present`/`present_to` consult VI metadata and
GPU targets only. Keep `is_rdram_image` for compatibility but document it as
permission to interpret VI addresses as physical image offsets, not permission
to dereference memory during presentation. `Hardware` gains no write method.

For native ports, remove the public `Rdram` implementation from `HostRam`.
Keep a descriptor whose public surface is safe `HostRam::new(frame: &'a [u8])`
and `segments: [u64; 16]`, initially zero. Construction only stores metadata;
it no longer carries the read-safety obligation. Document the fixed native
layout: native-endian 16-byte commands with full-width address words, unaligned
access, native packed Fixed matrices and Fixed/Float vertex strides. DataFormat
remains a renderer/task choice, not another microcode or arbitrary layout knob.
Under `capture`, expose `capture_layout() -> SourceLayout` containing
`MemoryLayout::host_native()` and the initial segment table. No public pointer
read or resolve method remains; the interpreter adapter is crate-private. Add:

```text
unsafe Renderer::process_dl_host(
    ram: HostRam<'_>, entry: u64, ucode: Microcode, diags: &mut dyn DiagSink
) -> DlSummary
```

The unsafe obligation belongs to that call: every command and reachable input
span the walk will read is allocated, readable, initialized, in the declared
layout, and stable until return. Borrowed texture bytes may not be mutated
concurrently. The descriptor's witness is only a lifetime aid. Numeric CIMG/ZIMG
identities are exempt unless a command actually reads them as input. Keep
unaligned access, native packed Fixed matrices, Float vertices and texture byte
arrays exactly as defined by the current backend. A dispatch cap is a liveness
guard, never a pointer validator. A bad native pointer remains a violation of
the unsafe contract; do not promise diagnostics for it.

Provide safe `present_last()` / `present_last_to(view)` conveniences, delegating
to the same scanout implementation with no VI. Helix changes its consume site
to the unsafe host entry and uses `present_last`; its `HelixHardware` adapter is
removed. The safety comment belongs beside the blocked-guest call. Add the
equivalent unsafe `CaptureFrame::process_dl_host` and no-VI capture presentation
convenience, so recording never exposes a raw reader through safe `Hardware`.
Replay remains a safe fallible reader over owned spans, including on wasm.

The consumer changes are small at each call, although the library break is large.
Today `HelixHardware` is a unit struct, and each `rdram()` already constructs a
fresh descriptor (`helix:src/render.rs:16`, `helix-capture:src/render.rs:16`). There
is no retained reader or segment state to migrate between tasks. These are
proposed call-site excerpts; surrounding error handling stays in helix.

Main before (`helix:src/render.rs:214`, `:225`):

```rust
self.renderer.set_data_format(data_format());
self.renderer.begin_frame();
let _ = self.renderer.process_dl(&HelixHardware, data_ptr as u64, microcode(), &mut DedupLogSink);
// In present(), after the guest is released:
self.renderer.present(&HelixHardware)
```

Main after, also used by the capture branch's unselected-task path:

```rust
self.renderer.set_data_format(data_format());
self.renderer.begin_frame();
let ram = fast3d::HostRam::new(&[]);
// SAFETY: the guest remains blocked until consume_dl returns; reachable inputs stay valid.
let _ = unsafe {
    self.renderer.process_dl_host(ram, data_ptr as u64, microcode(), &mut DedupLogSink)
};
// In present(), after the guest is released:
self.renderer.present_last()
```

Capture before (`helix-capture:src/render.rs:305`, `:330`), after
`CaptureFrame::begin` has begun the selected frame:

```rust
let result = capture.process_dl(
    &mut self.renderer, &HelixHardware, data_ptr as u64,
    microcode(), data_format(), &mut DedupLogSink,
);
// In present(), after the guest is released:
capture.present(&mut self.renderer, &HelixHardware)
```

Capture after:

```rust
let ram = fast3d::HostRam::new(&[]);
// SAFETY: recording and interpretation finish while the submitting guest is blocked.
let result = unsafe {
    capture.process_dl_host(
        &mut self.renderer, ram, data_ptr as u64,
        microcode(), data_format(), &mut DedupLogSink,
    )
};
// In present(), after the guest is released:
capture.present_last(&mut self.renderer)
```

`CaptureFrame::process_dl_host` returns `Result<DlSummary, CaptureError>`, sets
the task's DataFormat, and records through the private host adapter during that
same call. On return its task owns all read spans, layout and initial segments;
`self.capture_frame = Some(capture)` retains only owned recording state. The
loop still runs `consume_dl`, `done.send(())`, `present` in that order
(`helix-capture:src/render.rs:378`). Presentation and fixture serialization may
run after release because neither reads guest memory. Capture errors follow the
existing logged-result path; they do not extend the blocked interval into file
I/O. All these types stay in fast3d. `n64-gbi` remains dependency-free, as its
empty dependency table establishes (`n64-gbi/Cargo.toml:12`).

This is a deliberate source break for host consumers. Requiring registered
regions would be sound but would require an allocation inventory helix does not
supply (`helix:src/render.rs:16`). Do not impose that inventory speculatively.
An external consumer can implement a safe registered-region reader later using
the public trait. An unsafe trait marker alone would not fix safe arbitrary
address reads; nor would marking only `HostRam::new` unsafe.

Keep `Diagnostic { at, kind }`, `DiagSink` and `DlSummary`. Add `Copy`,
non-exhaustive variants for unsupported commands, bad vertex operands, bad
command parameters, unsupported texture formats and unsupported framebuffer
access. Invalid input or omitted drawing is Error; an explicitly harmless stub
is Warn. Deduplicate unsupported-command/format reports per task and operand
kind, not per texel. `Display` is explanatory text, not a stable protocol. The
format decoder returns an error; the draw snapshot attaches the command PC and
drops the affected draw. Remove the RGBA16 fallback for unknown formats.
Gate the existing optional host probes through `log::trace!`; they are tracing,
not `DiagKind` events (`fast3d/src/hle/host_mem.rs:24`).

### TLUT vocabulary and TMEM

Use the libultra count-minus-one field in bits 14..23 and tile in bits 24..26:
four entries on tile 7 are `(0xF0000000, 0x0700C000)`. The destination is
`tiles[tile].tmem_addr * 8`, with 4 KiB TMEM wrapping. Source entries are packed
16-bit BE values, advancing two bytes each; repeat each value in all four
halfwords of its destination word. rt64's `loadWord<..., TLUT=true>` samples
only the first two source bytes (`rt64:src/hle/rt64_rdp.cpp:370`): the `0x1` mask
repeats them across its eight destination bytes (`:393`). These follow
`sm64:include/PR/gbi.h:3456`, `sm64:include/PR/gbi.h:4257` and
`rt64:src/hle/rt64_rdp.cpp:549`.

Change `gdp_load_tlut(tile, count_minus_one)` to that contract, document the
semantic break, and migrate every call in the same PR. Include literal counts
0, 2, 15, 255 and 1023; the field describes 1..1024 transfers, not a clamped
256-entry palette. Standard CI4 helpers set destination `0x100 + 16*palette`;
CI8 uses `0x100`. The loaded range alone changes. TMEM replication also matters
when later non-CI loads or reads alias those bytes.

Initial support is the canonical single-row libultra command. Nonzero rectangle
origin/row fields get `UnsupportedCommandParameters`, with no TMEM mutation;
rt64's general multi-row TLUT operation is broader
(`rt64:src/hle/rt64_rdp.cpp:549`). Do not reinterpret the old low-bit count as a
compatibility format. Require an explicit n64.toys compiler migration for
generated TLUTs, including its missing `SetTile`. High-level saved CI source
should survive recompilation; raw words or custom low-level macros need a
source survey and versioned migration in n64.toys. The existence of affected
saved toys is unknown.

### F3DEX2 control and additional RDP state

| Operation | Target and reference | Deliberate boundary |
|---|---|---|
| MODIFYVTX `0x02` | Decode index bits 1..15 and attribute bits 16..23; reuse RGBA/ST/XY/Z logic and copy-on-use (`rt64:src/gbi/rt64_gbi_f3dex.cpp:19`, `rt64:src/hle/rt64_rsp.cpp:737`). | Track loaded cache slots explicitly; invalid indices/attributes diagnose. Preserve earlier modifications in copies, as the current fast3d implementation already does (`fast3d/src/hle/rsp.rs:405`). |
| QUAD `0x07` | Execute the two encoded triangles exactly like TRI2 (`rt64:src/gbi/rt64_gbi_f3dex2.cpp:147`). | Do not decode it using F3D's four-corner layout. |
| CULLDL `0x03` | Decode doubled first/last indices, inclusive. AND the loaded vertices' clip codes; a common outside plane acts as ENDDL (`sm64:include/PR/gbi.h:2233`). | rt64's handler is empty (`rt64:src/gbi/rt64_gbi_f3dex.cpp:41`); use the SDK semantics and independent control-flow assertions. |
| BRANCH_Z `0x04` | RDPHALF_1 latches the full address operand; compare the chosen vertex's screen Z with unsigned 16.16 threshold and branch without pushing (`sm64:include/PR/gbi.h:2433`, `rt64:src/hle/rt64_rsp.cpp:837`). | P7 stands: both SDK comments say less than or equal (`sm64:include/PR/gbi.h:2380`, `:2426`, with the raw wording at `:2427`). rt64 computes screen Z at `rt64:src/hle/rt64_rsp.cpp:842` and uses `<` at `:844`. Use `<=`; no forced-branch enhancement. |
| SPNOOP `0xE0` | Harmless no-op (`rt64:src/gbi/rt64_gbi_f3dex2.cpp:183`). | No extended-GBI hook or diagnostic for an ordinary no-op. |
| LINE3D `0x08` | Recognized unsupported draw; Error and one dropped draw (`rt64:src/gbi/rt64_gbi_f3dex2.cpp:156`). | No line rasterizer in this milestone. |
| DMA_IO `0xD6` | Recognized stub, Warn; do not dereference or write operands (`rt64:src/gbi/rt64_gbi_f3dex2.cpp:119`). | Does not emulate DMEM transfers. |
| SPECIAL `0xD3..0xD5` | Recognized unsupported state operations, Error and task rejection, preserving raw operands (`sm64:include/PR/gbi.h:117`). | rt64 implements a flagged SPECIAL_1 matrix case (`rt64:src/gbi/rt64_gbi_f3dex2.cpp:123`); silently continuing could use stale transforms. No such variant is selected here. |
| LOAD_UCODE `0xDD` | Recognize the command and halfword data address, report unsupported microcode load. | Abort and reject this task rather than decode the following stream with a stale family. rt64 actually loads a GBI (`rt64:src/gbi/rt64_gbi_f3dex.cpp:49`); hashes/runtime switching wait for a ROM consumer. |

Use an internal control result (`Continue`, `Call`, `Branch`, `Return`, `Abort`)
so structural operations share PC arithmetic, return-stack limits and command
accounting. RDPHALF_1 becomes persistent per-task state; TexRect consumes its own
continuations without spurious stray diagnostics. Reset the latch at task
start. A branch or load without a latch is invalid, not address zero by default.
Keep the full host address in the latch; numeric thresholds stay in `w1`.

CULLDL uses clip codes computed at vertex load, independent of face-culling
bits and `gSPClipRatio`; later MODIFYVTX does not regenerate those load-time
codes. Use homogeneous plane tests against the projection volume and retain a
vertex on a boundary. Bad ranges/unloaded slots diagnose and abort the task.
The SDK describes this as a cached-code test and local display-list return
([Nintendo gSPCullDisplayList](https://ultra64.ca/files/documentation/online-manuals/man-v5-1/n64man/gsp/gSPCullDisplayList.htm)).

BRANCH_Z requires CPU position evaluation using the vertex-load matrix and
viewport, not an asynchronous compute readback. Calculate raw screen Z before
the GPU's clip-depth fold; retain that value with each cache slot. ZSCREEN
modification replaces it with `value / 65536`. Test below, equal, above, changed
viewport after load, and modified Z. rt64 calculates CPU screen positions and
uses `DepthRange=1024` to undo its normalization
(`rt64:src/hle/rt64_rsp.cpp:23`, `rt64:src/hle/rt64_rsp.cpp:714`,
`rt64:src/hle/rt64_rsp.cpp:842`); fast3d need not store the intermediate normalized
value. Invalid/non-finite transforms diagnose instead of choosing a branch.
The threshold-equality and modified-Z cases need SDK-derived assertions where
rt64's result disagrees.

SETPRIMDEPTH stores raw `z: u16`, `dz: u16` and snapshots them on every triangle
and TexRect draw. A draw selecting `G_ZS_PRIM` emits
`DiagKind::UnsupportedPrimitiveDepthSource` at its command PC and is dropped.
It must not render with pixel Z or zero depth. Setting unused primitive-depth
registers is harmless: it leaves `DlSummary` counts and renderability unchanged.
Pixel-Z draws retain their current rendering. Shader wiring, normalization and
decal tolerance under primitive Z wait for a committed game reproducer; rt64's
implementation alone is insufficient scope evidence.

SETCONVERT stores six signed nine-bit coefficients; SETKEYR/GB store centers,
scales and widths without losing the untouched channels. Convert packing comes
from `sm64:include/PR/gbi.h:4470`; key packing is decoded at
`rt64:src/gbi/rt64_gbi_rdp.cpp:154` and `:161`. Snapshot
all registers per draw, including rectangles; a later setter cannot mutate an
earlier snapshot. Reject draws selecting KEY_CENTER or KEY_SCALE in an active
cycle with `UnsupportedKeyInput { selector }`, and K4 or K5 with
`UnsupportedConvertInput { selector }`. These named `DiagKind` variants and
`UnsupportedPrimitiveDepthSource` have Error severity, increment the existing
error/dropped-run accounting, and prevent the affected draw from reaching any
shader path. No `DlSummary` fields change. Inactive-cycle selectors do not
reject otherwise supported draws; unused setters add no warnings, errors or
dropped runs. Shader slots remain unwired and cannot be accepted as zeros.

One explicit reference disagreement: rt64 extracts unsigned nine-bit patterns
and passes them through to the shader (`rt64:src/gbi/rt64_gbi_rdp.cpp:144`,
`rt64:src/hle/rt64_rdp.cpp:1003`). Use signed values per Nintendo's documented
range -256..255; test literal negative register decoding independently of rt64
([Nintendo gDPSetConvert](https://ultra64.ca/files/documentation/online-manuals/man-v5-1/n64man/gdp/gDPSetConvert.htm)).
YUV conversion and full chroma-keying also remain unsupported. Preserve K0..K3
and widths; reject attempted conversion/key-enable draws with named
`UnsupportedTextureConversion` / `UnsupportedChromaKey` Errors. rt64 ignores key widths
(`rt64:src/hle/rt64_rdp.cpp:1013`). Setting unused registers alone is harmless.

### One workload and persistent depth

Normalize every interpreted scene into ordered operations carrying target,
scissor and draw state. A pairless scene gets an internal `Legacy` target and
logical extent 320x240; its existing output allocation/presentation scaling is
preserved (`fast3d/src/render/mod.rs:1553`). Guest address zero is an ordinary
address, not this sentinel. Triangles before the first explicit CIMG remain
legacy draws; once explicit targets start,
continue in stream order without losing the earlier operations. Snapshot
scissor for legacy draws too. Rectangles retain their explicit CIMG requirement
(`fast3d/src/hle/interp.rs:210`, `fast3d/src/hle/interp.rs:304`). Do not expose `Scene`
or the operation enum as a public serialization contract.

Land workload normalization with the existing execution paths still available
and pixel-identical, then replace those paths in a second bisectable commit
within PR 6. `render/mod.rs` is nearly 4000 lines; the pairless path renders
n64.toys (`fast3d/src/render/mod.rs:1422`, `toys:crates/web/src/lib.rs:241`). Require
every existing golden to remain pixel-identical except PR 6's two named semantic
classes, whose masks and independent expectations are specified in section 4.

One executor handles store rendering and internal test rendering. It segments
only where attachment use demands it: depth-writing draws, sampled-depth decal
draws, clears and texture reads. Each segment sees all prior writes and no later
writes. A decal-first segment initializes an uninitialized target before
sampling; alternating opaque/decal/rect operations never move across one
another. Retain existing filtering, dither seed and logical-coordinate policies.

Use separate color and depth stores with full `u64` identities and explicit
guest format/stride metadata. Depth selected by `Some(address)` survives CIMG
switches, separate `process_dl` calls and frame boundaries under `Persist`.
`PerFrame` initializes each color/depth address once per frame; explicit clears
always execute. New storage starts with background color/far depth. Color
resize does not reset another address's depth. At a fixed stride, grow storage
height preserving old rows and initializing new rows; a smaller scissor does
not shrink the target. A stride/format reinterpretation ends that generation
and reports the unsupported preservation requirement until item 11 supplies
conversion. Do not preallocate all occurrences of an address using its final
extent before executing earlier operations.

Recognize a depth fill at the operation level: CIMG aliases the selected ZIMG,
the cycle is Fill, and the destination is a supported 16-bit depth layout.
Intersect inclusive fill bounds with the current scissor and target bounds.
Decode the fill word's two packed Z halves, respecting pixel parity, and write
only that region to the stored depth attachment. The sm64 far-clear value is
`GPACK_ZDZ(0x3fff, 0)` in each half
(`sm64:include/PR/gbi.h:296`, `sm64:src/game/game_init.c:145`). Preserve the N64
packed-depth interpretation for non-far fills using rt64's exponent/mantissa
conversion (`rt64:src/shaders/Depth.hlsli:44`); do not treat arbitrary values as
far clears. A whole target of uniform far depth can use a load clear as an
optimization. Rectangular or scissored clears need a depth-writing draw.
CIMG=ZIMG with other operations
must diagnose an unsupported alias, not silently clear the attachment.

rt64 separates target identity from attachment pairing, tracks depth by its own
address, and transfers previous write types when needed
(`rt64:src/hle/rt64_state.cpp:1213`, `rt64:src/hle/rt64_state.cpp:1224`,
`rt64:src/hle/rt64_state.cpp:1399`). Adopt those observable ordering/identity
semantics without its worker queues or general RAM-difference machinery.

Ship this depth model now. sm64's explicit far clear and shared ZIMG justify the
scope, but do not prove that it needs prior-frame depth. Close authored acceptance
with A/Z then B/Z then A/Z draws, two tasks without `begin_frame`, two frames
under both policies, two independent Z addresses, partial clears and a
decal-first case. Close the live sm64 claim only with a recorded frame/sequence
whose output changes when depth is incorrectly reset between its dependent
operations. An authored fixture closes authored semantics only. Until the live
experiment is run, the sm64 gate is open. A completed experiment finding no
dependency closes PR 8's live gate as `closed-by-absence` for the recorded
scenes/boundaries, with cross-frame need unclaimed. Missing captures, failed
replay or incomplete scene coverage are still open, not absence evidence.

Add a versioned sequence container around existing frame fixtures, beginning at
renderer reset and replaying all tasks/frames through one renderer. Keep v1
single-frame decoding. Do not weaken the self-contained single-frame admission
rule to admit a frame that needs warm-up. Extend its contamination check to
initialize depth to distinct near/far values, as well as color. Sequences record
their warm-up explicitly and compare after the same initialization, rather than
requiring `PerFrame` and `Persist` to match when persistence is their subject.

#### Live sm64 depth experiment for Claude

Use helix `81e9a70` / PR #33 with the matching fast3d capture pin, then the PR 1
companion for the final acceptance run. Build the oxideports sm64 revision in
section 1 against that helix worktree, using its devenv. Record game/build flags,
Fixed/Float choice, input route, adapter/backend, extent, revisions and capture
seed. Use a fresh output directory; the hook refuses to overwrite files
(`helix-capture:src/render.rs:193`). The current hook selects zero-based
`consume_dl` serials, one task per renderer frame; these are not VI frame numbers
(`helix-capture:src/render.rs:284`). At the fast3d code revision cited here,
`CaptureFrame::begin` records renderer serials starting at one, so selection 120
writes `frame-000121.f3dcap` (`fast3d/src/hle/capture/replay.rs:17`). Preserve that
mapping and the dither seed during replay; do not renumber a cropped sequence.

1. Capture a continuous startup-to-gameplay run, selecting every serial 0..3599.
   From the built sm64 worktree, the hook can be invoked as follows (the executable
   path is the US target in `sm64:devenv.nix:108`):

   ```sh
   FAST3D_CAPTURE_DIR=/Users/ci/hub/scratch/fast3d/depth-live-01 \
   FAST3D_CAPTURE_FRAMES="$(seq -s, 0 3599)" \
   FAST3D_CAPTURE_REVISION=4a9dcf0d0a82a637b19b401f969639c9f4e0c83a \
   FAST3D_CAPTURE_SYMBOLS='game_init.c:init_z_buffer,select_framebuffer; castle route' \
     devenv shell -- ./build-cmake/sm64-us
   ```

   Include boot/title, file selection, courtyard with Mario's shadow, entry into
   the castle, then a painting/level transition. Annotate the actual serials at
   each scene; no fixed serial is presumed to reach a particular room. If 3600
   tasks do not cover the route, repeat with a larger contiguous selection in a
   fresh directory. Retain all warm-up tasks. Inspect 120 consecutive tasks of
   courtyard gameplay, 120 indoors, and 30 before/after each transition, as well
   as every CIMG switch and Z fill in the full sequence. This spans the three
   color allocations sharing Z (`sm64:src/game/game_init.c:639`).
2. In PR 8's sequence replay harness, run the exact same recorded tasks from
   reset under the PR 7 depth store with `Persist`. Render lossless offscreen
   color PNGs/RGBA bytes at each selected presentation and retain diagnostic and
   CIMG/ZIMG/fill/scissor command logs. The sequence harness and diagnostic reset
   switches are PR 8 work, not commands available at this design revision.
3. Replay three controlled variants: discard only depth at each CIMG switch,
   at each task boundary, and at each frame boundary. Keep color, explicit fills,
   order, seed and presentation identical. Compare each variant to the persistent
   run, not to a different live playthrough. The current hook makes task and
   frame boundaries coincide; report that limitation instead of claiming two
   independent live proofs. Authored multi-task fixtures cover the distinction.
4. Diff exact color bytes and depth probes before/after candidate dependent
   operations. Report hashes, changed-pixel counts/bounds, masks, and the
   writer/read command PCs with the shared Z address. A depth-storage difference
   without a color difference does not establish a visible sm64 dependency.
   Minimize any visible difference to the necessary reset-to-output prefix and
   have the PNG/mask reviewed under the golden rule. HOST64 replay is semantic
   evidence; use rt64 only for separately authored/exportable IMAGE equivalents.
5. If all controlled variants have zero visible delta, retain their zero-diff
   results and show from the command logs that each candidate prior depth write
   is cleared or unused before observation. Mark the live gate `closed-by-absence`
   for this route and these boundaries. If coverage or replay is incomplete,
   record precisely what is missing and leave it open. Do not require a positive
   sm64 dependency forever; do not claim universal absence from this bounded run.

### Framebuffer manager: bounded now, general later

Now: centralize target descriptors, checked ranges and source validation with
the workload. Preserve the existing exact-base, prior-color TexRect shortcut as
a documented fast3d convenience. Give it an explicit source generation/extent;
validate matching format and row stride, and diagnose same-target feedback,
interior offsets, overlap/reinterpretation and missing sources instead of
asserting or sampling the wrong pixels. Detect reads that overlap known GPU
targets before attempting a guest texture read. Unsupported accesses reject
the dependent draw; an actual memory-read failure rejects the task as above.
Carry command PCs into operations so these errors reach the original DiagSink.
Collect preparation diagnostics before calculating `DlSummary`.

The convenience does not establish libultra framebuffer-load semantics: an
equivalent oracle fixture must issue a real LoadTile/LoadBlock. Do not export the
current load-free shortcut and claim rt64 agrees. Keep its existing golden as a
compatibility test while adding assertions that reject unsupported aliases.

Defer general range ownership, offset copies, triangle framebuffer textures,
same-target snapshots, format reinterpretation, CPU writes and GPU write-back.
rt64's references for that work are:

| Behavior | Semantic reference | Evidence required to start/close the deferred work |
|---|---|---|
| Range ownership | Track dimensions/byte ranges and last writer (`rt64:src/hle/rt64_framebuffer_manager.cpp:33`, `rt64:src/hle/rt64_framebuffer_manager.cpp:64`, `rt64:src/hle/rt64_framebuffer_manager.cpp:905`). | A second-game trace with overlapping views. Close with byte-range ownership tests, including partial overlaps and newer writers that cover only part of a request. |
| Region copies and triangle reads | Derive source row/column from byte offset; associate TMEM regions with load operations (`rt64:src/hle/rt64_framebuffer_manager.cpp:390`, `rt64:src/hle/rt64_framebuffer_manager.cpp:517`, `rt64:src/hle/rt64_state.cpp:362`). | A captured load from a rendered target followed by TexRect/triangles. Close with offsets, strides, intervening writes and both draw kinds; IMAGE fixture against rt64. |
| Reinterpretation | Convert the represented guest bits for the source/destination fmt/siz (`rt64:src/hle/rt64_framebuffer_manager.cpp:247`, `rt64:src/hle/rt64_framebuffer_manager.cpp:310`). | A named game's actual format pair. Close that pair with literal packed-bit expectations, TLUT mutation if relevant, and oracle pixels. No universal conversion matrix by default. |
| CPU/GPU coherence | RAM hashes detect edits, and changed-pixel masks update targets (`rt64:src/hle/rt64_framebuffer_manager.cpp:828`, `rt64:src/hle/rt64_framebuffer_changes.cpp:36`). | A consumer demonstrates CPU edits or reads of rendered bytes. Close with interleaved CPU/GPU writes preserving untouched pixels. Do not scan native process memory looking for changes. |
| FullSync write-back | Flush ordered work, wait, convert and copy target rows to RAM (`rt64:src/hle/rt64_state.cpp:792`, `rt64:src/hle/rt64_state.cpp:1457`). | A ROM consumer supplies its synchronization boundary. Close with bytes visible before its DP completion event and an async browser completion test. |

For deferred write-back, use a separate opt-in API yielding owned address-tagged
byte patches after GPU completion. The consumer applies them to writable guest
memory before reporting DP completion. Do not change `Hardware::rdram` to a
mutable global buffer, retain guest borrows through an async readback, or make
ordinary `present` block on RAM conversion. This API needs David's consumer
choice before its signatures are frozen. No evidence inspected here establishes
that sm64 needs it; the observed clear/scanout code above only establishes the
smaller requirement.

### Next family and CPU work

F3DEX is next. Its different vertex/triangle packing can reuse the completed
modify/branch operations, F3D matrices and move commands
(`rt64:src/gbi/rt64_gbi_f3dex.cpp:15`, `rt64:src/gbi/rt64_gbi_f3dex.cpp:53`). Do
not implement F3DZEX2 or S2DEX in parallel. Start F3DEX only when David commits a
game/build needing it, with a declared family and at least one captured task.
The sm64 build option alone is insufficient because its host declaration is
currently ambiguous, as noted above. Choosing the second game remains David's
decision, not a guess that Zelda or a sprite title is next.
`Microcode` gets no new variant until D1. Helix's FFI table remains
0=F3DEX2, 1=F3D throughout this delivery (`helix:src/render.rs:92`,
`helix:include/helix/runtime.h:26`); P13's detection cleanup does not change it.

Keep explicit family selection for ports and web. Remove the invented hashes
from production; `detect_microcode(u64)` returns `None` until a real table is
backed by a consumer. Test the lookup mechanism using test-only records. For
the first ROM consumer, define the hashed text/data byte ranges, byte order,
algorithm and revision together; store provenance (game, region, revision and
microcode variant), and verify a task selects the intended table. Unknown
hashes must not default to F3DEX2. Do not hash `GBI_FLOATS`, which is a port data
layout, or copy rt64's database without adopting and verifying its hash recipe.
LOAD_UCODE becomes functional only in that follow-up, including reset semantics
and capture serialization of family changes.

Item 13 is a separate evidence-gated set, outside the initial delivery. Before
either PR 11 or PR 12 starts, record decode count, GPU upload count and CPU ms
for one captured sm64 frame (with any warm-up retained) and one n64.toys animated
source scene with fixed textures and sampled times. Separate assembler time
from fast3d interpretation/preparation/upload time; a Rust scene is not a
substitute for the playground workload. Record cold and steady-state results,
revision/build, adapter/backend, resolution and timing method. No numbers have
been measured in this design round. Baseline instrumentation may start the set;
optimization may not start without those numbers and an identified cost.

If measurement justifies PR 11, split draw values from immutable decoded texture
images and sampling descriptors. Intern images by content within a renderer,
using a bounded hash table with exact-byte comparison on collisions. Decode identity
includes TMEM content, palette content/format, tile fmt/siz/base/line and the
sampling footprint; prim/env, fog, primitive depth and combiner constants do
not belong in that identity. Whole-TMEM snapshots are an acceptable first key
(4 KiB, bounded); optimize touched subranges only after measurement. Never use
the guest pointer as content identity. Own the snapshot before the guest resumes.

GPU image identity includes decoded bytes and allocation extent. Sampling/bind
group identity includes descriptors, all physical texture roles, independent
LOD levels and detail images; changing a descriptor need not upload identical
pixels. Use renderer-owned shared images in retained scenes. Bound cache memory,
evict only entries not referenced by retained/in-flight work, and clear device
objects on reconfigure. rt64 also separates TMEM-derived texture identity from
draw state and evicts old entries
(`rt64:src/hle/rt64_state.cpp:644`, `rt64:src/hle/rt64_state.cpp:1588`); its global
texture manager is not required here.

If the same baseline justifies PR 12, reuse growable upload buffers for
source/state tables, indices, uniforms, rectangle vertices and compute outputs
per extent. Allocate disjoint regions for every task still referenced by queued
commands; never overwrite a buffer
range while a prior draw needs it. Reuse CPU scratch after submission where
ownership allows it. Validate allocation/upload/decode counts on fixed workloads
and report timings; do not promise an unmeasured frame-rate gain. Compiling source
once and updating time-dependent operands stays in n64.toys, using `n64-gbi`.

## 4. Ordered PR-sized work

Sizes here are S=1–3 working days, M=4–8, L=9–15. Estimates include fixtures and
one review/revision cycle, excluding time waiting for David's captures and
Claude's GPU capacity. Each semantic test must fail on the preceding revision
and pass with the fix. All tests named below are proposed unless section 1
identifies them as existing. `hle/`, `render/` and `tests/` below mean
`fast3d/src/hle/`, `fast3d/src/render/` and `fast3d/src/tests/`.

Delivery order is PR 2, 3, 4, 5, then PR 1 with its consumer companions, then
PRs 6, 7, 8, then the reduced PRs 9 and 10. The numbering below is kept from the
first draft so review comments stay addressable; the order is in "Dependencies
and verification gate".

### PR 1 — External memory API and sound host entry. M, 6–8 days

Files: `fast3d/src/{lib,hardware,diag}.rs`, `hle/{mem,host_mem,interp,rsp,rdp}.rs`,
both RSP dispatch modules, `hle/capture.rs`, `hle/capture/replay.rs`, `README.md`;
new external integration test `fast3d/tests/memory_contract.rs` and updates to
existing memory/capture tests. A companion helix change replaces its adapter
and call sites; n64.toys' `WebHardware` loop keeps its shape.

Tests: `external_rdram_uses_only_public_types`, `image_read_exact_or_error`,
`image_operand_bounds_all_read_kinds`, `image_address_overflow`,
`unsupported_float_image_is_diagnostic`, `failed_task_submits_no_operations`,
`host_entry_preserves_high_pointer_and_numeric_words`,
`host_present_never_reads_guest`, and capture/replay missing-span equivalence.
Use Rust-authored malformed commands, native Fixed/Float graphs, and the
existing high-address wasm capture. Compile-fail docs demonstrate that HostRam
cannot be used as a safe Rdram reader or walked without an unsafe block.

Acceptance: an external crate can implement every signature without naming HLE;
all safe supplied backends report exact spans, no memory-failed task reaches GPU
submission, and helix releases the guest at the same boundary. No invalid-pointer
test deliberately executes UB. Golden impact: none for valid input; malformed
fixtures assert errors, not new images. Land after PR 5, when David takes the
memory API break, with the helix main/capture and n64.toys companions ready.
PRs 2–5 use the existing memory API.

### PR 2 — Libultra TLUT encoder and destination. S–M, 3–4 days

Ships first; independent of PR 1. `tlut_truncated_source_rejects_task` is written
against today's `in_bounds` and re-expressed as a `MemoryRead` diagnostic in PR 1.
Files: `n64-gbi/src/encode.rs`, `n64-gbi/tests/conformance.rs`,
`hle/{rdp,tmem}.rs`, `tests/{scene_builders,decode,gbi_roundtrip}.rs`, CI palette
fixtures and new IMAGE oracle writers. Coordinate the relocated n64.toys
compiler's encoder pin and palette SetTile migration in a companion PR. Its
`crates/asm/src/asm.rs`, compatibility corpus and source-map tests must change
together; command insertion must still map every generated word to its source.

Tests: `tlut_words_match_libultra`, `tlut_count_and_destination_roundtrip`,
`tlut_ci4_bank15_partial_update`, `tlut_ci8_256_entries`,
`tlut_rgba16_and_ia16`, `tlut_replicates_four_halfwords`,
`tlut_wraps_tmem_without_clamping_count`, `tlut_legacy_row_fields_diagnosed`,
`tlut_truncated_source_rejects_task`. Literals distinguish bits 14..23 from the
old encoding and check zero/maximum count fields.

Acceptance: ordinary libultra palette macros work, updates preserve other banks,
and two independent CI palettes render correctly in one list on rt64 and fast3d.
Golden impact: migrate authored setup as part of `LibultraTlutLayout`; normal
CI0 pixels should stay identical after equivalent setup, with changed command
hashes recorded. Existing pixels that genuinely change need the full golden
rule. Do not preserve the old encoding to keep a fixture green.

### PR 3 — Structured unsupported behavior and opcode inventory. S, 2–3 days

Independent of PR 1; the memory-read diagnostic variants land with PR 1.
Files: `fast3d/src/diag.rs`, `hle/{tmem,texdec,combiner}.rs`,
`hle/{interp,rsp_f3dex2}.rs`, `n64-gbi/src/{consts,encode}.rs`, conformance and
`tests/gbi_roundtrip.rs`. Include SPNOOP, LINE3D, DMA_IO, SPECIAL and the aborting
LOAD_UCODE stub; move synthetic detection records under tests in
`hle/gbi/detect.rs` and correct the public detection docs.

Tests: `f3dex2_known_stub_inventory`, `spnoop_e0_is_silent`,
`load_ucode_rejects_following_stream`, `unsupported_format_emits_once_at_draw`,
`unsupported_format_never_decodes_rgba16`, `diagnostic_severity_and_rollup`,
`special_rejects_task_without_reading_operands`,
`unsupported_move_state_rejects_task`, `production_detection_rejects_fixture_hashes`.
Add literal opcode words in
`n64-gbi` independently of dispatcher constants.

Acceptance: no texture-format stderr path, no unknown-opcode report for these
known commands, no false successful draw for LINE3D or a stream after unsupported
state/microcode changes. Retain the public unhandled-move variants but classify
omitted state changes as Error and reject their task.
Golden impact: none for supported fixtures; any fixture depending on the old
fallback becomes an explicit error test under `UnsupportedTextureFormat`.
No rt64 pixel oracle for stubs.

### PR 4 — MODIFYVTX and F3DEX2 QUAD. S, 2–3 days

Depends on PR 3. Files: `hle/{rsp,rsp_f3dex2}.rs`, `n64-gbi/src/{consts,encode}.rs`,
conformance, `tests/{gbi_roundtrip,rsp_tests}.rs`, new
`tests/f3dex2_fixtures.rs` using `DlBuilder`.

Tests: `modifyvtx_four_attributes`, `modifyvtx_used_vertex_is_copied`,
`modifyvtx_preserves_previous_screen_changes`, `modifyvtx_invalid_slot_or_attr`,
`quad_is_encoded_tri2`, `modify_rgba_disables_fog_and_lighting`,
`modify_st_disables_texgen`. Test successive draws before/after each change and
an invalid unloaded slot after a different slot has been loaded.

Acceptance: literal SDK words, scene-state assertions and IMAGE oracle fixtures
for visible RGBA/ST/XY/Z changes and QUAD winding. Golden impact: existing valid
F3D images unchanged; new F3DEX2 images pin the newly supported operations.

### PR 5 — Conditional display-list control. M, 4–6 days

Depends on PR 4. Files: `hle/{interp,rsp,rsp_f3dex2,rdp}.rs`,
`n64-gbi/src/{consts,encode}.rs`, conformance, `tests/dl_plumbing.rs`,
`tests/f3dex2_fixtures.rs`, capture typed-read coverage.

Tests: `culldl_each_plane_and_boundary`, `culldl_mixed_planes_not_culled`,
`culldl_nested_returns_to_parent`, `culldl_uses_inclusive_range`,
`culldl_ignores_face_mode_and_clipratio`, `branchz_below_equal_above`,
`branchz_uses_load_viewport_and_modified_z`, `branchz_half1_preserves_host_address`,
`half1_then_texrect_then_branch`, `conditional_loop_hits_dispatch_cap`,
`conditional_invalid_vertex_rejects_task`. Test entry-level return and segmented
targets as well as nested calls.

Acceptance: no GPU synchronization for branch decisions; only reached commands
are recorded/read. Oracle BRANCH_Z fixtures avoid equality and known modified-Z
normalization disagreement; equality and CULLDL use the SDK-derived expected
command sequence. Golden impact: existing valid straight-line lists unchanged;
`F3dex2ConditionalControl` explains any previously ignored branch/cull image.

### PR 6 — One ordered render workload. M, 5–7 days

Depends on PRs 1 and 3. Files: `fast3d/src/scene.rs`, `hle/{interp,rsp}.rs`,
`render/mod.rs`, new internal `render/workload.rs`,
`tests/{facade,framebuffer,fb_store}.rs` and Rust scene builders. Normalize legacy
input, preserve command PCs, and route all execution through the ordered path.

Tests: `paired_pairless_clear_policy_equivalence`,
`paired_pairless_scissor_equivalence`, `opaque_decal_rect_order_is_preserved`,
`legacy_draws_before_cimg_are_retained`, `decal_first_initializes_depth`,
`logical_extent_survives_canvas_resize`. Exercise both clear policies at matching
logical/output extents, then preserve existing pairless resize behavior.

Acceptance: two bisectable commits. First normalize the workload while keeping
the existing execution paths and every existing golden pixel-identical. Then
replace those paths with one executor; alternating draws see only earlier
depth, scissor and color writes. Keep the existing shadow and rectangle oracles;
add a paired IMAGE interleaving scene with an independent overlap mask.

Golden impact in the second commit is limited to two classes:

- `OrderedWorkload`: pixels affected by the old decal bucketing, lost per-draw
  scissor, or discarded legacy draws before CIMG. Derive expected coverage and
  draw order from the command stream; bound the mask to the affected draws and
  their overlaps or scissor differences.
- `PairlessClearPolicy`: pixels whose prior contents were erased by an implicit
  pairless clear under `Persist`, or a repeated clear within a `PerFrame` frame.
  Use a known prior color/depth pattern and explicit draw coverage to derive
  which pixels should survive.

Name each affected existing golden before updating it, with the hashes, exact
mask and independent expectations required below. Every other golden, including
ordinary single-run and sm64 opaque-then-decal fixtures, stays pixel-identical.
No general framebuffer copies yet.

### PR 7 — Persistent Z images and real depth fills. M, 5–8 days

Depends on PR 6. Files: `render/{mod,workload}.rs`, new
`render/framebuffers.rs`, `hle/{rdp,rsp,interp}.rs`, `fast3d/src/scene.rs`,
new `tests/depth_persistence.rs`, `tests/scene_builders/framebuffers.rs`.

Tests: `depth_shared_across_color_switches`, `depth_survives_task_boundary`,
`depth_persist_vs_perframe`, `depth_addresses_are_independent`,
`depth_address_zero_is_valid`, `depth_clear_hits_selected_storage`,
`depth_clear_scissor_and_fill_parity`, `depth_height_growth_preserves_rows`,
`depth_stride_change_is_explicit`, `depth_alias_nonfill_is_diagnostic`.

Acceptance: clear-before-draw, clear-after-draw and partial non-far clears change
exactly the intended depth samples. A shared-Z multi-target IMAGE fixture agrees
with rt64's occlusion output; direct depth probes use independently decoded
packed values. Golden impact: `PersistentDepthByAddress` and
`DepthFillTargetsStorage`, with masks restricted to newly correct occlusion or
clear regions. This closes authored depth semantics, not live sm64 acceptance.

### PR 8 — Recorded workload acceptance and bounded framebuffer aliases. M, 4–6 days

Depends on PR 7. Files: `hle/capture{,/format,/replay}.rs`,
`render/{workload,framebuffers}.rs`, `hle/{interp,rdp}.rs`,
`tests/{sm64_corpus,browser_fixtures}.rs`, new
`tests/framebuffer_aliases.rs`, oracle exporter/runner if sequence execution
needs it. Companion helix hook records inside the blocked interval.

Tests: `capture_sequence_replays_from_reset`,
`single_frame_rejects_prior_depth_dependency`,
`v1_capture_still_replays`, `exact_base_fb_alias_keeps_compatibility`,
`fb_alias_interior_or_same_target_is_diagnostic`,
`fb_texture_load_overlap_never_reads_stale_ram`,
`fb_format_change_does_not_reuse_old_view`, `framebuffer_range_overflow`.

Acceptance: a recorded sequence retains depth through the facade, while standalone
fixtures prove initialization of color and depth. Existing shortcut pixels stay
stable and unsupported aliases report the originating PC without assertions.
Run section 3's live sm64 experiment. Close the live gate with either the
reset-induced difference mask, dependent commands and a reviewed PNG, or the
complete zero-diff results and command evidence for `closed-by-absence` on the
recorded route. Missing captures, failed replay or incomplete coverage leave it
open; authored acceptance alone does not close it. For exportable sequences,
run identical task order through rt64; HOST64 recordings remain replay/semantic
evidence, not rt64 inputs. Golden impact:
`BoundedFramebufferAlias` only for formerly incorrect aliases; sequence images
are new, provenance-labelled evidence.

### PR 9 — Primitive depth registers. S, 1–2 days

Depends on PR 3. Files: `hle/{rdp,rsp}.rs`, `fast3d/src/scene.rs`, `fast3d/src/diag.rs`,
`n64-gbi` constants/encoder/conformance, new `tests/prim_depth.rs`. No shader
changes.

Tests: `primdepth_words_and_roundtrip`, `primdepth_snapshot_per_draw`,
`primdepth_setter_alone_is_silent`, `primdepth_source_draw_is_rejected`
(triangle and TexRect, both cycle types, `UnsupportedPrimitiveDepthSource` at
the draw's PC, `errors` and `dropped_runs` incremented, no pixels written),
`pixel_z_draws_unchanged`.

Acceptance: registers decode and snapshot per draw; a draw under `G_ZS_PRIM` is
rejected with the named error and nothing else changes. Golden impact: none;
existing supported fixtures use `G_ZS_PIXEL`. Shader wiring, normalization and
decal tolerance under primitive Z are deferred to a game that issues it (section 3).

### PR 10 — Convert/key registers. S, 1–2 days

Depends on PR 3. Files: `hle/{rdp,combiner,rsp}.rs`, `fast3d/src/diag.rs`,
`n64-gbi` constants/encoders/conformance, the selector support table and new
`tests/convert_key.rs`. No shader changes.

Tests: `convert_key_words_and_roundtrip`, `convert_signed_nine_bit_literals`,
`convert_k2_crosses_word_boundary`, `convert_key_setters_alone_are_silent`,
`keyr_keygb_preserve_other_channels`, `key_width_is_retained`,
`convert_key_snapshot_per_draw`, `key_selector_active_cycle_is_rejected`,
`k_selector_active_cycle_is_rejected`, `inactive_cycle_selector_does_not_reject`,
`yuv_and_chroma_key_mode_are_rejected`. Test -256, -1, 0, 255 coefficients,
both cycles, rectangles, and a setter appended after the last draw.

Acceptance: registers decode with signed values per the SDK and snapshot per
draw; draws selecting KEY_CENTER, KEY_SCALE, K4 or K5 in an active cycle, or
enabling YUV conversion or chroma keying, are rejected with the named errors and
the support matrix says so. Golden impact: none; no fixture selects these
inputs. Combiner wiring waits for a game that uses them (section 3).

### Item 13, evidence-gated: measure first

PRs 11 and 12 are outside the initial delivery. They start only after a baseline
PR (S, 1–2 days) records decode count, GPU upload count and CPU time for one
captured sm64 frame and one n64.toys animated scene as section 3 specifies, and
the numbers name a cost worth removing. Neither PR may change a pixel.

#### PR 11 — Shared texture content and cache identity. M, 4–6 days

Depends on PRs 2, 6 and 10; use the final draw-state layout. Files:
`hle/{tmem,combiner,rsp}.rs`, `fast3d/src/scene.rs`, `render/mod.rs`, new
`render/texture_cache.rs`, `tests/{run_split,facade}.rs` and new
`tests/texture_cache.rs` with test-only counters.

Tests: `prim_env_animation_decodes_once`, `material_reorder_reuses_images`,
`palette_mutation_invalidates_ci`,
`same_address_new_bytes_misses_cache`, `sampling_change_reuses_pixel_upload`,
`lod_detail_and_tex1_change_independently`, `hash_collision_checks_bytes`,
`cache_eviction_keeps_live_scene_images`.

Acceptance: one decode per unchanged snapshot/descriptor and one GPU upload per
unique live image after warm-up, independent of material order. Check bounded
cache accounting and compare decode/upload counts and CPU timings against the
same captured sm64 and animated n64.toys baseline workloads. Rust-authored color
scenes may isolate costs but do not replace that comparison. If a whole-TMEM key
causes extra misses, report them; do not claim palette-subrange precision. Golden impact: none;
all old and new image hashes must stay identical. No new rt64 comparison is
needed beyond unchanged replay of the semantic corpus.

#### PR 12 — Reusable scene uploads. S–M, 3–5 days

Depends on PRs 6 and 11. Files: `render/{mod,workload}.rs`, new
`render/upload.rs`, tests for multi-task and mixed-extent scenes, benchmark
driver using the recorded sm64 and n64.toys baseline inputs, with Rust-authored
cases for isolated allocation checks.

Tests: `upload_buffers_reused_after_warmup`, `upload_growth_keeps_prior_task`,
`multiple_extents_have_distinct_output_ranges`,
`many_tasks_one_frame_do_not_overwrite_uniforms`,
`reconfigure_releases_cached_device_resources`.

Acceptance: no new buffers at steady workload capacity, identical images and
diagnostics, measured allocation/upload counts and CPU time against the same
baseline workloads, with adapter/build metadata. Default WebGPU limits remain
sufficient. Golden impact: none. Do not include the n64.toys compiled-program
implementation in this PR.

### Deferred PRs, activated by a committed second game

These are scoped deliverables, not permission to begin an XL framebuffer rewrite.
Each activation requires the corresponding trace/evidence in section 3.

| PR | Size / dependency | Files and acceptance tests | Golden/oracle impact |
|---|---|---|---|
| D1 — F3DEX family | M, 4–6 days; PRs 4–5, David's game/build | New `hle/{gbi/f3dex,rsp_f3dex}.rs`; microcode facade/capture tags; `n64-gbi` family module and literal vectors. Vertex count/start, triangle packing, matrices, halfwords and declaration tests; one real task plus an IMAGE scene. Companion helix/sm64 declaration fix if that is the selected consumer. | Existing families unchanged; new family goldens and rt64 F3DEX task comparison. |
| D2 — Range ownership | M, 5–8 days; PR 8, overlapping-game trace | `render/framebuffers.rs`, workload source metadata. `range_latest_writer`, `partial_overlap_preserves_unwritten_bytes`, `format_stride_generation`, overflow/alias tests. | No unexplained changes to independent sm64 targets. Named `FramebufferRangeOwnership` masks for overlap fixes. |
| D3 — Load-time region snapshots for all draws | M, 5–8 days; D2 | `hle/{rdp,combiner}.rs`, workload, framebuffers and sampling descriptors. `loadtile_fb_offset`, `loadblock_fb_stride`, `triangle_and_rect_share_fb_load`, `load_then_producer_write_keeps_snapshot`, same-target snapshot tests. | `FramebufferRegionLoad` masks and real-load IMAGE oracle; replace reliance on the shortcut for hardware claims. |
| D4 — Required format reinterpretation | M, 5–8 days per evidenced format pair; D3 | Framebuffers, TMEM decode and a conversion shader if needed. Literal packed-pixel vectors, CI palette changes, row offsets and conversion round trips for the named pair. | `FramebufferReinterpretation`; oracle where rt64 supports the pair, independent bit expectations everywhere. |
| D5 — RAM edits and optional FullSync patches | L, 9–15 days; D2–D4 as used, ROM consumer boundary | Separate readback/patch module, full-sync operation, capture CPU-write events and consumer integration. CPU-after-GPU read, CPU partial overwrite, two FullSyncs in one task, ordered native completion and async browser completion. | `FramebufferRamCoherence`; compare packed RAM bytes as well as pixels. Default read-only consumers unchanged. |
| D6 — Real microcode detection / LOAD_UCODE | M, 4–6 days; ROM consumer, supported target families | `hle/gbi/detect.rs`, interpreter family switching and capture format. Reproducible hash vectors, unknown rejection, reset-from-load, family switch and replay tests. | New ROM-task/oracle fixtures; no guessed hashes or change to explicit port selection. |

### Release documentation after David's decision. S, 1 day

Files: `README.md`, workspace/crate manifests and a migration note. Default
proposal: document an immutable git revision with the reviewed facade and its
matching `n64-gbi` vocabulary. David decides the release version and registry
publication separately. Acceptance for that later PR: the README dependency
example resolves the documented public API in an external example, the feature
list has no `asm`, and the HostRam/TLUT breaks have explicit before/after call
examples. If publication is chosen, version the leaf dependency and check both
package manifests before a separate publish action. No renderer tests, fixture
or golden changes belong to the documentation portion.

### Dependencies and verification gate

Order: PR 2 (TLUT), PR 3 (stubs, inventory, format errors), PR 4 (MODIFYVTX,
QUAD), PR 5 (CULLDL, BRANCH_Z). In the first two working weeks, land PR 2 then
PR 3 (5–7 days), then PR 4 (2–3 days); PR 5 follows (4–6 days). All four take
11–16 days. A correct `n64-gbi` pin lets n64.toys update its relocated compiler;
the F3DEX2 controls become authorable when its companion adds their syntax.
These PRs preserve the memory API; PR 2 still changes TLUT input semantics.
PR 1 follows when David takes the memory break, with the helix and n64.toys
companions ready to land together (6–8 days). PRs 6, 7 and 8 then fix the workload
and establish the depth acceptance boundary (14–21 days), followed by reduced
PRs 9 and 10 (2–4 days). Each depends technically on PR 3; the delivery order
keeps them after PR 8. The initial delivery is 33–49 working days. The
item 13 baseline and PRs 11–12, and D1–D6, are outside it.

Claude verifies every implementation PR independently on the GPU matrix and in
the actual browser job, including forced fallback and dual-source where
applicable. Run workspace default/all-features tests, fmt and Clippy, and wasm
builds with current default/capture/debug-ui combinations; no removed `asm`
feature. Missing GPU/browser adapters are a blocked verification result, not a
passing skip. Use Rust fixture writers and the pinned rt64 oracle for exportable
IMAGE cases (`tools/rt64-oracle/README.md:37`,
`tools/rt64-oracle/README.md:66`). Do not convert HOST64 graphs into IMAGE by
truncating pointers. CULLDL, stubs and the stated reference disagreements need
independent assertions in place of a false oracle claim.

Golden changes follow `docs/design/sm64-fidelity.md:516`: channel tolerance stays
two; each changed existing `.bin` needs a named semantic fix, before/after
hashes, changed-pixel count and bounds, the exact mask, independent values or
coverage rules explaining it, and no differences outside that mask. Input
fixture corrections also record old/new command hashes and their libultra
derivation. Oracle RGBA16 quantization budgets remain separate from the fast3d
golden tolerance. New goldens need independent semantic probes and reviewed
images; neither broad regeneration nor agreement between an encoder and its
decoder is acceptance. Performance/refactor PRs permit no pixel delta.

## 5. Consumer risks

| Risk | Consumer / break | Migration or protection |
|---|---|---|
| Fallible public readers | External Rdram implementors must change signatures | One documented source break, public-only integration test, exact span examples; `WebHardware` itself still returns `RdramImage`. |
| HostRam no longer implements Rdram | helix's adapter stops compiling | Move the unsafe obligation to `consume_dl`, use no-VI presentation, migrate capture in the same consumer update; keep native Fixed/Float and high-pointer tests. |
| Deferred guest reads | helix resumes before presentation | Retained scenes own bytes; poison/drop source after consume in safe recorded tests, and verify presentation never requests a reader. |
| TLUT input meaning changes without a Rust type change | n64.toys compiler or external encoder caller emits a different count | Explicit migration note, update every in-tree call, pin encoder and compiler together, survey low-level saved source before rollout. |
| Stricter unsupported-format errors (PR 3) | Playground source can emit the 23 unsupported pairs and the RGBA32 LoadBlock layout case listed in section 1 | Report at the draw PC through the existing diagnostic API; preserve supported RGBA32 paths. Survey saved source before moving the production pin. No dependency on PR 1. |
| Unwired RDP inputs are rejected (PRs 9–10) | Numeric playground modes/selectors can request primitive Z or key/K inputs; no committed game establishes shader use | Snapshot registers, keep unused setters silent, and drop consuming draws with the named Errors. Test active/inactive cycles, triangle/TexRect PCs and unchanged `DlSummary` shape; shader support remains deferred. |
| Depth now survives | helix could expose previously masked missing clears | Partial-clear and contamination tests; run the live experiment before pinning output. Close with reviewed difference evidence or bounded `closed-by-absence`; incomplete capture/replay stays open. |
| Pairless policy/order correction | n64.toys multi-pass or animated source may change pixels | Preserve 320x240 logical coordinates and `PerFrame`; compare ordered scissor/decal fixtures and saved-source renders in its repo. |
| Alias guards replace wrong sampling | Offscreen toys may receive new errors | Keep exact-base shortcut, report unsupported offsets/feedback precisely; defer broader support with a named reproducer. |
| Cache identities omit palette/LOD/format (evidence-gated PR 11) | Both consumers could show stale textures | Measured consumer baseline first, then mutation, descriptor, collision and task-reorder tests; zero pixel delta. |
| Buffer reuse overwrites queued work (evidence-gated PR 12) | Multiple tasks or extents could use last task's values | Measured consumer baseline first, then disjoint live ranges and a many-task browser fixture, with allocation counters separate from pixel checks. |
| F3DEX added as an enum variant (deferred D1) | Consumers with exhaustive Microcode matches will need a new arm | No new variant or FFI ID in the initial delivery. D1 keeps existing IDs and adds an explicit ID, capture tag and declaration test in the chosen consumer. |
| Write-back assumed at present | Future ROM consumer reads stale bytes or blocks wasm | Separate opt-in completion/patch API and DP-event evidence; freeze it only with that consumer. |

## 6. Needs David

1. When to take the memory API break (PR 1). It is a source break for helix and
   for any external `Rdram` implementor, and nothing in PRs 2–5 needs it.
   Recommendation: land PRs 2–5 first, then PR 1 together with the helix and
   n64.toys companion PRs so both consumers move in one step.
2. Playground impact of removing the RGBA16 fallback (PR 3). The 23 unsupported
   `(fmt, siz)` pairs and the RGBA32 non-word-aligned LoadBlock case in section 1
   become named errors instead of plausible pixels. Recommendation: accept, and
   survey saved n64.toys sources for low-level `gsDPSetTile` operands before
   moving its production pin.
3. Supply or authorize the sm64 capture experiment in section 3 to determine
   whether depth affects output across color switches, tasks or frames.
   If no dependency is found, the live gate closes by absence for that route;
   it does not stay open indefinitely.
4. Confirm the next committed game/build. F3DEX is the selected next family; a
   game requiring F3DZEX2 or S2DEX changes that position before implementation.
   The possible usefulness of sm64's optional F3DEX build is an inference, not
   a commitment from the consumer.
5. Establish whether saved n64.toys sources use raw TLUT words or low-level
   palette macros, and how compiler versions are rolled out. Only source storage
   code was inspected; no production database or saved user content was read.
6. Confirm whether any intended ROM consumer needs CPU framebuffer/depth reads,
   writes, and DP completion synchronization. Default here is no RAM write-back;
   no inspected consumer establishes a need for it.
7. Decide release naming and publication. README requests `fast3d = "1.0"`, the
   workspace says `1.0.0`, and the roadmap reports the registry's legacy 2023
   `0.5.0` (`README.md:13`, `Cargo.toml:7`, `docs/ROADMAP.md:94`). This is a reported
   registry mismatch, not a registry release audit. Proposed interim action:
   document a known git revision for the redesigned facade; publish only after
   these intentional API breaks and companion consumers are reviewed. A publish
   PR must also make the path-only `n64-gbi` dependency publishable
   (`Cargo.toml:17`). No publication is authorized by this design.
8. Confirm any intent to change the inherited accuracy/presentation decisions.
   This design retains native-resolution SDK/rt64 semantics and existing
   approximations, with the specific divergences above. Helix currently computes
   widescreen aspect itself (`helix:src/render.rs:55`); upscaling ownership and
   extended GBI remain open roadmap decisions (`docs/ROADMAP.md:97`). No second
   aspect policy or extension protocol is introduced here.

## Settled decisions (challenge, 2026-09-06)

Claude challenged `ed42e50` against the code, the consumers and rt64. Positions
P1–P7, P9, P11–P13 and P15 are accepted; P10 is refined below. Two overrides,
three refinements:

**P8 overridden.** Primitive depth, convert and key become stored draw state
with named rejections, not shader inputs. sm64 issues none of these commands
(section 1); rt64 parity alone is not scope evidence. PRs 9 and 10 shrink to S.

**P14 overridden.** Item 13 leaves the initial delivery. A baseline measurement
on a captured sm64 frame and an n64.toys animated scene precedes any
optimization, and optimization PRs permit no pixel delta.

**Ordering refined.** PRs 2–5 land before PR 1. Nothing in TLUT or the F3DEX2
controls needs the fallible trait; the API break is taken when David schedules
the consumer update, with both companions ready.

**P10 refined.** Depth persistence by address ships with authored acceptance.
The live sm64 gate closes after the experiment in section 3, either by a
reviewed reset-induced difference or by absence for the recorded route. It
remains open until that evidence exists; no positive dependency is required.

**PR 6 refined.** The workload normalization lands with the existing execution
paths still present and pixel-identical, then replaces them in a second
bisectable commit. Every existing golden stays pixel-identical except the two
named classes.

**Reference disagreements recorded.** BRANCH_Z follows the SDK's less-or-equal
(`sm64:include/PR/gbi.h:2380`, `:2426`) over rt64's strict comparison
(`rt64:src/hle/rt64_rsp.cpp:844`); CULLDL follows the SDK where rt64's handler is
empty; convert coefficients are signed per the SDK where rt64 extracts them
unsigned. Each carries an independent assertion in its PR instead of an oracle
claim.

**Facts settled.** The helix capture hook is branch `helix-capture` (`81e9a70`,
helix PR #33). `Microcode` gains no variant before D1, so helix's FFI table is
unchanged by this delivery. `n64-gbi` stays dependency-free.
