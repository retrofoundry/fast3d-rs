// skeleton.wgsl — base combiner ubershader fragment entry.
// Assembled in lib.rs AFTER combiner_prelude.wgsl (which defines VsOut, the Combiner bindings,
// the selector-decode helpers, run_cycle, and eval_combiner). This file holds the base
// `@location(0)` fragment entries, `fs_main` and the depth-exporting `fs_depth`; the dual-source
// primary path lives in blender_dualsrc.wgsl.

fn shade_fragment(in: VsOut) -> vec4<f32> {
    let r = eval_combiner(in);
    var rgb = r.rgb;
    if (combiner.primitive_depth.w == 0u) {
        rgb = blend_fog(rgb, r.alpha, in.color.a);
    }
    alpha_discard(r, in.clip_position.xy);
    return vec4<f32>(rgb, r.alpha);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return shade_fragment(in);
}

@fragment
fn fs_depth(in: VsOut) -> DepthOutput {
    return DepthOutput(shade_fragment(in), fragment_depth(in.clip_position.z));
}
