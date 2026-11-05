@group(2) @binding(0) var decal_depth_tex: texture_depth_2d;

fn shade_decal_dual(in: VsOut) -> FragOut {
    let color = shade_dual(in);

    // ── ZMODE_DEC in-shader test against the scene depth written by earlier draws (see decal.wgsl). ──
    let coord = vec2<i32>(i32(in.clip_position.x), i32(in.clip_position.y));
    let sampled = textureLoad(decal_depth_tex, coord, 0);
    let pixel_z = fragment_depth(in.clip_position.z);
    let eps = 1.0e-4;
    if (pixel_z > sampled + eps) {
        discard;
    }
    let dz = abs(dpdx(pixel_z)) + abs(dpdy(pixel_z));
    if (abs(pixel_z - sampled) > max(dz, eps)) {
        discard;
    }

    return color;
}

@fragment
fn fs_decal(in: VsOut) -> FragOut {
    return shade_decal_dual(in);
}

@fragment
fn fs_decal_depth(in: VsOut) -> DualDepthOutput {
    let color = shade_decal_dual(in);
    return DualDepthOutput(color.color0, color.color1, fragment_depth(in.clip_position.z));
}
