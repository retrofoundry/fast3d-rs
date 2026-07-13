// Generate examples/toys/morphcube.n64 — vertex morphing (the SDK morphcube technique), but a GENUINE
// cube↔sphere morph rather than a uniform scaling. The trick: 8 cube corners projected onto a sphere
// are still a (smaller) cube, so the old 8-corner "sphere" was just the cube scaled down — the morph
// read as a zoom, not a shape change. A real morph needs a SUBDIVIDED cube: with face-center and
// edge-midpoint verts present, the sphere target pulls the corners IN (radius 69→40) while the face
// centers stay put (radius 40), rounding the silhouette into an actual sphere.
//
// Geometry: a spherified cube at frequency 2 — a 3×3 grid per face (coords in {−S, 0, +S}). Deduping
// the verts shared along edges/corners across the 6 faces yields 26 unique verts:
//   8 corners (±S,±S,±S) + 12 edge-midpoints (e.g. ±S,±S,0) + 6 face-centers (e.g. 0,0,±S) = 26.
// 26 ≤ 127, so the whole pool fits one gsSPVertex (F3DEX2 caps v0+n at 127). Each face is a 2×2 grid
// of quads → 2 tris/quad × 4 quads × 6 faces = 48 triangles (24 gsSP2Triangles). Per-vertex colors are
// assigned by position (signed octant) for a legible Gouraud look that survives the morph.
//
//   VtxSet cube   = the 26 grid points on the CUBE surface.
//   VtxSet sphere = each grid point normalized to radius S (p/|p|·S, rounded). Corners pull in from
//                   radius S√3≈69 to S=40, edge-mids from S√2≈56 to S=40, face-centers stay at S — so
//                   ALL sphere verts sit at radius ~40: a real sphere, not a scaled cube.
//   morph verts   = lerp(cube, sphere, (1 - cos(time)) / 2): t=0 → cube (stable t=0 endpoint), t=PI → sphere.
//
// No deps; pure Node text emitter. Authored .n64 stays portable to real N64 C (morph bakes at assemble
// time — no time in hle/renderer). Run: node web-app/tools/gen_morphcube.mjs
import { writeFileSync } from "node:fs";

const S = 40; // cube half-extent; sphere target radius. Verts well inside the z=200 camera frustum.
const F = 2; // grid frequency: 3×3 grid per face (coords −S,0,+S) → 26 unique verts after dedup.

// The 6 cube faces as (origin, uAxis, vAxis): the face plane is origin + u·U + v·V for u,v ∈ [−1,1].
// Winding (U×V outward) makes each face's grid-quad triangles front-face outward; the demo doesn't
// cull (Z-buffer handles occlusion) so winding is cosmetic, but kept consistent.
const faces = [
  { o: [0, 0, 1], u: [1, 0, 0], v: [0, 1, 0] }, // +Z
  { o: [0, 0, -1], u: [-1, 0, 0], v: [0, 1, 0] }, // −Z
  { o: [1, 0, 0], u: [0, 0, -1], v: [0, 1, 0] }, // +X
  { o: [-1, 0, 0], u: [0, 0, 1], v: [0, 1, 0] }, // −X
  { o: [0, 1, 0], u: [1, 0, 0], v: [0, 0, -1] }, // +Y
  { o: [0, -1, 0], u: [1, 0, 0], v: [0, 0, 1] }, // −Y
];

// Dedup cube grid verts by rounded integer position (corner/edge verts are shared across faces).
const verts = []; // each: [x,y,z] on the cube surface in {−S,0,+S}
const vmap = new Map();
const key = (p) => p.map((x) => Math.round(x)).join(",");
function addVert(p) {
  const k = key(p);
  if (vmap.has(k)) return vmap.get(k);
  verts.push(p.map((x) => Math.round(x)));
  const idx = verts.length - 1;
  vmap.set(k, idx);
  return idx;
}

