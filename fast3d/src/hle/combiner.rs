//! Combiner word decoder: SIX selector tables (one per N64 RDP combiner slot).
//!
//! Convention (pinned): combine_l = w0, combine_h = w1; combine64 = (w1 << 32) | w0.
//!
//! decode_combine exists ONLY for the unwired-selector diagnostic; the shader receives
//! raw combine_l/combine_h words (one source of truth).

/// Extract `n` bits from `v` starting at bit `pos`.
#[inline]
fn bits(v: u32, pos: u32, n: u32) -> u32 {
    (v >> pos) & ((1 << n) - 1)
}

/// Color combiner input selector for slots A, B, C, D.
/// Wired = renderable this milestone; unwired = diagnostic + refuse-to-draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorIn {
    // Wired inputs
    Combined,
    Texel0,
    Primitive,
    Shade,
    Environment,
    Zero,
    One,
    // TEXEL1 is wired: a 2-cycle two-texture combiner is renderable. A 1-cycle DL that references
    // TEXEL1 is refused in `build_material` (the 1-cycle-TEXEL1 hardware bug is not modelled).
    Texel1,
    KeyCenter,
    K4,
    KeyScale,
    CombinedAlpha,
    Texel0Alpha,
    Texel1Alpha,
    PrimitiveAlpha,
    ShadeAlpha,
    EnvAlpha,
    LodFraction,
    PrimLodFrac,
    K5,
    Noise,
}

impl ColorIn {
    /// Returns true if this selector is rendered this milestone.
    pub fn wired(self) -> bool {
        matches!(
            self,
            ColorIn::Combined
                | ColorIn::Texel0
                | ColorIn::Shade
                | ColorIn::Primitive
                | ColorIn::Environment
                | ColorIn::Zero
                | ColorIn::One
                // TEXEL0_ALPHA as a color input: the shader's color_c slot implements it
                // (`color_c_rgb` index 8 → vec3(texel_a)). On hardware this selector is only
                // valid in the C (multiply) slot, which is where it appears.
                | ColorIn::Texel0Alpha
                // TEXEL1 / TEXEL1_ALPHA wired for 2-cycle two-texture; the 1-cycle gate lives in
                // `build_material` (this table is cycle-agnostic).
                | ColorIn::Texel1
                | ColorIn::Texel1Alpha
                // LOD_FRACTION / PRIM_LOD_FRAC wired: the WGSL color_c slot returns lod_fraction
                // (the non-LOD default 1.0) / lod_params.z (prim_lod_frac). No golden references
                // these (byte-identity guarded by the scene mux-decode regression test).
                | ColorIn::LodFraction
                | ColorIn::PrimLodFrac
        )
    }
}

/// Alpha combiner input selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaIn {
    // Wired inputs
    Combined,
    Texel0,
    Primitive,
    Shade,
    Environment,
    Zero,
    One,
    // TEXEL1 is wired for 2-cycle two-texture; 1-cycle TEXEL1 is refused in `build_material`.
    Texel1,
    LodFraction,
    PrimLodFrac,
}

impl AlphaIn {
    /// Returns true if this selector is rendered this milestone.
    pub fn wired(self) -> bool {
        matches!(
            self,
            AlphaIn::Combined
                | AlphaIn::Texel0
                | AlphaIn::Shade
                | AlphaIn::Primitive
                | AlphaIn::Environment
                | AlphaIn::Zero
                | AlphaIn::One
                // TEXEL1 wired for 2-cycle two-texture; 1-cycle gate is in build_material.
                | AlphaIn::Texel1
                // LOD_FRACTION / PRIM_LOD_FRAC wired: the WGSL alpha_c slot returns lod_fraction
                // (the non-LOD default 1.0) / lod_params.z (prim_lod_frac). No golden references
                // these (byte-identity guarded by the scene mux-decode regression test).
                | AlphaIn::LodFraction
                | AlphaIn::PrimLodFrac
        )
    }
}

// --- SIX distinct decode functions (one per N64 RDP combiner slot) ---

/// color_a slot: 4-bit index. 0-5 common; 6=ONE, 7=NOISE, else ZERO.
pub fn color_a(idx: u32) -> ColorIn {
    match idx & 0xF {
        0 => ColorIn::Combined,
        1 => ColorIn::Texel0,
        2 => ColorIn::Texel1,
        3 => ColorIn::Primitive,
        4 => ColorIn::Shade,
        5 => ColorIn::Environment,
        6 => ColorIn::One,
        7 => ColorIn::Noise,
        _ => ColorIn::Zero,
    }
}

/// color_b slot: 4-bit index. 0-5 common; 6=KEY_CENTER, 7=K4, else ZERO.
pub fn color_b(idx: u32) -> ColorIn {
    match idx & 0xF {
        0 => ColorIn::Combined,
        1 => ColorIn::Texel0,
        2 => ColorIn::Texel1,
        3 => ColorIn::Primitive,
        4 => ColorIn::Shade,
        5 => ColorIn::Environment,
        6 => ColorIn::KeyCenter,
        7 => ColorIn::K4,
        _ => ColorIn::Zero,
    }
}

/// True iff `color_a(a_idx)` and `color_b(b_idx)` are PROVABLY the same value for every possible
/// runtime state, i.e. the `(a - b)` term these two color-combiner slots feed is guaranteed to
/// cancel to zero. Index equality ALONE is not sufficient: the two mux tables are asymmetric —
/// `color_a` idx 6 = ONE (constant 1.0) but `color_b` idx 6 = KEY_CENTER (unwired, resolves to
/// 0.0), so `a_idx == b_idx == 6` must NOT be treated as annulled. Provably equal iff
/// `a_idx == b_idx` and both are in `0..=5` (the identical runtime source feeds both sides), or
/// `a_idx` is in `7..=15` and `b_idx` is in `6..=15` (both sides are the constant ZERO). Used by
/// the LOD byte-identity regression guard (`tests/goldens.rs`) to decide whether a color-C LOD
/// selector (idx 13/14) can affect output through a given A/B pair. Test-only.
#[cfg(test)]
pub(crate) fn color_ab_provably_equal(a_idx: u32, b_idx: u32) -> bool {
    (a_idx == b_idx && a_idx <= 5) || (a_idx >= 7 && b_idx >= 6)
}

/// color_c slot: 5-bit index. 16 entries: 0-5 common; 6=KEY_SCALE, 7=COMBINED_ALPHA,
/// 8=TEXEL0_ALPHA, 9=TEXEL1_ALPHA, 10=PRIMITIVE_ALPHA, 11=SHADE_ALPHA, 12=ENV_ALPHA,
/// 13=LOD_FRACTION, 14=PRIM_LOD_FRAC, 15=K5, else ZERO.
pub fn color_c(idx: u32) -> ColorIn {
    match idx & 0x1F {
        0 => ColorIn::Combined,
        1 => ColorIn::Texel0,
        2 => ColorIn::Texel1,
        3 => ColorIn::Primitive,
        4 => ColorIn::Shade,
        5 => ColorIn::Environment,
        6 => ColorIn::KeyScale,
        7 => ColorIn::CombinedAlpha,
        8 => ColorIn::Texel0Alpha,
        9 => ColorIn::Texel1Alpha,
        10 => ColorIn::PrimitiveAlpha,
        11 => ColorIn::ShadeAlpha,
        12 => ColorIn::EnvAlpha,
        13 => ColorIn::LodFraction,
        14 => ColorIn::PrimLodFrac,
        15 => ColorIn::K5,
        _ => ColorIn::Zero,
    }
}

/// color_d slot: 3-bit index. 0-5 common; 6=ONE, else ZERO.
pub fn color_d(idx: u32) -> ColorIn {
    match idx & 0x7 {
        0 => ColorIn::Combined,
        1 => ColorIn::Texel0,
        2 => ColorIn::Texel1,
        3 => ColorIn::Primitive,
        4 => ColorIn::Shade,
        5 => ColorIn::Environment,
        6 => ColorIn::One,
        _ => ColorIn::Zero,
    }
}

/// alpha_abd slot: 3-bit index. 0=COMBINED,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,
/// 5=ENVIRONMENT,6=ONE, else ZERO.
pub fn alpha_abd(idx: u32) -> AlphaIn {
    match idx & 0x7 {
        0 => AlphaIn::Combined,
        1 => AlphaIn::Texel0,
        2 => AlphaIn::Texel1,
        3 => AlphaIn::Primitive,
        4 => AlphaIn::Shade,
        5 => AlphaIn::Environment,
        6 => AlphaIn::One,
        _ => AlphaIn::Zero,
    }
}

/// alpha_c slot: 3-bit index. 0=LOD_FRACTION,1=TEXEL0,2=TEXEL1,3=PRIMITIVE,4=SHADE,
/// 5=ENVIRONMENT,6=PRIM_LOD_FRAC, else ZERO.
pub fn alpha_c(idx: u32) -> AlphaIn {
    match idx & 0x7 {
        0 => AlphaIn::LodFraction,
        1 => AlphaIn::Texel0,
        2 => AlphaIn::Texel1,
        3 => AlphaIn::Primitive,
        4 => AlphaIn::Shade,
        5 => AlphaIn::Environment,
        6 => AlphaIn::PrimLodFrac,
        _ => AlphaIn::Zero,
    }
}

/// One cycle's decoded selectors.
#[derive(Clone, Debug, PartialEq)]
pub struct CycleSel {
    pub ca: ColorIn,
    pub cb: ColorIn,
    pub cc: ColorIn,
    pub cd: ColorIn,
    pub aa: AlphaIn,
    pub ab: AlphaIn,
    pub ac: AlphaIn,
    pub ad: AlphaIn,
}

