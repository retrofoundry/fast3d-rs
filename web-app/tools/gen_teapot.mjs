// Generate examples/toys/lights.n64 — the authentic SDK "lights" hardware-lighting demo on a
// low-poly Utah teapot. The teapot is the recognizability-critical shape: a teapot reads as a
// teapot only if the SPOUT and HANDLE are present, so the vertex budget is spent on those two
// silhouette features rather than on a smooth body.
//
// VERTEX BUDGET (hard cap = 127): F3DEX2's G_VTX encodes the load end index (v0+n) in a 7-bit
// field, so v0+n <= 127 per gsSPVertex, AND every gsSP2Triangles index is 7-bit (0..127). We load
// the whole mesh in ONE gsSPVertex from pool start, so total verts <= 127 satisfies both.
//
// CONSTRUCTION (all procedural / public-domain geometry — no Nintendo gfx[] bytes):
//   - body+lid: a surface of revolution of a hand-authored teapot silhouette profile (a few rings
//     x a few radial sectors). Bottom is left open (sits on a table; never seen by the spinning
//     camera) to save verts; the lid knob caps the top.
//   - spout: a tapered tube swept along a curve arcing up-and-out from the body's upper-front.
//   - handle: a tube swept along a C-curve arcing out the back.
// Normals are averaged-face (accumulate each triangle's geometric normal onto its 3 verts, then
// normalize) — analytic normals are awkward across the swept seams, and averaged-face gives the
// smooth Gouraud look G_SHADING_SMOOTH wants. Emitted as VtxN s8 normals (*127).
//
// LIGHTING: the authentic SDK recipe — gdSPDefLights2 with two yellow directional lights + a dim
// ambient, G_LIGHTING|G_SHADE|G_SHADING_SMOOTH|G_ZBUFFER, a SHADE-only combiner (no texture), a
// perspective camera, and an update{guRotate} spin so the lighting sweeps across the form.
import { writeFileSync } from "node:fs";

// ---------- vector helpers ----------
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const len = (a) => Math.hypot(a[0], a[1], a[2]);
const norm = (a) => { const l = len(a) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };

// Accumulating mesh: verts (positions), faces (index triples). Verts are deduped by rounded pos so
// adjacent surfaces share rings and normals average smoothly across the seam.
const verts = [];
const vmap = new Map();
const faces = [];
const vkey = (p) => p.map((x) => Math.round(x * 1e3)).join(",");
function addVert(p) {
  const k = vkey(p);
  if (vmap.has(k)) return vmap.get(k);
  verts.push([p[0], p[1], p[2]]);
  const idx = verts.length - 1;
  vmap.set(k, idx);
  return idx;
}
function quad(a, b, c, d) {
  // two CCW tris of a quad ring (winding is free — Z-buffer handles occlusion, no culling)
  faces.push([a, b, c]);
  faces.push([a, c, d]);
}

// ============================================================================
// BODY + LID — surface of revolution about the Y (up) axis.
// Profile = (radius, height) silhouette of the classic teapot, bottom-open.
// Y up, the pot ~120 tall, centered roughly at the origin.
// ============================================================================
const SECT = 8; // radial sectors (8 keeps the budget small; reads as round enough when lit)
// profile rings from the foot up to the lid rim, then the lid dome + knob.
//  r = radius, y = height. Hand-tuned teapot silhouette: bulbous belly, tucked shoulder, lid.
const bodyProfile = [
  [34, -46], // foot rim (bottom, open below this)
  [56, -30], // belly lower
  [62, -6], //  belly widest
  [54, 18], //  shoulder
  [40, 36], //  neck / lid seat
];
const lidProfile = [
  [40, 36], // lid rim (== neck, shared ring -> seam welds via dedup)
  [30, 48], // lid dome
  [12, 58], // lid shoulder
  [10, 66], // knob base
  [4, 74], //  knob top
];
function revolve(profile) {
  // build a ring of SECT verts for each profile sample, stitch adjacent rings.
  const rings = profile.map(([r, y]) => {
    const ring = [];
    for (let s = 0; s < SECT; s++) {
      const a = (s / SECT) * Math.PI * 2;
      ring.push(addVert([r * Math.cos(a), y, r * Math.sin(a)]));
    }
    return ring;
  });
  for (let i = 0; i < rings.length - 1; i++) {
    for (let s = 0; s < SECT; s++) {
      const sn = (s + 1) % SECT;
      quad(rings[i][s], rings[i][sn], rings[i + 1][sn], rings[i + 1][s]);
    }
  }
  return rings;
}
revolve(bodyProfile);
const lidRings = revolve(lidProfile);
// cap the knob top with a fan to a single apex vertex
const topRing = lidRings[lidRings.length - 1];
const apex = addVert([0, 78, 0]);
for (let s = 0; s < SECT; s++) {
  const sn = (s + 1) % SECT;
  faces.push([topRing[s], topRing[sn], apex]);
}

