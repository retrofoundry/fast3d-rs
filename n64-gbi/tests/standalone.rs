//! Proves `n64-gbi` is usable standalone: no engine, no renderer, no wgpu.

#[test]
fn vocabulary_is_reachable_without_an_engine() {
    // Opcode identity, asserted against the libultra gbi.h values.
    assert_eq!(n64_gbi::consts::rdp::G_SETTIMG, 0xFD);
    assert_eq!(n64_gbi::consts::G_ZBUFFER, 0x0000_0001);
    // Bare `G_VTX` is F3DEX2's (0x01), via the top-level glob; F3D's is 0x04.
    assert_eq!(n64_gbi::consts::rsp_f3d::G_VTX, 0x04);
    assert_ne!(n64_gbi::consts::G_VTX, n64_gbi::consts::rsp_f3d::G_VTX);
}

#[test]
fn encoders_reachable_and_use_the_shared_vocabulary() {
    // gsSPVertex(v0=0, n=3, addr): count at bits[19:12], (v0+n) at bits[7:1].
    let (w0, w1) = n64_gbi::encode::gsp_vertex(0, 3, 0x0010_0000);
    assert_eq!(w0, 0x0100_3006);
    assert_eq!(w1, 0x0010_0000);
}

#[test]
fn translate_puts_translation_in_row_3() {
    // libultra row-vector convention: [x,y,z,1] * M = [x+tx, y+ty, z+tz, 1].
    let m = n64_gbi::gu::gu_translate(1.0, 2.0, 3.0);
    assert_eq!(m[3], [1.0, 2.0, 3.0, 1.0]);
    assert_eq!(m[0], [1.0, 0.0, 0.0, 0.0]);
}
