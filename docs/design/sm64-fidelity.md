# sm64 fidelity design (roadmap item 1)

Design written by Codex Astra on 2026-09-05, reviewed by Claude. Section "Settled decisions" at
the end records the review outcome and overrides; where it conflicts with the body, it wins.
Paths under `/tmp/ref/` are local reference clones of rt64 and the sm64 decomp.

My position is to make command semantics, memory layout, and state capture independently testable before treating an sm64 screenshot as evidence of correctness. The implementation should extend the existing interpreter, compute pass, fragment shaders, and test harness. It does not need a second renderer.

I inspected `ad5ad51956e8a461d8e68a0fa8cf670455ef8926`. Relative to the assessed implementation, the change is the addition of `docs/ROADMAP.md`; the defects below remain. I modified no files and ran no builds or tests in this pass. Test names below are proposed unless explicitly identified as existing.

**1. Fact inventory**

**1. Capture/replay**

The memory abstraction already supports the two required address spaces, but they are different binary formats:

- `RdramImage` reads two big-endian `u32` words at an eight-byte command stride. Its segmented-address resolution and alignment masking are image-specific. [mem.rs:102](fast3d/src/hle/mem.rs:102)
- `HostRam` reads two native-endian `usize` words at a sixteen-byte stride. `Command.w1` preserves the numeric low 32 bits, while `Command.w1_addr` preserves the complete pointer. Masked resolution deliberately does not apply the image address mask. [host_mem.rs:93](fast3d/src/hle/host_mem.rs:93)
- Host fixed matrices use native 32-bit packed words; float matrices use native `f32`. Float vertices have a 24-byte stride. Converting an entire snapshot to big endian would corrupt this distinction. Texture byte arrays also cannot be indiscriminately byte-swapped. [host_mem.rs:127](fast3d/src/hle/host_mem.rs:127)

Helix uses this host-pointer contract directly. The guest remains blocked while the renderer consumes its pointers; Helix supplies no VI registers and uses persistent framebuffer contents. [helix render.rs:9](helix:src/render.rs:9), [render.rs:198](helix:src/render.rs:198)

There is useful existing coverage, but no captured sm64 framebuffer corpus:

- `dlmemory_equivalence.rs` compares equivalent image and host-memory walks. [dlmemory_equivalence.rs:1](fast3d/src/tests/dlmemory_equivalence.rs:1)
- The ignored BOB test requires a prepared, decompressed segment image and checks the walk, not rendered fidelity. It is not a ROM-backed capture golden. [rsp_f3d.rs:1399](fast3d/src/hle/rsp_f3d.rs:1399)
- Much of `goldens.rs` manually connects interpreter, compute, and raster stages. The public-facade pattern already exists in `renderer_present.rs`. [goldens.rs:93](fast3d/src/tests/goldens.rs:93), [renderer_present.rs:29](fast3d/src/tests/renderer_present.rs:29)

The assessments’ suggestion to capture “through `RdramImage`” is insufficient for Helix unless accompanied by an explicit format conversion. I reject that conversion for the replay backend.

**2. Texgen units**

The compute shader emits spherical coordinates in approximately `[0,1]`, and its “linear” mode uses a cubic approximation in the same range. It then multiplies by `sc/65536`. [rsp_process.wgsl:128](fast3d/src/render/rsp_process.wgsl:128)

The renderer deliberately skips tile normalization for texgen, inspecting the first vertex of a run. This cements the wrong units and introduces a per-run coordinate convention. [render/mod.rs:495](fast3d/src/render/mod.rs:495)

The existing `kernel_texgen_spherical_and_cubic_uv` test and `ref_texgen_uv` oracle reproduce that implementation. They currently defend the defect. [tests/render.rs:999](fast3d/src/tests/render.rs:999), [render.rs:1384](fast3d/src/tests/render.rs:1384)

The concrete trigger remains `mario_metal_butt`: a 64×32 texture with scales `0x0F80`, `0x07C0`. [Mario model:391](/tmp/ref/sm64/actors/mario/model.inc.c:391) RT64 produces `(d+1)*512`, or `acos(-d)*1024/π`, before multiplying by those scales. [TextureGen.hlsli:19](/tmp/ref/rt64/src/shaders/TextureGen.hlsli:19)

**3. Fog and `G_AC_DITHER`**

Fog enable is already captured when vertices load. Multiplier and offset are not: `Scene.fog` is only a boolean, and `interpret()` installs final-list fog parameters and color into scene globals. [rsp.rs:352](fast3d/src/hle/rsp.rs:352), [scene.rs:126](fast3d/src/scene.rs:126), [interp.rs:350](fast3d/src/hle/interp.rs:350)

These values have two different capture times: factors belong to vertex processing; fog color belongs to the draw. Capturing all three merely “per draw” would still mishandle reused vertices.

JRB provides two concrete settings:

- `jrb_seg7_dl_070069B0`: color `(15,65,100,255)`, raw factor `0x0724F9DC`, meaning multiplier `1828`, offset `-1572`. [JRB 1/5:367](/tmp/ref/sm64/levels/jrb/areas/1/5/model.inc.c:367)
- `jrb_seg7_dl_07004940`: color `(5,80,75,255)`, `FogPosition(900,1000)`, meaning `1280`, `-1024`. [JRB 1/2:533](/tmp/ref/sm64/levels/jrb/areas/1/2/model.inc.c:533)

`CombinerUniform::from_run` implements threshold compare and a coverage-times-alpha cutoff, but no dither discard. Its mutually exclusive selection also lets coverage-times-alpha suppress alpha compare. [render/mod.rs:161](fast3d/src/render/mod.rs:161)

Transparent Mario explicitly enables `G_AC_DITHER`. [mario_misc.c:302](/tmp/ref/sm64/src/game/mario_misc.c:302)

