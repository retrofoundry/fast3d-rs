/**
 * gen_fogworld.mjs — generate fogworld.n64 toy source.
 *
 * The fogworld toy demonstrates distance fog via G_RM_FOG_SHADE_A + G_CYC_2CYCLE.
 * Two quads at different z-depths produce a visible fog gradient:
 *   Far quad  (z=110): fog_alpha ≈ 0.86 → pixel ≈ fog_color [0x80,0x80,0x80] (gray)
 *   Near quad (z=0):   fog_alpha = 0.0  → pixel = surface [200,50,50] (crisp red)
 *
 * Source is checked in at examples/toys/fogworld.n64; this script documents the derivation.
 * Run with: node web-app/tools/gen_fogworld.mjs
 */

const FOG_COLOR = [0x80, 0x80, 0x80, 0xff];
const SURFACE_COLOR = [200, 50, 50, 255];

// gsSPFogPosition(min=500, max=1000): fm=256, fo=0.
// fog_alpha = clamp(fz * 256, 0, 255) / 255  where fz = clip.z / clip.w.
// With scale(1/128): clip.z = z_obj / 128, clip.w = 1.
// Far quad (z=110): fz = 110/128 ≈ 0.859 → fog_alpha = 0.859*256/255 ≈ 0.863.
// Near quad (z=0):  fz = 0 → fog_alpha = 0.

const src = `// fogworld — distance fog demo (Phase C complete).
// Two quads at different z-depths prove the G_RM_FOG_SHADE_A + G_CYC_2CYCLE fog pipeline.
// Far quad (z=110): fog_alpha ≈ 0.86 → pixel ≈ fog_color [0x80,0x80,0x80] (gray).
// Near quad (z=0):  fog_alpha = 0.0  → pixel = surface color [200,50,50] (crisp red).
// gsSPFogPosition(500, 1000): fm=256, fo=0 → fog starts at z_ndc=0.5, saturates at 1.0.
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
// Far quad — covers top-left screen region (≈ screen 30,30). Heavily fogged.
Vtx { -80,  80, 110, 0, 0, 0, 200,  50,  50, 255 }
Vtx {  10,  80, 110, 0, 0, 0, 200,  50,  50, 255 }
Vtx {  10,  10, 110, 0, 0, 0, 200,  50,  50, 255 }
Vtx { -80,  10, 110, 0, 0, 0, 200,  50,  50, 255 }
// Near quad — covers bottom-right screen region (≈ screen 70,60). Crisp, no fog.
Vtx {  20,  10,   0, 0, 0, 0, 200,  50,  50, 255 }
Vtx { 100,  10,   0, 0, 0, 0, 200,  50,  50, 255 }
Vtx { 100, -80,   0, 0, 0, 0, 200,  50,  50, 255 }
Vtx {  20, -80,   0, 0, 0, 0, 200,  50,  50, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH | G_FOG | G_ZBUFFER)
gsDPSetOtherMode_H(G_CYC_2CYCLE)
gsDPSetRenderMode(G_RM_FOG_SHADE_A, G_RM_AA_ZB_OPA_SURF2)
gsDPSetFogColor(0x80, 0x80, 0x80, 0xFF)
gsSPFogPosition(500, 1000)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 8, 0)
// Far quad first (back-to-front; no z-write in this render mode)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
// Near quad second
gsSP1Triangle(4, 5, 6, 0)
gsSP1Triangle(4, 6, 7, 0)
gsSPEndDisplayList()
`;

console.log(src);
console.log(`// fog_color: [${FOG_COLOR.join(",")}]`);
console.log(`// surface:   [${SURFACE_COLOR.join(",")}]`);
console.log("// Source written to examples/toys/fogworld.n64");
