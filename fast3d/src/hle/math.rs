//! Row-vector matrix math: position is a row, transformed as `pos * M`.
//! Matches hlslpp (`tfPos = mul(float4(x,y,z,1), mvp)`).

pub type Mat4 = [[f32; 4]; 4];

pub fn identity() -> Mat4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Row-vector * matrix: out[c] = sum_k v[k] * m[k][c].
#[cfg_attr(not(all(test, feature = "asm")), allow(dead_code))]
pub fn mul_row_vec4(v: [f32; 4], m: Mat4) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for c in 0..4 {
        out[c] = v[0] * m[0][c] + v[1] * m[1][c] + v[2] * m[2][c] + v[3] * m[3][c];
    }
    out
}

/// Column-vector product on the upper 3×3: out[i] = sum_k m[i][k] * v[k] (M·v).
///
/// Our matrices are stored row-major and pre-transposed for the row-vector POINT path
/// (`mul_row_vec4(pos, M)` is the forward object→clip transform). For a DIRECTION that must be
/// brought from eye/world space INTO object space (light/lookat dirs, for the object-space N·L),
/// the needed transform is the INVERSE rotation — which, with that storage, is exactly this
/// column-vector multiply (`mul_row_vec4` would instead apply the forward rotation, co-rotating
/// the light with the geometry). Translation (column 3) is intentionally ignored.
pub fn mul_col_vec3(m: Mat4, v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Matrix product a*b (row-vector convention: (v*a)*b == v*(a*b)).
pub fn mul4(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] =
                a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j];
        }
    }
    out
}
