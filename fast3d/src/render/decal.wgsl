@group(2) @binding(0) var depth_tex: texture_depth_2d;

fn shade_decal(in: VsOut) -> vec4<f32> {
    let r = eval_combiner(in);
    var rgb = r.rgb;
    // C3: fog mix — applied when cyc1 blender P == CLR_FOG (3). Matches skeleton.wgsl/fs_main.
    let p1 = (combiner.blender_mux >> 14u) & 3u;
    if (p1 == 3u && combiner.primitive_depth.w == 0u) {
        rgb = mix(rgb, combiner.fog_color.rgb, in.color.a);
    }
    alpha_discard(r, in.clip_position.xy);

    // ── ZMODE_DEC in-shader test against the scene depth written by earlier draws. ──
    // `in.clip_position` is the rasterizer @builtin(position): .xy are framebuffer pixel coords,
    // .z is the NDC depth (post perspective-divide), matching the Depth32Float buffer.
    let coord = vec2<i32>(i32(in.clip_position.x), i32(in.clip_position.y));
    let sampled = textureLoad(depth_tex, coord, 0);
    let pixel_z = fragment_depth(in.clip_position.z);
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

@fragment
fn fs_decal(in: VsOut) -> @location(0) vec4<f32> {
    return shade_decal(in);
}

@fragment
fn fs_decal_depth(in: VsOut) -> DepthOutput {
    return DepthOutput(shade_decal(in), fragment_depth(in.clip_position.z));
}