impl CycleSel {
    /// Returns the slot names of unwired selectors in this cycle.
    /// Returns `["CA", "CB", ...]` for any selector where `.wired()` is false.
    pub fn unwired(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.ca.wired() {
            out.push("CA");
        }
        if !self.cb.wired() {
            out.push("CB");
        }
        if !self.cc.wired() {
            out.push("CC");
        }
        if !self.cd.wired() {
            out.push("CD");
        }
        if !self.aa.wired() {
            out.push("AA");
        }
        if !self.ab.wired() {
            out.push("AB");
        }
        if !self.ac.wired() {
            out.push("AC");
        }
        if !self.ad.wired() {
            out.push("AD");
        }
        out
    }

    /// Bitmask of unwired slots (bit 0=CA … bit 7=AD; same order as `unwired`). `Copy`, feeds
    /// `DiagKind::UnwiredSelector`.
    pub fn unwired_mask(&self) -> u16 {
        let mut m = 0u16;
        if !self.ca.wired() {
            m |= 1 << 0;
        }
        if !self.cb.wired() {
            m |= 1 << 1;
        }
        if !self.cc.wired() {
            m |= 1 << 2;
        }
        if !self.cd.wired() {
            m |= 1 << 3;
        }
        if !self.aa.wired() {
            m |= 1 << 4;
        }
        if !self.ab.wired() {
            m |= 1 << 5;
        }
        if !self.ac.wired() {
            m |= 1 << 6;
        }
        if !self.ad.wired() {
            m |= 1 << 7;
        }
        m
    }

    /// Bitmask of the slots that select TEXEL1 / TEXEL1_ALPHA (same bit order as `unwired_mask`:
    /// bit 0 = CA … bit 7 = AD). Used to diagnose a 1-cycle TEXEL1 reference as `UnwiredSelector`
    /// — the exact slot set that was flagged before TEXEL1 was wired into `wired()`.
    pub fn texel1_mask(&self) -> u16 {
        let mut m = 0u16;
        if self.ca == ColorIn::Texel1 {
            m |= 1 << 0;
        }
        if self.cb == ColorIn::Texel1 {
            m |= 1 << 1;
        }
        if self.cc == ColorIn::Texel1 || self.cc == ColorIn::Texel1Alpha {
            m |= 1 << 2;
        }
        if self.cd == ColorIn::Texel1 {
            m |= 1 << 3;
        }
        if self.aa == AlphaIn::Texel1 {
            m |= 1 << 4;
        }
        if self.ab == AlphaIn::Texel1 {
            m |= 1 << 5;
        }
        if self.ac == AlphaIn::Texel1 {
            m |= 1 << 6;
        }
        if self.ad == AlphaIn::Texel1 {
            m |= 1 << 7;
        }
        m
    }
}

/// Both cycles' decoded selectors, plus the raw words for the shader.
#[derive(Clone, Debug, PartialEq)]
pub struct CombinerSelectors {
    /// Raw w0 — passed directly to the shader (one source of truth).
    pub raw_l: u32,
    /// Raw w1 — passed directly to the shader (one source of truth).
    pub raw_h: u32,
    pub cyc0: CycleSel,
    pub cyc1: CycleSel,
}

/// The second texture (TEXEL1) for a 2-cycle two-texture combiner. Built by `build_material` only
/// when `tile_count == 2`; mirrors the tex0 fields carried directly on `Material` (decoded RGBA8
/// bytes + dims + wrap/format).
#[derive(Clone, Debug, PartialEq)]
pub struct Tex1 {
    /// Decoded RGBA8 texture (length = tex_w * tex_h * 4).
    pub texture: Vec<u8>,
    pub tex_w: u32,
    pub tex_h: u32,
    /// Wrap mode from the TEXEL1 tile (cms/cmt): 0=WRAP 1=MIRROR 2=CLAMP.
    pub wrap_s: u8,
    pub wrap_t: u8,
    /// Tile format/size of the TEXEL1 tile (diagnostic; decode is CPU-side).
    pub fmt: u8,
    pub siz: u8,
}

/// N64 hardware maximum LOD level count (the G_TEXTURE `level` field is 3 bits → 0..7, so
/// `num_levels = level + 1` is at most 8). The decode caps `num_levels` here to match the renderer's
/// fixed per-level bindings. Kept in sync with `render::MAX_LOD`.
pub const MAX_LOD_LEVELS: u32 = 8;

/// One decoded LOD level. `texture` is the decoded RGBA8 buffer (length = `w * h * 4`) for a single
/// independent LOD level; `mip_levels[0]` mirrors the `Material.texture` / `tex_w` / `tex_h` level-0
/// fields. Levels are NOT required to halve — each carries its own `(w, h)`. Also used for the DETAIL
/// tile (`Material.detail_tex`).
#[derive(Clone, Debug, PartialEq)]
pub struct MipLevel {
    /// Decoded RGBA8 texture (length = w * h * 4).
    pub texture: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// The complete material produced by the HLE from the display list.
/// Carries decoded texture (RGBA8), combiner selectors (for diagnostic), raw combine
/// words (for the shader), cycle type, prim/env, and whether texture is enabled.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Decoded RGBA8 texture (length = tex_w * tex_h * 4).
    pub texture: Vec<u8>,
    pub tex_w: u32,
    pub tex_h: u32,
    /// Decoded selectors — used ONLY for the unwired diagnostic; shader gets raw words.
    pub selectors: CombinerSelectors,
    /// Cycle type from othermode.H bits [21:20]. 0 = 1-cycle, 1 = 2-cycle.
    pub cycle_type: u32,
    pub prim: [u8; 4],
    pub env: [u8; 4],
    /// True if SPTexture is on and the combiner uses TEXEL0.
    pub tex_enable: bool,
    /// Wrap mode from the render tile (cms/cmt): 0=WRAP 1=MIRROR 2=CLAMP. Consumed by the renderer sampler.
    pub wrap_s: u8,
    pub wrap_t: u8,
    /// Tile format/size (diagnostic; decode is CPU-side so the GPU never sees these).
    pub fmt: u8,
    pub siz: u8,
    /// Blend color RGBA (sourced from gsDPSetBlendColor; Phase D dependency).
    /// Defaulted to [0, 0, 0, 255] until B1+ plumbing wires the real RDP register.
    pub blend_color: [u8; 4],
    /// Tile-count decision: 2 = 2-cycle two-texture (TEXEL1 used), 1 = single texture
    /// (TEXEL0 used), 0 = textureless. TEXEL0 <- tiles[base]; when 2, TEXEL1 <- tiles[(base+1)&7]
    /// (base = the G_TEXTURE render-tile index).
    pub tile_count: u8,
    /// The second texture (TEXEL1), `Some` iff `tile_count == 2`.
    pub tex1: Option<Tex1>,
    /// Primitive LOD fraction (lodFrac/256) captured from G_SETPRIMCOLOR. Feeds
    /// the combiner PRIM_LOD_FRAC selector. Default 0.0 for non-LOD materials.
    pub prim_lod_frac: f32,
    /// Primitive min LOD level (lodMin/32). Floors `maxDst` under DETAIL/SHARPEN
    /// in the shader (see `compute_lod`). Default 0.0.
    pub prim_min_level: f32,
    /// True when G_TL_LOD (othermode_h bit 16) is set and a faithful per-level texture set was decoded.
    pub lod: bool,
    /// LOD level count: 1 for non-LOD materials, else the decoded level count (`min(level + 1,
    /// MAX_LOD_LEVELS)`).
    pub num_levels: u8,
    /// G_MDSFT_TEXTDETAIL bits (sharpen=bit0, detail=bit1) from othermode. 0 for non-LOD materials.
    pub text_detail: u8,
    /// Decoded per-level textures (N64-faithful, INDEPENDENT levels — no halving constraint). Empty
    /// for non-LOD materials (the renderer then uploads a single level from `texture`). When `lod` is
    /// true this holds exactly `num_levels` entries: `mip_levels[k]` is level k with its own dims
    /// `(w, h)` decoded from tile `tiles[(base+k)&7]`; `mip_levels[0]` mirrors `texture`/`tex_w`/`tex_h`.
    /// The renderer uploads each as its own wgpu texture (level 0 → `tex0`, levels 1.. → `tex_lod*`).
    pub mip_levels: Vec<MipLevel>,
    /// The DETAIL tile (tiles index 0), decoded independently under DETAIL mode (`text_detail`
    /// bit1). `Some` only when LOD is active AND the detail tile takes the faithful decode path
    /// and is sampled by the shader under DETAIL mode. `None` otherwise.
    pub detail_tex: Option<MipLevel>,
}

/// Decode a single cycle from the combine words.
///
/// Combine-word parse positions (N64 RDP):
/// - color: cyc0 `a=L[20,4] b=H[28,4] c=L[15,5] d=H[15,3]`
///   cyc1 `a=L[5,4]  b=H[24,4] c=L[0,5]  d=H[6,3]`
/// - alpha: cyc0 `a=L[12,3] b=H[12,3] c=L[9,3]  d=H[9,3]`
///   cyc1 `a=H[21,3] b=H[3,3]  c=H[18,3] d=H[0,3]`
fn decode_cycle(l: u32, h: u32, second: bool) -> CycleSel {
    if !second {
        CycleSel {
            ca: color_a(bits(l, 20, 4)),
            cb: color_b(bits(h, 28, 4)),
            cc: color_c(bits(l, 15, 5)),
            cd: color_d(bits(h, 15, 3)),
            aa: alpha_abd(bits(l, 12, 3)),
            ab: alpha_abd(bits(h, 12, 3)),
            ac: alpha_c(bits(l, 9, 3)),
            ad: alpha_abd(bits(h, 9, 3)),
        }
    } else {
        CycleSel {
            ca: color_a(bits(l, 5, 4)),
            cb: color_b(bits(h, 24, 4)),
            cc: color_c(bits(l, 0, 5)),
            cd: color_d(bits(h, 6, 3)),
            aa: alpha_abd(bits(h, 21, 3)),
            ab: alpha_abd(bits(h, 3, 3)),
            ac: alpha_c(bits(h, 18, 3)),
            ad: alpha_abd(bits(h, 0, 3)),
        }
    }
}