Two assessment corrections matter:

- The existing coverage cutoff is `0.125`, not `8/255`. The source comment contains the same arithmetic mistake.
- RT64’s disabled “Alpha dither” block concerns alpha-channel dithering. Its subsequent `G_AC_DITHER` random-threshold discard is active. [RasterPS.hlsl:186](/tmp/ref/rt64/src/shaders/RasterPS.hlsl:186)

**4. Filters, tile addressing, and texture rectangles**

Tile origin, mask, shift, and clamp/mirror fields are parsed. `SetTileSize` nevertheless calculates width and height from the lower-right coordinate alone. [rdp.rs:177](fast3d/src/hle/rdp.rs:177)

The fragment path uses normalized texture sampling, and the samplers select linear filtering irrespective of `G_TF_POINT`, `G_TF_BILERP`, or `G_TF_AVERAGE`. Per-level sampling also maps the same normalized base-tile coordinates over independently sized textures, losing each tile’s shift and origin. [render/mod.rs:1915](fast3d/src/render/mod.rs:1915), [combiner_prelude.wgsl:299](fast3d/src/render/combiner_prelude.wgsl:299)

The TexRect decoder throws away the tile field and fractional screen coordinates. `build_rect_material` uses tile zero. Both rectangle geometry and UV width unconditionally add the lower-right pixel. [interp.rs:187](fast3d/src/hle/interp.rs:187), [combiner.rs:867](fast3d/src/hle/combiner.rs:867), [render/mod.rs:459](fast3d/src/render/mod.rs:459)

The power meter is triangle geometry, not a TexRect example. Its list explicitly requests point filtering and loads vertices before configuring the tile. The existing power-meter test verifies geometry, not filtering. [power meter:90](/tmp/ref/sm64/actors/power_meter/model.inc.c:90), [hud_power_meter.rs:47](fast3d/src/tests/hud_power_meter.rs:47)

The HUD list’s explicit point-filter command at `segment2.c:11785` is `VERSION_EU`-conditional. The US copy-mode list must also sample without filtering; correctness cannot depend on that explicit command. [segment2.c:11773](/tmp/ref/sm64/bin/segment2.c:11773)

**5. Combiner validation and texture roles**

The CPU rejects primitive, shade, and environment alpha as color inputs, although WGSL implements them. CPU validation checks only cycle 1, including for two-cycle draws. Rectangle material construction bypasses that validation. [combiner.rs:46](fast3d/src/hle/combiner.rs:46), [combiner.rs:664](fast3d/src/hle/combiner.rs:664), [combiner_prelude.wgsl:126](fast3d/src/render/combiner_prelude.wgsl:126)

The second-physical-texture dependency calculation is already swap-aware and correct. The first-texture calculation and WGSL swap remain incorrectly conditional on requiring the second physical texture. The empty-TMEM check also examines an incomplete set of dependencies. [combiner.rs:539](fast3d/src/hle/combiner.rs:539), [combiner.rs:650](fast3d/src/hle/combiner.rs:650), [combiner.rs:728](fast3d/src/hle/combiner.rs:728), [combiner_prelude.wgsl:433](fast3d/src/render/combiner_prelude.wgsl:433)

Claude’s assessment that the “full selector set” and `NOISE` are wired is incorrect. Noise and several key/conversion inputs remain unsupported; their shader placeholders do not constitute implementations.

**6. Framebuffer dimensions**

`rsp_process.wgsl` hardcodes 320×240 for both viewport folding and `MODIFYVTX` screen-XY conversion. Rectangles use framebuffer dimensions supplied by the renderer. [rsp_process.wgsl:7](fast3d/src/render/rsp_process.wgsl:7)

There is a further structural constraint: the renderer currently computes one transformed vertex buffer before processing the framebuffer pairs. Replacing the constants with one scene-wide uniform would still be wrong for a scene containing different pair dimensions. [render/mod.rs:2383](fast3d/src/render/mod.rs:2383)

Pair height being frozen from the opening scissor, and repeated-address allocation problems, are separate known defects. This change must not claim to solve them. [rsp.rs:796](fast3d/src/hle/rsp.rs:796)

One additional assessment correction affects the regression corpus: decal tolerance already includes depth derivatives with a minimum epsilon. It is not solely a constant epsilon. [decal.wgsl:34](fast3d/src/render/decal.wgsl:34)

**2. Mechanisms**

**1. Capture/replay: preserve the source address space as virtual addresses**

Add `hle/capture.rs`, with public recording and replay types exposed under an opt-in `capture` feature. Replay must compile for wasm; the constructor that dereferences live `HostRam` remains native-only.

The data flow is:

`live Rdram → RecordingRdram → existing interpreter → existing Scene → public Renderer`

and, independently:

`fixture bytes → ReplayHardware/ReplayRdram → the same public Renderer`.

Do not serialize `Scene`. That would bypass the state-capture defects this corpus must detect.

The version-one fixture contains:

| Record | Required contents |
|---|---|
| Header | Magic, version, explicit little-endian container encoding |
| Memory layout | Image or host resolution rules; source byte order; command word width and stride; fixed-matrix packing |
| Frame | Frame serial and dither seed; renderer configuration; output extent; clear policy; optional VI snapshot |
| Task | Entry address as `u64`; microcode; `DataFormat`; initial segment bases; ordered position within its frame |
| Memory spans | Source virtual address as `u64`, length, payload offset, verbatim bytes |
| Provenance | Decomp revision, source symbols, command-vector identity, synthetic-data description |

There is a separate sparse memory snapshot for each task. An address can therefore hold different bytes in successive tasks.

