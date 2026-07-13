// decal.wgsl — base decal fragment entry (E2): combiner + in-shader Z occlusion + coplanar discard.
// Assembled in lib.rs AFTER combiner_prelude.wgsl + skeleton.wgsl (it adds the decal-only entry
// `fs_decal` alongside skeleton's non-decal `fs_main`). Bound to the Replace/AlphaOver decal
// pipelines (decal layout g0+g1+g2). The decal pass has NO depth attachment, so the ROP cannot
// do the Z test; instead we sample the prior depth pass's output (`@group(2)`) and do the
// ZMODE_DEC test in-shader:
//   - occlusion (Z_CMP): a decal strictly BEHIND the stored scene surface is hidden;
//   - coplanar bind: paint only where the decal lies ON (within tolerance of) that surface, so a
//     coplanar decal shows WITHOUT z-fighting.
// NDC-space epsilon (a small constant), NOT the N64's hardware-encoded exponent bands [IMP11].

@group(2) @binding(0) var depth_tex: texture_depth_2d;

@fragment
fn fs_decal(in: VsOut) -> @location(0) vec4<f32> {
    let r = eval_combiner(in);
    var rgb = r.rgb;
    // C3: fog mix — applied when cyc1 blender P == CLR_FOG (3). Matches skeleton.wgsl/fs_main.
    let p1 = (combiner.blender_mux >> 14u) & 3u;
    if (p1 == 3u) {
        rgb = mix(rgb, combiner.fog_color.rgb, in.color.a);
    }
    // Phase D: alpha-test discard (only active when alpha_mode != 0).
    if (combiner.alpha_mode != 0u && r.alpha < combiner.alpha_threshold) {
        discard;
    }

    // ── ZMODE_DEC in-shader test against the scene depth written by pass 1. ──
    // `in.clip_position` is the rasterizer @builtin(position): .xy are framebuffer pixel coords,
    // .z is the NDC depth (post perspective-divide), matching the Depth32Float buffer.
    let coord = vec2<i32>(i32(in.clip_position.x), i32(in.clip_position.y));
    let sampled = textureLoad(depth_tex, coord, 0);
    let pixel_z = in.clip_position.z;
    let eps = 1.0e-4;
    // Occlusion (Z_CMP): a strictly-nearer surface in the depth buffer hides the decal.
    if (pixel_z > sampled + eps) {
        discard;
    }
    // Coplanar bind: discard fragments NOT coplanar with the stored surface (per-pixel slope
    // tolerance dz absorbs depth gradient on tilted surfaces; floored at eps for flat ones).
    let dz = abs(dpdx(pixel_z)) + abs(dpdy(pixel_z));
    if (abs(pixel_z - sampled) > max(dz, eps)) {
        discard;
    }
    return vec4<f32>(rgb, r.alpha);
}
