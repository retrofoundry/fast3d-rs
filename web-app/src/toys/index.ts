import { OFFICIAL_OWNER, type Toy } from "./types";
import onetri from "@toys/onetri.n64?raw";
import texturedQuad from "@toys/textured-quad.n64?raw";
import flatColor from "@toys/flat-color.n64?raw";
import backfaceCulling from "@toys/backface-culling.n64?raw";
import matrixStack from "@toys/matrix-stack.n64?raw";
import segmentedSubDl from "@toys/segmented-sub-dl.n64?raw";
import perspectiveCube from "@toys/perspective-cube.n64?raw";
import lights from "@toys/lights.n64?raw";
import chromeIcosphere from "@toys/chrome-icosphere.n64?raw";
import morphcube from "@toys/morphcube.n64?raw";
import twoCycleCombiner from "@toys/two-cycle-combiner.n64?raw";
import i8Ramp from "@toys/i8-ramp.n64?raw";
import i4Ramp from "@toys/i4-ramp.n64?raw";
import ia16Ramp from "@toys/ia16-ramp.n64?raw";
import ia8Ramp from "@toys/ia8-ramp.n64?raw";
import ia4Ramp from "@toys/ia4-ramp.n64?raw";
import wrapRepeat from "@toys/wrap-repeat.n64?raw";
import mirrorRepeat from "@toys/mirror-repeat.n64?raw";
import ci8Ramp from "@toys/ci8-ramp.n64?raw";
import ci8Canary from "@toys/ci8-canary.n64?raw";
import ci4Grid from "@toys/ci4-grid.n64?raw";
import ci4Canary from "@toys/ci4-canary.n64?raw";
import multiMaterial from "@toys/multi-material.n64?raw";
import tron from "@toys/tron.n64?raw";
import fogworld from "@toys/fogworld.n64?raw";
import alphaThreshold from "@toys/alpha-threshold.n64?raw";
import decal from "@toys/decal.n64?raw";
import highPoly from "@toys/high-poly.n64?raw";
import fillTexrect from "@toys/fill-texrect.n64?raw";
import hudOver3d from "@toys/hud-over-3d.n64?raw";
import offscreenThenSample from "@toys/offscreen-then-sample.n64?raw";
import texrectflip from "@toys/texrectflip.n64?raw";
import brickTexUrl from "../brick_tex.png";
import checkerTexUrl from "../checker_tex.png";
import chromeEnvUrl from "../chrome_env.png";
import gradientTexUrl from "../gradient_tex.png";
import ci8BinUrl from "../ci8_tex.bin?url";
import ci4BinUrl from "../ci4_tex.bin?url";
import quadTexBinUrl from "../quad_tex.bin?url";