const tris = [];
for (const { o, u, v } of faces) {
  // grid[i][j] for i,j in 0..F: position = (o + (2i/F−1)·u + (2j/F−1)·v) · S
  const grid = [];
  for (let i = 0; i <= F; i++) {
    grid[i] = [];
    for (let j = 0; j <= F; j++) {
      const su = (2 * i) / F - 1;
      const sv = (2 * j) / F - 1;
      const p = [
        (o[0] + su * u[0] + sv * v[0]) * S,
        (o[1] + su * u[1] + sv * v[1]) * S,
        (o[2] + su * u[2] + sv * v[2]) * S,
      ];
      grid[i][j] = addVert(p);
    }
  }
  // 2×2 quads → 2 tris each. Quad (i,j): verts (i,j),(i+1,j),(i+1,j+1),(i,j+1).
  for (let i = 0; i < F; i++) {
    for (let j = 0; j < F; j++) {
      const a = grid[i][j];
      const b = grid[i + 1][j];
      const c = grid[i + 1][j + 1];
      const d = grid[i][j + 1];
      tris.push([a, b, c]);
      tris.push([a, c, d]);
    }
  }
}

if (verts.length !== 26) {
  throw new Error(`unexpected vert count ${verts.length} (expected 26: 8 corners + 12 edge-mids + 6 face-centers)`);
}
if (tris.length !== 48) {
  throw new Error(`unexpected tri count ${tris.length} (expected 48: 2×4×6)`);
}
if (verts.some((p) => p.some((c) => Math.abs(c) > 127))) {
  throw new Error("vert index/coord out of range");
}

// Per-vertex color by signed octant of the cube position: gives each region a distinct hue so the
// Gouraud shading reads the rounding clearly. sign(x) ∈ {−1,0,+1} maps to a low/mid/high channel.
const chan = (s) => (s > 0 ? 220 : s < 0 ? 80 : 150);
const colorOf = (p) => [chan(p[0]), chan(p[1]), chan(p[2])];

// Sphere target: normalize each cube grid point to radius S (rounded to int).
const sphereOf = (p) => {
  const l = Math.hypot(p[0], p[1], p[2]);
  return [Math.round((p[0] / l) * S), Math.round((p[1] / l) * S), Math.round((p[2] / l) * S)];
};

// Self-check: every sphere vert must be at radius ≈ S (a real sphere), while the cube spans S..S√3.
for (const p of verts) {
  const s = sphereOf(p);
  const rs = Math.hypot(s[0], s[1], s[2]);
  if (Math.abs(rs - S) > 2) {
    throw new Error(`sphere vert not at radius ~${S}: |${s}| = ${rs.toFixed(2)}`);
  }
}

// VtxSet blocks MUST be single-line (the parser is line-by-line). Use the color `Vtx` form (not VtxN)
// so the morph keeps per-vertex Gouraud colors with no lighting block. Layout: { x,y,z, flag, s,t, r,g,b,a }.
const vtxBlock = (positions) =>
  positions
    .map((p, i) => {
      const [r, g, b] = colorOf(verts[i]); // color keyed to the CUBE octant for both operands (1:1 order)
      return `Vtx { ${p[0]}, ${p[1]}, ${p[2]}, 0, 0, 0, ${r}, ${g}, ${b}, 255 }`;
    })
    .join(" ");

const cubeLine = vtxBlock(verts);
const sphereLine = vtxBlock(verts.map(sphereOf));

// Triangle list: pack pairs into gsSP2Triangles (48 is even → 24 calls).
const triLines = [];
for (let i = 0; i < tris.length; i += 2) {
  const [a, b, c] = tris[i];
  const [d, e, f] = tris[i + 1];
  triLines.push(`gsSP2Triangles(${a}, ${b}, ${c}, 0, ${d}, ${e}, ${f}, 0)`);
}

const src = `// morphcube — a subdivided cube (26 verts) morphs to a sphere via per-vertex lerp:
// weight = (1 − cos(time)) / 2; Gouraud per-octant colors; spins about Y.
Mtx proj  = perspective(45, 1.3333, 10, 1000, 1)
Mtx view  = lookat(0, 0, 200, 0, 0, 0, 0, 1, 0)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
VtxSet cube = { ${cubeLine} }
VtxSet sphere = { ${sphereLine} }
morph verts = lerp(cube, sphere, (1 - cos(time)) / 2)
update {
  guRotate(model, time * 30, 0, 1, 0)
}
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPPerspNormalize(proj)
gsSPMatrix(view, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_MUL | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, ${verts.length}, 0)
${triLines.join("\n")}
gsSPEndDisplayList()
`;

writeFileSync(new URL("../../examples/toys/morphcube.n64", import.meta.url), src);
console.log(
  `wrote morphcube.n64: ${verts.length} verts, ${tris.length} tris (${triLines.length} gsSP2Triangles); sphere target at radius ~${S}`,
);
