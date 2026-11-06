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

struct DepthOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

fn fragment_depth(pixel_z: f32) -> f32 {
    if combiner.primitive_depth.x != 0u {
        let z18 = (combiner.primitive_depth.y & 0x7fffu) << 3u;
        return f32(z18) / 262144.0;
    }
    return clamp(pixel_z, 0.0, 1.0);
}

override depth_clip_control: bool = false;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = in.position;
    if !depth_clip_control && combiner.inv_tex_size.w != 0.0 && in.position.w > 0.0 {
        // Without depth-clip control, vertex clamping approximates depth across crossing triangles.
        // Vertices behind the eye keep their depth so near clipping still removes them.
        out.clip_position.z = clamp(in.position.z, 0.0, in.position.w);
    }
    if combiner.is_rect == 0u {
        out.clip_position.x += in.position.w / f32(combiner.frame.z);
        out.clip_position.y -= in.position.w / f32(combiner.frame.w);
    }
    out.color = in.color;
    out.uv = in.uv;
    return out;
}

struct Combiner {
    combine_l:       u32,       // raw w0 (combine_l = w0)
    combine_h:       u32,       // raw w1 (combine_h = w1)
    cycle_type:      u32,       // 0 = 1-cycle, 1 = 2-cycle, 2 = COPY
    tex_enable:      u32,       // 1 if texture is enabled
    blender_mux:     u32,       // raw blender mux (other_mode_l bits [31:16]) — wired in B3
    force_blend:     u32,       // 1 if FORCE_BL is set — wired in B3
    alpha_flags:     u32,       // low two bits: compare mode; bit 2: coverage approximation
    alpha_threshold: f32,       // blend-color alpha threshold
    prim:            vec4<f32>, // primitive color RGBA normalized
    env:             vec4<f32>, // environment color RGBA normalized
    blend_color:     vec4<f32>, // blend color RGBA — wired in B3
    fog_color:       vec4<f32>, // fog color RGBA — wired in Phase C
    inv_tex_size:    vec4<f32>, // .xy = 1/(tex_w, tex_h) for texel-space triangle and rectangle UVs.
                                // .z = 1 for rectangles, 0 for triangles; .w enables NoN depth clamping.
    inv_tex1_size:   vec4<f32>, // TEXEL1 mirror of inv_tex_size: .xy = 1/(tex1_w, tex1_h); .z =
                                // tex_enable1 flag (1.0 when the second texture is used, else 0.0);
                                // .w = filter mode (0 = point, 2 = bilerp, 3 = average).
    lod_params:      vec4<f32>, // LOD params: .x = lod_enable (1.0 when the mip chain is active),
                                // .y = num_levels (declared; re-clamped to the real uploaded count
                                // in eval_combiner), .z = prim_lod_frac (the primitive LOD fraction), .w =
                                // lod_scale (1.0, native res). In LOCKSTEP with the Rust CombinerUniform.
    inv_detail_size: vec4<f32>, // DETAIL params: .xy = 1/(detail_w, detail_h) when a DETAIL tile is
                                // present, else (1,1). .z = prim_lod_min (the primitive LOD minimum), consumed
                                // by compute_lod under DETAIL/SHARPEN. .w = detail_mode bits (bit0 =
                                // SHARPEN, bit1 = DETAIL — DETAIL set only when a real tile was
                                // decoded). In LOCKSTEP with the Rust CombinerUniform.
    frame:           vec4<u32>, // serial, seed, raster viewport width and height
    is_rect:         u32,
    coverage_slot:   u32,       // draw slot + 1, written to the guest write-back coverage target
    fill_word:       u32,       // FillRect's raw fill-color register
    fill_siz:        u32,       // FillRect color-image siz + 1; 0 for other draws
    primitive_depth: vec4<u32>,
    raster:          vec4<u32>,
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
// constraint) — level 0 is `tex0` (@binding 0), levels 1..7 are these fixed bindings. `sample_tile`
// switch-selects between them so a non-halving level set (e.g. two 32×32 TRILERP levels) is sampled
// faithfully. Unused level slots bind the
// 1×1 dummy; they are never selected because `compute_lod` clamps the level to the uploaded count.
@group(0) @binding(6)  var tex_lod1: texture_2d<f32>;
@group(0) @binding(7)  var tex_lod2: texture_2d<f32>;
@group(0) @binding(8)  var tex_lod3: texture_2d<f32>;
@group(0) @binding(9)  var tex_lod4: texture_2d<f32>;
@group(0) @binding(10) var tex_lod5: texture_2d<f32>;
@group(0) @binding(11) var tex_lod6: texture_2d<f32>;
@group(0) @binding(12) var tex_lod7: texture_2d<f32>;
struct TileSampling {
    bounds: vec4<i32>,
    shift_mask: vec4<u32>,
    modes: vec4<u32>,
    image: vec4<u32>,
    tmem: vec4<u32>,
    palette: vec4<u32>,
};
@group(0) @binding(13) var<uniform> tile_sampling: array<TileSampling, 10>;
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
    compare_alpha: f32,
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

