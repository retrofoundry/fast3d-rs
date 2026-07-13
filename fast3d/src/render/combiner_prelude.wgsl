// combiner_prelude.wgsl — shared combiner ubershader prelude.
// Assembled in lib.rs ahead of a fragment entry: `skeleton.wgsl` (base) or `blender_dualsrc.wgsl`
// (dual-source primary blender). Defines VsIn/VsOut, vs_main, the Combiner struct + bindings,
// the selector-decode helpers, run_cycle, and `eval_combiner` (the full (a-b)*c+d evaluation).
// One source of truth with the HLE diagnostic decoder (hle::combiner).
// Combiner mux bit positions from the N64 RDP color combiner.

struct VsIn {
    @location(0) position: vec4<f32>, // clip-space (x, y, z, w)
    @location(1) color:    vec4<f32>, // RGBA, linear 0..1
    @location(2) uv:       vec2<f32>, // normalized UV [0,1]
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv:    vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = in.position; // GPU does the perspective divide
    out.color = in.color;
    out.uv = in.uv;
    return out;
}

struct Combiner {
    combine_l:       u32,       // raw w0 (combine_l = w0)
    combine_h:       u32,       // raw w1 (combine_h = w1)
    cycle_type:      u32,       // 0 = 1-cycle (G_CYC_1CYCLE), 1 = 2-cycle
    tex_enable:      u32,       // 1 if texture is enabled
    blender_mux:     u32,       // raw blender mux (other_mode_l bits [31:16]) — wired in B3
    force_blend:     u32,       // 1 if FORCE_BL is set — wired in B3
    alpha_mode:      u32,       // 0=off,1=CVG_X_ALPHA,2=THRESHOLD — wired in Phase D
    alpha_threshold: f32,       // alpha discard threshold — wired in Phase D
    prim:            vec4<f32>, // primitive color RGBA normalized
    env:             vec4<f32>, // environment color RGBA normalized
    blend_color:     vec4<f32>, // blend color RGBA — wired in B3
    fog_color:       vec4<f32>, // fog color RGBA — wired in Phase C
    inv_tex_size:    vec4<f32>, // .xy = 1/(tex_w, tex_h): draw-time tile-size normalization for the
                                // TEXEL-space triangle texcoord. (1,1) for rects/texgen whose uv is
                                // already normalized.
};

@group(0) @binding(0) var tex0:  texture_2d<f32>;
@group(0) @binding(1) var samp0: sampler;
@group(1) @binding(0) var<uniform> combiner: Combiner;

fn bits(v: u32, pos: u32, n: u32) -> u32 {
    return (v >> pos) & ((1u << n) - 1u);
}

// Selector decode functions (N64 RDP color combiner)

// color_a: 4-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,7=NOISE,else ZERO
fn color_a_rgb(idx: u32, texel: vec3<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0xFu;
    if i == 0u { return combined; }
    if i == 1u { return texel; }
    // 2=TEXEL1 (unwired->magenta sentinel), 3=PRIMITIVE
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    if i == 6u { return vec3<f32>(1.0); } // ONE
    // 7=NOISE, else ZERO
    if i == 2u { return vec3<f32>(1.0, 0.0, 1.0); } // TEXEL1 sentinel magenta
    return vec3<f32>(0.0); // ZERO (and NOISE -> defense-in-depth zero)
}

// color_b: 4-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=KEY_CENTER,7=K4,else ZERO
fn color_b_rgb(idx: u32, texel: vec3<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0xFu;
    if i == 0u { return combined; }
    if i == 1u { return texel; }
    if i == 2u { return vec3<f32>(1.0, 0.0, 1.0); } // TEXEL1 sentinel
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    // 6=KEY_CENTER, 7=K4 -> unwired -> ZERO (sentinel only for wired; magenta only for Texel1)
    return vec3<f32>(0.0);
}

