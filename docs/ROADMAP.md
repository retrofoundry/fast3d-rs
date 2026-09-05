# Roadmap

Written 2026-09-05 from a joint review of fast3d against rt64 (the accuracy and architecture
reference). Consumers: helix (native ports, sm64 first) and n64.toys (web). Items are ordered by
consumer impact and by what unblocks other work. Each item names the evidence that closes it.
Sizes: S days, M one to two weeks, L several weeks, XL open-ended.

## 1. sm64 correctness

Goal: sm64 through helix matches an independent reference at the places the code is known to
diverge today. Every fix lands with a fixture that fails before and passes after.

Items 1–6 landed as PRs #27–#38, including the rt64 oracle in #33. The authored
regression corpus and browser execution land with this PR. Live-game acceptance
still requires reviewed captures and independent reference comparisons.

1. Captured display-list corpus. A recording `Rdram` backend that snapshots every byte the
   interpreter reads from live guest memory into a relocatable fixture, plus a loader that
   replays it through the public facade as a golden. Seed it with sm64 frames covering: metal
   Mario, Jolly Roger Bay fog, power meter and text (point filter), transparent Mario (alpha
   dither), castle TRILERP floor, shadows and decals, water, cutout foliage. S–M.
   Evidence: fixtures render on the GPU CI matrix; goldens stored alongside.
2. Texgen units. Generated coordinates in texel space (`(d+1)*512`, linear via acos) before
   texture scaling, matching hardware and rt64's `TextureGen.hlsli`. S.
   Evidence: `gsSPTexture(0x0F80, 0x07C0)` on a 64×32 tile yields a 0..62 × 0..31 texel range;
   metal Mario fixture matches reference.
3. Fog and alpha dither. Fog multiplier, offset and color captured with the draw that uses
   them, not from the end-of-list state. `G_AC_DITHER` gets a real discard pattern. M.
   Evidence: JRB fixture with mixed fog settings in one frame; mirror Mario fixture.
4. Texture filtering and rectangle semantics. `G_TF_POINT`, N64 three-point bilerp and average
   selected from othermode; tile origin, mask and shift reach the sampler; `TexRect` honours
   its tile index; the lower-right +1 edge only in copy mode. L.
   Evidence: power meter and text stay sharp; rect fixtures with nonzero tile and origin.
5. Combiner validation matches the shader. Accept PRIM/SHADE/ENV alpha as color inputs (the
   shader already implements them), validate both cycles in two-cycle mode, fix the TEXEL1 role
   swap gate. M.
   Evidence: selector fixtures for every wired slot on both dual-source and fallback devices.
6. Remove the 320×240 constant from the compute RSP; viewport fold uses the pair's framebuffer
   size. S. Evidence: a 640×480 fixture renders geometry and rects aligned.

- Independent rt64 oracle for the capture corpus: `tools/rt64-oracle` exports IMAGE-layout
  fixtures to RDRAM, renders them through rt64, and compares native pixels with fast3d replay.
  The Metal Mario and mixed JRB scenes check texgen, fog and the combiners used by those scenes;
  synthetic payloads do not establish whole-frame game fidelity. Helix HOST64 captures cannot
  be fed to rt64. Evidence: reviewed difference masks and explicit pixel budgets for the same
  self-contained display lists, with RGBA16 quantization accounted for.

## 2. Library contract and other games

7. TLUT encoding. `gdp_load_tlut` and `load_tlut` use the libultra layout (count in bits 14..23,
   destination from the tile's TMEM address). The assembler follows. Migrate saved n64.toys
   sources if any rely on the old encoding. M.
8. Complete F3DEX2: MODIFYVTX, CULLDL, BRANCH_Z, QUAD, LINE3D (stub), SPNOOP 0xE0, LOAD_UCODE,
   DMA_IO and SPECIAL as stubs with diagnostics. Add SETPRIMDEPTH, SETCONVERT, SETKEYR/GB. M.
9. Public memory contract. Re-export `Command` and `RawVertex` so external `Rdram` impls are
   possible; bounds-check image reads and return diagnostics instead of panicking; decide the
   `HostRam` safety boundary (trusted pointers behind `unsafe`, or registered regions). M.
10. One recorded workload. Persistent depth per depth-image address, clears that hit the real
    depth, identical clear/scissor/decal behaviour on the paired and pairless paths. L.
11. Framebuffer manager. Range ownership, region copies with offsets, triangles reading
    framebuffers, reinterpretation, optional RAM write-back at full sync. XL.
12. Microcode families by next committed game: F3DEX, then F3DZEX2 or S2DEX. Real ucode hashes
    only when a ROM consumer exists. L per family.
13. Per-frame CPU work. Texture identity separate from prim/env colour; content-addressed
    texture cache; reusable upload buffers; compiled assembler programs for n64.toys. M–L.

## 3. rt64 parity

14. Internal resolution multiplier, MSAA, downsample on scanout; explicit aspect policy for
    widescreen. L.
15. Replay debugger on the retained scenes; a small versioned extended-GBI protocol, ideally
    rt64's `gEX*` opcode space so ports written for rt64 carry over. L.
16. GPU TMEM decode (compute) with a faithful CPU fallback; texture replacement keyed by TMEM
    hash. L–XL.
17. Frame interpolation with explicit transform IDs first, heuristic matching later. XL.

## Housekeeping

- Delete stale branches `add-rgba32-texture`, `feat/n64-gbi-extraction`, `vnext-relocate`.
- README says `fast3d = "1.0"`; crates.io has the 2023 `0.5.0` legacy crate. Publish or fix.
- Replace `eprintln!` in `tmem.rs` and `texdec.rs` with `DiagKind` variants.

## Decisions still open

- Accuracy target: console-like (three-point filter, dither, coverage) or enhanced presentation.
- Who owns upscaling and widescreen, the renderer or helix.
- Adopt rt64's extended GBI verbatim or define fast3d's own.
- Whether any consumer needs GPU results written back to guest RAM, and at what boundary.
