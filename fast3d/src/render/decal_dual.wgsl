@group(2) @binding(0) var decal_depth_tex: texture_depth_2d;

fn shade_decal_dual(in: VsOut) -> FragOut {
    let r = eval_combiner(in);
    let comb3 = blend_fog(r.rgb, r.alpha, in.color.a);
    let out_a = r.alpha;
    alpha_discard(r, in.clip_position.xy);

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

    // Framebuffer-cycle dual-source blend — identical decode to blender_dualsrc.wgsl/fs_main.
    let bi = combiner.blender_mux;
    let cc2 = combiner.cycle_type == 1u;
    let p_sel = select((bi >> 14u) & 3u, (bi >> 12u) & 3u, cc2);
    let a_sel = select((bi >> 10u) & 3u, (bi >> 8u) & 3u, cc2);
    let b_sel = select((bi >> 2u) & 3u, bi & 3u, cc2);

    let pcol = blend_p(p_sel, comb3, combiner.blend_color.rgb, combiner.fog_color.rgb);
    let a = blend_a(a_sel, out_a, combiner.fog_color.a, in.color.a);
    let b = blend_b(b_sel, a);
    let denom = max(a + b, 1.0 / 255.0);

    var o: FragOut;
    o.color0 = vec4<f32>(pcol * (a / denom), 1.0);
    o.color1 = vec4<f32>(vec3<f32>(b / denom), 1.0);
    return o;
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
