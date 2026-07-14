// skeleton.wgsl — base combiner ubershader fragment entry.
// Assembled in lib.rs AFTER combiner_prelude.wgsl (which defines VsOut, the Combiner bindings,
// the selector-decode helpers, run_cycle, and eval_combiner). This file holds only the base
// `@location(0)` fragment entry; the dual-source primary path lives in blender_dualsrc.wgsl.

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let r = eval_combiner(in);
    var rgb = r.rgb;
    // C3: fog mix — applied when cyc1 blender P == CLR_FOG (3).
    // fog_factor = in.color.a (shade alpha, written by the C2 RSP-process fog pass).
    // Non-fog runs have P != CLR_FOG, so this branch is never taken → zero output change.
    let p1 = (combiner.blender_mux >> 14u) & 3u;
    if (p1 == 3u) {
        rgb = mix(rgb, combiner.fog_color.rgb, in.color.a);
    }
    // Phase D: alpha-test discard. Only active when alpha_mode != 0 (CVG_X_ALPHA or THRESHOLD).
    // alpha_mode == 0 → NO discard → non-cutout runs are byte-identical (zero regression risk).
    if (combiner.alpha_mode != 0u && r.alpha < combiner.alpha_threshold) {
        discard;
    }
    return vec4<f32>(rgb, r.alpha);
}
