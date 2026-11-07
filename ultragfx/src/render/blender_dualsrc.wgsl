// blender_dualsrc.wgsl — dual-source primary blender fragment entry.
//
// Assembled in lib.rs as: `enable dual_source_blending;` + combiner_prelude.wgsl + THIS file.
// The `enable` directive is PREPENDED by lib.rs (it must precede every declaration, so it cannot
// live in this file, which is concatenated AFTER the prelude). Likewise the shared combiner
// prelude (VsOut, Combiner bindings, eval_combiner, …) is prepended by lib.rs — do not redefine
// it here.
//
// CRITICAL [§5.3]: a module containing `@blend_src` fails naga validation at create_shader_module
// on any adapter WITHOUT DUAL_SOURCE_BLENDING. lib.rs therefore guards the assembly + module
// creation + pipeline construction behind a `device.features()` dual-source check — the fallback
// device NEVER receives this module.
//
// color0 supplies the local contribution; color1 scales framebuffer memory in either P or M.

struct FragOut {
    @location(0) @blend_src(0) color0: vec4<f32>,
    @location(0) @blend_src(1) color1: vec4<f32>,
}

fn shade_dual(in: VsOut) -> FragOut {
    let r = eval_combiner(in);
    let comb3 = blend_fog(r.rgb, r.alpha, in.color.a);
    alpha_discard(r, in.clip_position.xy);

    let blend = blend_framebuffer(comb3, r.alpha, in.color.a);
    var o: FragOut;
    o.color0 = vec4<f32>(blend.rgb * blend.weights.x, 1.0);
    o.color1 = vec4<f32>(vec3<f32>(blend.weights.y), 1.0);
    return o;
}

struct DualDepthOutput {
    @location(0) @blend_src(0) color0: vec4<f32>,
    @location(0) @blend_src(1) color1: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fs_main(in: VsOut) -> FragOut {
    return shade_dual(in);
}

@fragment
fn fs_depth(in: VsOut) -> DualDepthOutput {
    let color = shade_dual(in);
    return DualDepthOutput(color.color0, color.color1, fragment_depth(in.clip_position.z));
}
