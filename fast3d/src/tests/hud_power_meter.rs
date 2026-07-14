//! Reproduction of the sm64 HUD power-meter transform chain, end-to-end through `interpret_rdram`.
//!
//! sm64 draws the power meter (a 64x64 frame + 32x32 dial, geometry — not TexRects) under an
//! ORTHO projection built by `create_dl_ortho_matrix`:
//!   create_dl_identity_matrix: LOAD identity -> MODELVIEW ; LOAD identity -> PROJECTION
//!   gSPPerspNormalize(0xFFFF)
//!   guOrtho(0, 320, 0, 240, -10, 10, 1.0) MUL -> PROJECTION
//! then per element `create_dl_translation_matrix(PUSH, x, y, 0)` MUL -> MODELVIEW.
//!
//! The ortho's non-uniform NDC scale (2/320 in x, 2/240 in y) EXACTLY compensates the
//! non-square NDC->pixel viewport (160 px per NDC-x, 120 px per NDC-y), so a raw 64x64 quad must
//! land as 64x64 PIXELS on screen. This test guards the matrix path (fixed-point decode, MUL order,
//! viewport fold) so the meter stays square independent of texturing.

use crate::asm::encode::*;
use crate::hle::interpret_rdram;

/// guOrtho(0, 320, 0, 240, -10, 10, scale=1.0), row-major (row-vector convention).
fn ortho_320x240() -> [[f32; 4]; 4] {
    [
        [2.0 / 320.0, 0.0, 0.0, 0.0],
        [0.0, 2.0 / 240.0, 0.0, 0.0],
        [0.0, 0.0, -2.0 / 20.0, 0.0],
        [-1.0, -1.0, 0.0, 1.0],
    ]
}

/// guTranslate(tx, ty, tz), row-major.
fn translate(tx: f32, ty: f32, tz: f32) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [tx, ty, tz, 1.0],
    ]
}

fn mul_row_vec4(v: [f32; 4], m: [[f32; 4]; 4]) -> [f32; 4] {
    let mut o = [0.0f32; 4];
    for c in 0..4 {
        o[c] = v[0] * m[0][c] + v[1] * m[1][c] + v[2] * m[2][c] + v[3] * m[3][c];
    }
    o
}

#[test]
fn hud_power_meter_stays_square() {
    let mut rdram: Vec<u8> = Vec::new();

    // Full-screen viewport: raw scale 640/480 -> /4 = (160,120); trans 320/240 -> /4 = (160,120).
    let vp_addr = rdram.len() as u32;
    rdram.extend_from_slice(
        &Vp {
            vscale: [640, 480, 511, 511],
            vtrans: [320, 240, 0, 511],
        }
        .to_bytes(),
    );

    let ident_addr = rdram.len() as u32;
    rdram.extend_from_slice(&mtx_identity_bytes());

    let ortho_addr = rdram.len() as u32;
    rdram.extend_from_slice(&mtx_to_bytes(ortho_320x240()));

    // Power meter position (HUD pixel coords). Shape is position-independent; any placement works.
    let (tx, ty) = (140.0f32, 40.0f32);
    let trans_addr = rdram.len() as u32;
    rdram.extend_from_slice(&mtx_to_bytes(translate(tx, ty, 0.0)));

    // 64x64 quad centered on the raw origin — the power-meter frame verts (-32..32).
    let vtx_addr = rdram.len() as u32;
    let corners = [(-32i16, -32i16), (32, -32), (32, 32), (-32, 32)];
    for (x, y) in corners {
        rdram.extend_from_slice(
            &VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            }
            .to_bytes(),
        );
    }
    while !rdram.len().is_multiple_of(8) {
        rdram.push(0);
    }
    let entry = rdram.len() as u32;

    let mut cmds: Vec<u8> = Vec::new();
    let push = |c: &mut Vec<u8>, (w0, w1): (u32, u32)| {
        c.extend_from_slice(&w0.to_be_bytes());
        c.extend_from_slice(&w1.to_be_bytes());
    };
    push(&mut cmds, gsp_viewport(vp_addr));
    // create_dl_identity_matrix: LOAD identity onto MODELVIEW then PROJECTION.
    push(&mut cmds, gsp_matrix(ident_addr, false, true, false));
    push(&mut cmds, gsp_matrix(ident_addr, true, true, false));
    // create_dl_ortho_matrix: perspNorm (no-op) then ortho MUL onto PROJECTION.
    push(&mut cmds, gsp_persp_normalize(0xFFFF));
    push(&mut cmds, gsp_matrix(ortho_addr, true, false, false));
    // render_dl_power_meter: translate MUL onto MODELVIEW (PUSH).
    push(&mut cmds, gsp_matrix(trans_addr, false, false, true));
    push(&mut cmds, gsp_vertex(0, 4, vtx_addr));
    push(&mut cmds, gsp_1triangle(0, 1, 2));
    push(&mut cmds, gsp_1triangle(0, 2, 3));
    push(&mut cmds, gsp_enddl());
    rdram.extend_from_slice(&cmds);

    let res = interpret_rdram(&rdram, entry);
    assert!(res.diags.is_empty(), "unexpected diags: {:?}", res.diags);

    // MVP the meter verts were transformed by.
    let mi = res.scene.mtx_index[0] as usize;
    let mvp = res.scene.mvp_table[mi];
    eprintln!("meter MVP row-major = {mvp:?}");
    eprintln!("  mvp[0][0] (x-scale) = {}  expect 0.00625", mvp[0][0]);
    eprintln!("  mvp[1][1] (y-scale) = {}  expect 0.0083333", mvp[1][1]);

    // Viewport fold factors for this viewport (scale 160,120 ; trans 160,120).
    let (vpsx, vpsy) = (160.0f32, 120.0f32);
    let (vptx, vpty) = (160.0f32, 120.0f32);
    let fold = |clip: [f32; 4]| -> (f32, f32) {
        let w = if clip[3] == 0.0 { 1e-6 } else { clip[3] };
        let ox = clip[0] * (2.0 * vpsx / 320.0) + w * (2.0 * vptx / 320.0 - 1.0);
        let oy = clip[1] * (2.0 * vpsy / 240.0) + w * (1.0 - 2.0 * vpty / 240.0);
        (ox / w, oy / w) // NDC
    };

    let (mut xl, mut xh, mut yl, mut yh) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for i in 0..4 {
        let rp = res.scene.raw_pos[i];
        let clip = mul_row_vec4([rp[0], rp[1], rp[2], 1.0], mvp);
        let (nx, ny) = fold(clip);
        xl = xl.min(nx);
        xh = xh.max(nx);
        yl = yl.min(ny);
        yh = yh.max(ny);
    }
    // NDC extents -> pixels (x spans 320, y spans 240 across the [-1,1] NDC range).
    let px_w = (xh - xl) * 0.5 * 320.0;
    let px_h = (yh - yl) * 0.5 * 240.0;
    eprintln!("on-screen pixel extent: {px_w:.2} x {px_h:.2} (expect 64 x 64)");

    assert!(
        (mvp[0][0] - 0.00625).abs() < 1e-4,
        "x-scale wrong: {} (expected 0.00625)",
        mvp[0][0]
    );
    assert!(
        (mvp[1][1] - 0.0083333).abs() < 1e-4,
        "y-scale wrong: {} (expected 0.0083333)",
        mvp[1][1]
    );
    assert!(
        (px_w - 64.0).abs() < 1.0 && (px_h - 64.0).abs() < 1.0,
        "power meter not square on screen: {px_w:.2} x {px_h:.2} px (expected 64 x 64)"
    );
}
