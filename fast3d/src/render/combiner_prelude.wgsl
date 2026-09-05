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
    inv_tex1_size:   vec4<f32>, // TEXEL1 mirror of inv_tex_size: .xy = 1/(tex1_w, tex1_h); .z =
                                // tex_enable1 flag (1.0 when the second texture is used, else 0.0);
                                // .w = pad. In LOCKSTEP with the Rust CombinerUniform.
    lod_params:      vec4<f32>, // LOD params: .x = lod_enable (1.0 when the mip chain is active),
                                // .y = num_levels (declared; re-clamped to the real uploaded count
                                // in eval_combiner), .z = prim_lod_frac (the primitive LOD fraction), .w =
                                // lod_scale (1.0, native res). In LOCKSTEP with the Rust CombinerUniform.
    inv_detail_size: vec4<f32>, // DETAIL params: .xy = 1/(detail_w, detail_h) when a DETAIL tile is
                                // present, else (1,1). .z = prim_lod_min (the primitive LOD minimum), consumed
                                // by compute_lod under DETAIL/SHARPEN. .w = detail_mode bits (bit0 =
                                // SHARPEN, bit1 = DETAIL — DETAIL set only when a real tile was
                                // decoded). In LOCKSTEP with the Rust CombinerUniform.
};

@group(0) @binding(0) var tex0:  texture_2d<f32>;
@group(0) @binding(1) var samp0: sampler;
// The second texture (TEXEL1) + its sampler. Always bound — single-texture draws bind a 1×1 dummy
// so the group(0) layout is uniformly satisfied. Wired into the combiner in `eval_combiner`, which
// also applies the 2-cycle TEXEL0<->TEXEL1 role swap.
@group(0) @binding(2) var tex1:  texture_2d<f32>;
@group(0) @binding(3) var samp1: sampler;
// The DETAIL tile + its sampler (N64-faithful LOD DETAIL mode). Always bound — non-detail
// draws bind a 1×1 dummy so the group(0) layout is uniformly satisfied; sampled only when
// eval_combiner's `detail_active` gate is true (LOD active AND a real DETAIL tile was decoded).
@group(0) @binding(4) var tex_detail:  texture_2d<f32>;
@group(0) @binding(5) var samp_detail: sampler;
// N64-faithful per-level LOD textures. Each N64 LOD level is an INDEPENDENT texture (no halving
// constraint) — level 0 is `tex0` (@binding 0), levels 1..7 are these fixed bindings. `sample_level`
// switch-selects between them so a non-halving level set (e.g. two 32×32 TRILERP levels) is sampled
// faithfully. All levels share `samp0` (mip levels share wrap/filter). Unused level slots bind the
// 1×1 dummy; they are never selected because `compute_lod` clamps the level to the uploaded count.
@group(0) @binding(6)  var tex_lod1: texture_2d<f32>;
@group(0) @binding(7)  var tex_lod2: texture_2d<f32>;
@group(0) @binding(8)  var tex_lod3: texture_2d<f32>;
@group(0) @binding(9)  var tex_lod4: texture_2d<f32>;
@group(0) @binding(10) var tex_lod5: texture_2d<f32>;
@group(0) @binding(11) var tex_lod6: texture_2d<f32>;
@group(0) @binding(12) var tex_lod7: texture_2d<f32>;
@group(1) @binding(0) var<uniform> combiner: Combiner;

fn bits(v: u32, pos: u32, n: u32) -> u32 {
    return (v >> pos) & ((1u << n) - 1u);
}

// Selector decode functions (N64 RDP color combiner).
//
// TEXEL0 / TEXEL1 selectors read the PRE-RESOLVED texel pair `t0` / `t1` (each a vec4 of rgb + a):
// TEXEL0 (index 1) always returns `t0`, TEXEL1 (index 2) always returns `t1`. The caller supplies
// the already-swapped pair, so the 2-cycle TEXEL0<->TEXEL1 role swap lives entirely in
// `eval_combiner` (C_TEXEL0 -> secondCycle ? texVal1 : texVal0).

