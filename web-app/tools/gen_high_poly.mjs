/**
 * gen_high_poly.mjs — generate high-poly.n64 toy source.
 *
 * Demonstrates multi-batch vertex loading AND guards the A5 cache→global-index accumulation
 * path beyond the 127-entry mark — with a marker that UNIQUELY depends on the final batch.
 *
 * Assembler constraint: every `gsSPVertex(verts,n,v0)` loads from the SAME base address, so
 * slot S always receives `verts[S]`. To make the red marker provable ONLY by the post-127
 * batch, the marker verts live at slots 28-30, which the blue-mesh batches (count=28 → slots
 * 0-27) NEVER load. Only the final batch (count=31) reaches verts[28..31] and places the red
 * marker at global indices 168-170. Global indices 0-2 hold BLUE mesh corners, not red — so a
 * slot-reuse / wrong-global-index regression in the reload path would draw the marker the wrong
 * color (blue/background) and FAIL the pixel assertion, rather than silently passing.
 *
 * Vertex pool (31 entries):
 *   verts[0..28]  — blue 4×7 grid mesh (slots 0-27)
 *   verts[28..31] — red marker triangle (slots 28-30); loaded ONLY by the final batch
 *
 * Display list:
 *   Batches 1-5: gsSPVertex(verts,28,0) + 18 gsSP2Triangles → blue mesh; globals 0-139 (>127)
 *   Batch 6:     gsSPVertex(verts,31,0) + gsSP1Triangle(28,29,30) → red marker; globals 140-170,
 *                marker at 168-170 (post-127, uniquely from this batch)
 *
 * Run with: node web-app/tools/gen_high_poly.mjs > examples/toys/high-poly.n64
 */

// Grid x and y positions for the blue mesh body (4 columns × 7 rows = 28 verts).
const XS = [0, 43, 85, 128];
const YS = [128, 85, 43, 0, -43, -85, -128];

const N_MESH = XS.length * YS.length; // 28
const MESH_BATCH = N_MESH; // 28 → slots 0-27
const MARKER_BATCH = N_MESH + 3; // 31 → slots 0-30 (marker at 28-30)
const N_MESH_BATCHES = 5; // 5 × 28 = 140 globals (> 127) before the marker batch

const lines = [];

lines.push(
  `// high-poly — multi-batch vertex-loading guard (marker uniquely from the post-127 batch).`,
  `// 5 × gsSPVertex(verts,28,0) reloads accumulate 140 global entries (> 127) of the blue mesh;`,
  `// the 6th batch gsSPVertex(verts,31,0) loads the red marker verts (slots 28-30 — NEVER touched`,
  `// by the mesh batches) at global indices 168-170. Global 0-2 hold BLUE mesh corners, so the red`,
  `// marker at pixel (10,10) proves the A5 cache→global-index path resolved the post-127 batch`,
  `// correctly: a slot-reuse / wrong-index regression would draw blue/background there and FAIL.`,
  `Mtx proj  = scale(0.0078125)`,
  `Mtx model = identity()`,
  `Vp { 640, 480, 511, 0, 640, 480, 511, 0 }`,
);

// Mesh body (slots 0-27): blue 4×7 grid at x≥0 → screen x≥48, never at pixel (10,10).
lines.push(`// Mesh body (slots 0-27): blue 4×7 grid at x≥0 (screen x≥48 in a 96×96 render).`);
for (const y of YS) {
  for (const x of XS) {
    lines.push(
      `Vtx { ${String(x).padStart(4)}, ${String(y).padStart(4)}, 0, 0, 0, 0,   0,  50, 200, 255 }`,
    );
  }
}

// Marker vertices (slots 28-30): red, top-left triangle. Loaded ONLY by the final batch.
// Screen mapping with scale(0.0078125) + Vp{640,480,...}: ndc = vtx/128.
//   V28 (-128,128) → screen (0,0)    V29 (-70,128) → screen (22,0)
//   V30 (-128,70)  → screen (0,22)   → triangle covers pixel (10,10). ✓
lines.push(
  `// Marker (slots 28-30): red, top-left — covers pixel (10,10). Loaded ONLY by the final batch,`,
  `// so it lands at global 168-170 (post-127); the mesh batches (count=28) never reach these slots.`,
  `Vtx { -128,  128, 0, 0, 0, 0, 255,   0,   0, 255 }`,
  `Vtx {  -70,  128, 0, 0, 0, 0, 255,   0,   0, 255 }`,
  `Vtx { -128,   70, 0, 0, 0, 0, 255,   0,   0, 255 }`,
  ``,
  `gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)`,
  `gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)`,
  `gsSPViewport(vp)`,
  `gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER)`,
  `gsDPSetOtherMode_H(G_CYC_1CYCLE)`,
  `gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)`,
  `gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)`,
);

// Build the 18 triangle-pair commands for the blue mesh (6 row-intervals × 3 col-intervals).
const meshTris = [];
for (let r = 0; r < YS.length - 1; r++) {
  for (let c = 0; c < XS.length - 1; c++) {
    const v0 = r * XS.length + c; // top-left   (slots 0-27)
    const v1 = v0 + 1; // top-right
    const v2 = v0 + XS.length; // bottom-left
    const v3 = v2 + 1; // bottom-right
    meshTris.push(`gsSP2Triangles(${v0}, ${v1}, ${v2}, 0, ${v1}, ${v3}, ${v2}, 0)`);
  }
}

// Emit the blue-mesh reload batches (each count=28 → slots 0-27).
for (let batch = 1; batch <= N_MESH_BATCHES; batch++) {
  const lo = (batch - 1) * MESH_BATCH;
  const hi = batch * MESH_BATCH - 1;
  lines.push(
    ``,
    `// Batch ${batch}: global ${lo}-${hi} — reload the 28 blue mesh verts, draw the grid.`,
    `gsSPVertex(verts, ${MESH_BATCH}, 0)`,
    ...meshTris,
  );
}

// Marker batch: loads count=31 (verts[0..31]); the red marker verts land at slots 28-30,
// global 168-170 (post-127). Only the marker triangle is drawn from this batch.
const markerLo = N_MESH_BATCHES * MESH_BATCH; // 140
const markerGlobal = markerLo + N_MESH; // 168
lines.push(
  ``,
  `// Marker batch: global ${markerLo}-${markerLo + MARKER_BATCH - 1} — load 31 verts; the red`,
  `// marker (slots 28-30) lands at global ${markerGlobal}-${markerGlobal + 2} (post-127, batch-exclusive).`,
  `gsSPVertex(verts, ${MARKER_BATCH}, 0)`,
  `gsSP1Triangle(${N_MESH}, ${N_MESH + 1}, ${N_MESH + 2}, 0)`,
  ``,
  `gsSPEndDisplayList()`,
);

process.stdout.write(lines.join("\n") + "\n");
