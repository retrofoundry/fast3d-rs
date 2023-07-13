use fast3d::gbi::defines::Mtx;

#[allow(non_snake_case)]
pub fn guMtxF2L(float_mtx: &[[f32; 4]; 4], output: *mut Mtx) {
    let mut m1 = unsafe { &mut (*output).m[0][0] as *mut u32 };
    let mut m2 = unsafe { &mut (*output).m[2][0] as *mut u32 };

    for row in 0..4 {
        for col in 0..2 {
            let tmp1 = (float_mtx[row][2 * col] * 65536.0) as u32;
            let tmp2 = (float_mtx[row][2 * col + 1] * 65536.0) as u32;
            unsafe {
                *m1 = tmp1 & 0xFFFF0000 | ((tmp2 >> 16) & 0xFFFF);
                *m2 = (tmp1 << 16) & 0xFFFF0000 | (tmp2 & 0xFFFF);
                m1 = m1.offset(1);
                m2 = m2.offset(1);
            }
        }
    }
}

#[allow(non_snake_case)]
pub fn guOrtho(
    matrix: *mut Mtx,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
    scale: f32,
) {
    let mut float_matrix: [[f32; 4]; 4] = [[0.0; 4]; 4];
    guOrthoF(
        &mut float_matrix,
        left,
        right,
        bottom,
        top,
        near,
        far,
        scale,
    );
    guMtxF2L(&float_matrix, matrix);
}

#[allow(non_snake_case)]
pub fn guOrthoF(
    matrix: &mut [[f32; 4]; 4],
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
    scale: f32,
) {
    guMtxIdentF(matrix);
    matrix[0][0] = 2.0 / (right - left);
    matrix[1][1] = 2.0 / (top - bottom);
    matrix[2][2] = -2.0 / (far - near);
    matrix[3][0] = -(right + left) / (right - left);
    matrix[3][1] = -(top + bottom) / (top - bottom);
    matrix[3][2] = -(far + near) / (far - near);
    matrix[3][3] = 1.0;

    for row in 0..4 {
        for col in 0..4 {
            matrix[row][col] *= scale;
        }
    }
}

#[allow(non_snake_case)]
pub fn guMtxIdentF(matrix: &mut [[f32; 4]; 4]) {
    for row in 0..4 {
        for col in 0..4 {
            if row == col {
                matrix[row][col] = 1.0;
            } else {
                matrix[row][col] = 0.0;
            }
        }
    }
}

#[allow(non_snake_case)]
pub fn guRotate(matrix: *mut Mtx, angle: f32, x: f32, y: f32, z: f32) {
    let mut float_matrix: [[f32; 4]; 4] = [[0.0; 4]; 4];
    guRotateF(&mut float_matrix, angle, x, y, z);
    guMtxF2L(&float_matrix, matrix);
}

#[allow(non_snake_case)]
fn guNormalize(x: &f32, y: &f32, z: &f32) -> (f32, f32, f32) {
    let mut mag: f32 = (x * x + y * y + z * z).sqrt();
    if mag == 0.0 {
        mag = 1.0;
    }
    (x / mag, y / mag, z / mag)
}

#[allow(non_snake_case)]
pub fn guRotateF(matrix: &mut [[f32; 4]; 4], angle: f32, x: f32, y: f32, z: f32) {
    guNormalize(&x, &y, &z);

    let angle = angle.to_radians();
    let sin_a = angle.sin();
    let cos_a = angle.cos();

    let prod1 = x * y * (1.0 - cos_a);
    let prod2 = y * z * (1.0 - cos_a);
    let prod3 = z * x * (1.0 - cos_a);

    guMtxIdentF(matrix);

    let xx = x * x;
    matrix[0][0] = (1.0 - xx) * cos_a + xx;
    matrix[2][1] = prod2 - x * sin_a;
    matrix[1][2] = prod2 + x * sin_a;
    let yy = y * y;
    matrix[1][1] = (1.0 - yy) * cos_a + yy;
    matrix[2][0] = prod3 + y * sin_a;
    matrix[0][2] = prod3 - y * sin_a;
    let zz = z * z;
    matrix[2][2] = (1.0 - zz) * cos_a + zz;
    matrix[1][0] = prod1 - z * sin_a;
    matrix[0][1] = prod1 + z * sin_a;
}
