use crate::tests::common;

use crate::asm::encode::{
    gsp_enddl, gsp_matrix, gsp_popmatrix, gsp_vertex, mtx_to_bytes, VtxColored,
};
use crate::hle::interpret_rdram;

fn put(rdram: &mut [u8], off: usize, w0: u32, w1: u32) {
    rdram[off..off + 4].copy_from_slice(&w0.to_be_bytes());
    rdram[off + 4..off + 8].copy_from_slice(&w1.to_be_bytes());
}

// Row-vector translate: [x,y,z,1] * M = [x+tx, y+ty, z+tz, 1] (translation in the last row).
fn translate(tx: f32, ty: f32, tz: f32) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [tx, ty, tz, 1.0],
    ]
}

#[test]
fn push_mul_pop_restores_modelview() {
    // Data: Vp @0x00, one vtx @0x10 (origin), mtxA=translate(10) @0x20, mtxB=translate(5) @0x60.
    let mut rdram = vec![0u8; 0x200];
    // No gsSPViewport is emitted -> the default viewport applies: vp_scale.x = vp_trans.x =
    // FB_WIDTH/2 = 160, so position.x = (tx*160 + 160)/320*2 - 1 = tx exactly.
    let v = VtxColored {
        x: 0,
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
    rdram[0x10..0x20].copy_from_slice(&v.to_bytes());
    rdram[0x20..0x60].copy_from_slice(&mtx_to_bytes(translate(10.0, 0.0, 0.0)));
    rdram[0x60..0xA0].copy_from_slice(&mtx_to_bytes(translate(5.0, 0.0, 0.0)));

    // Commands @0xA0: load A (NOPUSH), vtx -> v0; push+MUL B, vtx -> v1; pop 1, vtx -> v2; end.
    let mut pc = 0xA0;
    let mut emit = |rdram: &mut Vec<u8>, w: (u32, u32)| {
        put(rdram, pc, w.0, w.1);
        pc += 8;
    };
    emit(&mut rdram, gsp_matrix(0x20, false, true, false)); // modelview LOAD A (NOPUSH)
    emit(&mut rdram, gsp_vertex(0, 1, 0x10)); // -> vertices[0]
    emit(&mut rdram, gsp_matrix(0x60, false, false, true)); // modelview MUL B (PUSH)
    emit(&mut rdram, gsp_vertex(0, 1, 0x10)); // -> vertices[1]
    emit(&mut rdram, gsp_popmatrix(1)); // pop back to A
    emit(&mut rdram, gsp_vertex(0, 1, 0x10)); // -> vertices[2]
    emit(&mut rdram, gsp_enddl());

    let r = interpret_rdram(&rdram, 0xA0);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    assert_eq!(r.scene.raw_pos.len(), 3);
    assert!(
        (common::ref_pos(&r.scene, 0)[0] - 10.0).abs() < 1e-3,
        "A: {:?}",
        common::ref_pos(&r.scene, 0)
    );
    assert!(
        (common::ref_pos(&r.scene, 1)[0] - 15.0).abs() < 1e-3,
        "B*A: {:?}",
        common::ref_pos(&r.scene, 1)
    );
    assert!(
        (common::ref_pos(&r.scene, 2)[0] - 10.0).abs() < 1e-3,
        "popped to A: {:?}",
        common::ref_pos(&r.scene, 2)
    );
}

#[test]
fn pop_below_one_is_clamped() {
    // Load A=translate(10); pop 3 times (must clamp at 1, no underflow); vtx still reflects A.
    let mut rdram = vec![0u8; 0x200];
    // No gsSPViewport is emitted -> the default viewport applies: vp_scale.x = vp_trans.x =
    // FB_WIDTH/2 = 160, so position.x = (tx*160 + 160)/320*2 - 1 = tx exactly.
    let v = VtxColored {
        x: 0,
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
    rdram[0x10..0x20].copy_from_slice(&v.to_bytes());
    rdram[0x20..0x60].copy_from_slice(&mtx_to_bytes(translate(10.0, 0.0, 0.0)));

    let mut pc = 0xA0;
    let mut emit = |rdram: &mut Vec<u8>, w: (u32, u32)| {
        put(rdram, pc, w.0, w.1);
        pc += 8;
    };
    emit(&mut rdram, gsp_matrix(0x20, false, true, false));
    emit(&mut rdram, gsp_popmatrix(3));
    emit(&mut rdram, gsp_vertex(0, 1, 0x10));
    emit(&mut rdram, gsp_enddl());

    let r = interpret_rdram(&rdram, 0xA0);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    assert_eq!(r.scene.raw_pos.len(), 1);
    assert!((common::ref_pos(&r.scene, 0)[0] - 10.0).abs() < 1e-3);
}

#[test]
fn push_beyond_32_is_clamped_no_panic() {
    // 40 PUSH+MUL(identity) commands must not panic / index out of bounds; identity MUL keeps A.
    let mut rdram = vec![0u8; 0x400];
    // No gsSPViewport is emitted -> the default viewport applies: vp_scale.x = vp_trans.x =
    // FB_WIDTH/2 = 160, so position.x = (tx*160 + 160)/320*2 - 1 = tx exactly.
    let v = VtxColored {
        x: 0,
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
    rdram[0x10..0x20].copy_from_slice(&v.to_bytes());
    rdram[0x20..0x60].copy_from_slice(&mtx_to_bytes(translate(10.0, 0.0, 0.0))); // LOAD A
    rdram[0x60..0xA0].copy_from_slice(&mtx_to_bytes(translate(0.0, 0.0, 0.0))); // identity-ish (no shift)

    let mut pc = 0xA0;
    let mut emit = |rdram: &mut Vec<u8>, w: (u32, u32)| {
        put(rdram, pc, w.0, w.1);
        pc += 8;
    };
    emit(&mut rdram, gsp_matrix(0x20, false, true, false)); // LOAD A
    for _ in 0..40 {
        emit(&mut rdram, gsp_matrix(0x60, false, false, true)); // PUSH + MUL identity-translate
    }
    emit(&mut rdram, gsp_vertex(0, 1, 0x10));
    emit(&mut rdram, gsp_enddl());

    let r = interpret_rdram(&rdram, 0xA0);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    assert_eq!(r.scene.raw_pos.len(), 1);
    assert!((common::ref_pos(&r.scene, 0)[0] - 10.0).abs() < 1e-3);
}