// color_c: 5-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,
//   6=KEY_SCALE,7=COMBINED_ALPHA,8=TEXEL0_ALPHA,9=TEXEL1_ALPHA,10=PRIM_ALPHA,
//   11=SHADE_ALPHA,12=ENV_ALPHA,13=LOD_FRAC,14=PRIM_LOD_FRAC,15=K5,else ZERO
fn color_c_rgb(idx: u32, texel: vec3<f32>, shade: vec3<f32>, texel_a: f32, shade_a: f32, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>, prim_a: f32, env_a: f32) -> vec3<f32> {
    let i = idx & 0x1Fu;
    if i == 0u { return combined; }
    if i == 1u { return texel; }
    if i == 2u { return vec3<f32>(1.0, 0.0, 1.0); } // TEXEL1 sentinel
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    // 6..15 -> wired subset: 8=TEXEL0_ALPHA, 10=PRIM_ALPHA, 11=SHADE_ALPHA, 12=ENV_ALPHA
    if i == 8u  { return vec3<f32>(texel_a); }
    if i == 10u { return vec3<f32>(prim_a); }
    if i == 11u { return vec3<f32>(shade_a); }
    if i == 12u { return vec3<f32>(env_a); }
    return vec3<f32>(0.0);
}

// color_d: 3-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,else ZERO
fn color_d_rgb(idx: u32, texel: vec3<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0x7u;
    if i == 0u { return combined; }
    if i == 1u { return texel; }
    if i == 2u { return vec3<f32>(1.0, 0.0, 1.0); } // TEXEL1 sentinel
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    if i == 6u { return vec3<f32>(1.0); } // ONE
    return vec3<f32>(0.0);
}

// alpha_abd: 3-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,else ZERO
fn alpha_abd(idx: u32, texel_a: f32, shade_a: f32, combined_a: f32, prim_a: f32, env_a: f32) -> f32 {
    let i = idx & 0x7u;
    if i == 0u { return combined_a; }
    if i == 1u { return texel_a; }
    if i == 2u { return 1.0; } // TEXEL1 sentinel (magenta not applicable for alpha)
    if i == 3u { return prim_a; }
    if i == 4u { return shade_a; }
    if i == 5u { return env_a; }
    if i == 6u { return 1.0; } // ONE
    return 0.0;
}

// alpha_c: 3-bit. 0=LOD_FRACTION,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,
//   6=PRIM_LOD_FRAC, else ZERO
fn alpha_c(idx: u32, texel_a: f32, shade_a: f32, combined_a: f32, prim_a: f32, env_a: f32) -> f32 {
    let i = idx & 0x7u;
    // 0=LOD_FRACTION -> unwired -> 0 (defense-in-depth; HLE should have refused to draw)
    if i == 1u { return texel_a; }
    if i == 2u { return 1.0; } // TEXEL1 sentinel
    if i == 3u { return prim_a; }
    if i == 4u { return shade_a; }
    if i == 5u { return env_a; }
    // 6=PRIM_LOD_FRAC -> unwired -> 0
    return 0.0;
}

// (a-b)*c+d combiner cycle
struct CycleResult {
    rgb:   vec3<f32>,
    alpha: f32,
};

