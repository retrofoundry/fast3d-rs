// Generate examples/toys/chrome-icosphere.n64 with a frequency-3 geodesic icosphere
// (92 verts, 180 faces) so the spherical reflection reads as a smooth mirror ball rather than a
// coarse 12-vertex blob. 92 is the smoothest geodesic that fits a SINGLE gsSPVertex: F3DEX2's
// G_VTX encodes the end index (v0+n) in a 7-bit field, so v0+n <= 127 per load, and our authoring
// loads only from the pool start (no multi-batch offset). The next geodesic step (frequency 4) is
// 162 verts, which overflows that field. Verts lie on a sphere of the icosahedron radius
// (40*sqrt(1+phi^2) ~= 76); normals = normalized position (s8 *127). No culling in the demo
// (Z-buffer handles occlusion), so winding is free.
import { writeFileSync } from "node:fs";

const phi = (1 + Math.sqrt(5)) / 2;
const S = 40;                          // base scale -> icosahedron verts at (+-40,+-65,0) etc.
const R = S * Math.hypot(1, phi);      // sphere radius ~76.08
const F = 3;                           // geodesic frequency (92 verts, 180 faces)

const normalize = (v) => { const l = Math.hypot(v[0], v[1], v[2]); return [v[0] / l, v[1] / l, v[2] / l]; };

// base icosahedron
const ico = [
  [-1, phi, 0], [1, phi, 0], [-1, -phi, 0], [1, -phi, 0],
  [0, -1, phi], [0, 1, phi], [0, -1, -phi], [0, 1, -phi],
  [phi, 0, -1], [phi, 0, 1], [-phi, 0, -1], [-phi, 0, 1],
].map(normalize);
const icoFaces = [
  [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
  [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
  [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
  [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
];

// frequency-F barycentric subdivision of each face, deduping shared verts by rounded position
const verts = [];
const vmap = new Map();
const key = (p) => p.map((x) => Math.round(x * 1e5)).join(",");
function addVert(p) {
  const n = normalize(p), k = key(n);
  if (vmap.has(k)) return vmap.get(k);
  verts.push(n); const idx = verts.length - 1; vmap.set(k, idx); return idx;
}
const faces = [];
for (const [ia, ib, ic] of icoFaces) {
  const A = ico[ia], B = ico[ib], C = ico[ic];
  const pt = (i, j) => { // barycentric (i along A->B, j along A->C); weights sum to F
    const a = (F - i - j) / F, b = i / F, c = j / F;
    return [A[0] * a + B[0] * b + C[0] * c, A[1] * a + B[1] * b + C[1] * c, A[2] * a + B[2] * b + C[2] * c];
  };
  const grid = [];
  for (let i = 0; i <= F; i++) { grid[i] = []; for (let j = 0; j <= F - i; j++) grid[i][j] = addVert(pt(i, j)); }
  for (let i = 0; i < F; i++) for (let j = 0; j < F - i; j++) {
    faces.push([grid[i][j], grid[i + 1][j], grid[i][j + 1]]);                  // up
    if (i + j < F - 1) faces.push([grid[i + 1][j], grid[i + 1][j + 1], grid[i][j + 1]]); // down
  }
}
if (verts.length !== 92 || faces.length !== 180) {
  throw new Error(`unexpected geodesic: ${verts.length} verts, ${faces.length} faces (expected 92/180)`);
}

// emit
const r = (x) => Math.round(x);
const s8 = (x) => Math.max(-128, Math.min(127, Math.round(x * 127)));
const vtxLines = verts.map((n) =>
  `VtxN { ${r(n[0] * R)}, ${r(n[1] * R)}, ${r(n[2] * R)}, 0, 0, 0, ${s8(n[0])}, ${s8(n[1])}, ${s8(n[2])}, 255 }`
).join("\n");
const triLines = [];
for (let i = 0; i < faces.length; i += 2) {
  const [a, b, c] = faces[i], [d, e, f] = faces[i + 1];
  triLines.push(`gsSP2Triangles(${a}, ${b}, ${c}, 0, ${d}, ${e}, ${f}, 0)`); // 180 even -> 90 calls
}

const src = `// chrome-icosphere — a mirror-ball geodesic icosphere (freq-3, 92 verts) with spherical reflection
// mapping (G_TEXTURE_GEN + LookAt): a world-anchored env spheremap sampled via TEXEL0 passthrough.
Texture env = { 32, 32, RGBA16 }
Lights l1 = { dir(-32, -64, -32) col(100, 100, 0); dir(15, 30, 120) col(50, 50, 0); ambient(5, 5, 5) }
LookAt l = lookat_reflect(0, 0, 200, 0, 0, 0, 0, 1, 0)
Mtx proj  = perspective(45, 1.3333, 10, 1000, 1)
Mtx view  = lookat(0, 0, 200, 0, 0, 0, 0, 1, 0)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
${vtxLines}
update {
  guRotate(model, time * 30, 0, 1, 0)
}
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPPerspNormalize(proj)
gsSPMatrix(view, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_MUL | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_TEXTURE_GEN | G_LIGHTING | G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER)
gsSPSetLights(l1)
gsSPLookAt(l)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
// decal: reflected texel straight through; lights present only to drive G_TEXTURE_GEN texgen.
gsDPSetCombineLERP(0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0)
gsDPLoadTextureBlock(env, G_IM_FMT_RGBA, G_IM_SIZ_16b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, ${verts.length}, 0)
${triLines.join("\n")}
gsSPEndDisplayList()
`;

writeFileSync(new URL("../../examples/toys/chrome-icosphere.n64", import.meta.url), src);
console.log(`wrote chrome-icosphere.n64: ${verts.length} verts, ${faces.length} faces, ${triLines.length} gsSP2Triangles`);