**Host-pointer relocation is an interval mapping, not command rewriting.** Suppose a command contains host address `0x0000000123456780`. The fixture keeps that value as a virtual address. Replay finds the span containing it and computes:

`owned_payload_offset + (address - span.virtual_base)`.

The subtraction and range check happen in `u64`; conversion to `usize` occurs only after the offset is proven to fit the owned payload. The original value is never cast to a pointer. Thus replay works on wasm even when every captured address exceeds `u32::MAX`.

This preserves interior pointers, overlapping reads, shared data, segment bases, and framebuffer-address identity. It also avoids guessing whether a numeric `w1` is a pointer. `Command.w1` and `w1_addr` are reconstructed according to the recorded layout, including sixteen-byte continuation-command stepping.

Authored fixtures use symbolic allocations assigned virtual addresses. Only operands declared as address operands receive those symbol values. A second authored layout can place the same graph in BE image memory for equivalence tests. This authoring operation is distinct from loading a captured fixture.

`RecordingRdram` must intercept all read methods, including matrix and vertex overrides. Merely wrapping `read_bytes` misses host typed reads. Recording constructors for the existing image and host backends know their exact layouts and initial segment state; the design does not pretend to infer an arbitrary third-party backend’s semantics.

Record the bytes actually used by structured reads, including disjoint vertex fields, without requiring unused padding to be initialized. Share byte-decoding helpers between recording/replay and the existing backends where practical. The existing independent layout-equivalence tests remain necessary because shared helpers can share defects.

Within a task, conflicting overlapping reads invalidate the capture. Missing replay bytes produce a capture error, not zeros accepted as a successful frame. Because `Rdram` is currently infallible, replay can latch an error while returning safe placeholders internally; the public replay operation must then return failure and reject the output.

The capture session records facade operations as well as memory reads: frame boundaries, data format, task order, VI, and presentation. Helix’s existing blocked-guest interval is the place to copy memory. Encoding the completed snapshot can happen afterward.

Persistent framebuffer contents require an explicit starting point. Version one records a sequence beginning from a renderer reset, or a self-contained frame whose commands initialize every relevant target. It does not claim that a mid-game RDRAM snapshot includes pre-existing GPU color or depth.

**Fixtures available now.** Preserve decomp command words, nested list structure, vertex counts, and triangle indices; substitute texture, vertex, matrix, light, and look-at payloads. An initialization prologue is identified separately from the exact decomp body. An extracted prefix must be labelled as a prefix, never presented as the complete source list.

Seed bodies are metal Mario, the power-meter lists, US and EU HUD variants, both JRB fog settings, generated transparent-Mario commands, and the castle TRILERP floor. Synthetic data should make selected triangles measurable and irrelevant geometry degenerate where necessary. Small `.n64` scenes isolate individual defects; literal F3D vectors preserve the actual sm64 command sequences. Extend `n64-gbi` conformance tests rather than making encoder/decoder round trips the encoding oracle.

Expected pixels for these controlled scenes come from separately calculated sampling, fog, and blending results. Production rendering does not generate its own expected answer. Raw RGBA8 goldens supplement those assertions.

**Rejected alternative:** flatten native memory into `RdramImage`. It requires opcode-aware pointer rewriting and structured-data transcoding, introduces another interpreter-like component, and can conceal the very stride and endian defects under test.

**2. Texgen: one texel-space convention**

Keep `Rsp::set_vertex` capturing the texture scale and transformed look-at state at vertex load. Replace only the generated-coordinate calculation and remove the normalized-texgen exception:

```text
d = clamp(dot(normal, look_at_axis), -1, 1)
spherical = (d + 1) * 512
linear    = acos(-d) * (1024 / π)
output    = generated * texture_scale / 65536
```

These are RT64’s formulas. [TextureGen.hlsli:19](/tmp/ref/rt64/src/shaders/TextureGen.hlsli:19)

`GpuTexcoord` retains ordinary and generated scale factors. All compute output UVs are texel coordinates. `triangle_inv_tex_size` must stop inspecting the first vertex’s texgen flag; until the explicit sampler lands, it normalizes every textured triangle by the draw-time tile size. The run split whose purpose is selecting normalized versus texel units becomes unnecessary.

No tile origin or tile dimension is applied at vertex load. Power-meter ordering and tile changes after `gsSPVertex` require those to remain draw state.

WGSL uses `f32` and `acos`; no `f64` is needed. Tests use independently calculated double-precision expectations on the CPU, with an absolute coordinate tolerance of `1/1024` texel.

For Mario’s scales, the required result is:

| Dot product | Spherical S | Spherical T |
|---:|---:|---:|
| −1 | 0 | 0 |
| 0 | 31 | 15.5 |
| +1 | 62 | 31 |

Linear tests include non-endpoint values, because endpoints alone cannot distinguish the cubic approximation from `acos`.

**Rejected alternative:** multiply the current normalized result by an sm64-specific correction. It leaves mixed coordinate conventions and the incorrect linear function intact.

**3. Fog: factors at vertex load, color at draw; dither as alpha compare**

Replace the boolean interpretation of `Scene.fog` with an index-plus-one:

- `0`: fog disabled when this vertex loaded.
- `n+1`: use `Scene.fog_table[n]`, containing signed multiplier and offset.

`Rsp::set_vertex` uses its currently ignored RDP argument to intern the current factor pair. Vertex copying preserves the index. `MODIFYVTX RGBA` continues to clear fog processing so its supplied alpha survives.

Add one compute storage binding for the fog table. The current seven storage buffers become eight, fitting the default WebGPU storage-buffer budget. Keep the source-vertex field and its stride unchanged.

The shader retains the existing raw clip-depth formula, substituting the indexed parameters:

`clamp(max(clip.z, 0) / clip.w * multiplier + offset, 0, 255) / 255`.

