use crate::hle::math::{identity, mul4, mul_row_vec4};
use crate::hle::mem::RdramImage;
use n64_gbi::encode::{mtx_identity_bytes, mtx_to_bytes};

#[test]
fn identity_is_neutral() {
    let v = [1.0, 2.0, 3.0, 1.0];
    assert_eq!(mul_row_vec4(v, identity()), v);
    assert_eq!(mul4(identity(), identity()), identity());
}

#[test]
fn translate_lives_in_last_row() {
    // Row-vector translation lives in the last ROW (m[3]).
    let mut t = identity();
    t[3] = [10.0, 20.0, 0.0, 1.0];
    let out = mul_row_vec4([1.0, 1.0, 0.0, 1.0], t);
    assert_eq!(out, [11.0, 21.0, 0.0, 1.0]);
}

#[test]
fn reads_be_scalars() {
    let buf = [0x12u8, 0x34, 0xFF, 0xFF];
    let r = RdramImage::new(&buf);
    assert_eq!(r.read_i16(0), 0x1234);
    assert_eq!(r.read_i16(2), -1);
}

#[test]
fn decodes_identity_matrix() {
    let b = mtx_identity_bytes();
    let r = RdramImage::new(&b);
    assert_eq!(r.read_matrix(0), identity());
}

#[test]
fn nonsymmetric_matrix_round_trips_without_swap() {
    // Encode with gbi, decode with hle: must be bit-exact (no j^1 anywhere).
    let m = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [2.0, 3.0, 0.0, 1.0],
    ];
    let b = mtx_to_bytes(m);
    let r = RdramImage::new(&b);
    assert_eq!(r.read_matrix(0), m);
}