    return CycleResult(out_a, out_rgb, out_a);
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

fn tile_shift(value: f32, shift: u32) -> f32 {
    if shift <= 10u { return value / f32(1u << shift); }
    return value * f32(1u << (16u - shift));
}

fn tile_coordinate(tile: TileSampling, uv: vec2<f32>, high_resolution: bool) -> vec2<f32> {
    let shifted = vec2<f32>(tile_shift(uv.x, tile.shift_mask.x), tile_shift(uv.y, tile.shift_mask.y));
    if high_resolution {
        return shifted - vec2<f32>(tile.bounds.xy) / 4.0;
    }
    return round(shifted * 128.0) / 128.0 - vec2<f32>(tile.bounds.xy) / 4.0;
}

fn tile_address_axis(tap: i32, mode: u32, mask: u32, extent: u32) -> i32 {
    var addressed = tap;
    if (mode & 2u) != 0u || mask == 0u {
        addressed = clamp(addressed, 0, i32(extent) - 1);
    }
    if mask != 0u {
        let period = i32(1u << mask);
        if (mode & 1u) != 0u {
            let cycle = period * 2;
            let phase = addressed & (cycle - 1);
            addressed = select(cycle - 1 - phase, phase, phase < period);
        } else {
            addressed = addressed & (period - 1);
        }
    }
    return addressed;
}

struct TileTaps {
    tap00: vec2<i32>,
    tap10: vec2<i32>,
    tap01: vec2<i32>,
    tap11: vec2<i32>,
};

fn tile_taps(tile: TileSampling, base: vec2<i32>) -> TileTaps {
    let x0 = u32(tile_address_axis(base.x, tile.modes.x, tile.shift_mask.z, tile.modes.z));
    var x1 = u32(tile_address_axis(base.x + 1, tile.modes.x, tile.shift_mask.z, tile.modes.z));
    let addressed_y0 = tile_address_axis(base.y, tile.modes.y, tile.shift_mask.w, tile.modes.w);
    let addressed_y1 = tile_address_axis(base.y + 1, tile.modes.y, tile.shift_mask.w, tile.modes.w);
    var y0 = addressed_y0;
    var y1 = addressed_y1;
    let tlut = tile.palette.y >= 2u && tile.image.z != 2u;
    let nearest_tlut = tlut && (tile.palette.w & 8u) == 0u;
    if tile.image.z == 1u && !nearest_tlut {
        y0 = addressed_y0 & 255;
        y1 = y0 + addressed_y1 - addressed_y0;
        if tile.shift_mask.w != 0u && (tile.modes.y & 1u) == 0u && addressed_y1 < addressed_y0 {
            y1 = 0;
        }
    }
    if tlut && tile.tmem.z == 1u {
        x1 = u32(i32(x0) + 2 * (i32(x1) - i32(x0)));
    }
    if nearest_tlut {
        x1 = x0;
        y1 = y0;
    }

    // RGBA32 occupies two bytes per bank; the CPU lookup already resolves both banks.
    let half_byte_shift = select(min(tile.tmem.w, 2u), 1u, tlut && tile.tmem.z == 1u);
    let tmem_x0 = (x0 << half_byte_shift) >> 1u;
    let tmem_x1 = (x1 << half_byte_shift) >> 1u;
    let tmem_y0 = u32(y0) * tile.tmem.y;
    let tmem_y1 = u32(y1) * tile.tmem.y;
    let row_x0 = x0 & 1u;
    let row_x1 = x1 & 1u;
    let row_y0 = (u32(y0) & 1u) * 2u;
    let row_y1 = (u32(y1) & 1u) * 2u;
    let lookup = tile.image.z == 1u;
    let tap00 = select(vec2<u32>(x0, u32(y0)), vec2<u32>((tmem_y0 + tmem_x0) & 4095u, row_y0 + row_x0), lookup);
    let tap10 = select(vec2<u32>(x1, u32(y0)), vec2<u32>((tmem_y0 + tmem_x1) & 4095u, row_y0 + row_x1), lookup);
    let tap01 = select(vec2<u32>(x0, u32(y1)), vec2<u32>((tmem_y1 + tmem_x0) & 4095u, row_y1 + row_x0), lookup);
    let tap11 = select(vec2<u32>(x1, u32(y1)), vec2<u32>((tmem_y1 + tmem_x1) & 4095u, row_y1 + row_x1), lookup);
    return TileTaps(vec2<i32>(tap00), vec2<i32>(tap10), vec2<i32>(tap01), vec2<i32>(tap11));
}

fn tile_interpolate(c00: vec4<f32>, c10: vec4<f32>, c01: vec4<f32>, c11: vec4<f32>, fraction: vec2<f32>, mode: u32) -> vec4<f32> {
    if mode == 3u && all(abs(fraction - 0.5) <= vec2<f32>(1.0 / 128.0)) {
        return (c00 + c10 + c01 + c11) * 0.25;
    }
    if fraction.x + fraction.y < 1.0 {
        return c00 + fraction.x * (c10 - c00) + fraction.y * (c01 - c00);
    }
    return c11 + (1.0 - fraction.x) * (c01 - c11) + (1.0 - fraction.y) * (c10 - c11);
}

fn replacement_axis(tap: i32, mode: u32, mask: u32, extent: u32, scale: u32) -> i32 {
    var addressed = tap;
    if (mode & 2u) != 0u || mask == 0u {
        addressed = clamp(addressed, 0, i32(extent * scale) - 1);
    }
    if mask != 0u {
        let period = i32((1u << mask) * scale);
        let cycle = select(period, period * 2, (mode & 1u) != 0u);
        var phase = addressed % cycle;
        if phase < 0 { phase += cycle; }
        addressed = select(cycle - 1 - phase, phase, phase < period);
    }
    return addressed;
}

fn replacement_taps(tile: TileSampling, base: vec2<i32>) -> TileTaps {
    let scale = tile.image.w;
    let x0 = replacement_axis(base.x, tile.modes.x, tile.shift_mask.z, tile.modes.z, scale);
    let x1 = replacement_axis(base.x + 1, tile.modes.x, tile.shift_mask.z, tile.modes.z, scale);
    let y0 = replacement_axis(base.y, tile.modes.y, tile.shift_mask.w, tile.modes.w, scale);
    let y1 = replacement_axis(base.y + 1, tile.modes.y, tile.shift_mask.w, tile.modes.w, scale);
    return TileTaps(vec2<i32>(x0, y0), vec2<i32>(x1, y0), vec2<i32>(x0, y1), vec2<i32>(x1, y1));
}

struct TilePlan {
    taps: TileTaps,
    mode: u32,
    fraction: vec2<f32>,
    fraction_rg: vec2<f32>,
    yuv: bool,
    swap_alpha: bool,
};

struct TileQuad {
    c00: vec4<f32>,
    c10: vec4<f32>,
    c01: vec4<f32>,
    c11: vec4<f32>,
};

struct SampleCoordinates {
    unsnapped: vec2<f32>,
    snapped: vec2<f32>,
};

// Texture-independent work, planned once per sample so the per-binding switches only select
// loads: inlining the whole filter for every binding produced a shader WARP miscompiles.
fn tile_plan(index: u32, coordinates: SampleCoordinates, cycle: u32, dimensions: vec2<u32>) -> TilePlan {
    let high_resolution = (combiner.raster.w & (1u << index)) != 0u;
    let uv = select(coordinates.snapped, coordinates.unsnapped, high_resolution);
    var tile = tile_sampling[index];
    var coordinate: vec2<f32>;
    if tile.image.z == 2u {
        let extent = select(dimensions, tile.modes.zw, tile.image.w != 0u);
        tile.modes = vec4<u32>(2u, 2u, extent.x, extent.y);
        coordinate = tile_coordinate(tile, uv * vec2<f32>(extent), high_resolution);
    } else {
        coordinate = tile_coordinate(tile, uv, high_resolution);
    }
    let scaled = tile.image.z != 1u && tile.image.w > 1u;
    if scaled { coordinate *= f32(tile.image.w); }
    let base = vec2<i32>(floor(coordinate));
    var plan: TilePlan;
    plan.taps = tile_taps(tile, base);
    if scaled { plan.taps = replacement_taps(tile, base); }
    plan.mode = u32(combiner.inv_tex1_size.w);
    let tlut = tile.palette.y >= 2u && tile.image.z != 2u;
    let bilerp = (tile.palette.w & select(2u, 1u, cycle != 0u)) != 0u;
    if tlut && !scaled && (tile.palette.w & 8u) == 0u && bilerp && combiner.cycle_type != 2u {
        plan.mode = select(2u, 3u, (tile.palette.w & 4u) != 0u);
    }
    if plan.mode == 0u {
        return plan;
    }

    var fraction = fract(coordinate);
    if tlut && tile.palette.z != 0u {
        if ((tile.modes.x & 2u) != 0u || tile.shift_mask.z == 0u) &&
            (coordinate.x < 0.0 || coordinate.x >= f32(tile.modes.z - 1u)) {
            fraction.x = 0.0;
        }
        if ((tile.modes.y & 2u) != 0u || tile.shift_mask.w == 0u) &&
            (coordinate.y < 0.0 || coordinate.y >= f32(tile.modes.w - 1u)) {
            fraction.y = 0.0;
        }
    }
    var fraction_rg = fraction;
    plan.yuv = tlut && tile.tmem.z == 1u;
    if plan.yuv {
        let s = tile_address_axis(base.x, tile.modes.x, tile.shift_mask.z, tile.modes.z);
        fraction_rg.x = f32((u32(fraction.x * 32.0) >> 1u) | (u32(s & 1) << 4u)) / 32.0;
    }
    let upper = fraction.x + fraction.y >= 1.0;
    let upper_rg = fraction_rg.x + fraction_rg.y >= 1.0;
    if tlut && tile.palette.z != 0u {
        let lane_xor = select(0u, 3u, upper_rg);
        plan.taps.tap00.y += i32((0u ^ lane_xor) * tile.palette.z);
        plan.taps.tap10.y += i32((1u ^ lane_xor) * tile.palette.z);
        plan.taps.tap01.y += i32((2u ^ lane_xor) * tile.palette.z);
        plan.taps.tap11.y += i32((3u ^ lane_xor) * tile.palette.z);
    }
    plan.fraction = fraction;
    plan.fraction_rg = fraction_rg;
    plan.swap_alpha = plan.yuv && upper != upper_rg;
    return plan;
}

fn tile_load(texture: texture_2d<f32>, plan: TilePlan) -> TileQuad {
    var quad: TileQuad;
    quad.c00 = textureLoad(texture, plan.taps.tap00, 0);
    if plan.mode != 0u {
        quad.c10 = textureLoad(texture, plan.taps.tap10, 0);
        quad.c01 = textureLoad(texture, plan.taps.tap01, 0);
        quad.c11 = textureLoad(texture, plan.taps.tap11, 0);
    }
    return quad;
}

fn tile_resolve(quad: TileQuad, plan: TilePlan) -> vec4<f32> {
    if plan.mode == 0u {
        return quad.c00;
    }
    var c00 = quad.c00;
    var c10 = quad.c10;
    var c01 = quad.c01;
    var c11 = quad.c11;
    if plan.swap_alpha {
        let ba00 = c00.ba;
        let ba10 = c10.ba;
        c00 = vec4<f32>(c00.rg, c11.ba);
        c10 = vec4<f32>(c10.rg, c01.ba);
        c01 = vec4<f32>(c01.rg, ba10);
        c11 = vec4<f32>(c11.rg, ba00);
    }
    var result = tile_interpolate(c00, c10, c01, c11, plan.fraction, plan.mode);
    if plan.yuv {
        result = vec4<f32>(tile_interpolate(c00, c10, c01, c11, plan.fraction_rg, plan.mode).rg, result.ba);
    }
    return result;
}

fn sample_physical(texture: texture_2d<f32>, index: u32, uv: SampleCoordinates, cycle: u32) -> vec4<f32> {
    let plan = tile_plan(index, uv, cycle, textureDimensions(texture));
    return tile_resolve(tile_load(texture, plan), plan);
}

// Levels 0..7 select tex0 and tex_lod1..7 (sampling slots 0 and 3..9); 8 selects tex_detail.
fn sample_tile(idx: f32, uv: SampleCoordinates, detail_active: bool, cycle: u32) -> vec4<f32> {
    let detail = detail_active && idx == 0.0;
    let level = u32(clamp(select(idx, idx - 1.0, detail_active), 0.0, 7.0));
    let texture = select(level, 8u, detail);
    let index = select(select(level + 2u, 0u, level == 0u), 2u, detail);
    var dimensions: vec2<u32>;
    switch texture {
        case 0u: { dimensions = textureDimensions(tex0); }
        case 1u: { dimensions = textureDimensions(tex_lod1); }
        case 2u: { dimensions = textureDimensions(tex_lod2); }
        case 3u: { dimensions = textureDimensions(tex_lod3); }
        case 4u: { dimensions = textureDimensions(tex_lod4); }
        case 5u: { dimensions = textureDimensions(tex_lod5); }
        case 6u: { dimensions = textureDimensions(tex_lod6); }
        case 7u: { dimensions = textureDimensions(tex_lod7); }
        default: { dimensions = textureDimensions(tex_detail); }
    }
    let plan = tile_plan(index, uv, cycle, dimensions);
    var quad: TileQuad;
    switch texture {
        case 0u: { quad = tile_load(tex0, plan); }
        case 1u: { quad = tile_load(tex_lod1, plan); }
        case 2u: { quad = tile_load(tex_lod2, plan); }
        case 3u: { quad = tile_load(tex_lod3, plan); }
        case 4u: { quad = tile_load(tex_lod4, plan); }
        case 5u: { quad = tile_load(tex_lod5, plan); }
        case 6u: { quad = tile_load(tex_lod6, plan); }
        case 7u: { quad = tile_load(tex_lod7, plan); }
        default: { quad = tile_load(tex_detail, plan); }
    }
    return tile_resolve(quad, plan);
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
    let uv_scale = combiner.inv_tex_size.xy * vec2<f32>(tile_sampling[0].image.xy);
    let target_uv = in.uv * uv_scale;
    var uv = SampleCoordinates(target_uv, target_uv);
    if combiner.raster.x > 1u && combiner.raster.z != 0u {
        let m = f32(combiner.raster.x);
        let delta = in.clip_position.xy - (floor(in.clip_position.xy / m) + 0.5) * m;
        uv.snapped -= (delta.x * ddx_uv + delta.y * ddy_uv) * uv_scale;
    }

    if combiner.primitive_depth.w != 0u {
        return CycleResult(1.0, combiner.blend_color.rgb, 1.0);
    }
    var pixel = u32(in.clip_position.y) * combiner.frame.z + u32(in.clip_position.x);
    if combiner.raster.x > 1u {
        let native = vec2<u32>(in.clip_position.xy / f32(combiner.raster.x));
        pixel = native.y * combiner.raster.y + native.x;
    }
    if combiner.fill_siz == 3u {
        let word = select(combiner.fill_word >> 16u, combiner.fill_word & 0xffffu, (pixel & 1u) != 0u);
        let rgb5 = vec3<u32>((word >> 11u) & 31u, (word >> 6u) & 31u, (word >> 1u) & 31u);
        let rgb = vec3<f32>((rgb5 << vec3<u32>(3u)) | (rgb5 >> vec3<u32>(2u))) / 255.0;
        let alpha = f32(word & 1u);
        return CycleResult(alpha, rgb, alpha);
    }
    if combiner.fill_siz == 2u {
        let intensity = f32((combiner.fill_word >> ((3u - (pixel & 3u)) * 8u)) & 255u) / 255.0;
        return CycleResult(intensity, vec3<f32>(intensity), intensity);
    }

    let l = combiner.combine_l;
    let h = combiner.combine_h;

    var texel: vec4<f32>;
    if combiner.tex_enable != 0u {
        texel = sample_physical(tex0, 0u, uv, 0u);
    } else {
        texel = vec4<f32>(1.0);
    }

    if combiner.cycle_type == 2u {
        return CycleResult(texel.a, texel.rgb, texel.a);
    }

    let use_tex1 = combiner.inv_tex1_size.z != 0.0;
    let texel1 = sample_physical(tex1, 1u, uv, select(0u, 1u, combiner.cycle_type == 1u));
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
        // switch in `sample_tile` can never select a level past `tex_lod7`.
        let n_levels = min(combiner.lod_params.y, 8.0);

        let lod = compute_lod(ddx_uv, ddy_uv, n_levels, combiner.inv_detail_size.z, combiner.lod_params.w, combiner.inv_detail_size.w);
        texel = sample_tile(lod.level0, uv, detail_active, 0u);
        t1_cyc0 = sample_tile(lod.level1, uv, detail_active, 1u);
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
        result.compare_alpha = r0.alpha;
    }