// color_a: 4-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,7=NOISE,else ZERO
fn color_a_rgb(idx: u32, t0: vec4<f32>, t1: vec4<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0xFu;
    if i == 0u { return combined; }
    if i == 1u { return t0.rgb; }
    if i == 2u { return t1.rgb; }
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    if i == 6u { return vec3<f32>(1.0); } // ONE
    // 7=NOISE, else ZERO
    return vec3<f32>(0.0);
}

// color_b: 4-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=KEY_CENTER,7=K4,else ZERO
fn color_b_rgb(idx: u32, t0: vec4<f32>, t1: vec4<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0xFu;
    if i == 0u { return combined; }
    if i == 1u { return t0.rgb; }
    if i == 2u { return t1.rgb; }
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    // 6=KEY_CENTER, 7=K4 -> unwired -> ZERO
    return vec3<f32>(0.0);
}

// color_c: 5-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,
//   6=KEY_SCALE,7=COMBINED_ALPHA,8=TEXEL0_ALPHA,9=TEXEL1_ALPHA,10=PRIM_ALPHA,
//   11=SHADE_ALPHA,12=ENV_ALPHA,13=LOD_FRAC,14=PRIM_LOD_FRAC,15=K5,else ZERO
fn color_c_rgb(idx: u32, t0: vec4<f32>, t1: vec4<f32>, shade: vec3<f32>, shade_a: f32, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>, prim_a: f32, env_a: f32, lod_fraction: f32, prim_lod_frac: f32) -> vec3<f32> {
    let i = idx & 0x1Fu;
    if i == 0u { return combined; }
    if i == 1u { return t0.rgb; }
    if i == 2u { return t1.rgb; }
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    // 6..15 -> wired subset: 8=TEXEL0_ALPHA, 9=TEXEL1_ALPHA, 10=PRIM_ALPHA, 11=SHADE_ALPHA,
    // 12=ENV_ALPHA, 13=LOD_FRACTION, 14=PRIM_LOD_FRAC (color-C mux slots).
    if i == 8u  { return vec3<f32>(t0.a); }
    if i == 9u  { return vec3<f32>(t1.a); }
    if i == 10u { return vec3<f32>(prim_a); }
    if i == 11u { return vec3<f32>(shade_a); }
    if i == 12u { return vec3<f32>(env_a); }
    if i == 13u { return vec3<f32>(lod_fraction); }
    if i == 14u { return vec3<f32>(prim_lod_frac); }
    return vec3<f32>(0.0);
}

// color_d: 3-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,else ZERO
fn color_d_rgb(idx: u32, t0: vec4<f32>, t1: vec4<f32>, shade: vec3<f32>, combined: vec3<f32>, prim: vec3<f32>, env: vec3<f32>) -> vec3<f32> {
    let i = idx & 0x7u;
    if i == 0u { return combined; }
    if i == 1u { return t0.rgb; }
    if i == 2u { return t1.rgb; }
    if i == 3u { return prim; }
    if i == 4u { return shade; }
    if i == 5u { return env; }
    if i == 6u { return vec3<f32>(1.0); } // ONE
    return vec3<f32>(0.0);
}

// alpha_abd: 3-bit. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,6=ONE,else ZERO
fn alpha_abd(idx: u32, t0_a: f32, t1_a: f32, shade_a: f32, combined_a: f32, prim_a: f32, env_a: f32) -> f32 {
    let i = idx & 0x7u;
    if i == 0u { return combined_a; }
    if i == 1u { return t0_a; }
    if i == 2u { return t1_a; }
    if i == 3u { return prim_a; }
    if i == 4u { return shade_a; }
    if i == 5u { return env_a; }
    if i == 6u { return 1.0; } // ONE
    return 0.0;
}

// alpha_c: 3-bit. 0=LOD_FRACTION,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,5=ENVIRONMENT,
//   6=PRIM_LOD_FRAC, else ZERO
fn alpha_c(idx: u32, t0_a: f32, t1_a: f32, shade_a: f32, combined_a: f32, prim_a: f32, env_a: f32, lod_fraction: f32, prim_lod_frac: f32) -> f32 {
    let i = idx & 0x7u;
    if i == 0u { return lod_fraction; } // 0 = LOD_FRACTION (the non-LOD default 1.0)
    if i == 1u { return t0_a; }
    if i == 2u { return t1_a; }
    if i == 3u { return prim_a; }
    if i == 4u { return shade_a; }
    if i == 5u { return env_a; }
    if i == 6u { return prim_lod_frac; } // 6 = PRIM_LOD_FRAC (the primitive LOD fraction)
    return 0.0;
}