/// Decode both cycles from the raw combine words.
///
/// combine_l = w0, combine_h = w1; combine64 = (w1 << 32) | w0.
/// The result carries raw_l/raw_h for the shader and decoded selectors for
/// the unwired diagnostic ONLY.
pub fn decode_combine(w0: u32, w1: u32) -> CombinerSelectors {
    CombinerSelectors {
        raw_l: w0,
        raw_h: w1,
        cyc0: decode_cycle(w0, w1, false),
        cyc1: decode_cycle(w0, w1, true),
    }
}

/// Decode RGBA16 (big-endian, 5/5/5/1) source bytes to RGBA8. The single decoder shared
/// across the workspace: re-exported as `hle::decode_rgba16`, and `renderer` re-exports it
/// in turn so the texture-upload path and the renderer tests share one implementation.
/// Each 5-bit channel is bit-replicated: (c5 << 3) | (c5 >> 2) for full 8-bit range.
pub fn decode_rgba16(src: &[u8]) -> Vec<u8> {
    let n = src.len() / 2;
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        let v = u16::from_be_bytes([src[i * 2], src[i * 2 + 1]]);
        let r5 = ((v >> 11) & 0x1F) as u8;
        let g5 = ((v >> 6) & 0x1F) as u8;
        let b5 = ((v >> 1) & 0x1F) as u8;
        let a1 = (v & 0x1) as u8;
        out[i * 4] = (r5 << 3) | (r5 >> 2);
        out[i * 4 + 1] = (g5 << 3) | (g5 >> 2);
        out[i * 4 + 2] = (b5 << 3) | (b5 >> 2);
        out[i * 4 + 3] = if a1 != 0 { 255 } else { 0 };
    }
    out
}

/// Returns true if a single combiner CYCLE references TEXEL0 in any color/alpha slot.
fn cycle_uses_texel0(c: &CycleSel) -> bool {
    c.ca == ColorIn::Texel0
        || c.cb == ColorIn::Texel0
        || c.cc == ColorIn::Texel0
        || c.cc == ColorIn::Texel0Alpha // TEXEL0_ALPHA samples the texture
        || c.cd == ColorIn::Texel0
        || c.aa == AlphaIn::Texel0
        || c.ab == AlphaIn::Texel0
        || c.ac == AlphaIn::Texel0
        || c.ad == AlphaIn::Texel0
}

/// Returns true if the combiner words reference TEXEL0 in cycle 1 (the 1-cycle canonical slot).
fn combiner_uses_texel0(combine_l: u32, combine_h: u32) -> bool {
    cycle_uses_texel0(&decode_combine(combine_l, combine_h).cyc1)
}

/// Returns true if a single combiner CYCLE references TEXEL1 in any color/alpha slot
/// (TEXEL1 or TEXEL1_ALPHA). Cycle-agnostic; the 1-cycle gate lives in `cycle_uses_texel1`.
fn cyc_sel_uses_texel1(c: &CycleSel) -> bool {
    c.texel1_mask() != 0
}

/// True iff the SECOND texture (tile base+1) is sampled. Only
/// meaningful in 2-cycle mode, so 1-cycle / COPY / FILL always return false.
///
/// In 2-cycle mode this accounts for the N64 pipelined TEXEL0<->TEXEL1 role swap (
/// in the second cycle a TEXEL0 selector reads `texVal1`, the base+1 tile). So the second texture is
/// used iff `cyc0` references TEXEL1/_ALPHA (reads `texVal1` directly) OR `cyc1` references
/// TEXEL0/_ALPHA (reads `texVal1` via the swap) — a raw TEXEL1 token scan of both cycles would be
/// wrong, since a `cyc1` TEXEL1 reference reads `texVal0` under the swap.
fn cycle_uses_texel1(sel: &CombinerSelectors, cycle_type: u32) -> bool {
    if cycle_type != 1 {
        return false;
    }
    cyc_sel_uses_texel1(&sel.cyc0) || cycle_uses_texel0(&sel.cyc1)
}

/// Whether `tile` can be decoded through the N64-faithful byte-addressable path
/// (`Tmem::sample_tile`) rather than the legacy linear `FormatInfo::decode` fallback. The nine
/// supported formats (six non-paletted, CI4/CI8, RGBA32) qualify; faithful-vs-legacy then depends
/// on which load path populated the bank:
///   * LoadBlock packs rows contiguously, so the write and read swaps only cancel when each texel
///     row is a whole number of 64-bit words — hence the `line_bytes % 8 == 0` gate.
///   * LoadTile (`rdp.load_via_tile`) writes a genuine per-row `line<<3` stride, so `sample_tile`
///     reads it via the render tile's `line` for ANY width, regardless of alignment.
///
/// CRITICAL for the second texture (TEXEL1): the legacy fallback reads `rdp.tmem[..needed]` and
/// IGNORES `tile.tmem_addr`, so a tex1 that falls to it would silently read tex0's TMEM. Callers
/// building a second texture gate on this predicate and refuse-to-draw when false (see
/// `build_material`).
fn tile_takes_faithful_path(
    rdp: &crate::hle::rdp::Rdp,
    tile: &crate::hle::rdp::TileDescriptor,
    tex_w: u32,
) -> bool {
    let line_bytes = ((tex_w as usize) << tile.siz) >> 1;
    let format_ok = matches!(
        (tile.fmt, tile.siz),
        (0, 2) | (0, 3) | (4, 1) | (4, 0) | (3, 2) | (3, 1) | (3, 0) | (2, 0) | (2, 1)
    );
    format_ok && (line_bytes.is_multiple_of(8) || rdp.load_via_tile)
}

/// Decode `tile` to a `tex_w * tex_h * 4` RGBA8 buffer for a `Material`, via the N64-faithful path
/// when [`tile_takes_faithful_path`] allows (byte-identical to the linear decode where the
/// LoadBlock swaps cancel), else the historical linear `FormatInfo::decode`. The returned buffer is
/// always `tex_w * tex_h * 4` bytes, satisfying the renderer's `write_texture` contract.
fn decode_tile_texture(
    rdp: &crate::hle::rdp::Rdp,
    tile: &crate::hle::rdp::TileDescriptor,
    tex_w: u32,
    tex_h: u32,
    tlut_fmt: u8,
) -> Vec<u8> {
    if tile_takes_faithful_path(rdp, tile, tex_w) {
        return rdp.tmem_bank.sample_tile(tile, tlut_fmt);
    }

    // Legacy linear fallback (sub-word rows, or formats sample_tile does not handle). The palette
    // is single-sourced from the faithful bank: `tmem_bank.palette()` is the upper 2 KiB, so entry
    // i of palette 0 sits at byte i*8 — exactly what decode_ci4/ci8 expect from `tlut`.
    let tlut = rdp.tmem_bank.palette();
    let fi = crate::hle::texdec::FormatInfo {
        fmt: tile.fmt,
        siz: tile.siz,
    };
    let needed = fi.tmem_bytes(tex_w, tex_h);
    if rdp.tmem.len() >= needed {
        fi.decode(
            &rdp.tmem[..needed],
            tex_w,
            tex_h,
            tlut,
            tile.palette,
            tlut_fmt,
        )
    } else {
        // tmem is shorter than the tile dimensions imply — zero-pad so the decoded buffer always
        // satisfies texture.len() == tex_w*tex_h*4 (the renderer's write_texture contract).
        let mut padded = rdp.tmem.to_vec();
        padded.resize(needed, 0);
        fi.decode(&padded, tex_w, tex_h, tlut, tile.palette, tlut_fmt)
    }
}

