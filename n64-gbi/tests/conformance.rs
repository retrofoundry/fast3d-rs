//! Conformance vectors: expected command words derived from the libultra `gs*` macros.
//!
//! Keep every expectation a literal. Encoder-vs-interpreter round trips prove nothing about
//! opcode identity — both sides read the same `consts`.

use n64_gbi::encode::*;

#[test]
fn vtx_words_match_libultra() {
    // gsSPVertex(v0=0, n=3, addr): w0 bits[19:12]=3 (count), bits[7:1]=(0+3)=3. No *2.
    let (w0, w1) = gsp_vertex(0, 3, 0x0010_0000);
    assert_eq!(w0, 0x0100_3006);
    assert_eq!(w1, 0x0010_0000);
}

#[test]
fn tri1_words_match_libultra() {
    // gsSP1Triangle(0,1,2): index*2 at BYTE shifts 16/8/0. Decode p0(17,7)/p0(9,7)/p0(1,7)=0,1,2.
    let (w0, w1) = gsp_1triangle(0, 1, 2);
    assert_eq!(w0, 0x0500_0204);
    assert_eq!(w1, 0);
}

#[test]
fn enddl_words() {
    assert_eq!(gsp_enddl(), (0xDFu32 << 24, 0));
}

#[test]
fn geometrymode_set_and_clear() {
    // SetGeometryMode(G_SHADE): offMask keeps all (0xFFFFFF), onMask = bits.
    assert_eq!(
        gsp_set_geometrymode(0x0000_0004),
        ((0xD9u32 << 24) | 0x00FF_FFFF, 0x0000_0004)
    );
    // ClearGeometryMode(G_LIGHTING): offMask = ~bits & 0xFFFFFF, onMask = 0.
    assert_eq!(
        gsp_clear_geometrymode(0x0002_0000),
        ((0xD9u32 << 24) | (!0x0002_0000u32 & 0x00FF_FFFF), 0)
    );
}

#[test]
fn matrix_dma_length_and_param_bits() {
    // projection + load + nopush: params=(0x04|0x02|0x00), stream byte = params ^ 0x01 = 0x07.
    // DMA length field: ((64-1)/8)<<19 = 7<<19 = 0x0038_0000. w0 = 0xDA380007.
    let (w0, w1) = gsp_matrix(0x0020_0000, true, true, false);
    assert_eq!(w0, 0xDA38_0007);
    assert_eq!(w1, 0x0020_0000);
    // modelview + load + push: params=(0x00|0x02|0x01), stream byte = 0x03 ^ 0x01 = 0x02. w0 = 0xDA380002.
    let (w0b, _) = gsp_matrix(0x0020_0040, false, true, true);
    assert_eq!(w0b, 0xDA38_0002);
}

#[test]
fn viewport_dma_length_and_index_byte() {
    // DMA length field: ((16-1)/8)<<19 = 1<<19 = 0x0008_0000; index byte 0x08. w0 = 0xDC080008.
    let (w0, w1) = gsp_viewport(0x0020_0080);
    assert_eq!(w0, 0xDC08_0008);
    assert_eq!(w1, 0x0020_0080);
}

#[test]
fn vtx_bytes_authentic_libultra_order() {
    // Authentic Vtx_t order: x,y,z (s16), flag (u16), s,t (s16), r,g,b,a (u8). Big-endian. No swaps.
    let v = VtxColored {
        x: 0x0102,
        y: 0x0304,
        z: 0x0506,
        flag: 0x0708,
        s: 0x090A,
        t: 0x0B0C,
        r: 0xAA,
        g: 0xBB,
        b: 0xCC,
        a: 0xDD,
    };
    let b = v.to_bytes();
    assert_eq!(
        b,
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xAA, 0xBB,
            0xCC, 0xDD
        ]
    );
    assert_eq!(b.len(), 16);
}

#[test]
fn vp_bytes_big_endian() {
    // Vp_t: s16 vscale[4]; s16 vtrans[4]; big-endian, 16 bytes.
    let vp = Vp {
        vscale: [480, 640, 511, 511],
        vtrans: [480, 640, 0, 511],
    };
    let b = vp.to_bytes();
    assert_eq!(&b[0..2], &480i16.to_be_bytes());
    assert_eq!(&b[2..4], &640i16.to_be_bytes());
    assert_eq!(&b[8..10], &480i16.to_be_bytes());
    assert_eq!(b.len(), 16);
}

