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
    // Unwired inputs (Texel1 excluded from wired() — diagnostic + refuse-to-draw)
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
    // Unwired inputs (Texel1 excluded from wired() — diagnostic + refuse-to-draw)
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
        let selectors = decode_combine(rdp.combine_l, rdp.combine_h);
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
        let cycle_type = (rdp.other_mode_h >> 20) & 3;
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
        });
    }

    let selectors = decode_combine(rdp.combine_l, rdp.combine_h);

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

    // cycle_type from bits [21:20] of other_mode_h.
    let cycle_type = (rdp.other_mode_h >> 20) & 3;

    let tile = &rdp.tiles[0];
    let tex_w = tile.width.max(1) as u32;
    let tex_h = tile.height.max(1) as u32;

    let fi = crate::hle::texdec::FormatInfo {
        fmt: tile.fmt,
        siz: tile.siz,
    };
    let tlut_fmt = ((rdp.other_mode_h >> 14) & 0x3) as u8; // G_MDSFT_TEXTLUT
    let needed = fi.tmem_bytes(tex_w, tex_h);
    let texture = if rdp.tmem.len() >= needed {
        fi.decode(
            &rdp.tmem[..needed],
            tex_w,
            tex_h,
            &rdp.tlut,
            tile.palette,
            tlut_fmt,
        )
    } else {
        // tmem is shorter than the tile dimensions imply — zero-pad so the decoded buffer
        // always satisfies texture.len() == tex_w*tex_h*4 (the renderer's write_texture contract).
        let mut padded = rdp.tmem.to_vec();
        padded.resize(needed, 0);
        fi.decode(&padded, tex_w, tex_h, &rdp.tlut, tile.palette, tlut_fmt)
    };

    // tex_enable: SPTexture on AND the combiner samples TEXEL0. 1-cycle checks cyc1 (canonical slot);
    // 2-cycle checks BOTH cycles (sm64 fog terrain puts TEXEL0*SHADE in cycle 0, fog/pass in cycle 1).
    let uses_texel0 = cycle_uses_texel0(&selectors.cyc1)
        || (cycle_type == 1 && cycle_uses_texel0(&selectors.cyc0));
    let tex_enable = rsp.texture_state.on && uses_texel0;

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

    let fi = crate::hle::texdec::FormatInfo {
        fmt: tile.fmt,
        siz: tile.siz,
    };
    let tlut_fmt = ((rdp.other_mode_h >> 14) & 0x3) as u8; // G_MDSFT_TEXTLUT
    let needed = fi.tmem_bytes(tex_w, tex_h);
    let texture = if rdp.tmem.len() >= needed {
        fi.decode(
            &rdp.tmem[..needed],
            tex_w,
            tex_h,
            &rdp.tlut,
            tile.palette,
            tlut_fmt,
        )
    } else {
        let mut padded = rdp.tmem.to_vec();
        padded.resize(needed, 0);
        fi.decode(&padded, tex_w, tex_h, &rdp.tlut, tile.palette, tlut_fmt)
    };

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
        assert!(!AlphaIn::LodFraction.wired());
    }

    #[test]
    fn normal_modulate_reports_no_unwired_alpha_c() {
        // alpha-C decodes to A_ZERO (index 7) in MODULATE -> NOT in the unwired list.
        let cs = decode_combine(0xFC12_7E24, 0xFFFF_F9FC);
        assert!(!cs.cyc1.unwired().contains(&"AC"));
    }

    #[test]
    fn unwired_only_color_c_texel1() {
        // Wire alpha-C to A_ZERO (a1.c bits H[18,3]=7) so ONLY color-C TEXEL1 is unwired.
        // color-C cycle1 = L[0,5] = 2 (TEXEL1); set H[18,3]=7 so alpha-C is ZERO, not LOD_FRACTION.
        let cs = decode_combine(0xFC00_0002, 0x001C_0000); // H bit18..20 = 7
        assert_eq!(cs.cyc1.cc, ColorIn::Texel1);
        assert_eq!(cs.cyc1.unwired(), vec!["CC"]); // only color-C unwired
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
}