export const TOYS: Toy[] = [
  { slug: "onetri", id: "onetri", title: "One Triangle",
    description: "The SDK's simplest gfx sample: an untextured Gouraud quad spinning about Z.",
    owner: OFFICIAL_OWNER, category: "Basics", tags: ["shade", "animation"], schemaVersion: 1, source: onetri },
  { slug: "textured-quad", id: "textured-quad", title: "Textured Quad",
    description: "Engine-capability demo: RGBA16 single-tile texture mapping — a 32×32 brick texture on a quad, spinning about Z (TEXEL0 × SHADE, 1-cycle); no 1:1 SDK sample.",
    owner: OFFICIAL_OWNER, category: "Basics", tags: ["texture", "animation"], schemaVersion: 1,
    texture: brickTexUrl, source: texturedQuad },
  { slug: "flat-color", id: "flat-color", title: "Flat Color",
    description: "A quad filled with a constant primitive color via the combiner.",
    owner: OFFICIAL_OWNER, category: "Basics", tags: ["combiner"], schemaVersion: 1, source: flatColor },
  { slug: "backface-culling", id: "backface-culling", title: "Backface Culling",
    description: "A triangle culled when it turns away — G_CULL_BACK drops the back face as it spins.",
    owner: OFFICIAL_OWNER, category: "Geometry", tags: ["culling", "animation"], schemaVersion: 1, source: backfaceCulling },
  { slug: "matrix-stack", id: "matrix-stack", title: "Matrix Stack",
    description: "Three Gouraud-shaded siblings placed via gsSPMatrix PUSH/MUL + gsSPPopMatrix — flat siblings on the modelview stack, not a nested hierarchy.",
    owner: OFFICIAL_OWNER, category: "Geometry", tags: ["matrix"], schemaVersion: 1, source: matrixStack },
  { slug: "segmented-sub-dl", id: "segmented-sub-dl", title: "Segmented Sub-DL",
    description: "A checker-textured quad drawn twice via gsSPSegment + gsSPDisplayList sub-DL reuse, placed left and right with matrix push/pop.",
    owner: OFFICIAL_OWNER, category: "Geometry", tags: ["segments", "sub-dl"], schemaVersion: 1,
    texture: checkerTexUrl, source: segmentedSubDl },
  { slug: "perspective-cube", id: "perspective-cube", title: "Perspective Cube",
    description: "Hello-3D — a Gouraud cube under real perspective, depth-sorted by the Z-buffer, spinning about Y.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["perspective", "z-buffer", "animation"], schemaVersion: 1, source: perspectiveCube },
  { slug: "lights", id: "lights", title: "Lights",
    description: "The SDK lights demo — a low-poly Utah teapot under hardware lighting (gdSPDefLights2: two yellow directional lights + ambient), shaded per-vertex from s8 normals through a SHADE-only combiner, spinning about Y.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["lighting", "sdk", "animation"], schemaVersion: 1, source: lights },
  { slug: "chrome-icosphere", id: "chrome-icosphere", title: "Chrome Icosphere",
    description: "SDK chrome demo (G_TEXTURE_GEN + LookAt, authentic G_CC_DECALRGB combiner): world-anchored spherical reflection mapping, spinning about Y — divergence: reflects an icosphere instead of the SDK's Ultra64-logo mesh.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["texgen", "reflection", "animation"], schemaVersion: 1,
    texture: chromeEnvUrl, source: chromeIcosphere },
  { slug: "morphcube", id: "morphcube", title: "Morph Cube",
    description: "Vertex morphing — the SDK morphcube technique: a cube continuously morphs into a sphere via per-vertex lerp, baked each frame by the GBI assembler.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["morph", "animation", "sdk"], schemaVersion: 1, source: morphcube },
  { slug: "two-cycle-combiner", id: "two-cycle-combiner", title: "Two-Cycle Combiner",
    description: "Engine-capability demo: a vivid 4-corner SHADE gradient (red/green/blue/white) tinted by a warm-orange PRIMITIVE — cycle 0 routes the gradient into COMBINED, cycle 1 multiplies by PRIM, making the 2-cycle COMBINED path visually verifiable at a glance.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["combiner", "2cycle"], schemaVersion: 1, source: twoCycleCombiner },
  { slug: "i8-ramp", id: "i8-ramp", title: "I8 Intensity Ramp",
    description: "Texture-format demo: G_IM_FMT_I / G_IM_SIZ_8b — 8-bit intensity (grayscale). Source luma is stored in 256 levels; the decoder replicates each byte to RGB. Horizontal ramp shows a smooth gradient — compare I4's 16 banded steps.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "i8", "intensity"], schemaVersion: 1,
    texture: gradientTexUrl, source: i8Ramp },
  { slug: "i4-ramp", id: "i4-ramp", title: "I4 Intensity Ramp",
    description: "Texture-format demo: G_IM_FMT_I / G_IM_SIZ_4b — 4-bit intensity (16 levels). Two texels per byte; high nibble = even column. 4-bit value replicated to 8 bits: v8 = (v4 << 4) | v4. Horizontal ramp shows 16 visible bands — compare I8's smooth gradient.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "i4", "intensity"], schemaVersion: 1,
    texture: gradientTexUrl, source: i4Ramp },
  { slug: "ia16-ramp", id: "ia16-ramp", title: "IA16 Intensity+Alpha Ramp",
    description: "Texture-format demo: G_IM_FMT_IA / G_IM_SIZ_16b — 16-bit intensity+alpha (8+8 bits). Shows the decoded 8-bit INTENSITY as a smooth horizontal ramp. The 8-bit alpha decodes correctly too (verified by the renderer goldens), but visualizing it on-screen needs alpha compositing — the Tier-2 blender — so it isn't shown here.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ia16", "intensity", "alpha"], schemaVersion: 1,
    texture: gradientTexUrl, source: ia16Ramp },
  { slug: "ia8-ramp", id: "ia8-ramp", title: "IA8 Intensity+Alpha Ramp",
    description: "Texture-format demo: G_IM_FMT_IA / G_IM_SIZ_8b — 8-bit intensity+alpha (4+4 bits). High nibble = 4-bit intensity (shown, as ~16-level horizontal banding); low nibble = 4-bit alpha. Both expand via (v4<<4)|v4. The alpha decodes correctly (goldens) but visualizing it needs the Tier-2 blender.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ia8", "intensity", "alpha"], schemaVersion: 1,
    texture: gradientTexUrl, source: ia8Ramp },
  { slug: "ia4-ramp", id: "ia4-ramp", title: "IA4 Intensity+Alpha Ramp",
    description: "Texture-format demo: G_IM_FMT_IA / G_IM_SIZ_4b — 4-bit intensity+alpha (3+1 bits). Two texels per byte; bits [3:1] = 3-bit intensity (shown, as coarse ~8-level horizontal banding), bit [0] = 1-bit alpha. The alpha decodes correctly (goldens) but visualizing it needs the Tier-2 blender.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ia4", "intensity", "alpha"], schemaVersion: 1,
    texture: gradientTexUrl, source: ia4Ramp },
  { slug: "wrap-repeat", id: "wrap-repeat", title: "Wrap Repeat",
    description: "Sampler demo: G_TX_WRAP (cms=cmt=0) — UVs spanning [0,2] tile the checker texture 2×2 via the wgpu Repeat address mode, making wrap vs. clamp visually apparent.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "wrap", "sampler"], schemaVersion: 1,
    texture: checkerTexUrl, source: wrapRepeat },
  { slug: "mirror-repeat", id: "mirror-repeat", title: "Mirror Repeat",
    description: "Sampler demo: G_TX_MIRROR (cms=cmt=1) — same UVs as wrap-repeat but reflected at each tile boundary via the wgpu MirrorRepeat address mode.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "mirror", "sampler"], schemaVersion: 1,
    texture: checkerTexUrl, source: mirrorRepeat },
  { slug: "ci8-ramp", id: "ci8-ramp", title: "CI8 Color-Index Ramp",
    description: "Texture-format demo: G_IM_FMT_CI / G_IM_SIZ_8b — 8-bit color-indexed with RGBA16 TLUT (G_TT_RGBA16). Assembler auto-derives the 32-entry palette + emits G_LOADTLUT. Flat horizontal bands, each row a distinct palette color, make TMEM row-swaps instantly visible (swizzle canary). MODULATE combiner (TEXEL0 × SHADE) passes through the palette RGB.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ci8", "palette", "tlut"], schemaVersion: 1,
    texture: ci8BinUrl, source: ci8Ramp },
  { slug: "ci8-canary", id: "ci8-canary", title: "CI8 Alpha-Route Canary",
    description: "Texture-format demo: CI8 combine-route canary — TEXEL0_ALPHA (palette entry a1 bit) routed into the RGB output via the color_c slot (index 8). Palette has alternating a1=1/0 across the index ramp; output is alternating white/black bands. Validates the TEXEL0_ALPHA→color path with CI8+TLUT (guards the IA combine-route gap).",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ci8", "palette", "tlut", "alpha"], schemaVersion: 1,
    texture: ci8BinUrl, source: ci8Canary },
  { slug: "ci4-grid", id: "ci4-grid", title: "CI4 Color-Index Grid",
    description: "Texture-format demo: G_IM_FMT_CI / G_IM_SIZ_4b — 4-bit color-indexed with RGBA16 TLUT (G_TT_RGBA16). Assembler auto-derives the 16-entry palette + emits G_LOADTLUT. 32×32 image split into a 4×4 grid of 8×8 solid-color cells, each a distinct rainbow hue. Flat regions make palette-index scrambles (nibble-order or TMEM bugs) immediately visible. MODULATE combiner (TEXEL0 × SHADE) passes through the palette RGB.",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ci4", "palette", "tlut"], schemaVersion: 1,
    texture: ci4BinUrl, source: ci4Grid },
  { slug: "ci4-canary", id: "ci4-canary", title: "CI4 Alpha-Route Canary",
    description: "Texture-format demo: CI4 combine-route canary — TEXEL0_ALPHA (palette entry a1 bit) routed into the RGB output via the color_c slot (index 8). Palette has alternating a1=1/0 across the 16 color indices; output is alternating white/black 8×8 cells. Validates the TEXEL0_ALPHA→color path with CI4+TLUT (guards the IA combine-route gap).",
    owner: OFFICIAL_OWNER, category: "Texture Formats", tags: ["texture", "ci4", "palette", "tlut", "alpha"], schemaVersion: 1,
    texture: ci4BinUrl, source: ci4Canary },
  { slug: "multi-material", id: "multi-material", title: "Multi-Material",
    description: "Pipeline demo: three quads in one display list, each with its own gsDPSetCombineLERP + gsDPSetRenderMode — opaque textured (OPA_SURF), flat-primitive XLU (XLU_SURF, renders opaque until Phase B blender), and cutout-placeholder textured (TEX_EDGE, renders opaque until Phase D alpha-test). Proves per-run material binding: three distinct screen regions, not the old single-material collapse.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["multi-material", "combiner", "render-mode"], schemaVersion: 1,
    texture: quadTexBinUrl, source: multiMaterial },
  { slug: "tron", id: "tron", title: "Tron Panels",
    description: "Blender demo: two overlapping translucent neon panels (cyan + magenta) using G_RM_AA_ZB_XLU_SURF with SHADE alpha ≈ 0.5. The overlap band shows a blended mix of both colors — not either alone — proving the XLU translucent blender (Phase B). Canonical B=1MA lerp renders identically via dual-source and AlphaOver fallback paths.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["xlu", "blending", "render-mode", "animation"], schemaVersion: 1, source: tron },
  { slug: "fogworld", id: "fogworld", title: "Fog World",
    description: "Fog demo: two quads at different z-depths with G_RM_FOG_SHADE_A + G_CYC_2CYCLE distance fog. Near geometry renders crisp (fog_alpha=0); far geometry dissolves into the fog color (fog_alpha≈0.86). Proves the Phase C fog pipeline: per-vertex fog factor → shade alpha → shader mix(surface, fog_color, factor).",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["fog", "render-mode", "depth"], schemaVersion: 1, source: fogworld },
  { slug: "alpha-threshold", id: "alpha-threshold", title: "Alpha Threshold",
    description: "Alpha-test demo: G_AC_THRESHOLD alpha-compare mode with gsDPSetBlendColor. Gouraud vertex alpha varies left→right across a textured quad; the left half (combiner alpha < blendColor.a=128/255≈0.502) is discarded, the right half survives. Proves the THRESHOLD path — threshold set by gsDPSetBlendColor, distinct from TEX_EDGE's fixed 0.125 (Phase D).",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["alpha-test", "render-mode", "threshold"], schemaVersion: 1,
    texture: quadTexBinUrl, source: alphaThreshold },
  { slug: "decal", id: "decal", title: "Decal",
    description: "Decal demo: a coplanar ZMODE_DEC quad (G_RM_AA_ZB_OPA_DECAL) painted ON a base surface with no z-fighting, plus a nearer opaque quad that OCCLUDES it. The decal fragment samples the opaque pass's depth and does the Z-occlusion + NDC-space coplanar-tolerance discard in-shader (two-phase decal pass, depth-as-sampled-texture). Proves the Phase E decal path.",
    owner: OFFICIAL_OWNER, category: "3D", tags: ["decal", "z-buffer", "render-mode"], schemaVersion: 1, source: decal },
  { slug: "high-poly", id: "high-poly", title: "High-Poly Multi-Batch",
    description: "Multi-batch vertex-loading guard: 5 × gsSPVertex(verts,28,0) reloads accumulate 140 global vertex entries (> 127) of a blue grid via the A5 cache→global-index path, then a 6th batch loads a red marker triangle at slots the mesh batches never touch — placing it at global indices 168-170 (post-127). The marker is uniquely tied to that final batch (global 0-2 hold blue mesh, not red), so a slot-reuse / wrong-index reload regression would draw it the wrong color and be caught, proving accumulation resolves correctly beyond the 127-entry mark.",
    owner: OFFICIAL_OWNER, category: "Geometry", tags: ["multi-batch", "vertices", "z-buffer"], schemaVersion: 1, source: highPoly },
  { slug: "fill-texrect", id: "fill-texrect", title: "Fill + TexRect",
    description: "2D pipeline demo: FILL mode clears a 64×64 CIMG to a solid blue, then a COPY-cycle gsSPTextureRectangle blits the quad_tex 4×4 checker over the scanout — proving the two-pass FILL→COPY flow in one display list.",
    owner: OFFICIAL_OWNER, category: "2D", tags: ["2d", "fill", "texrect", "copy-cycle"],
    schemaVersion: 1, texture: quadTexBinUrl, source: fillTexrect },
  { slug: "hud-over-3d", id: "hud-over-3d", title: "HUD Over 3D",
    description: "CIMG-first composite: a Gouraud-shaded spinning quad (3D scene, G_CYC_1CYCLE) followed by a COPY-cycle TEXRECT overlaying the quad_tex checker as a HUD element — demonstrating the 3D + 2D overlay pattern within a single framebuffer-paired DL.",
    owner: OFFICIAL_OWNER, category: "2D", tags: ["2d", "hud", "texrect", "composite", "animation"],
    schemaVersion: 1, texture: quadTexBinUrl, source: hudOver3d },
  { slug: "offscreen-then-sample", id: "offscreen-then-sample", title: "Offscreen Then Sample",
    description: "Off-screen render: FILL mode paints a scratch 64×64 CIMG (addr 0x00200000) orange, then a second CIMG (scanout at 0x00100000) samples the scratch buffer via gsDPSetTextureImage + TEXRECT — exercising the two-pair offscreen-FB-as-texture pipeline.",
    owner: OFFICIAL_OWNER, category: "2D", tags: ["2d", "offscreen", "texrect", "framebuffer"],
    schemaVersion: 1, source: offscreenThenSample },
  { slug: "texrectflip", id: "texrectflip", title: "TexRect Flip",
    description: "2D pipeline demo: gsSPTextureRectangleFlip (opcode G_TEXRECTFLIP) maps the quad_tex 4×4 checker with S and T axes swapped, rotating the pattern 90° on-screen — isolating the flip-mode GBI encoding from the standard gsSPTextureRectangle path.",
    owner: OFFICIAL_OWNER, category: "2D", tags: ["2d", "texrect", "flip", "copy-cycle"],
    schemaVersion: 1, texture: quadTexBinUrl, source: texrectflip },
];
