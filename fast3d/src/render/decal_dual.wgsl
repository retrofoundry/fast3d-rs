// decal_dual.wgsl — dual-source DECAL fragment entry (E2). Concatenated in lib.rs AFTER
// blender_dualsrc.wgsl (only on a DUAL_SOURCE_BLENDING device), so it reuses that file's
// `blend_p`/`blend_a`/`blend_b` helpers and `FragOut`, plus the prelude's `eval_combiner`.
// It is the dual-source twin of decal.wgsl: the same in-shader ZMODE_DEC occlusion + coplanar
// discard, then the framebuffer-cycle dual-source blend output. Bound to the dual-source decal
// pipelines (decal layout g0+g1+g2). `fs_main` (the non-decal dual entry) is left untouched.

@group(2) @binding(0) var decal_depth_tex: texture_depth_2d;

@fragment
fn fs_decal(in: VsOut) -> FragOut {
    let r = eval_combiner(in);
    let p1 = (combiner.blender_mux >> 14u) & 3u;
    var comb3 = r.rgb;
    if (p1 == 3u) {
        comb3 = mix(comb3, combiner.fog_color.rgb, in.color.a);
    }
    let out_a = r.alpha;
    if (combiner.alpha_mode != 0u && out_a < combiner.alpha_threshold) {
        discard;
    }

    // ── ZMODE_DEC in-shader test against the scene depth written by pass 1 (see decal.wgsl). ──
    let coord = vec2<i32>(i32(in.clip_position.x), i32(in.clip_position.y));
    let sampled = textureLoad(decal_depth_tex, coord, 0);
    let pixel_z = in.clip_position.z;
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