This follows RT64’s per-vertex fog indexing and compute calculation. [rt64_rsp.cpp:625](/tmp/ref/rt64/src/hle/rt64_rsp.cpp:625), [RSPProcessCS.hlsl:69](/tmp/ref/rt64/src/shaders/RSPProcessCS.hlsl:69) Preserve raw `FogFactor` values; do not reconstruct a near/far interval. The libultra macros explicitly encode signed factors. [gbi.h:2769](/tmp/ref/sm64/include/PR/gbi.h:2769)

Add `fog_color: [u8;4]` to `DrawRun` and the TexRect draw snapshot. Include it in run coalescing. `CombinerUniform::from_run` receives that color instead of `Scene.fog_color`. Remove the final-scene fog globals as rendering inputs. This avoids decoding a texture again merely because fog color changed.

For the two JRB settings, vertices at `clip.z/clip.w = 0.9` must produce unnormalized fog values `73.2` and `128`. A subsequent fog-color change must affect a later draw of an already loaded vertex, while a factor change must not retroactively recompute that vertex.

For alpha compare:

- Replace the mutually exclusive coverage/threshold mode with independent alpha-compare and coverage flags.
- Extend the shared combiner result with `compare_alpha`.
- In one cycle, compare the cycle-1 alpha. In two cycles, compare the first cycle’s alpha, while retaining the second cycle’s alpha for the final color/blender. Copy mode compares the copied texel alpha.
- Apply the same discard helper in ordinary, decal, dual-source, and fallback fragment entry points.
- Preserve the existing coverage approximation separately; this PR does not turn it into full RDP coverage.

RT64 explicitly captures alpha for comparison before executing the second cycle. [rt64_color_combiner.h:611](/tmp/ref/rt64/src/shared/rt64_color_combiner.h:611)