fn run_cycle(
    ca_idx: u32, cb_idx: u32, cc_idx: u32, cd_idx: u32,
    aa_idx: u32, ab_idx: u32, ac_idx: u32, ad_idx: u32,
    texel: vec4<f32>, shade: vec4<f32>, combined: vec4<f32>,
    prim: vec4<f32>, env: vec4<f32>,
) -> CycleResult {
    let texel3  = texel.rgb;
    let shade3  = shade.rgb;
    let comb3   = combined.rgb;
    let prim3   = prim.rgb;
    let env3    = env.rgb;

    let a_rgb = color_a_rgb(ca_idx, texel3, shade3, comb3, prim3, env3);
    let b_rgb = color_b_rgb(cb_idx, texel3, shade3, comb3, prim3, env3);
    let c_rgb = color_c_rgb(cc_idx, texel3, shade3, texel.a, shade.a, comb3, prim3, env3, prim.a, env.a);
    let d_rgb = color_d_rgb(cd_idx, texel3, shade3, comb3, prim3, env3);
    let out_rgb = clamp((a_rgb - b_rgb) * c_rgb + d_rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let a_a = alpha_abd(aa_idx, texel.a, shade.a, combined.a, prim.a, env.a);
    let b_a = alpha_abd(ab_idx, texel.a, shade.a, combined.a, prim.a, env.a);
    let c_a = alpha_c(ac_idx, texel.a, shade.a, combined.a, prim.a, env.a);
    let d_a = alpha_abd(ad_idx, texel.a, shade.a, combined.a, prim.a, env.a);
    let out_a = clamp((a_a - b_a) * c_a + d_a, 0.0, 1.0);

    return CycleResult(out_rgb, out_a);
}

// Evaluate the full color combiner for a fragment, returning RGB + alpha.
// Shared by the base ubershader (skeleton.wgsl) and the dual-source blender (blender_dualsrc.wgsl).
fn eval_combiner(in: VsOut) -> CycleResult {
    let l = combiner.combine_l;
    let h = combiner.combine_h;

    var texel: vec4<f32>;
    if combiner.tex_enable != 0u {
        // Normalize the TEXEL-space triangle texcoord by the draw-time tile dims. inv_tex_size =
        // (1,1) leaves already-normalized rect / texgen uv untouched.
        texel = textureSample(tex0, samp0, in.uv * combiner.inv_tex_size.xy);
    } else {
        texel = vec4<f32>(1.0);
    }

    let shade  = in.color;
    let prim   = combiner.prim;
    let env    = combiner.env;

    // 1-cycle (cycle_type==0) uses cycle-1 slots (F3DEX2 convention); combined starts as zero.
    // 2-cycle evaluates cycle-0 first, then cycle-1 with cycle-0 output as combined.
    // Combiner mux parse positions (N64 RDP color combiner):
    //   cyc1: color a=L[5,4]  b=H[24,4] c=L[0,5]  d=H[6,3]
    //         alpha a=H[21,3] b=H[3,3]  c=H[18,3] d=H[0,3]
    //   cyc0: color a=L[20,4] b=H[28,4] c=L[15,5] d=H[15,3]
    //         alpha a=L[12,3] b=H[12,3] c=L[9,3]  d=H[9,3]
    let ca1 = bits(l, 5u, 4u);
    let cb1 = bits(h, 24u, 4u);
    let cc1 = bits(l, 0u, 5u);
    let cd1 = bits(h, 6u, 3u);
    let aa1 = bits(h, 21u, 3u);
    let ab1 = bits(h, 3u, 3u);
    let ac1 = bits(h, 18u, 3u);
    let ad1 = bits(h, 0u, 3u);

    var result: CycleResult;
    if combiner.cycle_type == 0u {
        let zero4 = vec4<f32>(0.0);
        result = run_cycle(ca1, cb1, cc1, cd1, aa1, ab1, ac1, ad1, texel, shade, zero4, prim, env);
    } else {
        let ca0 = bits(l, 20u, 4u);
        let cb0 = bits(h, 28u, 4u);
        let cc0 = bits(l, 15u, 5u);
        let cd0 = bits(h, 15u, 3u);
        let aa0 = bits(l, 12u, 3u);
        let ab0 = bits(h, 12u, 3u);
        let ac0 = bits(l, 9u, 3u);
        let ad0 = bits(h, 9u, 3u);
        let zero4 = vec4<f32>(0.0);
        let r0 = run_cycle(ca0, cb0, cc0, cd0, aa0, ab0, ac0, ad0, texel, shade, zero4, prim, env);
        let combined0 = vec4<f32>(r0.rgb, r0.alpha);
        result = run_cycle(ca1, cb1, cc1, cd1, aa1, ab1, ac1, ad1, texel, shade, combined0, prim, env);
    }

    return result;
}