// (a-b)*c+d combiner cycle
struct CycleResult {
    rgb:   vec3<f32>,
    alpha: f32,
};

// `t0` / `t1` are the PRE-RESOLVED texel pair for this cycle: `t0` is what a TEXEL0 selector reads,
// `t1` what a TEXEL1 selector reads. The caller applies the 2-cycle role swap before calling, so
// this function is swap-agnostic (see `eval_combiner`).
fn run_cycle(
    ca_idx: u32, cb_idx: u32, cc_idx: u32, cd_idx: u32,
    aa_idx: u32, ab_idx: u32, ac_idx: u32, ad_idx: u32,
    t0: vec4<f32>, t1: vec4<f32>, shade: vec4<f32>, combined: vec4<f32>,
    prim: vec4<f32>, env: vec4<f32>,
    lod_fraction: f32, prim_lod_frac: f32,
) -> CycleResult {
    let shade3  = shade.rgb;
    let comb3   = combined.rgb;
    let prim3   = prim.rgb;
    let env3    = env.rgb;

    let a_rgb = color_a_rgb(ca_idx, t0, t1, shade3, comb3, prim3, env3);
    let b_rgb = color_b_rgb(cb_idx, t0, t1, shade3, comb3, prim3, env3);
    let c_rgb = color_c_rgb(cc_idx, t0, t1, shade3, shade.a, comb3, prim3, env3, prim.a, env.a, lod_fraction, prim_lod_frac);
    let d_rgb = color_d_rgb(cd_idx, t0, t1, shade3, comb3, prim3, env3);
    let out_rgb = clamp((a_rgb - b_rgb) * c_rgb + d_rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let a_a = alpha_abd(aa_idx, t0.a, t1.a, shade.a, combined.a, prim.a, env.a);
    let b_a = alpha_abd(ab_idx, t0.a, t1.a, shade.a, combined.a, prim.a, env.a);
    let c_a = alpha_c(ac_idx, t0.a, t1.a, shade.a, combined.a, prim.a, env.a, lod_fraction, prim_lod_frac);
    let d_a = alpha_abd(ad_idx, t0.a, t1.a, shade.a, combined.a, prim.a, env.a);
    let out_a = clamp((a_a - b_a) * c_a + d_a, 0.0, 1.0);

    return CycleResult(out_rgb, out_a);
}

struct LodResult {
    level0:       f32,
    level1:       f32,
    lod_fraction: f32,
};

// N64-faithful LOD level/blend-fraction computation (the native-raster LOD path). `ddx_uv`/`ddy_uv`
// are texel-space level-0 UV screen derivatives; fast3d's TC_DIVISOR already matches the N64's
// texel-coordinate scale, so no extra scale is applied. `num_levels` is the REAL uploaded mip count
// (already clamped by the caller). `prim_lod_min` (the primitive LOD minimum) floors
// `maxDst`, but only under DETAIL/SHARPEN. `detail_mode` bits: bit0 = SHARPEN, bit1 = DETAIL.
//
// Tile/index-space adaptation: the N64 RDP samples each mip level via a dedicated tile; fast3d instead
// uploads the whole pyramid into one mipped `tex0` plus a separate `tex_detail` binding for the
// DETAIL tap. Under DETAIL, the hardware shifts `tileBase += 1` so index 0 is free for the tap — mirrored
// here: `level0`/`level1` are in that shifted space (0 = DETAIL tap, 1..num_levels = mip levels
// 0..num_levels-1), which `sample_tile` below un-shifts by one. `tile_max` is `num_levels - 1`
// normally, one extra slot under DETAIL for the shifted-in tap.
fn compute_lod(ddx_uv: vec2<f32>, ddy_uv: vec2<f32>, num_levels: f32, prim_lod_min: f32, lod_scale: f32, detail_mode: f32) -> LodResult {
    let mode = u32(detail_mode);
    let sharpen = (mode & 1u) != 0u;
    let detail = (mode & 2u) != 0u;

    let tile_max_pyramid = max(num_levels - 1.0, 0.0);
    let tile_max = select(tile_max_pyramid, tile_max_pyramid + 1.0, detail);

    let max_dd = max(abs(ddx_uv), abs(ddy_uv));
    var max_dst = max(max_dd.x, max_dd.y) * lod_scale;
    if detail || sharpen {
        max_dst = max(max_dst, prim_lod_min);
    }

    // Zero-derivative guard: max_dst == 0 (screen-aligned / constant UV) makes `log2(0)` undefined
    // in WGSL (unlike the IEEE -inf an HLSL target falls back on). Substitute a sentinel far
    // below every subsequent clamp/max so the branch logic resolves exactly as the -inf path
    // would; the SHARPEN override below still fires on the real `max_dst`, so its negative
    // extrapolation is unaffected by the sentinel.
    var tile_base: f32;
    if max_dst <= 0.0 {
        tile_base = -1.0e9;
    } else {
        tile_base = floor(log2(max_dst));
    }
    var lod_fraction = max_dst / pow(2.0, max(tile_base, 0.0)) - 1.0;

    if sharpen && max_dst < 1.0 {
        lod_fraction = max_dst - 1.0;
    }

    if detail {
        if lod_fraction < 0.0 {
            lod_fraction = max_dst;
        }
        tile_base = tile_base + 1.0;
    } else if tile_base >= tile_max {
        lod_fraction = 1.0;
    }

    if detail || sharpen {
        tile_base = max(tile_base, 0.0);
    } else {
        lod_fraction = max(lod_fraction, 0.0);
    }

    let level0 = clamp(tile_base, 0.0, tile_max);
    let level1 = clamp(tile_base + 1.0, 0.0, tile_max);
    return LodResult(level0, level1, lod_fraction);
}

// Switch-select one of the fixed per-level LOD textures by integer level index. Level 0 is `tex0`
// (@binding 0), levels 1..7 the `tex_lod*` bindings. `textureSampleLevel` (explicit LOD 0) is used —
// NOT `textureSample` — so the call is legal inside the non-uniform `switch` (implicit-derivative
// sampling requires uniform control flow). Each per-level texture is single-mip, so LOD 0 is its
// only level. `uv` is already normalized to [0,1] by the caller (level-0 dims); an independent
// level texture maps that same [0,1] range across its own extent, so a non-halving level samples
// correctly. The `default` clamps any out-of-range index to the highest binding (never reached for a
// real level — `compute_lod` clamps to the uploaded count).
fn sample_level(level_idx: u32, uv: vec2<f32>) -> vec4<f32> {
    switch level_idx {
        case 0u:  { return textureSampleLevel(tex0,     samp0, uv, 0.0); }
        case 1u:  { return textureSampleLevel(tex_lod1, samp0, uv, 0.0); }
        case 2u:  { return textureSampleLevel(tex_lod2, samp0, uv, 0.0); }
        case 3u:  { return textureSampleLevel(tex_lod3, samp0, uv, 0.0); }
        case 4u:  { return textureSampleLevel(tex_lod4, samp0, uv, 0.0); }
        case 5u:  { return textureSampleLevel(tex_lod5, samp0, uv, 0.0); }
        case 6u:  { return textureSampleLevel(tex_lod6, samp0, uv, 0.0); }
        default:  { return textureSampleLevel(tex_lod7, samp0, uv, 0.0); }
    }
}

// Sample one LOD level by its post-`compute_lod` index. Under DETAIL, index 0 is the DETAIL tap
// (its own UV normalization); index i >= 1 is LOD level `i - 1` (the `tileBase += 1` shift in
// `compute_lod` reserves index 0 for the tap). When DETAIL is off, index i is LOD level `i` directly.
// `idx` is always an exact non-negative integer (built from `floor`/`clamp` on integer-valued
// floats), so the comparisons below are exact, not epsilon-sensitive. The selected level is
// switch-dispatched to its independent per-level binding by `sample_level`.
fn sample_tile(idx: f32, uv: vec2<f32>, detail_active: bool) -> vec4<f32> {
    if detail_active && idx == 0.0 {
        return textureSampleLevel(tex_detail, samp_detail, uv * combiner.inv_detail_size.xy, 0.0);
    }
    let mip = select(idx, idx - 1.0, detail_active);
    // Clamp to the highest per-level binding so the switch never indexes past MAX_LOD-1.
    let level = u32(clamp(mip, 0.0, 7.0));
    return sample_level(level, uv * combiner.inv_tex_size.xy);
}

// Evaluate the full color combiner for a fragment, returning RGB + alpha.
// Shared by the base ubershader (skeleton.wgsl) and the dual-source blender (blender_dualsrc.wgsl).
fn eval_combiner(in: VsOut) -> CycleResult {
    // LOD derivatives. Computed UNCONDITIONALLY, in uniform control flow, before any lod-gated
    // branch below — WGSL `dpdx`/`dpdy` must not sit behind non-uniform control flow. `in.uv` is
    // the texel-space level-0 triangle texcoord (see compute_lod's doc comment for why no extra
    // tcScale is needed).
    let ddx_uv = dpdx(in.uv);
    let ddy_uv = dpdy(in.uv);

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

    let use_tex1 = combiner.inv_tex1_size.z != 0.0;
    let texel1 = textureSample(tex1, samp1, in.uv * combiner.inv_tex1_size.xy);
    let sentinel1 = vec4<f32>(1.0, 0.0, 1.0, 1.0); // unwired-TEXEL1 sentinel (never read when gated)
    // The value a TEXEL1 selector reads in CYCLE 0 (no swap yet): the tex1 sample when present, else
    // the sentinel.
    var t1_cyc0 = select(sentinel1, texel1, use_tex1);

    let shade  = in.color;
    let prim   = combiner.prim;
    let env    = combiner.env;

    // LOD_FRACTION selector value. The non-LOD default is 1.0.
    // PRIM_LOD_FRAC is the primitive LOD fraction, carried in lod_params.z.
    var lod_fraction = 1.0;
    let prim_lod_frac = combiner.lod_params.z;

    if combiner.lod_params.x != 0.0 {
        let detail_active = (u32(combiner.inv_detail_size.w) & 2u) != 0u;
        // `lod_params.y` is the REAL uploaded per-level count (Rust `uploaded_level_count`, capped at
        // MAX_LOD = 8). Levels are now INDEPENDENT textures (not a halving mip chain), so the count is
        // NOT bounded by `floor(log2(dims)) + 1` — a non-halving set (e.g. two 32×32 levels) has more
        // levels than that bound would allow. Clamp only to the fixed binding budget (8) so the
        // switch in `sample_level` can never select a level past `tex_lod7`.
        let n_levels = min(combiner.lod_params.y, 8.0);

        let lod = compute_lod(ddx_uv, ddy_uv, n_levels, combiner.inv_detail_size.z, combiner.lod_params.w, combiner.inv_detail_size.w);
        texel = sample_tile(lod.level0, in.uv, detail_active);
        t1_cyc0 = sample_tile(lod.level1, in.uv, detail_active);
        lod_fraction = lod.lod_fraction;
    }

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
        // 1-cycle: no pipeline swap. TEXEL0 -> tex0, TEXEL1 -> t1_cyc0 (sentinel unless enabled).
        let zero4 = vec4<f32>(0.0);
        result = run_cycle(ca1, cb1, cc1, cd1, aa1, ab1, ac1, ad1, texel, t1_cyc0, shade, zero4, prim, env, lod_fraction, prim_lod_frac);
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
        // Cycle 0 (secondCycle=false): TEXEL0 -> tex0, TEXEL1 -> tex1.
        let r0 = run_cycle(ca0, cb0, cc0, cd0, aa0, ab0, ac0, ad0, texel, t1_cyc0, shade, zero4, prim, env, lod_fraction, prim_lod_frac);
        let combined0 = vec4<f32>(r0.rgb, r0.alpha);
        // The RDP pipeline reverses physical texture roles in the second cycle.
        result = run_cycle(ca1, cb1, cc1, cd1, aa1, ab1, ac1, ad1, t1_cyc0, texel, shade, combined0, prim, env, lod_fraction, prim_lod_frac);
    }

    return result;
}
