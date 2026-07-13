use crate::tests::common;

use crate::asm::assemble_with_texture;
use crate::hle::interpret_rdram;

#[test]
fn g_tri2_emits_two_triangles_in_order() {
    use crate::asm::encode::{
        gdp_set_combine_lerp, gdp_set_cycle_type, gdp_set_render_mode, gsp_2triangles, gsp_enddl,
        gsp_vertex, CcPass, VtxColored, ZERO_A, ZERO_C,
    };
    use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
    let mut rdram = vec![0u8; 0x100];
    // 4 vertices @0x00 (16 bytes each).
    for i in 0..4u8 {
        let v = VtxColored {
            x: i as i16,
            y: 0,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        let o = i as usize * 16;
        rdram[o..o + 16].copy_from_slice(&v.to_bytes());
    }
    // Commands @0x40 (8-aligned, past the 64 bytes of vertex data).
    // snapshot_run requires a combiner before any triangle; add SHADE passthrough.
    let cc = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    }; // d=4 → Shade
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    let (cy0, cy1) = gdp_set_cycle_type(0); // 1-cycle
    rdram[0x40..0x44].copy_from_slice(&cy0.to_be_bytes());
    rdram[0x44..0x48].copy_from_slice(&cy1.to_be_bytes());
    let (rm0, rm1) = gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2);
    rdram[0x48..0x4C].copy_from_slice(&rm0.to_be_bytes());
    rdram[0x4C..0x50].copy_from_slice(&rm1.to_be_bytes());
    let (cl0, cl1) = gdp_set_combine_lerp(cc, ca, cc, ca);
    rdram[0x50..0x54].copy_from_slice(&cl0.to_be_bytes());
    rdram[0x54..0x58].copy_from_slice(&cl1.to_be_bytes());
    let (v0, v1) = gsp_vertex(0, 4, 0x00);
    rdram[0x58..0x5C].copy_from_slice(&v0.to_be_bytes());
    rdram[0x5C..0x60].copy_from_slice(&v1.to_be_bytes());
    let (t0, t1) = gsp_2triangles(0, 1, 2, 0, 2, 3);
    rdram[0x60..0x64].copy_from_slice(&t0.to_be_bytes());
    rdram[0x64..0x68].copy_from_slice(&t1.to_be_bytes());
    let (e0, e1) = gsp_enddl();
    rdram[0x68..0x6C].copy_from_slice(&e0.to_be_bytes());
    rdram[0x6C..0x70].copy_from_slice(&e1.to_be_bytes());

    let r = crate::hle::interpret_rdram(&rdram, 0x40);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    assert_eq!(r.scene.indices, vec![0, 1, 2, 0, 2, 3]);
}

const SAMPLE: &str = "\
// Walking-skeleton sample: one vertex-colored triangle (F3DEX2).
Mtx proj = scale(0.015625)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0, 0, 0, 255,   0,   0, 255 }
Vtx {  48, -48, 0, 0, 0, 0,   0, 255,   0, 255 }
Vtx {   0,  48, 0, 0, 0, 0,   0,   0, 255, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPClearGeometryMode(G_LIGHTING, G_CULL_BACK)
gsSPSetGeometryMode(G_SHADE, G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 3, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSPEndDisplayList()
";

#[test]
fn sample_dl_decodes_to_one_colored_triangle() {
    let img = assemble_with_texture(SAMPLE, &[255u8; 4], 1, 1).expect("assemble ok");
    let res = interpret_rdram(&img.rdram, img.entry_addr);
    assert!(res.diags.is_empty(), "diags: {:?}", res.diags);

    // 3 vertices, one triangle (3 indices, in order).
    assert_eq!(res.scene.raw_pos.len(), 3);
    assert_eq!(res.scene.indices, vec![0, 1, 2]);

    // Colors survive the pipeline (authentic Vtx_t RGBA order).
    let c0 = res.scene.cn[0];
    assert_eq!(
        [
            (c0 & 0xff) as u8,
            ((c0 >> 8) & 0xff) as u8,
            ((c0 >> 16) & 0xff) as u8,
            ((c0 >> 24) & 0xff) as u8
        ],
        [255, 0, 0, 255]
    );
    let c1 = res.scene.cn[1];
    assert_eq!(
        [
            (c1 & 0xff) as u8,
            ((c1 >> 8) & 0xff) as u8,
            ((c1 >> 16) & 0xff) as u8,
            ((c1 >> 24) & 0xff) as u8
        ],
        [0, 255, 0, 255]
    );
    let c2 = res.scene.cn[2];
    assert_eq!(
        [
            (c2 & 0xff) as u8,
            ((c2 >> 8) & 0xff) as u8,
            ((c2 >> 16) & 0xff) as u8,
            ((c2 >> 24) & 0xff) as u8
        ],
        [0, 0, 255, 255]
    );

    // scale(1/64) projection gives clip (x/64, y/64, 0, 1); the full-screen viewport maps x,y
    // back to NDC unchanged (-0.75/0.75) and z to vp_trans.z = 511/1024 (mid-depth, no flip on x).
    let z = 511.0 / 1024.0;
    assert_eq!(common::ref_pos(&res.scene, 0), [-0.75, -0.75, z, 1.0]);
    assert_eq!(common::ref_pos(&res.scene, 1), [0.75, -0.75, z, 1.0]);
    assert_eq!(common::ref_pos(&res.scene, 2), [0.0, 0.75, z, 1.0]);
}

#[test]
fn tri_index_x2_convention_round_trips() {
    // gsSP1Triangle(0,1,2) must decode back to cache slots 0,1,2 (index*2 encode, /2 decode).
    let img = assemble_with_texture(SAMPLE, &[255u8; 4], 1, 1).expect("assemble ok");
    let res = interpret_rdram(&img.rdram, img.entry_addr);
    assert_eq!(res.scene.indices, vec![0, 1, 2]);
}

#[test]
fn geometry_mode_clear_then_set_is_asymmetric_masked() {
    // Clear(G_LIGHTING|G_CULL_BACK) then Set(G_SHADE|G_SHADING_SMOOTH), starting from the
    // initial G_CLIPPING (0x800000). Result preserves G_CLIPPING and adds the set bits:
    // 0x800000 (G_CLIPPING) | 0x4 (G_SHADE) | 0x200000 (G_SHADING_SMOOTH) = 0x00A0_0004.
    let img = assemble_with_texture(SAMPLE, &[255u8; 4], 1, 1).expect("assemble ok");
    let res = interpret_rdram(&img.rdram, img.entry_addr);
    assert_eq!(res.geometry_mode, 0x00A0_0004);
}
