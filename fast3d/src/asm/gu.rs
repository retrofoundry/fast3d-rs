//! libultra-faithful `gu*` matrix builders. Produce **row-vector** 4x4 float matrices with
//! **translation in row 3**, matching `guRotateF`/`guTranslate`/`guScale`. The HLE composes a
//! freshly loaded matrix on the LEFT (`mul4(new, top)`) and transforms `v * M`, so this layout
//! is the same one a real N64 `gu*` call produces; bake via `encode::mtx_to_bytes`.

pub type Mtx4 = [[f32; 4]; 4];

pub fn gu_mtx_ident() -> Mtx4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Row-vector translate: `[x,y,z,1] * M = [x+tx, y+ty, z+tz, 1]` (translation in row 3).
pub fn gu_translate(x: f32, y: f32, z: f32) -> Mtx4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, z, 1.0],
    ]
}

/// Per-axis scale (libultra `guScale` is 3-arg; the decl shorthand `scale(f32)` is uniform).
pub fn gu_scale(x: f32, y: f32, z: f32) -> Mtx4 {
    [
        [x, 0.0, 0.0, 0.0],
        [0.0, y, 0.0, 0.0],
        [0.0, 0.0, z, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Axis-angle rotation, **degrees**, matching libultra `guRotateF` (Rodrigues, row-vector).
/// The axis is normalized as `guRotateF` does.
pub fn gu_rotate(deg: f32, x: f32, y: f32, z: f32) -> Mtx4 {
    let a = deg * std::f32::consts::PI / 180.0;
    let s = a.sin();
    let c = a.cos();
    let len = (x * x + y * y + z * z).sqrt();
    let (x, y, z) = if len > 0.0 {
        (x / len, y / len, z / len)
    } else {
        (0.0, 0.0, 0.0)
    };
    let ab = x * y * (1.0 - c);
    let bc = y * z * (1.0 - c);
    let ca = z * x * (1.0 - c);
    let mut m = gu_mtx_ident();
    m[0][0] = x * x + c * (1.0 - x * x);
    m[1][1] = y * y + c * (1.0 - y * y);
    m[2][2] = z * z + c * (1.0 - z * z);
    m[0][1] = ab + z * s;
    m[1][0] = ab - z * s;
    m[0][2] = ca - y * s;
    m[2][0] = ca + y * s;
    m[1][2] = bc + x * s;
    m[2][1] = bc - x * s;
    m
}

/// libultra `guPerspectiveF`: a **row-vector** perspective matrix (OpenGL `[-1,1]` NDC z,
/// `w = -z_eye`) plus the companion `perspNorm` u16. `fovy` is in DEGREES. Built from a zeroed
/// matrix, then every element is multiplied by `scale` (matching libultra). The viewport — not
/// this matrix — remaps NDC z to `[0,1]` (see hle `set_vertex`).
pub fn gu_perspective(fovy: f32, aspect: f32, near: f32, far: f32, scale: f32) -> (Mtx4, u16) {
    let fovy_rad = fovy * std::f32::consts::PI / 180.0;
    let cot = (fovy_rad / 2.0).cos() / (fovy_rad / 2.0).sin();
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = cot / aspect;
    m[1][1] = cot;
    m[2][2] = (near + far) / (near - far);
    m[2][3] = -1.0;
    m[3][2] = 2.0 * near * far / (near - far);
    m[3][3] = 0.0;
    for row in m.iter_mut() {
        for v in row.iter_mut() {
            *v *= scale;
        }
    }
    // libultra perspNorm: 0xFFFF when near+far <= 2; else (u16)((double)0x00020000 / (near+far)),
    // truncating toward zero and clamping up from 0 to 1.
    let pn = if near + far <= 2.0 {
        0xFFFF
    } else {
        let p = (131072.0f64 / (near as f64 + far as f64)) as i32;
        p.clamp(1, 0xFFFF) as u16
    };
    (m, pn)
}

/// libultra `guLookAtF`: right-handed view matrix (row-vector; basis vectors in the COLUMNS,
/// translation in row 3). `Look = normalize(eye - at)` (camera looks down -Z).
#[allow(clippy::too_many_arguments)]
pub fn gu_look_at(
    ex: f32,
    ey: f32,
    ez: f32,
    ax: f32,
    ay: f32,
    az: f32,
    ux: f32,
    uy: f32,
    uz: f32,
) -> Mtx4 {
    fn norm(v: [f32; 3]) -> [f32; 3] {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.0 {
            [v[0] / len, v[1] / len, v[2] / len]
        } else {
            [0.0, 0.0, 0.0]
        }
    }
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    let eye = [ex, ey, ez];
    let look = norm([ex - ax, ey - ay, ez - az]); // normalize(eye - at)
    let right = norm(cross([ux, uy, uz], look)); // up × look
    let up = norm(cross(look, right)); // look × right
    [
        [right[0], up[0], look[0], 0.0],
        [right[1], up[1], look[1], 0.0],
        [right[2], up[2], look[2], 0.0],
        [-dot(eye, right), -dot(eye, up), -dot(eye, look), 1.0],
    ]
}

/// guLookAtReflect basis: S-axis = Right = norm(up × Look), T-axis = Up' = Look × Right,
/// where Look = norm(eye - at). Each component → s8 via clamp(trunc(v*127), -128, 127).
pub fn gu_look_at_reflect(a: [f32; 9]) -> ([i8; 3], [i8; 3]) {
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.0 {
            [v[0] / l, v[1] / l, v[2] / l]
        } else {
            v
        }
    };
    let cross = |u: [f32; 3], v: [f32; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let look = norm([a[0] - a[3], a[1] - a[4], a[2] - a[5]]); // normalize(eye - at)
    let up = [a[6], a[7], a[8]];
    let right = norm(cross(up, look));
    let up2 = cross(look, right);
    let s8 = |v: [f32; 3]| {
        [
            (v[0] * 127.0) as i8,
            (v[1] * 127.0) as i8,
            (v[2] * 127.0) as i8,
        ]
    };
    (s8(right), s8(up2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::encode::mtx_to_bytes;

    fn approx(m: &Mtx4, want: &Mtx4) {
        for i in 0..4 {
            for j in 0..4 {
                assert!(
                    (m[i][j] - want[i][j]).abs() < 1e-4,
                    "m[{i}][{j}] = {} want {}",
                    m[i][j],
                    want[i][j]
                );
            }
        }
    }

    #[test]
    fn rotate_90_about_z_is_ccw_row_vector() {
        // Row-vector CCW about Z: M[0]=[cos,sin,..], M[1]=[-sin,cos,..]; at 90deg = [0,1..],[-1,0..]
        approx(
            &gu_rotate(90.0, 0.0, 0.0, 1.0),
            &[
                [0.0, 1.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        );
    }

    #[test]
    fn rotate_0_is_identity() {
        approx(&gu_rotate(0.0, 0.0, 1.0, 0.0), &gu_mtx_ident());
    }

    #[test]
    fn translate_is_row_3() {
        let m = gu_translate(1.0, 2.0, 3.0);
        assert_eq!(m[3], [1.0, 2.0, 3.0, 1.0]);
    }

    #[test]
    fn scale_is_per_axis_diagonal() {
        let m = gu_scale(2.0, 3.0, 4.0);
        assert_eq!([m[0][0], m[1][1], m[2][2], m[3][3]], [2.0, 3.0, 4.0, 1.0]);
    }

    #[test]
    fn bakes_to_s15_16_via_mtx_to_bytes() {
        // translate(1,2,3): row 3 ints are 1,2,3 at k=12,13,14 -> bytes[24..30]; identity 1.0 at [0][0].
        let b = mtx_to_bytes(gu_translate(1.0, 2.0, 3.0));
        assert_eq!(&b[24..30], &[0, 1, 0, 2, 0, 3]); // i16 BE: 1,2,3
        assert_eq!(&b[0..2], &[0, 1]); // [0][0] int = 1
    }

    #[test]
    fn perspective_90_aspect1_near1_far2_row_vector_form() {
        // fovy=90 => cot(45°)=1; near+far=3 => m[2][2]=3/(1-2)=-3, m[3][2]=2*1*2/(1-2)=-4.
        let (m, pn) = gu_perspective(90.0, 1.0, 1.0, 2.0, 1.0);
        approx(
            &m,
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, -3.0, -1.0],
                [0.0, 0.0, -4.0, 0.0],
            ],
        );
        // near+far=3 > 2 => floor(131072/3) = 43690 (truncate toward zero).
        assert_eq!(pn, 43690);
    }

    #[test]
    fn perspective_scale_multiplies_all_elements() {
        let (m, pn) = gu_perspective(90.0, 1.0, 1.0, 2.0, 2.0);
        // every nonzero element doubles, INCLUDING m[2][3] = -2.0 and m[3][2] = -8.0.
        approx(
            &m,
            &[
                [2.0, 0.0, 0.0, 0.0],
                [0.0, 2.0, 0.0, 0.0],
                [0.0, 0.0, -6.0, -2.0],
                [0.0, 0.0, -8.0, 0.0],
            ],
        );
        assert_eq!(pn, 43690); // perspNorm is scale-independent
    }

    #[test]
    fn perspective_near_short_circuits_persp_norm_to_ffff() {
        // near+far = 0.5 + 1.0 = 1.5 <= 2 => 0xFFFF.
        let (_m, pn) = gu_perspective(60.0, 1.0, 0.5, 1.0, 1.0);
        assert_eq!(pn, 0xFFFF);
    }

    #[test]
    fn look_at_eye_on_pos_z_is_minus_z_translation() {
        // eye (0,0,5) looking at origin, up +Y: basis is identity, translation = (0,0,-5) in row 3.
        let m = gu_look_at(0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
        approx(
            &m,
            &[
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, -5.0, 1.0],
            ],
        );
    }

    #[test]
    fn look_at_puts_basis_in_columns_and_neg_dot_in_row3() {
        // eye on +X looking at origin: Look = normalize(eye-at) = (1,0,0).
        // Right = normalize(up × Look) = normalize((0,1,0)×(1,0,0)) = (0,0,-1).
        // Up    = normalize(Look × Right) = (1,0,0)×(0,0,-1) = (0,1,0).
        let m = gu_look_at(4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0);
        // col0 = Right at m[0][0],m[1][0],m[2][0]
        approx(
            &m,
            &[
                [0.0, 0.0, 1.0, 0.0],  // m[0]= [Right.x, Up.x, Look.x, 0]
                [0.0, 1.0, 0.0, 0.0],  // m[1]= [Right.y, Up.y, Look.y, 0]
                [-1.0, 0.0, 0.0, 0.0], // m[2]= [Right.z, Up.z, Look.z, 0]
                [0.0, 0.0, -4.0, 1.0], // row3 = -dot(eye, {Right,Up,Look}) = (0, 0, -4)
            ],
        );
    }
}