#[test]
fn identity_matrix_split_int_frac_be_no_swap() {
    // 64 bytes: [16 s16 integer at k*2][16 u16 frac at 32+k*2], k=i*4+j, big-endian, NO j^1.
    // Identity: integer at k for row==col is 1; frac all 0.
    let b = mtx_identity_bytes();
    assert_eq!(b.len(), 64);
    assert_eq!(&b[0..2], &1i16.to_be_bytes()); // element[0][0], k=0
    assert_eq!(&b[10..12], &1i16.to_be_bytes()); // element[1][1], k=5 -> off 10
    assert_eq!(&b[2..4], &0i16.to_be_bytes()); // element[0][1], k=1 -> off 2
    assert!(b[32..64].iter().all(|&x| x == 0)); // frac block all zero
}

#[test]
fn scale_matrix_fixed_point_exact() {
    // scale(1/64): 1/64 = 0.015625; fixed = round(0.015625*65536) = 1024; intgr=0, frac=1024=0x0400.
    let b = mtx_to_bytes([
        [0.015625, 0.0, 0.0, 0.0],
        [0.0, 0.015625, 0.0, 0.0],
        [0.0, 0.0, 0.015625, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    // element[0][0] k=0: integer 0x0000 at off 0, frac 0x0400 at off 32.
    assert_eq!(&b[0..2], &0i16.to_be_bytes());
    assert_eq!(&b[32..34], &1024u16.to_be_bytes());
    // element[3][3] k=15: integer 0x0001 at off 30, frac 0x0000 at off 62.
    assert_eq!(&b[30..32], &1i16.to_be_bytes());
    assert_eq!(&b[62..64], &0u16.to_be_bytes());
}

#[test]
fn nonsymmetric_matrix_encode_places_translation_in_last_row() {
    // Translation row [2.0, 3.0, 0.0, 1.0] at i=3; diagonal otherwise 1. NO j^1: stored at k=i*4+j.
    let m = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [2.0, 3.0, 0.0, 1.0],
    ];
    let b = mtx_to_bytes(m);
    // element[3][0]=2.0 -> k=12, integer 0x0002 at off 24, frac 0 at off 56.
    assert_eq!(&b[24..26], &2i16.to_be_bytes());
    assert_eq!(&b[56..58], &0u16.to_be_bytes());
    // element[3][1]=3.0 -> k=13, integer 0x0003 at off 26.
    assert_eq!(&b[26..28], &3i16.to_be_bytes());
}

#[test]
fn gsp_displaylist_is_call_with_branch_bit_clear() {
    // gsSPDisplayList(addr): w0 = 0xDE000000 (branch bit p0(16,1) = 0 = call), w1 = addr.
    let (w0, w1) = gsp_displaylist(0x0900_0010);
    assert_eq!(w0, 0xDE00_0000);
    assert_eq!(w1, 0x0900_0010);
}

#[test]
fn gsp_branchlist_sets_branch_bit() {
    // gsSPBranchList(addr): w0 = 0xDE010000 (branch bit p0(16,1) = 1 = branch), w1 = addr.
    let (w0, w1) = gsp_branchlist(0x0900_0020);
    assert_eq!(w0, 0xDE01_0000);
    assert_eq!(w1, 0x0900_0020);
}

#[test]
fn gsp_segment_packs_type_and_segment_index() {
    // gsSPSegment(2, 0x09000000): G_MOVEWORD/G_MW_SEGMENT.
    // type = p0(16,8) = 0x06, seg = p0(2,4), value = w1.
    let (w0, w1) = gsp_segment(2, 0x0900_0000);
    assert_eq!(w0, 0xDB06_0008);
    assert_eq!(w1, 0x0900_0000);
}

#[test]
fn golden_sp_popmatrix() {
    use n64_gbi::encode::gsp_popmatrix;
    // F3DEX2 decodes count = w1 >> 6; w0 carries only the opcode.
    assert_eq!(gsp_popmatrix(1), (0xD800_0000, 0x0000_0040));
    assert_eq!(gsp_popmatrix(2), (0xD800_0000, 0x0000_0080));
}

#[test]
fn golden_sp_2triangles() {
    use n64_gbi::encode::gsp_2triangles;
    // libultra arg order (v00,v01,v02, v10,v11,v12); index*2 byte-packed like G_TRI1.
    // A=(0,1,2) -> w0=0x06000204 ; B=(0,2,3) -> w1=0x00000406 ; decodes to [0,1,2,0,2,3].
    assert_eq!(gsp_2triangles(0, 1, 2, 0, 2, 3), (0x0600_0204, 0x0000_0406));
}