/// Build a `Material` from the final RDP/RSP state after the dispatch loop.
///
/// Called AFTER the dispatch loop in `interpret()` (covers ENDDL and run-off-end).
/// Returns `None` if any combiner selector is unwired (and pushes a diagnostic).
pub fn build_material(
    rdp: &crate::hle::rdp::Rdp,
    rsp: &crate::hle::rsp::Rsp,
    diags: &mut Vec<crate::diag::Diagnostic>,
    pc: u64,
) -> Option<Material> {
    let selectors = decode_combine(rdp.combine_l, rdp.combine_h);
    // cycle_type from bits [21:20] of other_mode_h.
    let cycle_type = (rdp.other_mode_h >> 20) & 3;

    // usesTexel1: true only in 2-cycle mode with a TEXEL1 selector (false in 1-cycle).
    let uses_texel1 = cycle_uses_texel1(&selectors, cycle_type);

    // 1-cycle TEXEL1 gate: TEXEL1 is wired, so `cyc1.unwired()` no longer flags a stray TEXEL1 —
    // refuse-to-draw + diagnose a TEXEL1 reference outside 2-cycle mode explicitly. Gating on
    // `cycle_type != 1` (not `!uses_texel1`) keeps a legitimate 2-cycle `cyc1` TEXEL1 reference
    // drawable: under the role swap it reads the FIRST texture, so `usesTexture(1)` is false and a
    // `!uses_texel1` guard would wrongly refuse it. Runs before the empty-tmem split.
    if cycle_type != 1 && cyc_sel_uses_texel1(&selectors.cyc1) {
        diags.push(crate::diag::Diagnostic {
            at: pc,
            kind: crate::diag::DiagKind::UnwiredSelector {
                slots: selectors.cyc1.texel1_mask(),
            },
        });
        return None;
    }

    // TMEM gate: if no LoadBlock was executed, only diagnose+refuse when the combiner
    // actually references TEXEL0 (a texture-sampling combiner with no texture loaded).
    // SHADE-only / PRIMITIVE / ENVIRONMENT combiners are textureless by design; they
    // proceed with an empty texture so the material is still produced and the scene renders.
    if rdp.tmem.is_empty() {
        if rdp.combine_l == 0 && rdp.combine_h == 0 {
            // Combiner never configured — DL did not reach a SetCombine; skip silently.
            return None;
        }
        if combiner_uses_texel0(rdp.combine_l, rdp.combine_h) {
            diags.push(crate::diag::Diagnostic {
                at: pc,
                kind: crate::diag::DiagKind::NoTextureLoaded,
            });
            return None;
        }
        // Textureless combiner (SHADE / PRIMITIVE / etc.) — build a 1×1 dummy texture so
        // the rest of build_material can proceed normally; tex_enable will be false.
        let unwired = selectors.cyc1.unwired();
        if !unwired.is_empty() {
            diags.push(crate::diag::Diagnostic {
                at: pc,
                kind: crate::diag::DiagKind::UnwiredSelector {
                    slots: selectors.cyc1.unwired_mask(),
                },
            });
            return None;
        }
        return Some(Material {
            texture: vec![0u8; 4], // 1×1 dummy — tex_enable will be false
            tex_w: 1,
            tex_h: 1,
            selectors,
            cycle_type,
            prim: rdp.prim,
            env: rdp.env,
            tex_enable: false,
            wrap_s: 2,
            wrap_t: 2,
            fmt: 0,
            siz: 0,
            blend_color: rdp.blend_color,
            tile_count: 0, // textureless
            tex1: None,
            prim_lod_frac: rdp.prim_lod_frac,
            prim_min_level: rdp.prim_min_level,
            lod: false,
            num_levels: 1,
            text_detail: 0,
            mip_levels: Vec::new(),
            detail_tex: None,
        });
    }

    // 1-cycle mode uses cycle-1 (index-1) slots (F3DEX2 convention).
    let unwired = selectors.cyc1.unwired();
    if !unwired.is_empty() {
        diags.push(crate::diag::Diagnostic {
            at: pc,
            kind: crate::diag::DiagKind::UnwiredSelector {
                slots: selectors.cyc1.unwired_mask(),
            },
        });
        return None;
    }

    // Base render tile = the G_TEXTURE tile field (tracked on `rsp.texture_state.tile`).
    // TEXEL0 <- tiles[base]; TEXEL1 <- tiles[(base+1) & 7]. All existing goldens use
    // G_TX_RENDERTILE (0), so base == 0 for them and tex0 is byte-identical to the old `tiles[0]`.
    let base = (rsp.texture_state.tile & 7) as usize;
    let tile = &rdp.tiles[base];
    let tex_w = tile.width.max(1) as u32;
    let tex_h = tile.height.max(1) as u32;

    let tlut_fmt = ((rdp.other_mode_h >> 14) & 0x3) as u8; // G_MDSFT_TEXTLUT
    let texture = decode_tile_texture(rdp, tile, tex_w, tex_h, tlut_fmt);

    // tex_enable: SPTexture on AND the combiner samples the FIRST texture. This is the mirror of
    // `cycle_uses_texel1`: the WGSL TEXEL0<->TEXEL1 role swap makes a cyc1
    // TEXEL1 selector read the first texture. That swap is active only when `uses_texel1`
    // (tex_enable1); for single-texture draws it collapses to no-swap, so the swap-aware form is
    // gated on `uses_texel1` to keep every 1-cycle and single-texture 2-cycle golden byte-identical.
    let uses_texel0 = if cycle_type == 1 && uses_texel1 {
        // 2-cycle two-texture (swap active): usesTexture(0) = cyc0-TEXEL0 OR cyc1-TEXEL1.
        cycle_uses_texel0(&selectors.cyc0) || cyc_sel_uses_texel1(&selectors.cyc1)
    } else {
        // 1-cycle, or 2-cycle single-texture (no swap). 2-cycle checks BOTH cycles (sm64 fog terrain
        // puts TEXEL0*SHADE in cycle 0, fog/pass in cyc1).
        cycle_uses_texel0(&selectors.cyc1)
            || (cycle_type == 1 && cycle_uses_texel0(&selectors.cyc0))
    };
    let tex_enable = rsp.texture_state.on && uses_texel0;

    // tileCount decision: usesTexel1 (2-cycle only) -> 2; else usesTexel0 -> 1; else 0.
    let tile_count: u8 = if uses_texel1 {
        2
    } else if uses_texel0 {
        1
    } else {
        0
    };

    // When tileCount == 2, decode the SECOND texture from tiles[(base+1)&7]. The legacy fallback
    // ignores `tmem_addr`, so a tex1 that cannot take the faithful `sample_tile` path would silently
    // read tex0's TMEM — refuse-to-draw + diagnose rather than emit a mis-decoded second texture.
    let tex1 = if tile_count == 2 {
        let t1 = &rdp.tiles[(base + 1) & 7];
        let t1_w = t1.width.max(1) as u32;
        let t1_h = t1.height.max(1) as u32;
        if !tile_takes_faithful_path(rdp, t1, t1_w) {
            diags.push(crate::diag::Diagnostic {
                at: pc,
                kind: crate::diag::DiagKind::SecondTextureUndecodable,
            });
            return None;
        }
        Some(Tex1 {
            texture: decode_tile_texture(rdp, t1, t1_w, t1_h, tlut_fmt),
            tex_w: t1_w,
            tex_h: t1_h,
            wrap_s: t1.cms,
            wrap_t: t1.cmt,
            fmt: t1.fmt,
            siz: t1.siz,
        })
    } else {
        None
    };

    // N64-faithful LOD decode. LOD is active when G_TL_LOD (othermode_h bit 16) is set AND the
    // G_TEXTURE `level` field is > 0 (num_levels = level + 1).
    // Each level k comes from the CONSECUTIVE render tile `tiles[(base+k)&7]` and is decoded as an
    // INDEPENDENT per-level texture (`MipLevel { texture, w, h }`) — no halving
    // constraint. The renderer uploads each level as its OWN wgpu texture and the shader switch-
    // selects between them, so NON-HALVING level sets (e.g. sm64 Castle Inside's two 32×32 TRILERP
    // levels) engage LOD instead of falling back. The only remaining requirement is that every level
    // takes the faithful `sample_tile` decode path; a level that would need the legacy linear
    // fallback (which ignores `tmem_addr` and could read the wrong bank) forces `lod = false`,
    // byte-identical to the non-LOD path. The count is capped at `MAX_LOD_LEVELS`.
    let (lod, num_levels, mip_levels, text_detail, detail_tex) =
        if rdp.lod_enable() && rsp.texture_state.level > 0 {
            let n = (rsp.texture_state.level as u32 + 1).min(MAX_LOD_LEVELS);
            let mut levels: Vec<MipLevel> = Vec::with_capacity(n as usize);
            let mut faithful = true;
            for k in 0..n {
                let tk = &rdp.tiles[(base + k as usize) & 7];
                let lw = tk.width.max(1) as u32;
                let lh = tk.height.max(1) as u32;
                if !tile_takes_faithful_path(rdp, tk, lw) {
                    faithful = false;
                    break;
                }
                levels.push(MipLevel {
                    texture: decode_tile_texture(rdp, tk, lw, lh, tlut_fmt),
                    w: lw,
                    h: lh,
                });
            }
            if faithful {
                // DETAIL mode (text_detail bit1): the DETAIL tile is the finest tile (index 0),
                // decoded independently. Only carried when it also takes the faithful path.
                let td = rdp.text_detail();
                let detail = if td & 0b10 != 0 {
                    let dt = &rdp.tiles[0];
                    let dw = dt.width.max(1) as u32;
                    let dh = dt.height.max(1) as u32;
                    if tile_takes_faithful_path(rdp, dt, dw) {
                        Some(MipLevel {
                            texture: decode_tile_texture(rdp, dt, dw, dh, tlut_fmt),
                            w: dw,
                            h: dh,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };
                (true, n as u8, levels, td, detail)
            } else {
                (false, 1u8, Vec::new(), 0u8, None)
            }
        } else {
            (false, 1u8, Vec::new(), 0u8, None)
        };

    Some(Material {
        texture,
        tex_w,
        tex_h,
        selectors,
        cycle_type,
        prim: rdp.prim,
        env: rdp.env,
        tex_enable,
        wrap_s: tile.cms,
        wrap_t: tile.cmt,
        fmt: tile.fmt,
        siz: tile.siz,
        blend_color: rdp.blend_color,
        tile_count,
        tex1,
        prim_lod_frac: rdp.prim_lod_frac,
        prim_min_level: rdp.prim_min_level,
        lod,
        num_levels,
        text_detail,
        mip_levels,
        detail_tex,
    })
}

/// Build a `Material` for a 2D rectangle (TEXRECT). NON-generic, mirroring
/// `build_material(rdp, rsp, diags, pc)` — but a TEXRECT ALWAYS samples its tile, so this path
/// forces `tex_enable = true` and never returns `None` (it bypasses build_material's four
/// `return None` gates: empty-tmem, TEXEL0-without-texture, and the two unwired-selector exits).
/// `gsSPTexture(G_ON/G_OFF)` only scales triangle texcoords; it does NOT gate a texture rectangle.
/// `cycle_type` is read from othermode.H bits [21:20] for both COPY and non-COPY rects.
///
/// `diags`/`pc` are accepted for signature-parity with `build_material` (no diagnostics are
/// emitted on this path).
pub fn build_rect_material(
    rdp: &crate::hle::rdp::Rdp,
    rsp: &crate::hle::rsp::Rsp,
    diags: &mut Vec<crate::diag::Diagnostic>,
    pc: u64,
) -> Material {
    let _ = (diags, pc, rsp); // signature-parity with build_material; unused on this path.
    let selectors = decode_combine(rdp.combine_l, rdp.combine_h);
    let cycle_type = (rdp.other_mode_h >> 20) & 3;

    let tile = &rdp.tiles[0];
    let tex_w = tile.width.max(1) as u32;
    let tex_h = tile.height.max(1) as u32;

    let tlut_fmt = ((rdp.other_mode_h >> 14) & 0x3) as u8; // G_MDSFT_TEXTLUT
    let texture = decode_tile_texture(rdp, tile, tex_w, tex_h, tlut_fmt);

    Material {
        texture,
        tex_w,
        tex_h,
        selectors,
        cycle_type,
        prim: rdp.prim,
        env: rdp.env,
        tex_enable: true, // a TEXRECT always samples its tile, regardless of gsSPTexture state
        wrap_s: tile.cms,
        wrap_t: tile.cmt,
        fmt: tile.fmt,
        siz: tile.siz,
        blend_color: rdp.blend_color,
        // A TEXRECT always samples its single tile — one texture, never a second.
        tile_count: 1,
        tex1: None,
        prim_lod_frac: rdp.prim_lod_frac,
        prim_min_level: rdp.prim_min_level,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_modulate_all_16_selectors() {
        // RGB=TEXEL0*SHADE / ALPHA=SHADE passthrough: w0=0xFC127E24 w1=0xFFFFF9FC
        let cs = decode_combine(0xFC12_7E24, 0xFFFF_F9FC);
        assert_eq!(cs.raw_l, 0xFC12_7E24); // raw L=w0 to shader (one source of truth)
        assert_eq!(cs.raw_h, 0xFFFF_F9FC);
        // cycle 1 (both cycles identical): cA=TEXEL0, cB=ZERO, cC=SHADE, cD=ZERO,
        //                                  aA=ZERO, aB=ZERO, aC=ZERO, aD=SHADE
        assert_eq!(cs.cyc1.ca, ColorIn::Texel0);
        assert_eq!(cs.cyc1.cb, ColorIn::Zero);
        assert_eq!(cs.cyc1.cc, ColorIn::Shade); // C slot is SHADE, NOT TEXEL0
        assert_eq!(cs.cyc1.cd, ColorIn::Zero);
        assert_eq!(cs.cyc1.aa, AlphaIn::Zero);
        assert_eq!(cs.cyc1.ab, AlphaIn::Zero);
        assert_eq!(cs.cyc1.ac, AlphaIn::Zero);
        assert_eq!(cs.cyc1.ad, AlphaIn::Shade);
        assert!(cs.cyc1.unwired().is_empty()); // all wired
                                               // cycle 0 decodes identically (combine duplicated into both cycle slots)
        assert_eq!(cs.cyc0.ca, ColorIn::Texel0);
        assert_eq!(cs.cyc0.cc, ColorIn::Shade);
        assert_eq!(cs.cyc0.ad, AlphaIn::Shade);
    }

    #[test]
    fn alpha_c_index0_is_lod_fraction_not_combined() {
        assert_eq!(alpha_c(0), AlphaIn::LodFraction);
        // LOD_FRACTION is wired (returns the non-LOD default 1.0 in the shader).
        assert!(AlphaIn::LodFraction.wired());
    }

    #[test]
    fn lod_selectors_are_wired() {
        // Both LOD selectors in both color and alpha tables draw.
        assert!(ColorIn::LodFraction.wired());
        assert!(ColorIn::PrimLodFrac.wired());
        assert!(AlphaIn::LodFraction.wired());
        assert!(AlphaIn::PrimLodFrac.wired());
        // color_c idx 13/14 decode to the LOD selectors; alpha_c idx 0/6 likewise (N64 RDP mux slots).
        assert_eq!(color_c(13), ColorIn::LodFraction);
        assert_eq!(color_c(14), ColorIn::PrimLodFrac);
        assert_eq!(alpha_c(0), AlphaIn::LodFraction);
        assert_eq!(alpha_c(6), AlphaIn::PrimLodFrac);
    }

    #[test]
    fn color_ab_idx6_pair_is_not_provably_equal_and_guard_flags_it() {
        // Regression for the byte-identity-guard hardening fix. The two color mux tables are
        // asymmetric at idx 6: color_a(6) = ONE (constant 1.0), color_b(6) = KEY_CENTER, which the
        // shader resolves to 0.0 (unwired) — see `color_a_rgb`/`color_b_rgb` in
        // render/combiner_prelude.wgsl. A naive "a_idx == b_idx => annulled" test (the guard's
        // pre-fix logic) wrongly treated this pair as a byte-identity no-op.
        assert_eq!(color_a(6), ColorIn::One);
        assert_eq!(color_b(6), ColorIn::KeyCenter);
        assert!(
            !ColorIn::KeyCenter.wired(),
            "KEY_CENTER is unwired => the shader resolves it to the constant 0.0"
        );

        // The corrected, extracted annulment predicate must NOT treat this pair as provably zero.
        assert!(
            !color_ab_provably_equal(6, 6),
            "a_idx == b_idx == 6 must NOT be annulled: color_a(6) = ONE(1.0) != color_b(6) = \
             KEY_CENTER(0.0)"
        );

        // Positive-detection: a synthetic cyc1 combine word with A=6 (ONE), B=6 (KEY_CENTER),
        // C=13 (LOD_FRACTION) — color: cyc1 a=L[5,4] b=H[24,4] c=L[0,5] (see `decode_cycle`).
        let l = (6u32 << 5) | 13u32; // cyc1 a_idx=6, c_idx=13 (LOD_FRACTION)
        let h = 6u32 << 24; // cyc1 b_idx=6 (KEY_CENTER)
        let sel = decode_combine(l, h);
        assert_eq!(sel.cyc1.ca, ColorIn::One);
        assert_eq!(sel.cyc1.cb, ColorIn::KeyCenter);
        assert_eq!(sel.cyc1.cc, ColorIn::LodFraction);

        // Reproduce the guard's exact positive-detection formula (mirrors
        // `color_c_lod_affects_output` in tests/goldens.rs) directly against the raw bits, built
        // ONLY from the extracted, corrected predicate above (no scene/DL smuggling).
        let c_idx = bits(l, 0, 5);
        let a_idx = bits(l, 5, 4);
        let b_idx = bits(h, 24, 4);
        assert_eq!((c_idx, a_idx, b_idx), (13, 6, 6));
        let guard_flags_it = (c_idx == 13 || c_idx == 14) && !color_ab_provably_equal(a_idx, b_idx);
        assert!(
            guard_flags_it,
            "the corrected guard must flag a_idx==b_idx==6 with a color-C LOD selector as \
             output-affecting — this is exactly the case the pre-fix logic was blind to"
        );
    }

    #[test]
    fn normal_modulate_reports_no_unwired_alpha_c() {
        // alpha-C decodes to A_ZERO (index 7) in MODULATE -> NOT in the unwired list.
        let cs = decode_combine(0xFC12_7E24, 0xFFFF_F9FC);
        assert!(!cs.cyc1.unwired().contains(&"AC"));
    }

    #[test]
    fn texel1_is_now_wired_but_tracked_by_texel1_mask() {
        // color-C cycle1 = L[0,5] = 2 (TEXEL1); H[18,3]=7 so alpha-C is ZERO (not LOD_FRACTION).
        let cs = decode_combine(0xFC00_0002, 0x001C_0000); // H bit18..20 = 7
        assert_eq!(cs.cyc1.cc, ColorIn::Texel1);
        // TEXEL1 is wired, so it no longer appears in the unwired diagnostic list...
        assert!(cs.cyc1.unwired().is_empty());
        // ...but texel1_mask still flags color-C so the 1-cycle refuse path can diagnose it.
        assert_eq!(cs.cyc1.texel1_mask(), 0b0000_0100); // bit 2 = CC
        assert!(cyc_sel_uses_texel1(&cs.cyc1));
    }

    #[test]
    fn short_tmem_zero_pads_to_correct_decode_len() {
        // Regression: SP3b first-light crash. When tmem is shorter than tex_w*tex_h*2 (e.g. sm64
        // 8-bit textures loaded into a tile declared as RGBA16), the old code passed the short
        // slice directly to decode_rgba16, producing texture.len() < tex_w*tex_h*4 and triggering
        // wgpu's "Copy would overrun the bounds of the Source buffer" abort.
        //
        // The fix zero-pads tmem up to `needed` before decoding, guaranteeing:
        //   texture.len() == tex_w * tex_h * 4
        let tex_w: u32 = 8;
        let tex_h: u32 = 8;
        let needed = (tex_w * tex_h * 2) as usize; // 128 bytes for 8×8 RGBA16

        // Simulate a tmem that is shorter than needed (e.g. only half filled).
        let short_tmem: Vec<u8> = vec![0xAB; needed / 2]; // 64 bytes — shorter than 128
        assert!(short_tmem.len() < needed, "precondition: tmem IS short");

        let mut padded = short_tmem.to_vec();
        padded.resize(needed, 0);
        let texture = decode_rgba16(&padded);

        assert_eq!(
            texture.len(),
            (tex_w * tex_h * 4) as usize,
            "texture.len() must equal tex_w*tex_h*4 regardless of tmem shortfall"
        );
    }

    #[test]
    fn synthetic_loadtile_dl_decodes_faithfully_through_the_interpreter() {
        // End-to-end proof that G_LOADTILE (0xF4) is registered, reachable through the REAL ucode
        // dispatch table, and wired into the faithful sample path — for a SUB-WORD width that the
        // LoadBlock gate would send to the legacy linear decoder.
        //
        // Scene: an I8 (fmt=4, siz=1) source of actual width 16 in RDRAM; LoadTile copies a 3-row
        // region (1 word / 8 texels per row, source stride 16 bytes) into TMEM at line=1; the render
        // tile is 3×3 (line_bytes = 3 → NOT word-aligned). Because the load went through LoadTile
        // (`rdp.load_via_tile`), decode routes to sample_tile and recovers the source exactly (the
        // per-row odd-line swap on row 1 cancels between write and read).
        use crate::hle::interp::{Cmd, Ctx};
        use crate::hle::mem::RdramImage;

        // RDRAM: byte i = 0x10 + i, so I8 texel (row r, col c) = 0x10 + r*16 + c (rows 16 apart).
        let rdram: Vec<u8> = (0..48u8).map(|i| 0x10u8.wrapping_add(i)).collect();

        // (w0, w1) command pairs, dispatched through the real F3DEX2 table.
        let uls = 0u32;
        let ult = 0u32;
        let lrs = 8u32; // (lrs>>2)=2 → tile_width 2 → words_per_row 1 (I8); render width 3
        let lrt = 8u32; // (lrt>>2)=2 → row_count 3
        let cmds: [(u32, u32); 5] = [
            // SetTextureImage: fmt=4 (I), siz=1 (8b), width field = 15 (actual 16), addr = 0.
            ((0xFDu32 << 24) | (4 << 21) | (1 << 19) | 15, 0),
            // SetTile (load tile 7): fmt=4, siz=1, line=1, tmem=0.
            (
                (0xF5u32 << 24) | (4 << 21) | (1 << 19) | (1 << 9),
                7u32 << 24,
            ),
            // LoadTile (0xF4): tile 7, uls/ult=0, lrs=lrt=8.
            (
                (0xF4u32 << 24) | (uls << 12) | ult,
                (7u32 << 24) | (lrs << 12) | lrt,
            ),
            // SetTile (render tile 0): fmt=4, siz=1, line=1, tmem=0.
            ((0xF5u32 << 24) | (4 << 21) | (1 << 19) | (1 << 9), 0),
            // SetTileSize (tile 0): lrs=lrt=8 → width=height=3.
            ((0xF2u32 << 24) | (uls << 12) | ult, (8u32 << 12) | 8),
        ];

        let mut rdram_img = RdramImage::new(&rdram);
        let mut rsp = crate::hle::rsp::Rsp::default();
        let mut rdp = crate::hle::rdp::Rdp::default();
        let mut scene = crate::hle::rsp::Scene::default();
        let mut diags = Vec::new();
        let mut rec = crate::hle::rsp::PairRec::default();
        let mut dropped = 0u32;
        let mut seen = [false; 256];
        let table =
            crate::hle::gbi::Gbi::<RdramImage>::new(crate::hle::gbi::GbiUcode::F3dex2).table;

        for (w0, w1) in cmds {
            let cmd = Cmd {
                w0,
                w1,
                w1_addr: w1 as u64,
            };
            let mut cx = Ctx {
                rsp: &mut rsp,
                rdp: &mut rdp,
                mem: &mut rdram_img,
                scene: &mut scene,
                diags: &mut diags,
                pc: 0,
                gbi_consts: crate::hle::gbi::GbiUcode::F3dex2.constants(),
                rec: &mut rec,
                dropped_runs: &mut dropped,
                unknown_seen: &mut seen,
            };
            table[cmd.opcode() as usize](&cmd, &mut cx);
        }

        // 0xF4 reached a real handler, not the unknown-opcode path (which would push a diagnostic).
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert!(
            rdp.load_via_tile,
            "LoadTile must set load_via_tile so decode routes to the faithful bank"
        );
        assert_eq!((rdp.tiles[0].width, rdp.tiles[0].height), (3, 3));

        // Sub-word width: the LoadBlock gate (line_bytes % 8 == 0) would be FALSE here (3 % 8 != 0),
        // so only the LoadTile flag routes this to the faithful path.
        let line_bytes = ((rdp.tiles[0].width as usize) << rdp.tiles[0].siz) >> 1;
        assert_eq!(line_bytes, 3, "precondition: sub-word row (not 8-aligned)");

        let got = decode_tile_texture(&rdp, &rdp.tiles[0], 3, 3, 0);
        assert_eq!(got.len(), 3 * 3 * 4);

        // Hand-computed expectation: I8 texel (r, c) = 0x10 + r*16 + c → RGBA [v, v, v, v].
        for r in 0..3usize {
            for c in 0..3usize {
                let v = 0x10u8 + (r as u8) * 16 + c as u8;
                let o = (r * 3 + c) * 4;
                assert_eq!(
                    &got[o..o + 4],
                    &[v, v, v, v],
                    "I8 LoadTile texel ({c},{r}) mismatch"
                );
            }
        }
    }

    // --- multitexturing CPU half -------------------------------------------------------------

    /// Build an Rdp whose faithful TMEM bank holds a 128-byte gradient (byte i = i) as 16 RGBA16
    /// words, with `combine`/`other_mode_h` set. tiles[0] reads it at word 0, tiles[1] at word 8 —
    /// two DISTINCT word-aligned RGBA16 4×1 tiles, so their faithful decodes differ.
    fn rdp_two_distinct_rgba16_tiles(
        combine_l: u32,
        combine_h: u32,
        cycle_type: u32,
    ) -> crate::hle::rdp::Rdp {
        let mut rdp = crate::hle::rdp::Rdp {
            // Non-empty legacy buffer only to pass the `tmem.is_empty()` gate; the faithful RGBA16
            // path reads `tmem_bank`, not this.
            tmem: vec![0u8; 8],
            combine_l,
            combine_h,
            other_mode_h: cycle_type << 20,
            ..Default::default()
        };
        let data: Vec<u8> = (0..128u32).map(|i| i as u8).collect();
        rdp.tmem_bank.write_block(&data, 0, 0, 0, 16, 2);
        let mk = |tmem_words: u16| crate::hle::rdp::TileDescriptor {
            fmt: 0, // RGBA
            siz: 2, // 16b → line_bytes = (4<<2)>>1 = 8 (word-aligned → faithful)
            width: 4,
            height: 1,
            line: 1, // 4 RGBA16 texels = 8 bytes = 1 word
            tmem_addr: tmem_words,
            cms: 2,
            cmt: 2,
            ..Default::default()
        };
        rdp.tiles[0] = mk(0); // TEXEL0 source
        rdp.tiles[1] = mk(8); // TEXEL1 source — different tmem word → different bytes
        rdp
    }

    #[test]
    fn two_cycle_two_texture_builds_tex1_from_base_plus_one() {
        // (a) 2-cycle combiner that genuinely samples the SECOND texture: cyc1 color-D = TEXEL0.
        //     Under the N64 pipeline role swap a TEXEL0 selector in cycle 1 reads texVal1 (tile
        //     base+1), so this is the correct way to express "use the second texture" — a raw cyc1
        //     TEXEL1 token would instead read tile base (the first texture). cyc0 color-D = TEXEL0
        //     samples tile base. base = G_TEXTURE tile = 0, so TEXEL0 <- tiles[0], TEXEL1 <- tiles[1].
        //     Combine words: cyc0 = (0-0)*0 + TEXEL0, cyc1 = (0-0)*0 + TEXEL0; alpha = ONE both.
        let (cl, ch, ct) = (0x0088_7F10u32, 0x88FC_FC7Eu32, 1u32); // ct=1 → 2-cycle
        let rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0; // base
        rsp.texture_state.on = true;

        let sel = decode_combine(cl, ch);
        assert_eq!(sel.cyc1.cd, ColorIn::Texel0, "cyc1 D-slot must be TEXEL0");
        assert!(
            cycle_uses_texel1(&sel, ct),
            "2-cycle cyc1-TEXEL0 (base+1 via swap) → uses_texel1"
        );

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0).expect("2-cycle two-texture must draw");
        assert!(
            diags.is_empty(),
            "no refuse diagnostic in 2-cycle: {diags:?}"
        );
        assert_eq!(mat.tile_count, 2, "usesTexel1 → tileCount 2");

        let tex1 = mat.tex1.as_ref().expect("tex1 built when tileCount == 2");
        // tex1 is the decode of tiles[(base+1)&7] = tiles[1] ...
        let expect1 = decode_tile_texture(&rdp, &rdp.tiles[1], 4, 1, 0);
        assert_eq!(tex1.texture, expect1);
        // ... and it genuinely differs from tex0 (tiles[0]), proving two distinct tiles were decoded.
        assert_ne!(
            mat.texture, tex1.texture,
            "tex0 (tiles[0]) must differ from tex1 (tiles[1])"
        );
        assert_eq!(
            mat.texture,
            decode_tile_texture(&rdp, &rdp.tiles[0], 4, 1, 0)
        );
    }

    #[test]
    fn one_cycle_texel1_reference_is_refused_not_drawn() {
        // (b) The SAME TEXEL1 combiner in 1-cycle mode. usesTexel1 short-circuits to false, so the
        //     1-cycle gate must REFUSE (return None) + diagnose — NOT draw with a dummy.
        let (cl, ch, ct) = (0xFC00_0002u32, 0x001C_0000u32, 0u32); // ct=0 → 1-cycle
        let rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.on = true;

        let sel = decode_combine(cl, ch);
        assert!(
            !cycle_uses_texel1(&sel, ct),
            "1-cycle TEXEL1 → uses_texel1 FALSE"
        );

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0);
        assert!(
            mat.is_none(),
            "1-cycle TEXEL1 must refuse-to-draw (None), not draw a dummy"
        );
        assert!(
            diags.iter().any(|d| matches!(
                d.kind,
                crate::diag::DiagKind::UnwiredSelector { slots } if slots == 0b0000_0100
            )),
            "1-cycle TEXEL1 must diagnose the CC slot exactly as before it was wired: {diags:?}"
        );
    }

    #[test]
    fn single_texture_texel0_only_has_tilecount_1_and_no_tex1() {
        // (c) Normal single-texture MODULATE (TEXEL0*SHADE), 1-cycle. Unchanged behavior.
        let (cl, ch, ct) = (0xFC12_7E24u32, 0xFFFF_F9FCu32, 0u32);
        let rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.on = true;

        assert!(!cycle_uses_texel1(&decode_combine(cl, ch), ct));

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0).expect("single-texture draws");
        assert!(diags.is_empty());
        assert_eq!(mat.tile_count, 1, "TEXEL0-only → tileCount 1");
        assert!(
            mat.tex1.is_none(),
            "single-texture material carries no tex1"
        );
        assert!(mat.tex_enable, "SPTexture on + TEXEL0 used → tex_enable");
    }

    // --- role-swap-aware detection + tex1 decode gate ----------------------------------------

    #[test]
    fn two_cycle_cyc1_texel1_is_single_texture_not_refused() {
        // N64 role swap: a 2-cycle combiner whose ONLY texel1-family token is a cyc1 TEXEL1
        // reference. Under the swap it reads texVal0 (the FIRST texture), so it is a legitimate
        // SINGLE-texture draw, not a second-texture use. cyc0 D=TEXEL0 (base), cyc1 D=TEXEL1 (reads
        // base via swap). A `!uses_texel1 && cyc1-has-TEXEL1` gate would wrongly refuse this; the
        // `cycle_type != 1` gate must let it draw.
        let (cl, ch, ct) = (0x0088_7F10u32, 0x88FC_FCBEu32, 1u32);
        let rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0;
        rsp.texture_state.on = true;

        let sel = decode_combine(cl, ch);
        assert_eq!(sel.cyc1.cd, ColorIn::Texel1, "cyc1 D-slot must be TEXEL1");
        assert_eq!(sel.cyc0.cd, ColorIn::Texel0, "cyc0 D-slot must be TEXEL0");
        assert!(
            cyc_sel_uses_texel1(&sel.cyc1),
            "precondition: cyc1 raw-scans as TEXEL1"
        );
        assert!(
            !cycle_uses_texel1(&sel, ct),
            "cyc1-TEXEL1 reads the FIRST texture under the swap → NOT a second-texture use"
        );

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0)
            .expect("2-cycle cyc1-TEXEL1 is single-texture and must draw, not refuse");
        assert!(diags.is_empty(), "must not refuse/diagnose: {diags:?}");
        assert_eq!(mat.tile_count, 1, "single texture → tileCount 1");
        assert!(mat.tex1.is_none(), "no second texture built");
    }

    #[test]
    fn two_cycle_second_texture_non_faithful_refuses_to_draw() {
        // A genuine two-texture combiner (cyc1 D=TEXEL0 → tileCount 2) whose SECOND tile
        // cannot take the faithful sample_tile path. The legacy fallback ignores tmem_addr and would
        // silently read tex0's TMEM, so build_material must refuse-to-draw + diagnose.
        let (cl, ch, ct) = (0x0088_7F10u32, 0x88FC_FC7Eu32, 1u32); // cyc1 D=TEXEL0
        let mut rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        // Override tiles[1] (the TEXEL1 source) with a sub-word I8 tile: line_bytes = (4<<1)>>1 = 4,
        // not a whole 64-bit word, and no LoadTile — so tile_takes_faithful_path is false.
        rdp.tiles[1] = crate::hle::rdp::TileDescriptor {
            fmt: 4, // I
            siz: 1, // 8b
            width: 4,
            height: 1,
            line: 1,
            tmem_addr: 8,
            cms: 2,
            cmt: 2,
            ..Default::default()
        };
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0;
        rsp.texture_state.on = true;

        // Precondition: this IS a two-texture combiner and tex0 (tiles[0]) is faithful.
        assert!(cycle_uses_texel1(&decode_combine(cl, ch), ct));
        assert!(tile_takes_faithful_path(&rdp, &rdp.tiles[0], 4));
        assert!(
            !tile_takes_faithful_path(&rdp, &rdp.tiles[1], 4),
            "precondition: tex1 tile is NOT faithful"
        );

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0);
        assert!(
            mat.is_none(),
            "non-faithful second texture must refuse-to-draw, not mis-decode"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.kind == crate::diag::DiagKind::SecondTextureUndecodable),
            "must diagnose SecondTextureUndecodable: {diags:?}"
        );
    }

    #[test]
    fn two_cycle_cyc1_texel1_reads_base_tile_marks_texture0_used() {
        // (e) SWAP-SYMMETRIC usesTexture(0) fix. A 2-cycle two-texture combiner (tile_count == 2)
        //     whose ONLY reference to the base tile is a cyc1 TEXEL1 selector, with NO TEXEL0 token
        //     anywhere. Under the active WGSL role swap a cyc1 TEXEL1 selector reads the FIRST texture
        //     (texVal0), so the first texture is used and MUST be sampled
        //     (tex_enable). Pre-fix, `uses_texel0` scanned only for TEXEL0 tokens and would have
        //     wrongly returned false → tex_enable false → the base tile read white instead of tex0.
        //
        //     Words: cyc0 D = TEXEL1 (index 2) engages the SECOND texture (usesTexture(1) → tile_count
        //     2, swap active); cyc1 D = TEXEL1 (index 2) reads the FIRST texture via the swap. Neither
        //     cycle references TEXEL0 in any slot.
        let (cl, ch, ct) = (0x0088_7F10u32, 0x88FD_7CBEu32, 1u32);
        let rdp = rdp_two_distinct_rgba16_tiles(cl, ch, ct);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0; // base
        rsp.texture_state.on = true;

        let sel = decode_combine(cl, ch);
        // Preconditions: both cycles reference TEXEL1 in the D-slot; NO TEXEL0 anywhere.
        assert_eq!(sel.cyc0.cd, ColorIn::Texel1, "cyc0 D-slot must be TEXEL1");
        assert_eq!(sel.cyc1.cd, ColorIn::Texel1, "cyc1 D-slot must be TEXEL1");
        assert!(
            !cycle_uses_texel0(&sel.cyc0) && !cycle_uses_texel0(&sel.cyc1),
            "precondition: NO TEXEL0 / TEXEL0_ALPHA token in either cycle"
        );
        // usesTexture(1): cyc0 references TEXEL1 → second texture engaged → tile_count 2 (swap active).
        assert!(
            cycle_uses_texel1(&sel, ct),
            "cyc0-TEXEL1 → uses_texel1 (tile_count == 2, swap active)"
        );

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0)
            .expect("2-cycle two-texture combiner must draw");
        assert!(diags.is_empty(), "no refuse diagnostic: {diags:?}");
        assert_eq!(mat.tile_count, 2, "usesTexel1 → tileCount 2");
        // The FIX: the first texture is used (cyc1 TEXEL1 reads the base tile under the swap), so the
        // first texture is sampled. Pre-fix this was FALSE and the base tile read as white.
        assert!(
            mat.tex_enable,
            "cyc1-TEXEL1 reads the base tile via the role swap → usesTexture(0) → tex_enable"
        );
        // Second texture is still built from tiles[base+1].
        assert!(mat.tex1.is_some(), "two-texture material carries tex1");
    }

    // --- LOD mip-chain decode ------------------------------------------------------------------

    /// Build an Rdp with a 3-level RGBA16 mip chain in tiles[0..3]: level 0 = 4×4, level 1 = 2×2,
    /// level 2 = 1×1 (halving in both dims). A 128-byte gradient (byte i = i) fills the faithful
    /// TMEM bank; each level reads a DISTINCT word offset so its decode differs from the others.
    /// `load_via_tile = true` routes the sub-word (2-wide / 1-wide) rows through the faithful
    /// sample path. `other_mode_h` carries G_TL_LOD (bit 16) plus the given detail bits.
    fn rdp_three_level_chain(detail_bit: bool) -> crate::hle::rdp::Rdp {
        let mut other_mode_h = 1u32 << 16; // G_TL_LOD, cycle_type 0
        if detail_bit {
            other_mode_h |= 0b10 << 17; // G_MDSFT_TEXTDETAIL bit1 = DETAIL
        }
        let mut rdp = crate::hle::rdp::Rdp {
            tmem: vec![0u8; 8], // pass the tmem.is_empty() gate; faithful path reads tmem_bank
            combine_l: 0xFC12_7E24, // MODULATE: cyc1 references TEXEL0 → textured, tile_count 1
            combine_h: 0xFFFF_F9FC,
            other_mode_h,
            load_via_tile: true, // sub-word level rows take the faithful path regardless of width
            ..Default::default()
        };
        let data: Vec<u8> = (0..128u32).map(|i| i as u8).collect();
        rdp.tmem_bank.write_block(&data, 0, 0, 0, 16, 2);
        let mk = |w: u16, h: u16, tmem_words: u16| crate::hle::rdp::TileDescriptor {
            fmt: 0, // RGBA
            siz: 2, // 16b
            width: w,
            height: h,
            line: 1,
            tmem_addr: tmem_words,
            cms: 2,
            cmt: 2,
            ..Default::default()
        };
        rdp.tiles[0] = mk(4, 4, 0); // level 0 (also the DETAIL tile)
        rdp.tiles[1] = mk(2, 2, 4); // level 1 — distinct word offset
        rdp.tiles[2] = mk(1, 1, 6); // level 2 — distinct word offset
        rdp
    }

    #[test]
    fn lod_decodes_n_halving_levels_with_distinct_content() {
        let rdp = rdp_three_level_chain(false);
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0; // base render tile
        rsp.texture_state.level = 2; // N = level + 1 = 3
        rsp.texture_state.on = true;

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0).expect("LOD material must build");
        assert!(diags.is_empty(), "no diagnostics: {diags:?}");

        // N levels captured (num_levels = level + 1).
        assert!(
            mat.lod,
            "G_TL_LOD + level>0 with a faithful halving chain → lod"
        );
        assert_eq!(mat.num_levels, 3);
        assert_eq!(mat.mip_levels.len(), 3, "one MipLevel per level");

        // Halving dims in both axes.
        assert_eq!((mat.mip_levels[0].w, mat.mip_levels[0].h), (4, 4));
        assert_eq!((mat.mip_levels[1].w, mat.mip_levels[1].h), (2, 2));
        assert_eq!((mat.mip_levels[2].w, mat.mip_levels[2].h), (1, 1));

        // Each buffer is exactly w*h*4 bytes.
        for lvl in &mat.mip_levels {
            assert_eq!(lvl.texture.len(), (lvl.w * lvl.h * 4) as usize);
        }

        // Level 0 mirrors the flat `texture`/`tex_w`/`tex_h` fields.
        assert_eq!((mat.tex_w, mat.tex_h), (4, 4));
        assert_eq!(mat.mip_levels[0].texture, mat.texture);

        // Distinct content between the three levels (they read different TMEM regions).
        assert_ne!(mat.mip_levels[0].texture, mat.mip_levels[1].texture);
        assert_ne!(mat.mip_levels[1].texture, mat.mip_levels[2].texture);

        // No DETAIL bit → no detail tile.
        assert!(mat.detail_tex.is_none());
    }

    #[test]
    fn lod_detail_mode_decodes_detail_tile_index0() {
        let rdp = rdp_three_level_chain(true); // DETAIL bit set
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0;
        rsp.texture_state.level = 2;
        rsp.texture_state.on = true;
        assert_eq!(rdp.text_detail(), 0b10, "precondition: DETAIL bit1 set");

        let mut diags = Vec::new();
        let mat =
            build_material(&rdp, &rsp, &mut diags, 0).expect("LOD+detail material must build");

        assert!(mat.lod);
        assert_eq!(mat.num_levels, 3);
        assert_eq!(mat.text_detail, 0b10);
        // The DETAIL tile is tiles[0] (4×4), decoded independently.
        let detail = mat
            .detail_tex
            .as_ref()
            .expect("DETAIL mode carries a detail tile");
        assert_eq!((detail.w, detail.h), (4, 4));
        assert_eq!(
            detail.texture,
            decode_tile_texture(&rdp, &rdp.tiles[0], 4, 4, 0),
            "detail tile is the independent decode of tiles[0]"
        );
    }

    #[test]
    fn lod_engages_for_non_halving_same_size_levels() {
        // N64-faithful per-level rework: two SAME-SIZE levels (both 4×4) — the sm64 Castle Inside
        // non-halving TRILERP case. The OLD mip-chain gate REQUIRED level k to be (tex_w>>k,
        // tex_h>>k) and REJECTED this (4 != 4>>1 = 2), falling back to a single level-0 (lod = false)
        // — i.e. it rendered non-LOD (RED-only for a RED/GREEN pair). The rework drops that halving
        // constraint: levels are INDEPENDENT per-level textures, so this now ENGAGES LOD and carries
        // BOTH levels at their own (non-halving) dims for the shader to blend.
        let mut rdp = rdp_three_level_chain(false);
        // Level 1 is the SAME 4×4 as level 0, not the halved 2×2.
        rdp.tiles[1].width = 4;
        rdp.tiles[1].height = 4;
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0;
        rsp.texture_state.level = 1; // N = level + 1 = 2 levels
        rsp.texture_state.on = true;

        let mut diags = Vec::new();
        let mat =
            build_material(&rdp, &rsp, &mut diags, 0).expect("non-halving LOD material must build");
        assert!(
            mat.lod,
            "non-halving two-level set must now ENGAGE LOD (the old gate rejected it → non-LOD fallback)"
        );
        assert_eq!(mat.num_levels, 2);
        assert_eq!(
            mat.mip_levels.len(),
            2,
            "one MipLevel per independent level"
        );
        // Both levels carry their OWN (identical, non-halving) dims — no `>>k` shrinking.
        assert_eq!((mat.mip_levels[0].w, mat.mip_levels[0].h), (4, 4));
        assert_eq!(
            (mat.mip_levels[1].w, mat.mip_levels[1].h),
            (4, 4),
            "level 1 is SAME size as level 0 — non-halving, previously rejected"
        );
        // Distinct content (levels read different TMEM word offsets), so a shader blend is meaningful.
        assert_ne!(mat.mip_levels[0].texture, mat.mip_levels[1].texture);
    }

    #[test]
    fn lod_falls_back_to_single_level_when_a_level_is_not_faithfully_decodable() {
        // The ONLY remaining LOD build-gate constraint after the per-level rework: every level must
        // take the faithful `sample_tile` decode path (a level needing the legacy linear fallback —
        // which ignores `tmem_addr` and could read the wrong bank — forces `lod = false`). Here level
        // 1 uses a format `sample_tile` does not handle (fmt 5 / YUV), so the gate must reject the set
        // and fall back to a single level-0, byte-identical to a non-LOD material.
        let mut rdp = rdp_three_level_chain(false);
        rdp.load_via_tile = false; // force the LoadBlock alignment path for the fallback decision
        rdp.tiles[1].fmt = 5; // YUV — not one of the nine faithful formats
        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 0;
        rsp.texture_state.level = 2;
        rsp.texture_state.on = true;

        let mut diags = Vec::new();
        let mat =
            build_material(&rdp, &rsp, &mut diags, 0).expect("must still build (single level)");
        assert!(!mat.lod, "unfaithful level → gate fails → lod = false");
        assert_eq!(mat.num_levels, 1);
        assert!(
            mat.mip_levels.is_empty(),
            "fallback carries no per-level set"
        );
        assert!(mat.detail_tex.is_none());
    }

    #[test]
    fn lod_wraps_tile_index_for_nonzero_base_tile() {
        // N64 RDP behavior: each LOD level is gathered from
        // tile `(tileIndexBase + t) % RDP_TILES` with RDP_TILES == 8 — a WRAP, NOT a clamp. fast3d
        // mirrors this exactly with `tiles[(base + k) & 7]`. This pins that wrap for a NONZERO base
        // tile: with base = 7 and a 2-level set, level 0 = tiles[7] and level 1 = tiles[(7+1)&7] =
        // tiles[0] (the wrap target). A clamp (`.min(7)`) would make level 1 = tiles[7] again and
        // DIVERGE from hardware — this test would then fail, which is exactly the regression it guards.
        // G_TL_LOD is already set by the helper, along with tiles[0] = 4×4 @ word 0.
        let mut rdp = rdp_three_level_chain(false);
        // Base tile 7 (level 0): DISTINCT dims (2×2) and DISTINCT content (word offset 4) from the
        // wrap target tiles[0] (4×4 @ word 0), so clamp-vs-wrap is observable in the decoded pixels.
        rdp.tiles[7] = crate::hle::rdp::TileDescriptor {
            fmt: 0, // RGBA
            siz: 2, // 16b
            width: 2,
            height: 2,
            line: 1,
            tmem_addr: 4, // distinct TMEM word offset → distinct decode from tiles[0]
            cms: 2,
            cmt: 2,
            ..Default::default()
        };

        let mut rsp = crate::hle::rsp::Rsp::default();
        rsp.texture_state.tile = 7; // base render tile 7
        rsp.texture_state.level = 1; // N = level + 1 = 2 levels
        rsp.texture_state.on = true;

        let mut diags = Vec::new();
        let mat = build_material(&rdp, &rsp, &mut diags, 0).expect("LOD material must build");
        assert!(mat.lod, "G_TL_LOD + level>0 with faithful levels → lod");
        assert_eq!(mat.num_levels, 2);
        assert_eq!(mat.mip_levels.len(), 2, "one MipLevel per level");

        // Level 0 is the base tile itself (tiles[7], 2×2).
        assert_eq!((mat.mip_levels[0].w, mat.mip_levels[0].h), (2, 2));
        assert_eq!(
            mat.mip_levels[0].texture,
            decode_tile_texture(&rdp, &rdp.tiles[7], 2, 2, 0),
            "level 0 is the decode of the base tile tiles[7]"
        );

        // Level 1 WRAPS to tiles[(7+1)&7] = tiles[0], matching the N64 `(base + t) % RDP_TILES` wrap. A
        // clamp to tiles[7] would instead re-decode the base tile here.
        assert_eq!((mat.mip_levels[1].w, mat.mip_levels[1].h), (4, 4));
        assert_eq!(
            mat.mip_levels[1].texture,
            decode_tile_texture(&rdp, &rdp.tiles[0], 4, 4, 0),
            "level 1 wraps to tiles[0] (`% RDP_TILES`), not clamp to tiles[7]"
        );

        // A clamp would make level 1 == level 0; the wrap yields distinct per-level content.
        assert_ne!(
            mat.mip_levels[0].texture, mat.mip_levels[1].texture,
            "wrap gives distinct level content; a clamp would duplicate level 0"
        );
    }
}