// ============================================================================
// SPOUT — a tapered tube swept along a curve arcing up-and-out the +X front.
// Cross-sections are RSEG-gons; the path starts inside the belly (so it visually
// merges) and ends as the pour tip, raised and forward.
// ============================================================================
const RSEG = 5; // cross-section resolution of spout/handle tubes (5 keeps the budget under 127)
function sweepTube(path, radii, rseg) {
  // path: list of centers; radii: per-center tube radius. Build rings in a frame whose normal
  // follows the path tangent; stitch consecutive rings; cap both ends with a fan.
  const ringsIdx = [];
  for (let i = 0; i < path.length; i++) {
    const c = path[i];
    // tangent
    const t = norm(
      sub(path[Math.min(i + 1, path.length - 1)], path[Math.max(i - 1, 0)]),
    );
    // Fixed out-of-plane reference: both tube paths are planar in XY (z=0), so the tangent always
    // lies in XY and cross(t, [0,0,1]) is never near-zero — no per-ring switching, no twist.
    const up = [0, 0, 1];
    const u = norm(cross(t, up));   // in-plane, perpendicular to tangent — rotates smoothly with t
    const v = norm(cross(t, u));    // ≈ ±Z, out of plane
    const r = radii[i];
    const ring = [];
    for (let s = 0; s < rseg; s++) {
      const a = (s / rseg) * Math.PI * 2;
      const ca = Math.cos(a), sa = Math.sin(a);
      ring.push(
        addVert([
          c[0] + r * (ca * u[0] + sa * v[0]),
          c[1] + r * (ca * u[1] + sa * v[1]),
          c[2] + r * (ca * u[2] + sa * v[2]),
        ]),
      );
    }
    ringsIdx.push(ring);
  }
  for (let i = 0; i < ringsIdx.length - 1; i++) {
    for (let s = 0; s < rseg; s++) {
      const sn = (s + 1) % rseg;
      quad(ringsIdx[i][s], ringsIdx[i][sn], ringsIdx[i + 1][sn], ringsIdx[i + 1][s]);
    }
  }
  // cap the open pour tip (last ring) with a fan; the first ring stays open inside the body.
  const tip = ringsIdx[ringsIdx.length - 1];
  const tc = path[path.length - 1];
  const tipApex = addVert([tc[0], tc[1], tc[2]]);
  for (let s = 0; s < rseg; s++) {
    const sn = (s + 1) % rseg;
    faces.push([tip[s], tip[sn], tipApex]);
  }
  return ringsIdx;
}
// spout path: from inside the belly (+X) curving up to the raised pour tip.
sweepTube(
  [
    [44, 2, 0], // root inside belly
    [66, 8, 0], // emerging
    [82, 22, 0], // mid, climbing
    [92, 40, 0], // upper bend
    [96, 50, 0], // pour tip
  ],
  [16, 13, 10, 7, 5],
  RSEG,
);

// ============================================================================
// HANDLE — a tube swept along a C-curve arcing out the back (-X), top to bottom.
// ============================================================================
sweepTube(
  [
    [-40, 30, 0], // upper root (shoulder)
    [-66, 26, 0], // out the back, top
    [-74, 6, 0], //  back, mid
    [-66, -14, 0], // back, lower
    [-44, -22, 0], // lower root (belly)
  ],
  [8, 8, 8, 8, 8],
  RSEG,
);

// ============================================================================
// NORMALS — averaged face. Accumulate each tri's geometric normal onto its verts.
// ============================================================================
const acc = verts.map(() => [0, 0, 0]);
for (const [a, b, c] of faces) {
  const n = cross(sub(verts[b], verts[a]), sub(verts[c], verts[a]));
  for (const idx of [a, b, c]) {
    acc[idx][0] += n[0];
    acc[idx][1] += n[1];
    acc[idx][2] += n[2];
  }
}
const normals = acc.map(norm);

// ---------- budget guard ----------
if (verts.length > 127) {
  throw new Error(`teapot overflows the 127-vertex cap: ${verts.length} verts`);
}

// ============================================================================
// EMIT
// ============================================================================
const r = (x) => Math.round(x);
const s8 = (x) => Math.max(-128, Math.min(127, Math.round(x * 127)));
const vtxLines = verts
  .map(
    (p, i) =>
      `VtxN { ${r(p[0])}, ${r(p[1])}, ${r(p[2])}, 0, 0, 0, ${s8(normals[i][0])}, ${s8(
        normals[i][1],
      )}, ${s8(normals[i][2])}, 255 }`,
  )
  .join("\n");

// gsSP2Triangles pairs (odd tail -> a final gsSP1Triangle)
const triLines = [];
for (let i = 0; i + 1 < faces.length; i += 2) {
  const [a, b, c] = faces[i];
  const [d, e, f] = faces[i + 1];
  triLines.push(`gsSP2Triangles(${a}, ${b}, ${c}, 0, ${d}, ${e}, ${f}, 0)`);
}
if (faces.length % 2 === 1) {
  const [a, b, c] = faces[faces.length - 1];
  triLines.push(`gsSP1Triangle(${a}, ${b}, ${c}, 0)`);
}

const src = `// lights — gdSPDefLights2 (two yellow directional lights + ambient) on a low-poly teapot:
// G_LIGHTING shades per-vertex s8 normals through a SHADE-only combiner; spins about Y.
Lights l1 = { dir(-32, -64, -32) col(100, 100, 0); dir(15, 30, 120) col(50, 50, 0); ambient(5, 5, 5) }
Mtx proj  = perspective(45, 1.3333, 10, 1000, 1)
Mtx view  = lookat(0, 80, 230, 0, 0, 0, 0, 1, 0)
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
gsSPSetGeometryMode(G_LIGHTING | G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER)
gsSPSetLights(l1)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
// SHADE-only: hardware lighting drives the color directly (no texture).
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, ${verts.length}, 0)
${triLines.join("\n")}
gsSPEndDisplayList()
`;

writeFileSync(new URL("../../examples/toys/lights.n64", import.meta.url), src);
console.log(
  `wrote lights.n64: ${verts.length} verts, ${faces.length} faces, ${triLines.length} tri-calls`,
);
