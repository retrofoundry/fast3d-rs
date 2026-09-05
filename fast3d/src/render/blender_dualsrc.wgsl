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
// Reproduces the framebuffer-cycle N64 blender (P*A + M*B)/(A+B) with two blend sources:
//   color0 = vec4(P*(A/denom), 1.0); color1 = vec4(vec3(B/denom), 1.0)
//   pipeline blend color {src One, dst Src1, Add}, alpha {src One, dst Zero, Add}
//   → out_rgb = color0 + Src1*dst = P*(A/denom) + dst*(B/denom) = (P*A + M*B)/(A+B), with
//     M = dst = the framebuffer (CLR_MEM), supplied by the hardware blend rather than read in-shader.

struct FragOut {
    @location(0) @blend_src(0) color0: vec4<f32>,
    @location(0) @blend_src(1) color1: vec4<f32>,
}

// Blender P/A/B selector decode (N64 RDP blender). M (CLR_MEM) is the framebuffer dst,
// supplied by the hardware blend via Src1, so it is never read in-shader.
fn blend_p(sel: u32, comb: vec3<f32>, blendc: vec3<f32>, fogc: vec3<f32>) -> vec3<f32> {
    if sel == 0u { return comb; }   // CLR_IN  (combiner output)
    if sel == 2u { return blendc; } // CLR_BL  (blend color register)
    if sel == 3u { return fogc; }   // CLR_FOG (fog color register)
    return comb;                    // CLR_MEM (1) never read in-shader → harmless fallback
}
fn blend_a(sel: u32, comb_a: f32, fog_a: f32, shade_a: f32) -> f32 {
    if sel == 0u { return comb_a; }  // A_IN    (combiner alpha)
    if sel == 1u { return fog_a; }   // A_FOG   (fog color alpha)
    if sel == 2u { return shade_a; } // A_SHADE (shade alpha)
    return 0.0;                      // A_0
}
fn blend_b(sel: u32, a: f32) -> f32 {
    if sel == 0u { return 1.0 - a; } // 1MA
    if sel == 1u { return 1.0; }     // A_MEM → 1.0 (coverage unemulated, §9)
    if sel == 2u { return 1.0; }     // 1
    return 0.0;                      // 0
}

@fragment
fn fs_main(in: VsOut) -> FragOut {
    let r = eval_combiner(in);
    // C3: fog mix — when cyc1 blender P == CLR_FOG (3), mix combiner output toward fog color
    // before it is used as CLR_IN in the fb-cycle blender below.
    let p1 = (combiner.blender_mux >> 14u) & 3u;
    var comb3 = r.rgb;
    if (p1 == 3u) {
        comb3 = mix(comb3, combiner.fog_color.rgb, in.color.a);
    }
    let out_a = r.alpha;
    alpha_discard(r, in.clip_position.xy);

    // Decode the framebuffer-cycle P/A/B selectors from the blender mux. The framebuffer cycle is
    // cycle-2 for 2-cycle (cycle_type==1) and cycle-1 otherwise — mirrors hle::blender::classify.
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
    o.color0 = vec4<f32>(pcol * (a / denom), 1.0); // alpha-out = 1.0 (AA/coverage unemulated)
    o.color1 = vec4<f32>(vec3<f32>(b / denom), 1.0);
    return o;
}