    return result;
}

// RT64 Random.hlsli initRand (16 rounds), followed by nextRandUint.
fn dither_threshold(position: vec2<u32>) -> f32 {
    var v0 = combiner.frame.x ^ combiner.frame.y;
    var v1 = position.x + combiner.frame.z * position.y;
    var sum = 0u;
    for (var n = 0u; n < 16u; n++) {
        sum += 0x9e3779b9u;
        v0 += ((v1 << 4u) + 0xa341316cu) ^ (v1 + sum) ^ ((v1 >> 5u) + 0xc8013ea4u);
        v1 += ((v0 << 4u) + 0xad90777du) ^ (v0 + sum) ^ ((v0 >> 5u) + 0x7e95761eu);
    }
    let next = 1664525u * v0 + 1013904223u;
    return (f32(next >> 24u) + 0.5) / 256.0;
}

fn alpha_discard(result: CycleResult, position: vec2<f32>) {
    let coverage_threshold = select(0.125, 0.5, combiner.cycle_type == 2u);
    if (combiner.alpha_flags & 4u) != 0u && result.alpha < coverage_threshold {
        discard;
    }
    let compare = combiner.alpha_flags & 3u;
    var threshold = combiner.alpha_threshold;
    if compare == 3u {
        threshold = dither_threshold(vec2<u32>(position));
    }
    if compare != 0u && result.compare_alpha < threshold {
        discard;
    }
}