`G_AC_DITHER` compares against a deterministic pseudorandom threshold for each framebuffer pixel and frame. Nintendo documents a random threshold; an ordered Bayer matrix is not this mode. [Nintendo alpha-compare reference](https://ultra64.ca/files/documentation/online-manuals/man/n64man/gdp/gDPSetAlphaCompare.html)

My concrete approximation is:

1. Seed RT64’s 16-round integer `initRand` with `(frame_serial XOR capture_seed, x + framebuffer_width*y)`.
2. Advance its integer generator once.
3. Use `(high_byte + 0.5)/256` as the threshold.
4. Discard when `compare_alpha < threshold`.

The integer generator is specified in [Random.hlsli:10](/tmp/ref/rt64/src/shaders/Random.hlsli:10). The eight-bit midpoint threshold is our explicit approximation: it guarantees invisible alpha zero and visible alpha one. It is not a claim to reproduce the console’s LFSR phase or RT64’s exact mask.

The seed excludes host addresses and draw-run indices, so relocating a capture or changing batching cannot change its image. Frame serial is captured for replay. A uniform row for frame/seed/dimensions keeps `CombinerUniform` below its 256-byte allocation stride.

**Rejected alternatives:** draw-time fog factors would recompute cached vertices incorrectly; scene-wide fog color changes earlier draws; alpha-to-coverage would depend on MSAA and would not implement `G_AC_DITHER`; a fixed Bayer matrix would produce a different spatial pattern.

**4. Sampling: integer taps with per-tile state**

Add a `TileSampling` snapshot to each material texture, including TEXEL1, detail, and each independent LOD tile. It contains:

- Raw quarter-texel origin and bounds.
- S/T shift fields.
- S/T mask fields and clamp/mirror flags.
- Decoded-image extent and addressing representation.
- TMEM base, line stride, and format metadata when required.

Capture the filter mode from othermode at material creation. Copy mode explicitly forces point sampling. Do not infer copy behavior from the synthetic pass-through combiner.

Upload sampling descriptors in a separate fixed-size uniform array associated with the existing texture bindings. Do not grow the 160-byte combiner uniform into an oversized per-tile structure. Texture/bind-group cache equality must include these descriptors.

The coordinate path for every physical tile is:

1. Start with texel-space interpolated S/T.
2. Apply shift: fields `0…10` divide by `2^shift`; fields `11…15` multiply by `2^(16-shift)`.
3. Apply the pinned RT64 low-precision convention, rounding to `1/128` texel.
4. Subtract `uls/4`, `ult/4`.
5. Compute integer base and fractional parts.
6. Address each integer tap independently: clamp first, then mask/mirror.

A zero mask implies clamping. A nonzero mask gives period `2^mask`, independent of the decoded image dimensions. Mirror uses a `2*period` cycle and Euclidean modulo, including negative coordinates. These rules follow RT64’s ordering and Nintendo’s tile definitions. [TextureSampler.hlsli:74](/tmp/ref/rt64/src/shaders/TextureSampler.hlsli:74), [rt64_state.cpp:283](/tmp/ref/rt64/src/hle/rt64_state.cpp:283), [Nintendo tile reference](https://ultra64.ca/files/documentation/online-manuals/man/n64man/gdp/gDPSetTile.html)

Correct logical dimensions to `max(1, floor((lr-ul)/4)+1)` using signed intermediate arithmetic. Keep origin and clamp extent distinct from the allocation extent.

Use `textureLoad`, with explicit filtering:

- **POINT:** return the addressed `floor(S,T)` tap.
- **BILERP:** for fractional coordinates `(x,y)`, use  
  `c00 + x(c10-c00) + y(c01-c00)` when `x+y<1`; otherwise  
  `c11 + (1-x)(c01-c11) + (1-y)(c10-c11)`.
- **AVERAGE:** match RT64’s four-tap midpoint special case when both fractions are within `1/128` of `0.5`; elsewhere use the same three-point calculation.

Nintendo’s “bilerp” uses three texels. AVERAGE must not become unconditional four-tap box filtering. [Nintendo filter reference](https://ultra64.ca/files/documentation/online-manuals/man/n64man/gdp/gDPSetTextureFilter.html), [TextureSampler.hlsli:189](/tmp/ref/rt64/src/shaders/TextureSampler.hlsli:189)

A decoded image must cover every addressable tap. For ordinary bounded footprints, continue using the CPU-decoded tile. When a mask or stride can access beyond that image, use a bounded lookup texture built from the existing TMEM decoder:

- Enumerate row-relative byte offsets modulo 4096, odd-row state, and nibble parity.
- Decode each entry using the tile’s existing format/palette/base and `Tmem::decode_texel`.
- This requires at most `4096×2×2` RGBA8 entries: 64 KiB per tile.
- WGSL computes the relative offset from the addressed integer texel and fetches the corresponding decoded entry.

This preserves odd-row swaps, wrap, and RGBA32 banks without allocating a `32768×32768` texture or introducing a second GPU texture decoder. [tmem.rs:228](fast3d/src/hle/tmem.rs:228), [tmem.rs:275](fast3d/src/hle/tmem.rs:275)

This does not repair unrelated load/TLUT encoding defects. A fixture exposing one of those blocks acceptance and requires a separately named fix; it does not justify silently substituting the last linear texture load.

LOD derivatives remain evaluated before non-uniform branches and discard. Each selected LOD tile receives the original texel coordinate and applies its own descriptor. Preserve the castle’s two independently stored 32×32 tiles; do not replace them with a halving mip chain. [castle floor:22](/tmp/ref/sm64/levels/castle_inside/areas/1/1/model.inc.c:22)

For TexRect:

- Preserve its tile index and raw 10.2 bounds in `SceneOp::TexRect`.
- Pass the selected tile explicitly to shared material construction, without mutating `SPTexture` state.
- Convert rectangle UV generation to texel units.
- In one/two-cycle mode, use exclusive lower-right bounds.
- In copy mode, floor the upper-left, include the lower-right pixel, and apply signed `dsdx >> 2`.
- Use one bounds calculation for geometry and UV extents, including flipped rectangles.

Use RT64’s integer-bound conversion for fractional rectangles, including its fixed-point endpoint truncation and fractional-Y UV adjustment. [rt64_rdp.cpp:1163](/tmp/ref/rt64/src/hle/rt64_rdp.cpp:1163), [rt64_rdp.cpp:1267](/tmp/ref/rt64/src/hle/rt64_rdp.cpp:1267) An integer rectangle from `(0,0)` to `(9,9)` therefore covers 9×9 pixels in one/two-cycle mode and 10×10 in copy mode. [Nintendo rectangle reference](https://jrra.zone/n64/doc/n64man/gsp/gSPTextureRectangle.htm)

Keep fill/scissor representation separate so changing TexRect edge rules does not accidentally alter fill behavior.

**Rejected alternative:** selecting different wgpu sampler descriptors. It cannot implement three-point filtering, midpoint average, independent mask periods, or per-tap N64 address ordering.

**5. Combiner: one active-cycle contract and unconditional two-cycle role mapping**

Factor material validation into a shared function used by triangles and non-copy texture rectangles, before texture decoding:

- One cycle validates cycle 1.
- Two cycles validate cycles 0 and 1.
- Copy/fill bypass inactive combiner selectors.
- Primitive, shade, and environment alpha become accepted color inputs.
- Noise, keying, conversion constants, and combined-alpha color inputs remain rejected until implemented.

Use the existing `UnwiredSelector` mask’s lower eight bits for cycle 1 and upper eight bits for cycle 0. Diagnostic formatting names both cycle and slot. This preserves existing one-cycle mask values while making two-cycle errors actionable.

Define physical texture dependencies explicitly:

```text
one cycle:
  physical 0 = cycle 1 uses TEXEL0
  TEXEL1 remains rejected

two cycles:
  physical 0 = cycle 0 uses TEXEL0 OR cycle 1 uses TEXEL1
  physical 1 = cycle 0 uses TEXEL1 OR cycle 1 uses TEXEL0
```

Include alpha selectors in “uses.” These predicates drive texture decoding, enable flags, and missing-TMEM diagnostics.

WGSL always feeds `(physical1, physical0)` to the second cycle. There is no swap gate tied to texture allocation, LOD, or a dummy binding. This matches RT64’s `secondCycle` selector semantics. [rt64_color_combiner.h:468](/tmp/ref/rt64/src/shared/rt64_color_combiner.h:468)

Validation tests enumerate every encoding of every selector slot against a literal expected-support table. GPU tests exercise each accepted slot with distinct color and alpha inputs and an equation where that slot affects the result. The GPU oracle must not merely call the production decoder.

Run those tests on both forced-fallback and dual-source devices. Use opaque replacement cases for exact combiner parity; that assertion does not imply identical behavior for every partially supported blender mode.

**Rejected alternative:** accepting every decoded selector because WGSL returns something. Placeholder zero is an unsupported operation, not validation parity.

**6. Framebuffer dimensions: compute output per distinct pair extent**

Replace `RspProcessParams` with a 16-byte layout containing vertex count, padding, framebuffer width, and framebuffer height. Fog factors have moved into their table.

Use those dimensions in both viewport folding and modified screen-XY conversion. Remove the WGSL constants.

During scene rendering:

1. Resolve each pair’s actual render extent through one helper also used by target allocation and rectangles.
2. Upload source vertices and state tables once.
3. Dispatch compute once per distinct extent.
4. Bind that extent’s transformed output buffer when drawing its pairs.

Indices and captured vertex state remain unchanged. Reusing vertices across two differently sized pairs is legal; a single global transformed buffer cannot represent both folds.

For pairless legacy scenes, preserve the current logical 320×240 canvas as an explicit CPU-supplied fallback. It must not come from the browser canvas size or from the current viewport. Changing that legacy contract belongs in a separate decision.

The cost is `O(vertices × distinct extents)`. Ordinary sm64 frames normally have one relevant extent; optimize further only after measurement.

**Rejected alternative:** one scene-wide framebuffer-size uniform. It passes a lone 640×480 test while remaining wrong when the same scene draws to two dimensions.

**3. Ordered PR-sized work**

Fixtures go first because they make every later correction reviewable through the same facade Helix and n64.toys use. They also establish relocation and independent expected values before shader changes can influence the oracle.

Each semantic PR adds its reproducer before changing production behavior. Review evidence must show that test failing on the immediately preceding implementation and passing afterward.

**PR 1 — Recording, replay, and public-facade fixture harness. Size: M, roughly 5–8 days.**

Files: new `hle/capture.rs`; small changes to `hle/mem.rs`, `hle/host_mem.rs`, `hardware.rs`, `lib.rs`; extend `tests/common.rs`, `dlmemory_equivalence.rs`, `renderer_present.rs`; fixture data under `tests/`; `n64-gbi/tests/conformance.rs`; Cargo features.

Tests:

- `capture_host64_replays_after_source_drop`
- `capture_host64_preserves_interior_aliases_and_segments`
- `capture_host_layout_fixed_and_float`
- `capture_records_typed_reads_and_texrect_continuations`
- `capture_missing_span_is_error`
- `capture_task_snapshots_allow_memory_reuse`
- `fixture_public_facade_matches_live_backend`
- `sm64_seed_macro_words_match_gbi`

Acceptance: record an allocated host graph, destroy it, replay from unrelated allocations, and compare diagnostics, summaries, and pixels. Replay a checked-in high-address host fixture on wasm. A flat-color fixture closes the harness without depending on any semantic fix.

**PR 2 — Combiner support validation and texture-role mapping. Size: S–M, 3–5 days.**

Files: `hle/combiner.rs`, `diag.rs`, `render/combiner_prelude.wgsl`, affected material setup in `render/mod.rs`; selector scenes and conformance vectors.

Tests:

- `combiner_alpha_color_inputs_render`
- `combiner_support_matrix_all_slots`
- `combiner_rejects_unwired_cycle0`
- `combiner_ignores_inactive_cycle0`
- `combiner_texrect_uses_shared_validation`
- `combiner_cycle1_texel1_reads_only_physical0`
- `combiner_missing_texture_checks_both_cycles`
- `combiner_selector_pixels_dualsrc_and_fallback`

Acceptance: literal support matrix, cycle-labelled diagnostics, observable selector pixels, and the single-physical-texture swap case. No blanket acceptance of noise/key selectors.

**PR 3 — Texgen in texel units. Size: S, 2–3 days.**

Files: `render/rsp_process.wgsl`, `render/mod.rs`, run coalescing in `hle/rsp.rs`, existing kernel tests; metal fixture body and synthetic assets.

Tests:

- `texgen_metal_scale_yields_62_by_31`
- `texgen_linear_matches_acos`
- `texgen_mixed_vertices_share_texel_units`
- `fixture_sm64_mario_metal_butt`

Acceptance: compute-buffer readback at endpoints and interior angles; rotation cases using look-at state; a coordinate-coded texture visibly samples the intended range. Replace the incorrect cubic oracle with the independent formula.

**PR 4 — Fog capture timing. Size: S–M, 3–4 days.**

Files: `scene.rs`, `hle/rsp.rs`, `hle/interp.rs`, `render/mod.rs`, `render/rsp_process.wgsl`; JRB fixtures.

Tests:

- `fog_factors_are_vertex_load_state`
- `fog_color_is_draw_state`
- `fog_reused_vertex_keeps_factor`
- `fog_modify_rgba_preserves_supplied_alpha`
- `fixture_sm64_jrb_mixed_fog`

Acceptance: interleave factor changes, vertex loads, repeated draws, and color changes. Read back fog alpha and final pixels. Appending another fog command must leave earlier output unchanged.

**PR 5 — Compute dimensions per framebuffer extent. Size: S, 2–3 days.**

Files: `render/mod.rs`, `render/rsp_process.wgsl`, compute test helpers and framebuffer scenes.

Tests:

- `framebuffer_640x480_triangles_align_with_texrect`
- `framebuffer_mixed_extents_reuses_vertices`
- `framebuffer_modify_xy_uses_pair_extent`
- `framebuffer_pairless_logical_extent_is_unchanged`

Acceptance: untextured geometry and rectangles share exact pixel boundaries at 320×240 and 640×480, including two differently sized, differently addressed pairs in one scene. Existing pairless goldens remain unchanged.

This follows fog because both change the compute parameter layout.

**PR 6 — Tile addressing and bounded TMEM lookup. Size: M, 5–8 days.**

Files: `hle/rdp.rs`, `hle/combiner.rs`, `hle/tmem.rs`, `render/mod.rs`, `render/combiner_prelude.wgsl`; tile-addressing scenes.

Tests:

- `tile_size_subtracts_origin`
- `tile_origin_is_applied_after_shift`
- `tile_shift_all_16_values`
- `tile_mask_zero_clamps`
- `tile_mask_period_differs_from_image_extent`
- `tile_negative_wrap_and_mirror`
- `tile_clamp_precedes_mask_for_each_tap`
- `tile_large_mask_uses_bounded_tmem_lookup`
- `tile_lookup_matches_tmem_all_supported_formats`
- `lod_tiles_apply_independent_origin_and_shift`

Acceptance: exact addressed texel indices, GPU pixel probes, bounded allocation for mask 15, odd-row and split-bank cases, and unchanged ordinary TMEM decoding vectors.

**PR 7 — Filter selection and N64 filter arithmetic. Size: S–M, 3–4 days.**

Files: `hle/combiner.rs`, `render/mod.rs`, `render/combiner_prelude.wgsl`; power-meter and filter scenes.

Tests:

- `filter_point_has_no_neighbor_contribution`
- `filter_bilerp_uses_three_taps`
- `filter_average_midpoint_uses_four_taps`
- `filter_average_off_midpoint_uses_three_taps`
- `filter_mode_changes_between_draws`
- `filter_copy_forces_point`
- `fixture_sm64_power_meter_point`
- `fixture_sm64_castle_trilerp`

Acceptance: a deliberately asymmetric 2×2 texture distinguishes point, three-point, four-point, and ordinary GPU bilinear output. Power-meter edges and the equal-size castle LOD tiles receive full facade goldens.

**PR 8 — TexRect tile, bounds, and stepping. Size: S–M, 3–4 days.**

Files: `scene.rs`, `hle/interp.rs`, rectangle material entry point, rectangle helpers in `render/mod.rs`; `.n64` scenes and HUD fixture vectors.

Tests:

- `texrect_uses_command_tile_three`
- `texrect_nonzero_origin_matches_triangle_sampling`
- `texrect_lr_is_exclusive_in_one_and_two_cycle`
- `texrect_copy_lr_is_inclusive`
- `texrect_adjacent_edges_have_no_extra_pixel`
- `texrect_fractional_bounds_follow_reference`
- `texrect_flip_and_signed_copy_step`
- `fixture_sm64_hud_us_copy`
- `fixture_sm64_hud_eu_point`

Acceptance: exact coverage masks, UV endpoint assertions, a deliberately incorrect tile zero that makes wrong selection obvious, and both HUD variants. Existing fill goldens must remain unchanged.

**PR 9 — Alpha dither and compare timing. Size: S–M, 3–4 days.**

Files: frame state in `lib.rs`/`render/mod.rs`, shared combiner prelude, all fragment entry points, fixture frame metadata; transparent-Mario scene.

Tests:

- `alpha_dither_zero_half_one`
- `alpha_dither_seed_replays_exactly`
- `alpha_dither_changes_with_frame`
- `alpha_dither_is_invariant_to_run_splitting`
- `alpha_compare_uses_first_cycle_alpha`
- `alpha_compare_and_cvg_x_alpha_both_apply`
- `alpha_dither_dualsrc_matches_fallback_mask`
- `fixture_sm64_transparent_mario`

Acceptance: exact deterministic discard masks, monotonic visibility at a fixed seed, repeatable capture replay, and identical masks with dual-source disabled. Color/alpha-channel dithering is not used as a substitute.

**PR 10 — Complete the authored sm64 regression corpus and browser execution. Size: S–M, 3–5 days.**

Files: existing test harness and scene directories, fixture metadata/goldens, wasm test entry point, Cargo test dependencies, CI workflow.

Tests:

- `fixture_sm64_shadow_decal`
- `fixture_sm64_water_translucency`
- `fixture_sm64_cutout_foliage`
- `sm64_corpus_public_facade`
- `browser_sm64_fixture_replay`

Acceptance: all requested subject areas have provenance, synthetic-data descriptions, semantic assertions, and RGBA8 output. Browser WebGPU runs the high-address replay and selector/filter/dither cases without dual-source features.

This PR adds coverage, not an allowance to accept newly exposed renderer defects. A failing water or decal reference requires a named reproducer and a separate fix before the corpus gate closes.

Across these PRs, Claude should run the workspace tests with default and all features, the existing GPU matrix, formatting and Clippy, plus wasm builds with both `asm` and `capture`. The existing wasm-default build alone is insufficient. Browser execution must report adapter/backend information and fail when no adapter is available; it must not silently count a skipped GPU test as acceptance.

**Golden-change rule.** Keep the current channel tolerance of two. Each changed existing `.bin` requires:

- A named semantic fix, such as `TexgenTexelUnits`, `PointFilterSelection`, `TexRectExclusiveLR`, or `Cycle1TextureRoleSwap`.
- Before/after hashes, changed-pixel count and bounds, and the exact changed-pixel mask.
- Independent values or coverage rules explaining that mask.
- No differences outside the justified mask.

A default filter value of zero really means point; old textured goldens may therefore change when filter selection becomes effective. Editing the input scene to request bilerp merely to preserve the old image would evade the test. Broad `UPDATE_GOLDENS` output is not acceptance evidence.

**4. Consumer risks and how they are caught**

| Risk | Consumer impact | Required protection |
|---|---|---|
| Truncated addresses, eight-byte stepping, or incorrect native matrix decoding | Helix crashes or walks different commands | Host graph destruction/replay, high-address wasm fixture, fixed/float and segment-equivalence tests |
| Capturing memory after the guest resumes | Helix capture is inconsistent | Copy during the existing blocked consume interval; reject conflicting reads |
| Assuming a captured task includes prior GPU contents | Persistent Helix frames replay incorrectly | Reset/warm-up sequence requirement and multi-frame persistence fixture |
| Scene-wide fog or incomplete run keys survive the refactor | JRB settings overwrite each other | Reused-vertex and alternating-color tests, including appended irrelevant fog commands |
| Changing texgen units twice or leaving rects normalized | Metal Mario, HUD, and LOD sample different ranges | Compute readback plus triangle/TexRect sampling equivalence |
| Replacing N64 addressing with upload dimensions | Repeating or sliding textures break | Mask-versus-extent, negative-coordinate, and large-mask TMEM tests |
| Added shader resources exceed web limits | n64.toys fails pipeline creation | Default-limit device, eight compute storage buffers, wasm build and real browser replay |
| Shader control flow violates derivative rules | Browser pipeline validation fails | Derivatives remain unconditional; browser runs LOD and discard together |
| Sampling fixes alter saved playground scenes | n64.toys output changes | Named golden deltas and existing assembler scenes; preserve the BE image contract |
| Canvas size is mistaken for logical framebuffer size | n64.toys resizing moves geometry; Helix aspect policy changes | Pairless compatibility test and paired mixed-extent test |
| Dither depends on allocation or batching | Captures flicker differently after harmless refactors | Relocation and run-splitting invariance tests |
| Dual-source availability masks fallback defects | Native looks acceptable while web fails | Request a device without dual-source even on capable native adapters; compare masks and opaque selector output |

There are real remaining limits. This work does not implement full RDP coverage, complete two-cycle blending, framebuffer reinterpretation, or the known TLUT/load-command corrections. The shadows, water, decals, and foliage fixtures test the sm64 subset against those limits. A discrepancy in that subset blocks “sm64 correctness”; it cannot be dismissed merely because a more general implementation appears elsewhere in the roadmap.

The authored corpus proves specific command behavior now. Closing the roadmap’s live-game claim additionally requires Helix captures of the listed scenes after a ROM is available, replayed through the public facade and compared with independently obtained reference output. Synthetic assets must remain labelled synthetic; they do not establish Mario’s authentic reflection appearance or whole-frame JRB fidelity.

**5. Reviewable positions**

Each position can be overruled independently.

- **P1 — Accuracy target:** use the documented N64 operations and the pinned RT64 HLE formulas at native resolution; reason: this gives measurable semantics without claiming complete console rasterization.
- **P2 — Relocation:** retain captured `u64` addresses as virtual keys instead of rewriting commands; reason: it preserves host layout, aliasing, and numeric operands while remaining wasm-safe.
- **P3 — Capture exposure:** provide an opt-in capture feature and supplied backends; reason: Helix can record without first expanding the general external-memory API from roadmap item 9.
- **P4 — Initial GPU state:** require reset/warm-up or explicitly initialized targets; reason: memory reads cannot reconstruct persistent GPU attachments.
- **P5 — Texgen linear mode:** use WGSL `acos` with a stated coordinate tolerance; reason: the existing cubic is a different function, and `f64` is unnecessary.
- **P6 — Dither pattern:** use the specified frame/pixel PRNG with eight-bit midpoint thresholds; reason: it implements reproducible random rejection without pretending to know console RNG phase.
- **P7 — Sampling precision:** use RT64’s `1/128` coordinate rounding and midpoint-average rule; reason: these are a concrete, reviewable reference policy rather than unspecified “N64-like” filtering.
- **P8 — Large sampling footprints:** use the bounded CPU-decoded TMEM lookup texture; reason: masks must not require enormous uploads or a new GPU format decoder.
- **P9 — One-cycle TEXEL1:** keep explicit rejection; reason: removing the two-cycle swap bug does not establish a supported one-cycle interpretation.
- **P10 — Pairless dimensions:** preserve the explicit logical 320×240 fallback on the CPU; reason: removing shader constants should not silently redefine existing n64.toys scenes.
- **P11 — Golden changes:** permit only independently explained semantic deltas; reason: existing pixels are regression evidence, not an authority that can override command semantics.
- **P12 — Completion:** distinguish authored semantic acceptance from live sm64 acceptance; reason: no ROM-backed rendered evidence exists on this machine yet.
## Settled decisions (review, 2026-09-05)

Positions P1 through P11 are accepted as written. Two overrides and one refinement:

**Authored fixture layer is dropped.** Section 2.1 proposes symbolic allocations that assign
virtual addresses to authored command graphs. Since the design was written an sm64 US ROM
arrived and the oxideports port builds and runs on this machine, so PR 1 captures real frames
from helix instead. Isolating cases keep using what the repo already has: `.n64` asm scenes for
F3DEX2 and hand-encoded literal F3D vectors in the interpreter tests. The capture format,
recording wrapper, replay backend, and facade harness stay as designed.

**PR 1 includes the helix hook.** A small change in helix `src/render.rs`: when
`FAST3D_CAPTURE_DIR` is set, wrap `HelixHardware` in the recording backend and write one fixture
per frame listed in `FAST3D_CAPTURE_FRAMES` (frame serials, comma separated). Capture happens
inside the existing blocked-guest consume, as the design requires. Nothing else in helix
changes.

**P12 refined.** No console or rt64 reference output exists on this machine. For live fixtures
the acceptance is: independent semantic assertions on the captured data (texel ranges from the
texgen formula, fog alpha from the fog formula, filter taps from the tile state), a rendered PNG
reviewed by a person against known sm64 appearance, and only then the RGBA8 golden pinned as a
regression check. A golden pinned before review is not evidence.

**P4 in practice.** sm64 clears every framebuffer it draws each frame, so a single captured frame
is self-contained. The replay harness asserts this by replaying the frame under both
`ClearPolicy` values and requiring identical output; a frame that differs is rejected as a
fixture rather than pinned.

**Process.** One commit per PR-sized item on `fast3d-rs-sm64-fidelity`, subject line only.
Codex implements and may build and test while working; Claude re-runs the full matrix
independently (default and all features, GPU goldens, `--ignored` dual-source, fmt, clippy,
wasm32 with `asm` and `capture`) and reviews the diff before anything is considered done.
Golden changes follow the rule in section 3: named semantic fix, before and after hashes, the
changed-pixel mask, and an independent explanation of that mask.