fn blend_color(sel: u32, rgb: vec3<f32>) -> vec3<f32> {
    if sel == 2u { return combiner.blend_color.rgb; }
    if sel == 3u { return combiner.fog_color.rgb; }
    return rgb;
}

fn blend_a(sel: u32, comb_a: f32, shade_a: f32) -> f32 {
    if sel == 0u { return comb_a; }
    if sel == 1u { return combiner.fog_color.a; }
    if sel == 2u { return shade_a; }
    return 0.0;
}

fn framebuffer_mux() -> u32 {
    return select(combiner.blender_mux, combiner.blender_mux << 2u, combiner.cycle_type == 1u);
}

fn memory_in_p(mux: u32) -> bool {
    return combiner.cycle_type <= 1u && combiner.force_blend != 0u && ((mux >> 14u) & 3u) == 1u &&
        ((mux >> 6u) & 3u) != 1u && ((mux >> 2u) & 3u) == 0u;
}

fn blend_local(rgb: vec3<f32>, combiner_alpha: f32, shade_alpha: f32, mux: u32, blend_enabled: bool) -> vec3<f32> {
    let p = (mux >> 14u) & 3u;
    let a = (mux >> 10u) & 3u;
    let m = (mux >> 6u) & 3u;
    let b = (mux >> 2u) & 3u;
    let fog_lerp = (p == 0u && m == 3u) || (p == 3u && m == 0u);
    let local = fog_lerp || (combiner.cycle_type <= 1u && p != 1u && m != 1u && (p >= 2u || m >= 2u));
    if (!local || b == 1u) {
        if (p == 3u) {
            return mix(rgb, combiner.fog_color.rgb, shade_alpha);
        }
        return rgb;
    }
    let p_color = blend_color(p, rgb);
    let m_color = blend_color(m, rgb);
    // Without FORCE_BL a one-cycle draw blends only coverage edges, which fast3d does not model.
    if (!blend_enabled) {
        return p_color;
    }
    // Shade-alpha captures depend on the continuous float lerp.
    if (a == 2u && b == 0u && fog_lerp) {
        return mix(m_color, p_color, shade_alpha);
    }
    let alpha = blend_a(a, combiner_alpha, shade_alpha);
    let alpha_byte = u32(round(clamp(alpha, 0.0, 1.0) * 255.0));
    let weight_a = alpha_byte >> 3u;
    var weight_b = 0u;
    if (b == 0u) { weight_b = (255u - alpha_byte) >> 3u; }
    if (b == 2u) { weight_b = 31u; }
    let p_byte = vec3<u32>(round(clamp(p_color, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
    let m_byte = vec3<u32>(round(clamp(m_color, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
    let numerator = p_byte * weight_a + m_byte * (weight_b + 1u);
    // Two-cycle cycle 0 shifts even without FORCE_BL (blender_equation_cycle0_2).
    return vec3<f32>((numerator >> vec3<u32>(5u)) & vec3<u32>(255u)) / 255.0;
}

fn blend_fog(rgb: vec3<f32>, combiner_alpha: f32, shade_alpha: f32) -> vec3<f32> {
    let two_cycle = combiner.cycle_type == 1u;
    let first = blend_local(rgb, combiner_alpha, shade_alpha, combiner.blender_mux,
        two_cycle || (combiner.cycle_type == 0u && combiner.force_blend != 0u));
    let mux = framebuffer_mux();
    if two_cycle && ((mux >> 14u) & 3u) != 1u && ((mux >> 6u) & 3u) != 1u && ((mux >> 2u) & 3u) != 1u {
        return blend_local(first, combiner_alpha, shade_alpha, mux, combiner.force_blend != 0u);
    }
    return first;
}

struct FramebufferBlend {
    rgb: vec3<f32>,
    weights: vec2<f32>,
};

fn blend_framebuffer(rgb: vec3<f32>, combiner_alpha: f32, shade_alpha: f32) -> FramebufferBlend {
    let mux = framebuffer_mux();
    let p = (mux >> 14u) & 3u;
    let a = blend_a((mux >> 10u) & 3u, combiner_alpha, shade_alpha);
    let b_sel = (mux >> 2u) & 3u;
    var b = 0.0;
    if b_sel == 0u { b = 1.0 - a; }
    if b_sel == 1u || b_sel == 2u { b = 1.0; }
    if memory_in_p(mux) {
        return FramebufferBlend(blend_color((mux >> 6u) & 3u, rgb), vec2<f32>(b, a));
    }
    let denom = max(a + b, 1.0 / 255.0);
    return FramebufferBlend(blend_color(p, rgb), vec2<f32>(a, b) / denom);
}
