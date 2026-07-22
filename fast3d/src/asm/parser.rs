//! Line-based parser for the gbi-macro source subset.

use crate::asm::expr::{parse_expr, Expr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    pub line: usize,
    pub msg: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VtxDef {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub flag: u16,
    pub s: i16,
    pub t: i16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    /// S8 normal vector for `VtxN`; `None` for the color `Vtx` form.
    pub normal: Option<[i8; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VpDef {
    pub vscale: [i16; 4],
    pub vtrans: [i16; 4],
}

/// A matrix data declaration: `Mtx <name> = identity()`, `Mtx <name> = scale(<f32>)`, or
/// `Mtx <name> = translate(x, y, z)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MtxInit {
    Identity,
    Scale(f32),
    Translate([f32; 3]),
    /// `perspective(fovy_deg, aspect, near, far, scale)`
    Perspective {
        fovy: f32,
        aspect: f32,
        near: f32,
        far: f32,
        scale: f32,
    },
    /// `lookat(ex, ey, ez, ax, ay, az, ux, uy, uz)`
    LookAt([f32; 9]),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MtxDef {
    pub name: String,
    pub init: MtxInit,
}

/// An address-valued macro operand: a named symbol (block/Mtx/Texture, resolved at assemble
/// time to a physical rdram address), a `seg(N, off)` segmented address, or a raw literal.
#[derive(Clone, Debug, PartialEq)]
pub enum AddrOperand {
    Symbol(String),
    Segmented { seg: u8, off: u32 },
    Raw(u32),
}

/// A statement inside the `update { }` block: a libultra `gu*` matrix builder whose first
/// operand is the target `Mtx` slot name and whose remaining operands are expressions.
#[derive(Clone, Debug, PartialEq)]
pub enum GuStmt {
    Rotate {
        target: String,
        deg: Expr,
        x: Expr,
        y: Expr,
        z: Expr,
    },
    Translate {
        target: String,
        x: Expr,
        y: Expr,
        z: Expr,
    },
    Scale {
        target: String,
        x: Expr,
        y: Expr,
        z: Expr,
    },
    MtxIdent {
        target: String,
    },
}

impl GuStmt {
    pub fn target(&self) -> &str {
        match self {
            GuStmt::Rotate { target, .. }
            | GuStmt::Translate { target, .. }
            | GuStmt::Scale { target, .. }
            | GuStmt::MtxIdent { target } => target,
        }
    }
    /// True if any operand expression reads `time`/`frame`.
    pub fn references_time(&self) -> bool {
        match self {
            GuStmt::Rotate { deg, x, y, z, .. } => {
                deg.references_time()
                    || x.references_time()
                    || y.references_time()
                    || z.references_time()
            }
            GuStmt::Translate { x, y, z, .. } | GuStmt::Scale { x, y, z, .. } => {
                x.references_time() || y.references_time() || z.references_time()
            }
            GuStmt::MtxIdent { .. } => false,
        }
    }
}

fn parse_gu_stmt(line: &str) -> Result<GuStmt, String> {
    let open = line.find('(').ok_or("expected `gu...(`")?;
    let head = line[..open].trim();
    let inner = line[open..]
        .trim()
        .strip_prefix('(')
        .map(|s| s.trim_end().trim_end_matches(';').trim_end())
        .and_then(|s| s.strip_suffix(')'))
        .ok_or("missing closing `)`")?;
    let parts = split_top_level(inner);
    let target = parts
        .first()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or("missing target matrix name")?;
    let exprs: Result<Vec<Expr>, String> = parts[1..]
        .iter()
        .map(|p| parse_expr(p).map_err(|e| e.0))
        .collect();
    let e = exprs?;
    let want = |n: usize| -> Result<(), String> {
        if e.len() == n {
            Ok(())
        } else {
            Err(format!(
                "{head} expects {n} numeric arg(s), got {}",
                e.len()
            ))
        }
    };
    match head {
        "guRotate" => {
            want(4)?;
            Ok(GuStmt::Rotate {
                target,
                deg: e[0].clone(),
                x: e[1].clone(),
                y: e[2].clone(),
                z: e[3].clone(),
            })
        }
        "guTranslate" => {
            want(3)?;
            Ok(GuStmt::Translate {
                target,
                x: e[0].clone(),
                y: e[1].clone(),
                z: e[2].clone(),
            })
        }
        "guScale" => {
            want(3)?;
            Ok(GuStmt::Scale {
                target,
                x: e[0].clone(),
                y: e[1].clone(),
                z: e[2].clone(),
            })
        }
        "guMtxIdent" => {
            want(0)?;
            Ok(GuStmt::MtxIdent { target })
        }
        _ => Err(format!("unknown update statement: {head}")),
    }
}

/// Pre-pass: lift the single optional `update { … }` block out of the source. Returns the
/// source with update lines blanked (line numbers preserved for the command parser) plus the
/// parsed `gu*` statements and any diagnostics. `update` is a reserved leading keyword; no nesting.
pub fn extract_update(source: &str) -> (String, Vec<(usize, GuStmt)>, Vec<Diag>) {
    let mut diags = Vec::new();
    let mut gu = Vec::new();
    let mut out: Vec<&str> = Vec::new();
    let mut in_update = false;
    let mut seen = false;
    let total = source.lines().count();
    for (i, raw) in source.lines().enumerate() {
        let n = i + 1;
        let line = raw.split("//").next().unwrap_or(raw).trim();
        if !in_update {
            let is_open = line == "update {"
                || line == "update{"
                || (line.starts_with("update") && line[6..].trim_start().starts_with('{'));
            if is_open {
                if seen {
                    diags.push(Diag {
                        line: n,
                        msg: "only one update block is allowed".into(),
                    });
                }
                seen = true;
                in_update = true;
                out.push(""); // blank to preserve line numbers
                              // Handle content on the same line after the opening `{`.
                let brace = line.find('{').unwrap();
                let rest = line[brace + 1..].trim();
                if rest.ends_with('}') {
                    // Fully inline block: `update { ... }`
                    let body = rest.strip_suffix('}').unwrap().trim();
                    if !body.is_empty() {
                        match parse_gu_stmt(body) {
                            Ok(s) => gu.push((n, s)),
                            Err(msg) => diags.push(Diag { line: n, msg }),
                        }
                    }
                    in_update = false; // block opened and closed on the same line
                } else if !rest.is_empty() {
                    // Content after `{` with no closing `}` on this line
                    match parse_gu_stmt(rest) {
                        Ok(s) => gu.push((n, s)),
                        Err(msg) => diags.push(Diag { line: n, msg }),
                    }
                    // in_update stays true
                }
                continue;
            }
            out.push(raw);
        } else {
            out.push("");
            // Close detection: trimmed line starts with `}`
            if let Some(after) = line.strip_prefix('}') {
                in_update = false;
                let after = after.trim();
                if !after.is_empty() {
                    diags.push(Diag {
                        line: n,
                        msg: "unexpected content after `}`".into(),
                    });
                }
                continue;
            }
            if line.is_empty() {
                continue;
            }
            match parse_gu_stmt(line) {
                Ok(s) => gu.push((n, s)),
                Err(msg) => diags.push(Diag { line: n, msg }),
            }
        }
    }
    if in_update {
        diags.push(Diag {
            line: total,
            msg: "unterminated update block (missing `}`)".into(),
        });
    }
    (out.join("\n"), gu, diags)
}

/// Resolve a `G_RM_*` preset name to its numeric value (from crate::hle::consts::rdp).
/// Returns `None` if unrecognized; callers fall back to `parse_u32_token`.
fn render_mode_preset(name: &str) -> Option<u32> {
    use crate::hle::consts::rdp::*;
    match name.trim() {
        "G_RM_OPA_SURF" => Some(G_RM_OPA_SURF),
        "G_RM_OPA_SURF2" => Some(G_RM_OPA_SURF2),
        "G_RM_AA_ZB_OPA_SURF" => Some(G_RM_AA_ZB_OPA_SURF),
        "G_RM_AA_ZB_OPA_SURF2" => Some(G_RM_AA_ZB_OPA_SURF2),
        "G_RM_AA_ZB_XLU_SURF" => Some(G_RM_AA_ZB_XLU_SURF),
        "G_RM_AA_ZB_XLU_SURF2" => Some(G_RM_AA_ZB_XLU_SURF2),
        "G_RM_AA_ZB_TEX_EDGE" => Some(G_RM_AA_ZB_TEX_EDGE),
        "G_RM_AA_ZB_TEX_EDGE2" => Some(G_RM_AA_ZB_TEX_EDGE2),
        "G_RM_CLD_SURF" => Some(G_RM_CLD_SURF),
        "G_RM_CLD_SURF2" => Some(G_RM_CLD_SURF2),
        "G_RM_AA_ZB_OPA_DECAL" => Some(G_RM_AA_ZB_OPA_DECAL),
        "G_RM_AA_ZB_OPA_DECAL2" => Some(G_RM_AA_ZB_OPA_DECAL2),
        "G_RM_AA_ZB_XLU_DECAL" => Some(G_RM_AA_ZB_XLU_DECAL),
        "G_RM_AA_ZB_XLU_DECAL2" => Some(G_RM_AA_ZB_XLU_DECAL2),
        "G_RM_FOG_SHADE_A" => Some(G_RM_FOG_SHADE_A),
        _ => None,
    }
}

fn parse_addr_operand(tok: &str) -> Option<AddrOperand> {
    let t = tok.trim();
    if let Some(inner) = t.strip_prefix("seg(").and_then(|x| x.strip_suffix(')')) {
        let parts = split_top_level(inner);
        if parts.len() == 2 {
            let seg = parse_u32_token(parts[0])? as u8;
            let off = parse_u32_token(parts[1])?;
            return Some(AddrOperand::Segmented { seg, off });
        }
        return None;
    }
    if let Some(v) = parse_u32_token(t) {
        return Some(AddrOperand::Raw(v));
    }
    if !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Some(AddrOperand::Symbol(t.to_string()));
    }
    None
}

/// gsSPMatrix flag selection captured from the source.
#[derive(Clone, Debug, PartialEq)]
pub struct MtxFlags {
    pub proj: bool, // G_MTX_PROJECTION (else MODELVIEW)
    pub load: bool, // G_MTX_LOAD (else MUL)
    pub push: bool, // G_MTX_PUSH (else NOPUSH)
}

/// Texture data declaration: `Texture <name> = { width, height, RGBA16 }`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureDef {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Format string, e.g. "RGBA16".
    pub fmt: String,
}

/// A single directional light entry: direction (x,y,z) and color (r,g,b).
#[derive(Clone, Debug, PartialEq)]
pub struct DirLight {
    pub dir: [i8; 3],
    pub col: [u8; 3],
}

/// A lights data declaration: `Lights <name> = { dir(x,y,z) col(r,g,b); ...; ambient(r,g,b) }`.
#[derive(Clone, Debug, PartialEq)]
pub struct LightsDef {
    pub name: String,
    pub dirs: Vec<DirLight>,
    pub ambient: [u8; 3],
}

/// A look-at-reflect data declaration: `LookAt <name> = lookat_reflect(ex,ey,ez, ax,ay,az, ux,uy,uz)`.
/// The s8 S/T basis axes are computed at parse time via `gu_look_at_reflect`.
#[derive(Clone, Debug, PartialEq)]
pub struct LookAtDef {
    pub name: String,
    pub s_axis: [i8; 3],
    pub t_axis: [i8; 3],
}

/// A per-frame vertex-morph declaration: `morph <pool> = lerp(<setA>, <setB>, <weight>)`.
/// The named `VtxSet` blocks `a`/`b` are interpolated per vertex at `assemble_at` by the weight
/// expression, producing the `pool` vertex region. Portable to real N64 C: the morph is baked
/// into a static `Vtx` pool per frame (no SP-side blending).
#[derive(Clone, Debug, PartialEq)]
pub struct MorphDef {
    pub pool: String,
    pub a: String,
    pub b: String,
    pub weight: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Vtx(VtxDef),
    Viewport(VpDef),
    Mtx(MtxDef),
    Texture(TextureDef),
    Lights(LightsDef),
    LookAt(LookAtDef),
    /// A named vertex block `VtxSet <name> = { <Vtx/VtxN lines> }`. Lifted as named data — never
    /// emitted into a `Gfx` block and never written by the static vtx-pool loop. Used only as a
    /// morph operand; the interpolated output pool is what reaches RDRAM.
    VtxSet {
        name: String,
        verts: Vec<VtxDef>,
    },
    /// A per-frame vertex morph `morph <pool> = lerp(<setA>, <setB>, <weight>)`. Lifted as named
    /// data; materialized into the vtx region at `assemble_at` (see [`MorphDef`]).
    Morph(MorphDef),
    SpVertex {
        n: u8,
        v0: u8,
    },
    Sp1Triangle {
        v0: u8,
        v1: u8,
        v2: u8,
    },
    SpMatrix {
        name: String,
        flags: MtxFlags,
    },
    SpViewport,
    /// gsSPPerspNormalize(mtx_name) — emit the named perspective matrix's perspNorm coefficient.
    SpPerspNormalize {
        name: String,
    },
    SpSetGeometryMode(u32),
    SpClearGeometryMode(u32),
    SpEndDisplayList,
    /// gsDPLoadTextureBlock(tex_name, fmt_mnemonic, siz_mnemonic, width, height)
    DpLoadTextureBlock {
        tex_name: String,
        fmt: u32,
        siz: u32,
        width: u32,
        height: u32,
        cmt: u32,
        maskt: u32,
        cms: u32,
        masks: u32,
    },
    /// gsSPTexture(sc, tc, level, tile, on)
    SpTexture {
        sc: u16,
        tc: u16,
        level: u32,
        tile: u32,
        on: bool,
    },
    /// gsDPSetOtherMode_H(shift, length, data)
    DpSetOtherModeH {
        shift: u32,
        length: u32,
        data: u32,
    },
    /// gsDPSetCombineLERP: full 16-arg form (both cycles)
    DpSetCombineLerp {
        c0a: u32,
        c0b: u32,
        c0c: u32,
        c0d: u32,
        a0a: u32,
        a0b: u32,
        a0c: u32,
        a0d: u32,
        c1a: u32,
        c1b: u32,
        c1c: u32,
        c1d: u32,
        a1a: u32,
        a1b: u32,
        a1c: u32,
        a1d: u32,
    },
    /// gsDPSetPrimColor(minlevel, lodfrac, r, g, b, a) — or rgba as single u32 packed
    DpSetPrimColor {
        minlevel: u32,
        lodfrac: u32,
        rgba: u32,
    },
    /// gsDPSetEnvColor(r, g, b, a) — or rgba as single u32 packed
    DpSetEnvColor {
        rgba: u32,
    },
    Sp2Triangles {
        v0: u8,
        v1: u8,
        v2: u8,
        v3: u8,
        v4: u8,
        v5: u8,
    },
    SpDisplayList {
        target: AddrOperand,
    },
    SpBranchList {
        target: AddrOperand,
    },
    SpPopMatrix {
        num: u32,
    },
    SpSegment {
        seg: u8,
        base: AddrOperand,
    },
    /// gsDPSetFogColor(r, g, b, a) or (rgba): set scene-global fog color.
    DpSetFogColor {
        rgba: u32,
    },
    /// gsDPSetBlendColor(r, g, b, a) or (rgba): set the blend-color register (CLR_BL blender
    /// selector + THRESHOLD alpha-compare).
    DpSetBlendColor {
        rgba: u32,
    },
    /// gsSPFogPosition(min, max): set fog range; encodes fm/fo via G_MOVEWORD/G_MW_FOG.
    SpFogPosition {
        min: i32,
        max: i32,
    },
    /// gsDPSetRenderMode(mode1, mode2): write the render-mode field of other_mode_l.
    DpSetRenderMode {
        mode1: u32,
        mode2: u32,
    },
    /// gsDPSetOtherMode_L(shift, length, data): raw bit-field write into other_mode_l.
    DpSetOtherModeL {
        shift: u32,
        length: u32,
        data: u32,
    },
    /// gsSPSetLights(name): emit NumLights + N directional MOVEMEM + ambient MOVEMEM.
    /// `num_dir` is resolved from the named `LightsDef` at parse time.
    SpSetLights {
        name: String,
        num_dir: u32,
    },
    /// gsSPLookAt(name): emit two G_MOVEMEM(G_MV_LIGHT) to DMEM slots 0/1 (S/T basis axes).
    SpLookAt {
        name: String,
    },
    /// gsDPSetColorImage(fmt, siz, width, addr): set the color framebuffer target.
    DpSetColorImage {
        fmt: u32,
        siz: u32,
        width: u32,
        addr: AddrOperand,
    },
    /// gsDPSetDepthImage(addr): set the depth buffer address.
    DpSetDepthImage {
        addr: AddrOperand,
    },
    /// gsDPSetScissor(mode, ulx, uly, lrx, lry): set scissor rectangle (10.2 fixed-point values).
    DpSetScissor {
        mode: u32,
        ulx: u32,
        uly: u32,
        lrx: u32,
        lry: u32,
    },
    /// gsDPSetFillColor(rgba) or (r,g,b,a): set the fill color register.
    DpSetFillColor {
        rgba: u32,
    },
    /// gsDPFillRectangle(ulx, uly, lrx, lry): fill rectangle (10.2 fixed-point values).
    DpFillRectangle {
        ulx: u32,
        uly: u32,
        lrx: u32,
        lry: u32,
    },
    /// gsSPTextureRectangle / gsSPTextureRectangleFlip: textured rectangle (3 GBI words).
    /// Coordinates in 10.2 fixed-point; uls/ult/dsdx/dtdy in raw texture-coord units.
    SpTextureRectangle {
        ulx: u32,
        uly: u32,
        lrx: u32,
        lry: u32,
        tile: u32,
        uls: u32,
        ult: u32,
        dsdx: u32,
        dtdy: u32,
        flip: bool,
    },
    /// gsDPSetTextureImage(fmt, siz, width, addr): standalone SETTIMG (NOT ctx.tex-gated).
    /// addr is a literal/segmented operand pointing at a scratch framebuffer or texture.
    DpSetTextureImage {
        fmt: u32,
        siz: u32,
        width: u32,
        addr: AddrOperand,
    },
    /// gsDPSetTile(fmt, siz, line, tmem, tile, palette, cmt, maskt, shiftt, cms, masks, shifts).
    DpSetTile {
        fmt: u32,
        siz: u32,
        rd_line: u32,
        tmem: u32,
        tile: u32,
        palette: u32,
        cmt: u32,
        maskt: u32,
        shiftt: u32,
        cms: u32,
        masks: u32,
        shifts: u32,
    },
    /// gsDPSetTileSize(tile, uls, ult, lrs, lrt): tile size (10.2 fixed-point values).
    DpSetTileSize {
        tile: u32,
        uls: u32,
        ult: u32,
        lrs: u32,
        lrt: u32,
    },
    GfxBlockStart(String),
    GfxBlockEnd,
}

/// Split on top-level commas only (commas inside parentheses are preserved), so nested
/// operands like `seg(6, 0)` survive as a single argument.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

fn parse_u32_token(tok: &str) -> Option<u32> {
    let t = tok.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<i64>().ok().map(|v| v as u32)
    }
}

/// Parse an image format mnemonic to the numeric fmt value (0=RGBA, 1=YUV, 2=CI, 3=IA, 4=I).
fn parse_img_fmt(tok: &str) -> Option<u32> {
    match tok.trim() {
        "G_IM_FMT_RGBA" => Some(0),
        "G_IM_FMT_YUV" => Some(1),
        "G_IM_FMT_CI" => Some(2),
        "G_IM_FMT_IA" => Some(3),
        "G_IM_FMT_I" => Some(4),
        other => parse_u32_token(other),
    }
}

/// Parse an image size mnemonic to the numeric siz value (0=4b, 1=8b, 2=16b, 3=32b).
fn parse_img_siz(tok: &str) -> Option<u32> {
    match tok.trim() {
        "G_IM_SIZ_4b" => Some(0),
        "G_IM_SIZ_8b" => Some(1),
        "G_IM_SIZ_16b" => Some(2),
        "G_IM_SIZ_32b" => Some(3),
        other => parse_u32_token(other),
    }
}

/// Parse a tile index mnemonic.  G_TX_RENDERTILE = 0, G_TX_LOADTILE = 7.
fn parse_tile_token(tok: &str) -> Option<u32> {
    match tok.trim() {
        "G_TX_RENDERTILE" => Some(0),
        "G_TX_LOADTILE" => Some(7),
        other => parse_u32_token(other),
    }
}

/// Parse a color-combiner mnemonic to a numeric selector index.
/// Returns None if unrecognized (caller emits diag).
fn parse_cc_mnemonic(tok: &str) -> Option<u32> {
    use crate::asm::encode::{ZERO_A, ZERO_C};
    match tok.trim() {
        "COMBINED" => Some(0),
        "TEXEL0" => Some(1),
        "TEXEL1" => Some(2),
        "PRIMITIVE" => Some(3),
        "SHADE" => Some(4),
        "ENVIRONMENT" => Some(5),
        "1" | "ONE" => Some(6),
        "0" | "ZERO" => {
            // Context-ambiguous: caller must pick ZERO_C or ZERO_A.
            // We return ZERO_C here; the alpha slots may be remapped by the caller.
            Some(ZERO_C)
        }
        other => parse_u32_token(other).or_else(|| {
            if other == "ZERO_A" {
                Some(ZERO_A)
            } else if other == "ZERO_C" {
                Some(ZERO_C)
            } else {
                None
            }
        }),
    }
}

fn geom_flag(name: &str) -> Option<u32> {
    match name.trim() {
        "G_SHADE" => Some(crate::hle::consts::G_SHADE),
        "G_SHADING_SMOOTH" => Some(crate::hle::consts::G_SHADING_SMOOTH),
        "G_LIGHTING" => Some(crate::hle::consts::G_LIGHTING),
        "G_CULL_FRONT" => Some(crate::hle::consts::G_CULL_FRONT),
        "G_CULL_BACK" => Some(crate::hle::consts::G_CULL_BACK),
        "G_CULL_BOTH" => Some(crate::hle::consts::G_CULL_BOTH),
        "G_FOG" => Some(crate::hle::consts::G_FOG),
        "G_ZBUFFER" => Some(crate::hle::consts::G_ZBUFFER),
        "G_TEXTURE_GEN" => Some(crate::hle::consts::G_TEXTURE_GEN),
        "G_TEXTURE_GEN_LINEAR" => Some(crate::hle::consts::G_TEXTURE_GEN_LINEAR),
        _ => None,
    }
}

fn call_args<'a>(line: &'a str, head: &str) -> Option<Vec<&'a str>> {
    let rest = line.strip_prefix(head)?;
    let inner = rest.trim().strip_prefix('(')?;
    let inner = inner
        .trim_end()
        .trim_end_matches(';')
        .trim_end()
        .strip_suffix(')')?;
    Some(split_top_level(inner))
}

fn brace_fields(line: &str) -> Option<Vec<&str>> {
    let start = line.find('{')?;
    let end = line.rfind('}')?;
    Some(line[start + 1..end].split(',').map(|s| s.trim()).collect())
}

/// Parse a single `VtxN { x,y,z,flag,s,t,nx,ny,nz,a }` or `Vtx { x,y,z,flag,s,t,r,g,b,a }` token
/// into a `VtxDef`. `VtxN` sets `normal = Some([nx,ny,nz])` with zeroed RGB; `Vtx` sets
/// `normal = None`. Shared by the top-level statement parser and the `VtxSet` block-body parser.
fn parse_vtx_def(tok: &str) -> Result<VtxDef, String> {
    let tok = tok.trim();
    let is_normal = tok.starts_with("VtxN");
    let f = brace_fields(tok).ok_or_else(|| {
        if is_normal {
            "VtxN expects { x,y,z,flag,s,t,nx,ny,nz,a }".to_string()
        } else {
            "Vtx expects { x,y,z,flag,s,t,r,g,b,a }".to_string()
        }
    })?;
    if f.len() != 10 {
        return Err(if is_normal {
            "VtxN: could not parse 10 numeric fields".to_string()
        } else {
            "Vtx: could not parse 10 numeric fields".to_string()
        });
    }
    let vals: Vec<Option<u32>> = f.iter().map(|s| parse_u32_token(s)).collect();
    if vals.iter().any(|v| v.is_none()) {
        return Err(if is_normal {
            "VtxN: could not parse 10 numeric fields".to_string()
        } else {
            "Vtx: could not parse 10 numeric fields".to_string()
        });
    }
    let v: Vec<u32> = vals.into_iter().map(|v| v.unwrap()).collect();
    if is_normal {
        Ok(VtxDef {
            x: v[0] as i16,
            y: v[1] as i16,
            z: v[2] as i16,
            flag: v[3] as u16,
            s: v[4] as i16,
            t: v[5] as i16,
            r: 0,
            g: 0,
            b: 0,
            a: v[9] as u8,
            normal: Some([v[6] as i8, v[7] as i8, v[8] as i8]),
        })
    } else {
        Ok(VtxDef {
            x: v[0] as i16,
            y: v[1] as i16,
            z: v[2] as i16,
            flag: v[3] as u16,
            s: v[4] as i16,
            t: v[5] as i16,
            r: v[6] as u8,
            g: v[7] as u8,
            b: v[8] as u8,
            a: v[9] as u8,
            normal: None,
        })
    }
}

/// Split a `VtxSet` block body into individual `Vtx`/`VtxN { ... }` tokens. Tokens may be on one
/// line or several; each starts at a `Vtx`/`VtxN` keyword and runs through its closing `}`.
fn split_vtx_tokens(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find the next `Vtx` / `VtxN` keyword.
        let rest = &body[i..];
        let kw = rest.find("Vtx");
        let Some(kw) = kw else { break };
        let start = i + kw;
        // Find the closing brace for this token.
        let after = &body[start..];
        let Some(open) = after.find('{') else { break };
        let Some(close_rel) = after[open..].find('}') else {
            break;
        };
        let end = start + open + close_rel + 1;
        out.push(body[start..end].trim());
        i = end;
    }
    out
}

pub fn parse(source: &str) -> (Vec<(usize, Stmt)>, Vec<Diag>) {
    let mut stmts = Vec::new();
    let mut diags = Vec::new();
    for (i, raw) in source.lines().enumerate() {
        let line = match raw.split("//").next() {
            Some(s) => s.trim(),
            None => raw.trim(),
        };
        if line.is_empty() {
            continue;
        }
        let n = i + 1;

        if line.starts_with("Mtx ") || line.starts_with("Mtx\t") {
            let body = line.strip_prefix("Mtx").unwrap().trim();
            match body.split_once('=') {
                Some((name_part, rhs)) => {
                    let name = name_part.trim().to_string();
                    let rhs = rhs.trim();
                    if name.is_empty() {
                        diags.push(Diag {
                            line: n,
                            msg: "Mtx: missing name".into(),
                        });
                    } else if rhs.starts_with("identity") {
                        stmts.push((
                            n,
                            Stmt::Mtx(MtxDef {
                                name,
                                init: MtxInit::Identity,
                            }),
                        ));
                    } else if let Some(inner) = rhs.strip_prefix("scale") {
                        let inner = inner
                            .trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')')
                            .trim();
                        match inner.parse::<f32>() {
                            Ok(s) => stmts.push((
                                n,
                                Stmt::Mtx(MtxDef {
                                    name,
                                    init: MtxInit::Scale(s),
                                }),
                            )),
                            Err(_) => diags.push(Diag {
                                line: n,
                                msg: format!("Mtx scale: not a float: {inner}"),
                            }),
                        }
                    } else if rhs.starts_with("translate") {
                        let inner = rhs
                            .strip_prefix("translate")
                            .unwrap()
                            .trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')');
                        let parts = split_top_level(inner);
                        let vals: Vec<Option<f32>> =
                            parts.iter().map(|p| p.parse::<f32>().ok()).collect();
                        if parts.len() == 3 && vals.iter().all(|v| v.is_some()) {
                            stmts.push((
                                n,
                                Stmt::Mtx(MtxDef {
                                    name,
                                    init: MtxInit::Translate([
                                        vals[0].unwrap(),
                                        vals[1].unwrap(),
                                        vals[2].unwrap(),
                                    ]),
                                }),
                            ));
                        } else {
                            diags.push(Diag {
                                line: n,
                                msg: format!("Mtx translate: expected (x, y, z), got: {inner}"),
                            });
                        }
                    } else if rhs.starts_with("perspective") {
                        let inner = rhs
                            .strip_prefix("perspective")
                            .unwrap()
                            .trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')');
                        let parts = split_top_level(inner);
                        let vals: Vec<Option<f32>> =
                            parts.iter().map(|p| p.parse::<f32>().ok()).collect();
                        if parts.len() == 5 && vals.iter().all(|v| v.is_some()) {
                            stmts.push((
                                n,
                                Stmt::Mtx(MtxDef {
                                    name,
                                    init: MtxInit::Perspective {
                                        fovy: vals[0].unwrap(),
                                        aspect: vals[1].unwrap(),
                                        near: vals[2].unwrap(),
                                        far: vals[3].unwrap(),
                                        scale: vals[4].unwrap(),
                                    },
                                }),
                            ));
                        } else {
                            diags.push(Diag {
                                line: n,
                                msg: format!(
                                    "Mtx perspective: expected (fovy, aspect, near, far, scale), got: {inner}"
                                ),
                            });
                        }
                    } else if rhs.starts_with("lookat") {
                        let inner = rhs
                            .strip_prefix("lookat")
                            .unwrap()
                            .trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')');
                        let parts = split_top_level(inner);
                        let vals: Vec<Option<f32>> =
                            parts.iter().map(|p| p.parse::<f32>().ok()).collect();
                        if parts.len() == 9 && vals.iter().all(|v| v.is_some()) {
                            let mut a = [0.0f32; 9];
                            for (i, v) in vals.iter().enumerate() {
                                a[i] = v.unwrap();
                            }
                            stmts.push((
                                n,
                                Stmt::Mtx(MtxDef {
                                    name,
                                    init: MtxInit::LookAt(a),
                                }),
                            ));
                        } else {
                            diags.push(Diag {
                                line: n,
                                msg: format!(
                                    "Mtx lookat: expected (ex,ey,ez, ax,ay,az, ux,uy,uz), got: {inner}"
                                ),
                            });
                        }
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: format!(
                                "Mtx: expected identity(), scale(<f32>), translate(x,y,z), perspective(...) or lookat(...), got: {rhs}"
                            ),
                        });
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "Mtx expects `Mtx <name> = identity()|scale(<f32>)`".into(),
                }),
            }
        } else if line.starts_with("VtxSet ") || line.starts_with("VtxSet\t") {
            // `VtxSet <name> = { <Vtx/VtxN lines...> }` — a named vertex block, lifted as data and
            // used only as a morph operand. Body verts share the existing Vtx/VtxN line parser.
            let body = line.strip_prefix("VtxSet").unwrap().trim();
            match body.split_once('=') {
                Some((name_part, rhs)) => {
                    let name = name_part.trim().to_string();
                    let inner = rhs
                        .trim()
                        .strip_prefix('{')
                        .and_then(|s| s.strip_suffix('}'))
                        .map(|s| s.trim());
                    match (name.is_empty(), inner) {
                        (true, _) => diags.push(Diag {
                            line: n,
                            msg: "VtxSet: missing name".into(),
                        }),
                        (_, None) => diags.push(Diag {
                            line: n,
                            msg: "VtxSet expects `VtxSet <name> = { <Vtx/VtxN lines> }`".into(),
                        }),
                        (false, Some(inner)) => {
                            let mut verts = Vec::new();
                            let mut ok = true;
                            for tok in split_vtx_tokens(inner) {
                                match parse_vtx_def(tok) {
                                    Ok(v) => verts.push(v),
                                    Err(msg) => {
                                        diags.push(Diag { line: n, msg });
                                        ok = false;
                                        break;
                                    }
                                }
                            }
                            if ok && verts.is_empty() {
                                diags.push(Diag {
                                    line: n,
                                    msg: "VtxSet: must contain at least one Vtx/VtxN".into(),
                                });
                                ok = false;
                            }
                            if ok {
                                stmts.push((n, Stmt::VtxSet { name, verts }));
                            }
                        }
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "VtxSet expects `VtxSet <name> = { <Vtx/VtxN lines> }`".into(),
                }),
            }
        } else if line.starts_with("morph ") || line.starts_with("morph\t") {
            // `morph <pool> = lerp(<setA>, <setB>, <weight Expr>)`
            let body = line.strip_prefix("morph").unwrap().trim();
            match body.split_once('=') {
                Some((pool_part, rhs)) => {
                    let pool = pool_part.trim().to_string();
                    let inner = rhs
                        .trim()
                        .strip_prefix("lerp")
                        .map(|s| s.trim())
                        .and_then(|s| s.strip_prefix('('))
                        .and_then(|s| s.strip_suffix(')'));
                    match (pool.is_empty(), inner) {
                        (true, _) => diags.push(Diag {
                            line: n,
                            msg: "morph: missing pool name".into(),
                        }),
                        (_, None) => diags.push(Diag {
                            line: n,
                            msg: "morph expects `morph <pool> = lerp(<setA>, <setB>, <weight>)`"
                                .into(),
                        }),
                        (false, Some(inner)) => {
                            let parts = split_top_level(inner);
                            if parts.len() != 3 {
                                diags.push(Diag {
                                    line: n,
                                    msg: "morph lerp expects (setA, setB, weight)".into(),
                                });
                            } else {
                                let a = parts[0].trim().to_string();
                                let b = parts[1].trim().to_string();
                                match parse_expr(parts[2]) {
                                    Ok(weight) => stmts
                                        .push((n, Stmt::Morph(MorphDef { pool, a, b, weight }))),
                                    Err(e) => diags.push(Diag {
                                        line: n,
                                        msg: format!("morph weight: {}", e.0),
                                    }),
                                }
                            }
                        }
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "morph expects `morph <pool> = lerp(<setA>, <setB>, <weight>)`".into(),
                }),
            }
        } else if line.starts_with("VtxN") && line.contains('{') {
            // `VtxN { x,y,z, flag, s,t, nx,ny,nz, a }` — same 16-byte layout as Vtx but bytes
            // 12/13/14 carry s8 normals and byte 15 is alpha (no RGB color).
            match brace_fields(line) {
                Some(f) if f.len() == 10 => {
                    let p = |idx: usize| parse_u32_token(f[idx]);
                    match (p(0), p(1), p(2), p(3), p(4), p(5), p(6), p(7), p(8), p(9)) {
                        (
                            Some(x),
                            Some(y),
                            Some(z),
                            Some(flag),
                            Some(s),
                            Some(t),
                            Some(nx),
                            Some(ny),
                            Some(nz),
                            Some(a),
                        ) => {
                            stmts.push((
                                n,
                                Stmt::Vtx(VtxDef {
                                    x: x as i16,
                                    y: y as i16,
                                    z: z as i16,
                                    flag: flag as u16,
                                    s: s as i16,
                                    t: t as i16,
                                    r: 0,
                                    g: 0,
                                    b: 0,
                                    a: a as u8,
                                    normal: Some([nx as i8, ny as i8, nz as i8]),
                                }),
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "VtxN: could not parse 10 numeric fields".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "VtxN expects { x,y,z,flag,s,t,nx,ny,nz,a }".into(),
                }),
            }
        } else if line.starts_with("Vtx") && line.contains('{') {
            match brace_fields(line) {
                Some(f) if f.len() == 10 => {
                    let p = |idx: usize| parse_u32_token(f[idx]);
                    match (p(0), p(1), p(2), p(3), p(4), p(5), p(6), p(7), p(8), p(9)) {
                        (
                            Some(x),
                            Some(y),
                            Some(z),
                            Some(flag),
                            Some(s),
                            Some(t),
                            Some(r),
                            Some(g),
                            Some(b),
                            Some(a),
                        ) => {
                            stmts.push((
                                n,
                                Stmt::Vtx(VtxDef {
                                    x: x as i16,
                                    y: y as i16,
                                    z: z as i16,
                                    flag: flag as u16,
                                    s: s as i16,
                                    t: t as i16,
                                    r: r as u8,
                                    g: g as u8,
                                    b: b as u8,
                                    a: a as u8,
                                    normal: None,
                                }),
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "Vtx: could not parse 10 numeric fields".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "Vtx expects { x,y,z,flag,s,t,r,g,b,a }".into(),
                }),
            }
        } else if line.starts_with("Vp") && line.contains('{') {
            match brace_fields(line) {
                Some(f) if f.len() == 8 => {
                    let vals: Vec<Option<u32>> = f.iter().map(|s| parse_u32_token(s)).collect();
                    if vals.iter().all(|v| v.is_some()) {
                        let v: Vec<i16> = vals.iter().map(|v| v.unwrap() as i16).collect();
                        stmts.push((
                            n,
                            Stmt::Viewport(VpDef {
                                vscale: [v[0], v[1], v[2], v[3]],
                                vtrans: [v[4], v[5], v[6], v[7]],
                            }),
                        ));
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: "Vp: could not parse 8 numeric fields".into(),
                        });
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "Vp expects { vscale0..3, vtrans0..3 }".into(),
                }),
            }
        } else if line.starts_with("gsSPVertex") {
            match call_args(line, "gsSPVertex") {
                Some(a) if a.len() == 3 => match (parse_u32_token(a[1]), parse_u32_token(a[2])) {
                    (Some(nn), Some(v0)) => stmts.push((
                        n,
                        Stmt::SpVertex {
                            n: nn as u8,
                            v0: v0 as u8,
                        },
                    )),
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsSPVertex(pool, n, v0): n and v0 must be numbers".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPVertex expects (pool, n, v0)".into(),
                }),
            }
        } else if line.starts_with("gsSP1Triangle") {
            match call_args(line, "gsSP1Triangle") {
                Some(a) if a.len() == 4 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                    ) {
                        (Some(v0), Some(v1), Some(v2)) => stmts.push((
                            n,
                            Stmt::Sp1Triangle {
                                v0: v0 as u8,
                                v1: v1 as u8,
                                v2: v2 as u8,
                            },
                        )),
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSP1Triangle: v0,v1,v2 must be numbers".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSP1Triangle expects (v0, v1, v2, flag)".into(),
                }),
            }
        } else if line.starts_with("gsSPMatrix") {
            match call_args(line, "gsSPMatrix") {
                Some(a) if a.len() >= 2 => {
                    let name = a[0].to_string();
                    let flag_text = a[1..].join(",");
                    let flags = MtxFlags {
                        proj: flag_text.contains("G_MTX_PROJECTION"),
                        load: flag_text.contains("G_MTX_LOAD"),
                        push: flag_text.contains("G_MTX_PUSH"),
                    };
                    stmts.push((n, Stmt::SpMatrix { name, flags }));
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPMatrix expects (name, FLAGS)".into(),
                }),
            }
        } else if line.starts_with("gsSPViewport") {
            stmts.push((n, Stmt::SpViewport));
        } else if line.starts_with("gsSPPerspNormalize") {
            match call_args(line, "gsSPPerspNormalize") {
                Some(a) if a.len() == 1 => {
                    stmts.push((
                        n,
                        Stmt::SpPerspNormalize {
                            name: a[0].trim().to_string(),
                        },
                    ));
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPPerspNormalize expects (mtx_name)".into(),
                }),
            }
        } else if line.starts_with("gsSPSetGeometryMode") {
            let a = call_args(line, "gsSPSetGeometryMode").unwrap_or_default();
            let mut bits = 0u32;
            for tok in &a {
                for piece in tok.split('|') {
                    let piece = piece.trim();
                    if piece.is_empty() {
                        continue;
                    }
                    if let Some(f) = geom_flag(piece) {
                        bits |= f;
                    } else if let Some(v) = parse_u32_token(piece) {
                        bits |= v;
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: format!("unknown geometry-mode flag: {piece}"),
                        });
                    }
                }
            }
            stmts.push((n, Stmt::SpSetGeometryMode(bits)));
        } else if line.starts_with("gsSPClearGeometryMode") {
            let a = call_args(line, "gsSPClearGeometryMode").unwrap_or_default();
            let mut bits = 0u32;
            for tok in &a {
                for piece in tok.split('|') {
                    let piece = piece.trim();
                    if piece.is_empty() {
                        continue;
                    }
                    if let Some(f) = geom_flag(piece) {
                        bits |= f;
                    } else if let Some(v) = parse_u32_token(piece) {
                        bits |= v;
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: format!("unknown geometry-mode flag: {piece}"),
                        });
                    }
                }
            }
            stmts.push((n, Stmt::SpClearGeometryMode(bits)));
        } else if line.starts_with("gsSPEndDisplayList") {
            stmts.push((n, Stmt::SpEndDisplayList));
        } else if line.starts_with("Texture ") || line.starts_with("Texture\t") {
            let body = line.strip_prefix("Texture").unwrap().trim();
            match body.split_once('=') {
                Some((name_part, rhs)) => {
                    let name = name_part.trim().to_string();
                    if name.is_empty() {
                        diags.push(Diag {
                            line: n,
                            msg: "Texture: missing name".into(),
                        });
                    } else {
                        match brace_fields(rhs) {
                            Some(f) if f.len() == 3 => {
                                match (parse_u32_token(f[0]), parse_u32_token(f[1])) {
                                    (Some(w), Some(h)) => {
                                        stmts.push((
                                            n,
                                            Stmt::Texture(TextureDef {
                                                name,
                                                width: w,
                                                height: h,
                                                fmt: f[2].trim().to_string(),
                                            }),
                                        ));
                                    }
                                    _ => diags.push(Diag {
                                        line: n,
                                        msg: "Texture: width and height must be numbers".into(),
                                    }),
                                }
                            }
                            _ => diags.push(Diag {
                                line: n,
                                msg: "Texture expects { width, height, FMT }".into(),
                            }),
                        }
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "Texture expects `Texture <name> = { width, height, FMT }`".into(),
                }),
            }
        } else if line.starts_with("gsDPSetTileSize") {
            match call_args(line, "gsDPSetTileSize") {
                Some(a) if a.len() == 5 => match (
                    parse_tile_token(a[0]),
                    parse_u32_token(a[1]),
                    parse_u32_token(a[2]),
                    parse_u32_token(a[3]),
                    parse_u32_token(a[4]),
                ) {
                    (Some(tile), Some(uls), Some(ult), Some(lrs), Some(lrt)) => {
                        stmts.push((
                            n,
                            Stmt::DpSetTileSize {
                                tile,
                                uls,
                                ult,
                                lrs,
                                lrt,
                            },
                        ));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetTileSize(tile,uls,ult,lrs,lrt): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetTileSize expects (tile, uls, ult, lrs, lrt)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetTile") {
            match call_args(line, "gsDPSetTile") {
                Some(a) if a.len() == 12 => match (
                    parse_img_fmt(a[0]),
                    parse_img_siz(a[1]),
                    parse_u32_token(a[2]),
                    parse_u32_token(a[3]),
                    parse_tile_token(a[4]),
                    parse_u32_token(a[5]),
                    parse_u32_token(a[6]),
                    parse_u32_token(a[7]),
                    parse_u32_token(a[8]),
                    parse_u32_token(a[9]),
                    parse_u32_token(a[10]),
                    parse_u32_token(a[11]),
                ) {
                    (
                        Some(fmt),
                        Some(siz),
                        Some(rd_line),
                        Some(tmem),
                        Some(tile),
                        Some(palette),
                        Some(cmt),
                        Some(maskt),
                        Some(shiftt),
                        Some(cms),
                        Some(masks),
                        Some(shifts),
                    ) => stmts.push((
                        n,
                        Stmt::DpSetTile {
                            fmt,
                            siz,
                            rd_line,
                            tmem,
                            tile,
                            palette,
                            cmt,
                            maskt,
                            shiftt,
                            cms,
                            masks,
                            shifts,
                        },
                    )),
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetTile: parse error in args".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetTile expects 12 args (fmt,siz,line,tmem,tile,pal,cmt,maskt,shiftt,cms,masks,shifts)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetTextureImage") {
            match call_args(line, "gsDPSetTextureImage") {
                Some(a) if a.len() == 4 => match (
                    parse_img_fmt(a[0]),
                    parse_img_siz(a[1]),
                    parse_u32_token(a[2]),
                    parse_addr_operand(a[3]),
                ) {
                    (Some(fmt), Some(siz), Some(width), Some(addr)) => {
                        stmts.push((
                            n,
                            Stmt::DpSetTextureImage {
                                fmt,
                                siz,
                                width,
                                addr,
                            },
                        ));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetTextureImage(fmt,siz,width,addr): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetTextureImage expects (fmt, siz, width, addr)".into(),
                }),
            }
        } else if line.starts_with("gsDPLoadTextureBlock") {
            match call_args(line, "gsDPLoadTextureBlock") {
                Some(a) if a.len() >= 5 => {
                    let tex_name = a[0].trim().to_string();
                    match (
                        parse_img_fmt(a[1]),
                        parse_img_siz(a[2]),
                        parse_u32_token(a[3]),
                        parse_u32_token(a[4]),
                    ) {
                        (Some(fmt), Some(siz), Some(width), Some(height)) => {
                            let cmt = if a.len() > 5 {
                                parse_u32_token(a[5]).unwrap_or(0)
                            } else {
                                0
                            };
                            let maskt = if a.len() > 6 {
                                parse_u32_token(a[6]).unwrap_or(0)
                            } else {
                                0
                            };
                            let cms = if a.len() > 7 {
                                parse_u32_token(a[7]).unwrap_or(0)
                            } else {
                                0
                            };
                            let masks = if a.len() > 8 {
                                parse_u32_token(a[8]).unwrap_or(0)
                            } else {
                                0
                            };
                            stmts.push((
                                n,
                                Stmt::DpLoadTextureBlock {
                                    tex_name,
                                    fmt,
                                    siz,
                                    width,
                                    height,
                                    cmt,
                                    maskt,
                                    cms,
                                    masks,
                                },
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPLoadTextureBlock: invalid fmt/siz/width/height".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPLoadTextureBlock expects (tex_name, fmt, siz, width, height, ...)"
                        .into(),
                }),
            }
        } else if line.starts_with("gsSPTextureRectangleFlip") {
            match call_args(line, "gsSPTextureRectangleFlip") {
                Some(a) if a.len() == 9 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                        parse_u32_token(a[3]),
                        parse_tile_token(a[4]),
                        parse_u32_token(a[5]),
                        parse_u32_token(a[6]),
                        parse_u32_token(a[7]),
                        parse_u32_token(a[8]),
                    ) {
                        (
                            Some(ulx),
                            Some(uly),
                            Some(lrx),
                            Some(lry),
                            Some(tile),
                            Some(uls),
                            Some(ult),
                            Some(dsdx),
                            Some(dtdy),
                        ) => stmts.push((
                            n,
                            Stmt::SpTextureRectangle {
                                ulx,
                                uly,
                                lrx,
                                lry,
                                tile,
                                uls,
                                ult,
                                dsdx,
                                dtdy,
                                flip: true,
                            },
                        )),
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSPTextureRectangleFlip(ulx,uly,lrx,lry,tile,uls,ult,dsdx,dtdy): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPTextureRectangleFlip expects 9 args".into(),
                }),
            }
        } else if line.starts_with("gsSPTextureRectangle") {
            match call_args(line, "gsSPTextureRectangle") {
                Some(a) if a.len() == 9 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                        parse_u32_token(a[3]),
                        parse_tile_token(a[4]),
                        parse_u32_token(a[5]),
                        parse_u32_token(a[6]),
                        parse_u32_token(a[7]),
                        parse_u32_token(a[8]),
                    ) {
                        (
                            Some(ulx),
                            Some(uly),
                            Some(lrx),
                            Some(lry),
                            Some(tile),
                            Some(uls),
                            Some(ult),
                            Some(dsdx),
                            Some(dtdy),
                        ) => stmts.push((
                            n,
                            Stmt::SpTextureRectangle {
                                ulx,
                                uly,
                                lrx,
                                lry,
                                tile,
                                uls,
                                ult,
                                dsdx,
                                dtdy,
                                flip: false,
                            },
                        )),
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSPTextureRectangle(ulx,uly,lrx,lry,tile,uls,ult,dsdx,dtdy): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPTextureRectangle expects 9 args".into(),
                }),
            }
        } else if line.starts_with("gsSPTexture") {
            match call_args(line, "gsSPTexture") {
                Some(a) if a.len() == 5 => {
                    let sc = parse_u32_token(a[0]).map(|v| v as u16);
                    let tc = parse_u32_token(a[1]).map(|v| v as u16);
                    let level = parse_u32_token(a[2]);
                    let tile = parse_tile_token(a[3]);
                    let on = match a[4].trim() {
                        "1" | "true" | "G_ON" => Some(true),
                        "0" | "false" | "G_OFF" => Some(false),
                        _ => None,
                    };
                    match (sc, tc, level, tile, on) {
                        (Some(sc), Some(tc), Some(level), Some(tile), Some(on)) => {
                            stmts.push((
                                n,
                                Stmt::SpTexture {
                                    sc,
                                    tc,
                                    level,
                                    tile,
                                    on,
                                },
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSPTexture(sc, tc, level, tile, on): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPTexture expects (sc, tc, level, tile, on)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetOtherMode_H") {
            // gsDPSetOtherMode_H(shift, length, data) — 3-arg raw form, OR
            // gsDPSetOtherMode_H(G_CYC_1CYCLE|G_CYC_2CYCLE) — 1-arg cycle-type shorthand.
            match call_args(line, "gsDPSetOtherMode_H") {
                Some(a) if a.len() == 1 => {
                    let cyc = match a[0].trim() {
                        "G_CYC_1CYCLE" => Some(0u32),
                        "G_CYC_2CYCLE" => Some(1u32),
                        "G_CYC_COPY" => Some(2u32),
                        "G_CYC_FILL" => Some(3u32),
                        other => parse_u32_token(other),
                    };
                    match cyc {
                        Some(c) => stmts.push((
                            n,
                            Stmt::DpSetOtherModeH {
                                shift: 20,
                                length: 2,
                                data: c << 20,
                            },
                        )),
                        None => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetOtherMode_H: unrecognized cycle type mnemonic".into(),
                        }),
                    }
                }
                Some(a) if a.len() == 3 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                    ) {
                        (Some(shift), Some(length), Some(data)) => {
                            stmts.push((
                                n,
                                Stmt::DpSetOtherModeH {
                                    shift,
                                    length,
                                    data,
                                },
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetOtherMode_H(shift, length, data): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetOtherMode_H expects (shift, length, data)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetCombineLERP") {
            match call_args(line, "gsDPSetCombineLERP") {
                Some(a) if a.len() == 16 => {
                    let cc: Vec<Option<u32>> = a.iter().map(|tok| parse_cc_mnemonic(tok)).collect();
                    if cc.iter().all(|v| v.is_some()) {
                        let v: Vec<u32> = cc.into_iter().map(|v| v.unwrap()).collect();
                        stmts.push((
                            n,
                            Stmt::DpSetCombineLerp {
                                c0a: v[0],
                                c0b: v[1],
                                c0c: v[2],
                                c0d: v[3],
                                a0a: v[4],
                                a0b: v[5],
                                a0c: v[6],
                                a0d: v[7],
                                c1a: v[8],
                                c1b: v[9],
                                c1c: v[10],
                                c1d: v[11],
                                a1a: v[12],
                                a1b: v[13],
                                a1c: v[14],
                                a1d: v[15],
                            },
                        ));
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: "gsDPSetCombineLERP: unrecognized combiner mnemonic in args"
                                .into(),
                        });
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetCombineLERP expects exactly 16 args (both cycles)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetPrimColor") {
            match call_args(line, "gsDPSetPrimColor") {
                Some(a) if a.len() == 6 => {
                    match (parse_u32_token(a[0]), parse_u32_token(a[1]),
                           parse_u32_token(a[2]), parse_u32_token(a[3]),
                           parse_u32_token(a[4]), parse_u32_token(a[5])) {
                        (Some(ml), Some(lf), Some(r), Some(g), Some(b), Some(al)) => {
                            let rgba = (r << 24) | (g << 16) | (b << 8) | al;
                            stmts.push((n, Stmt::DpSetPrimColor { minlevel: ml, lodfrac: lf, rgba }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetPrimColor: parse error in args".into(),
                        }),
                    }
                }
                Some(a) if a.len() == 3 => {
                    match (parse_u32_token(a[0]), parse_u32_token(a[1]), parse_u32_token(a[2])) {
                        (Some(ml), Some(lf), Some(rgba)) => {
                            stmts.push((n, Stmt::DpSetPrimColor { minlevel: ml, lodfrac: lf, rgba }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetPrimColor: parse error in args".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetPrimColor expects (minlevel, lodfrac, r, g, b, a) or (minlevel, lodfrac, rgba)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetEnvColor") {
            match call_args(line, "gsDPSetEnvColor") {
                Some(a) if a.len() == 4 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                        parse_u32_token(a[3]),
                    ) {
                        (Some(r), Some(g), Some(b), Some(al)) => {
                            let rgba = (r << 24) | (g << 16) | (b << 8) | al;
                            stmts.push((n, Stmt::DpSetEnvColor { rgba }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetEnvColor: parse error in args".into(),
                        }),
                    }
                }
                Some(a) if a.len() == 1 => match parse_u32_token(a[0]) {
                    Some(rgba) => stmts.push((n, Stmt::DpSetEnvColor { rgba })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetEnvColor: parse error in arg".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetEnvColor expects (r, g, b, a) or (rgba)".into(),
                }),
            }
        } else if line.starts_with("Lights ") || line.starts_with("Lights\t") {
            // `Lights <name> = { dir(x,y,z) col(r,g,b); ...; ambient(r,g,b) }`
            let body = line.strip_prefix("Lights").unwrap().trim();
            match body.split_once('=') {
                Some((name_part, rhs)) => {
                    let name = name_part.trim().to_string();
                    if name.is_empty() {
                        diags.push(Diag {
                            line: n,
                            msg: "Lights: missing name".into(),
                        });
                    } else {
                        // Strip outer braces.
                        let inner = match rhs
                            .trim()
                            .strip_prefix('{')
                            .and_then(|s| s.strip_suffix('}'))
                        {
                            Some(s) => s.trim(),
                            None => {
                                diags.push(Diag {
                                    line: n,
                                    msg: "Lights: expected `{ ... }`".into(),
                                });
                                continue;
                            }
                        };
                        // Split on `;` to get entries. Last entry is `ambient(r,g,b)`.
                        let entries: Vec<&str> = inner
                            .split(';')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if entries.is_empty() {
                            diags.push(Diag {
                                line: n,
                                msg: "Lights: must have at least an ambient entry".into(),
                            });
                            continue;
                        }
                        // Last entry must be `ambient(r,g,b)`.
                        let amb_entry = *entries.last().unwrap();
                        let dir_entries = &entries[..entries.len() - 1];
                        // Parse ambient.
                        let amb_inner = match amb_entry
                            .strip_prefix("ambient(")
                            .and_then(|s| s.strip_suffix(')'))
                        {
                            Some(s) => s,
                            None => {
                                diags.push(Diag {
                                    line: n,
                                    msg: format!(
                                        "Lights: last entry must be `ambient(r,g,b)`, got: {amb_entry}"
                                    ),
                                });
                                continue;
                            }
                        };
                        let amb_parts = split_top_level(amb_inner);
                        let amb: Option<[u8; 3]> = if amb_parts.len() == 3 {
                            match (
                                parse_u32_token(amb_parts[0]),
                                parse_u32_token(amb_parts[1]),
                                parse_u32_token(amb_parts[2]),
                            ) {
                                (Some(r), Some(g), Some(b)) => Some([r as u8, g as u8, b as u8]),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        let amb = match amb {
                            Some(a) => a,
                            None => {
                                diags.push(Diag {
                                    line: n,
                                    msg: format!(
                                        "Lights: ambient(r,g,b) — could not parse: {amb_inner}"
                                    ),
                                });
                                continue;
                            }
                        };
                        // Parse directional lights: each entry is `dir(x,y,z) col(r,g,b)`.
                        let mut dirs = Vec::new();
                        let mut parse_ok = true;
                        for entry in dir_entries {
                            // Find `dir(...)` and `col(...)` within the entry.
                            let dir_start = match entry.find("dir(") {
                                Some(i) => i,
                                None => {
                                    diags.push(Diag {
                                        line: n,
                                        msg: format!(
                                            "Lights: directional entry missing `dir(...)`: {entry}"
                                        ),
                                    });
                                    parse_ok = false;
                                    break;
                                }
                            };
                            let dir_end = match entry[dir_start..].find(')') {
                                Some(i) => dir_start + i,
                                None => {
                                    diags.push(Diag {
                                        line: n,
                                        msg: format!("Lights: malformed dir(...): {entry}"),
                                    });
                                    parse_ok = false;
                                    break;
                                }
                            };
                            let dir_inner = &entry[dir_start + 4..dir_end]; // "dir(" is 4 chars
                            let dir_parts = split_top_level(dir_inner);
                            let dir: Option<[i8; 3]> = if dir_parts.len() == 3 {
                                match (
                                    parse_u32_token(dir_parts[0]),
                                    parse_u32_token(dir_parts[1]),
                                    parse_u32_token(dir_parts[2]),
                                ) {
                                    (Some(x), Some(y), Some(z)) => {
                                        Some([x as i8, y as i8, z as i8])
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            let dir = match dir {
                                Some(d) => d,
                                None => {
                                    diags.push(Diag {
                                        line: n,
                                        msg: format!(
                                            "Lights: dir(x,y,z) — could not parse: {dir_inner}"
                                        ),
                                    });
                                    parse_ok = false;
                                    break;
                                }
                            };
                            // Parse `col(r,g,b)` after the dir part.
                            let after_dir = &entry[dir_end + 1..].trim_start();
                            let col_inner = match after_dir
                                .strip_prefix("col(")
                                .and_then(|s| s.strip_suffix(')'))
                            {
                                Some(s) => s,
                                None => {
                                    diags.push(Diag {
                                        line: n,
                                        msg: format!(
                                            "Lights: directional entry missing `col(r,g,b)`: {entry}"
                                        ),
                                    });
                                    parse_ok = false;
                                    break;
                                }
                            };
                            let col_parts = split_top_level(col_inner);
                            let col: Option<[u8; 3]> = if col_parts.len() == 3 {
                                match (
                                    parse_u32_token(col_parts[0]),
                                    parse_u32_token(col_parts[1]),
                                    parse_u32_token(col_parts[2]),
                                ) {
                                    (Some(r), Some(g), Some(b)) => {
                                        Some([r as u8, g as u8, b as u8])
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            let col = match col {
                                Some(c) => c,
                                None => {
                                    diags.push(Diag {
                                        line: n,
                                        msg: format!(
                                            "Lights: col(r,g,b) — could not parse: {col_inner}"
                                        ),
                                    });
                                    parse_ok = false;
                                    break;
                                }
                            };
                            dirs.push(crate::asm::parser::DirLight { dir, col });
                        }
                        if parse_ok {
                            stmts.push((
                                n,
                                Stmt::Lights(LightsDef {
                                    name,
                                    dirs,
                                    ambient: amb,
                                }),
                            ));
                        }
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "Lights expects `Lights <name> = { ... }`".into(),
                }),
            }
        } else if line.starts_with("LookAt ") || line.starts_with("LookAt\t") {
            // `LookAt <name> = lookat_reflect(ex,ey,ez, ax,ay,az, ux,uy,uz)`
            let body = line.strip_prefix("LookAt").unwrap().trim();
            match body.split_once('=') {
                Some((name_part, rhs)) => {
                    let name = name_part.trim().to_string();
                    let rhs = rhs.trim();
                    if name.is_empty() {
                        diags.push(Diag {
                            line: n,
                            msg: "LookAt: missing name".into(),
                        });
                    } else if let Some(inner) = rhs.strip_prefix("lookat_reflect") {
                        let inner = inner
                            .trim()
                            .trim_start_matches('(')
                            .trim_end_matches(')')
                            .trim();
                        let parts = split_top_level(inner);
                        let vals: Vec<Option<f32>> =
                            parts.iter().map(|p| p.parse::<f32>().ok()).collect();
                        if parts.len() == 9 && vals.iter().all(|v| v.is_some()) {
                            let mut a = [0.0f32; 9];
                            for (i, v) in vals.iter().enumerate() {
                                a[i] = v.unwrap();
                            }
                            let (s_axis, t_axis) = crate::asm::gu::gu_look_at_reflect(a);
                            stmts.push((
                                n,
                                Stmt::LookAt(LookAtDef {
                                    name,
                                    s_axis,
                                    t_axis,
                                }),
                            ));
                        } else {
                            diags.push(Diag {
                                line: n,
                                msg: format!(
                                    "LookAt lookat_reflect: expected (ex,ey,ez, ax,ay,az, ux,uy,uz), got: {inner}"
                                ),
                            });
                        }
                    } else {
                        diags.push(Diag {
                            line: n,
                            msg: format!("LookAt: expected lookat_reflect(...), got: {rhs}"),
                        });
                    }
                }
                None => diags.push(Diag {
                    line: n,
                    msg: "LookAt expects `LookAt <name> = lookat_reflect(...)`".into(),
                }),
            }
        } else if line.starts_with("gsSPSetLights") {
            match call_args(line, "gsSPSetLights") {
                Some(a) if a.len() == 1 => {
                    let light_name = a[0].trim().to_string();
                    // Resolve num_dir from the already-parsed LightsDef in stmts.
                    let num_dir = stmts.iter().find_map(|(_, s)| {
                        if let Stmt::Lights(def) = s {
                            if def.name == light_name {
                                return Some(def.dirs.len() as u32);
                            }
                        }
                        None
                    });
                    match num_dir {
                        Some(n_dir) => stmts.push((
                            n,
                            Stmt::SpSetLights {
                                name: light_name,
                                num_dir: n_dir,
                            },
                        )),
                        None => diags.push(Diag {
                            line: n,
                            msg: format!("gsSPSetLights: unknown lights name: {light_name}"),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPSetLights expects (name)".into(),
                }),
            }
        } else if line.starts_with("gsSPLookAt") {
            match call_args(line, "gsSPLookAt") {
                Some(a) if a.len() == 1 => {
                    stmts.push((
                        n,
                        Stmt::SpLookAt {
                            name: a[0].trim().to_string(),
                        },
                    ));
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPLookAt expects (name)".into(),
                }),
            }
        } else if line.starts_with("Gfx ") || line.starts_with("Gfx\t") {
            // `Gfx <name>[] = {`  -> open a named command block.
            let body = line.strip_prefix("Gfx").unwrap().trim();
            match body.split_once('[') {
                Some((name, rest)) if rest.contains('{') => {
                    stmts.push((n, Stmt::GfxBlockStart(name.trim().to_string())));
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "Gfx expects `Gfx <name>[] = {`".into(),
                }),
            }
        } else if line == "}" {
            stmts.push((n, Stmt::GfxBlockEnd));
        } else if line.starts_with("gsSP2Triangles") {
            match call_args(line, "gsSP2Triangles") {
                Some(a) if a.len() == 8 => {
                    let p = |i: usize| parse_u32_token(a[i]);
                    match (p(0), p(1), p(2), p(4), p(5), p(6)) {
                        (Some(v0), Some(v1), Some(v2), Some(v3), Some(v4), Some(v5)) => {
                            stmts.push((
                                n,
                                Stmt::Sp2Triangles {
                                    v0: v0 as u8,
                                    v1: v1 as u8,
                                    v2: v2 as u8,
                                    v3: v3 as u8,
                                    v4: v4 as u8,
                                    v5: v5 as u8,
                                },
                            ))
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSP2Triangles: vertex indices must be numbers".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSP2Triangles expects (v0,v1,v2,f0, v3,v4,v5,f1)".into(),
                }),
            }
        } else if line.starts_with("gsSPDisplayList") {
            match call_args(line, "gsSPDisplayList") {
                Some(a) if a.len() == 1 => match parse_addr_operand(a[0]) {
                    Some(target) => stmts.push((n, Stmt::SpDisplayList { target })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsSPDisplayList: bad target operand".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPDisplayList expects (target)".into(),
                }),
            }
        } else if line.starts_with("gsSPBranchList") {
            match call_args(line, "gsSPBranchList") {
                Some(a) if a.len() == 1 => match parse_addr_operand(a[0]) {
                    Some(target) => stmts.push((n, Stmt::SpBranchList { target })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsSPBranchList: bad target operand".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPBranchList expects (target)".into(),
                }),
            }
        } else if line.starts_with("gsSPPopMatrix") {
            match call_args(line, "gsSPPopMatrix") {
                Some(a) if a.len() == 1 => match parse_u32_token(a[0]) {
                    Some(num) => stmts.push((n, Stmt::SpPopMatrix { num })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsSPPopMatrix: num must be a number".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPPopMatrix expects (num)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetRenderMode") {
            match call_args(line, "gsDPSetRenderMode") {
                Some(a) if a.len() == 2 => {
                    let parse_rm =
                        |tok: &str| render_mode_preset(tok).or_else(|| parse_u32_token(tok));
                    match (parse_rm(a[0]), parse_rm(a[1])) {
                        (Some(mode1), Some(mode2)) => {
                            stmts.push((n, Stmt::DpSetRenderMode { mode1, mode2 }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetRenderMode: unrecognized mode1 or mode2 operand".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetRenderMode expects (mode1, mode2)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetOtherMode_L") {
            match call_args(line, "gsDPSetOtherMode_L") {
                Some(a) if a.len() == 3 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                    ) {
                        (Some(shift), Some(length), Some(data)) => {
                            stmts.push((
                                n,
                                Stmt::DpSetOtherModeL {
                                    shift,
                                    length,
                                    data,
                                },
                            ));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetOtherMode_L(shift, length, data): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetOtherMode_L expects (shift, length, data)".into(),
                }),
            }
        } else if line.starts_with("gsSPSegment") {
            match call_args(line, "gsSPSegment") {
                Some(a) if a.len() == 2 => {
                    match (parse_u32_token(a[0]), parse_addr_operand(a[1])) {
                        (Some(seg), Some(base)) => stmts.push((
                            n,
                            Stmt::SpSegment {
                                seg: seg as u8,
                                base,
                            },
                        )),
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsSPSegment(seg, base): parse error".into(),
                        }),
                    }
                }
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPSegment expects (seg, base)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetColorImage") {
            match call_args(line, "gsDPSetColorImage") {
                Some(a) if a.len() == 4 => match (
                    parse_img_fmt(a[0]),
                    parse_img_siz(a[1]),
                    parse_u32_token(a[2]),
                    parse_addr_operand(a[3]),
                ) {
                    (Some(fmt), Some(siz), Some(width), Some(addr)) => {
                        stmts.push((
                            n,
                            Stmt::DpSetColorImage {
                                fmt,
                                siz,
                                width,
                                addr,
                            },
                        ));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetColorImage(fmt,siz,width,addr): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetColorImage expects (fmt, siz, width, addr)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetDepthImage") {
            match call_args(line, "gsDPSetDepthImage") {
                Some(a) if a.len() == 1 => match parse_addr_operand(a[0]) {
                    Some(addr) => stmts.push((n, Stmt::DpSetDepthImage { addr })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetDepthImage(addr): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetDepthImage expects (addr)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetScissor") {
            match call_args(line, "gsDPSetScissor") {
                Some(a) if a.len() == 5 => match (
                    parse_u32_token(a[0]),
                    parse_u32_token(a[1]),
                    parse_u32_token(a[2]),
                    parse_u32_token(a[3]),
                    parse_u32_token(a[4]),
                ) {
                    (Some(mode), Some(ulx), Some(uly), Some(lrx), Some(lry)) => {
                        stmts.push((
                            n,
                            Stmt::DpSetScissor {
                                mode,
                                ulx,
                                uly,
                                lrx,
                                lry,
                            },
                        ));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetScissor(mode,ulx,uly,lrx,lry): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetScissor expects (mode, ulx, uly, lrx, lry)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetFillColor") {
            match call_args(line, "gsDPSetFillColor") {
                Some(a) if a.len() == 4 => match (
                    parse_u32_token(a[0]),
                    parse_u32_token(a[1]),
                    parse_u32_token(a[2]),
                    parse_u32_token(a[3]),
                ) {
                    (Some(r), Some(g), Some(b), Some(al)) => {
                        let rgba = (r << 24) | (g << 16) | (b << 8) | al;
                        stmts.push((n, Stmt::DpSetFillColor { rgba }));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetFillColor: parse error in args".into(),
                    }),
                },
                Some(a) if a.len() == 1 => match parse_u32_token(a[0]) {
                    Some(rgba) => stmts.push((n, Stmt::DpSetFillColor { rgba })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetFillColor: parse error in arg".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetFillColor expects (r, g, b, a) or (rgba)".into(),
                }),
            }
        } else if line.starts_with("gsDPFillRectangle") {
            match call_args(line, "gsDPFillRectangle") {
                Some(a) if a.len() == 4 => match (
                    parse_u32_token(a[0]),
                    parse_u32_token(a[1]),
                    parse_u32_token(a[2]),
                    parse_u32_token(a[3]),
                ) {
                    (Some(ulx), Some(uly), Some(lrx), Some(lry)) => {
                        stmts.push((n, Stmt::DpFillRectangle { ulx, uly, lrx, lry }));
                    }
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsDPFillRectangle(ulx,uly,lrx,lry): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPFillRectangle expects (ulx, uly, lrx, lry)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetFogColor") {
            match call_args(line, "gsDPSetFogColor") {
                Some(a) if a.len() == 4 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                        parse_u32_token(a[3]),
                    ) {
                        (Some(r), Some(g), Some(b), Some(al)) => {
                            let rgba = (r << 24) | (g << 16) | (b << 8) | al;
                            stmts.push((n, Stmt::DpSetFogColor { rgba }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetFogColor: parse error in args".into(),
                        }),
                    }
                }
                Some(a) if a.len() == 1 => match parse_u32_token(a[0]) {
                    Some(rgba) => stmts.push((n, Stmt::DpSetFogColor { rgba })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetFogColor: parse error in arg".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetFogColor expects (r, g, b, a) or (rgba)".into(),
                }),
            }
        } else if line.starts_with("gsDPSetBlendColor") {
            match call_args(line, "gsDPSetBlendColor") {
                Some(a) if a.len() == 4 => {
                    match (
                        parse_u32_token(a[0]),
                        parse_u32_token(a[1]),
                        parse_u32_token(a[2]),
                        parse_u32_token(a[3]),
                    ) {
                        (Some(r), Some(g), Some(b), Some(al)) => {
                            let rgba = (r << 24) | (g << 16) | (b << 8) | al;
                            stmts.push((n, Stmt::DpSetBlendColor { rgba }));
                        }
                        _ => diags.push(Diag {
                            line: n,
                            msg: "gsDPSetBlendColor: parse error in args".into(),
                        }),
                    }
                }
                Some(a) if a.len() == 1 => match parse_u32_token(a[0]) {
                    Some(rgba) => stmts.push((n, Stmt::DpSetBlendColor { rgba })),
                    None => diags.push(Diag {
                        line: n,
                        msg: "gsDPSetBlendColor: parse error in arg".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsDPSetBlendColor expects (r, g, b, a) or (rgba)".into(),
                }),
            }
        } else if line.starts_with("gsSPFogPosition") {
            match call_args(line, "gsSPFogPosition") {
                Some(a) if a.len() == 2 => match (parse_u32_token(a[0]), parse_u32_token(a[1])) {
                    (Some(min), Some(max)) => stmts.push((
                        n,
                        Stmt::SpFogPosition {
                            min: min as i32,
                            max: max as i32,
                        },
                    )),
                    _ => diags.push(Diag {
                        line: n,
                        msg: "gsSPFogPosition(min, max): parse error".into(),
                    }),
                },
                _ => diags.push(Diag {
                    line: n,
                    msg: "gsSPFogPosition expects (min, max)".into(),
                }),
            }
        } else {
            diags.push(Diag {
                line: n,
                msg: format!("unrecognized statement: {line}"),
            });
        }
    }
    (stmts, diags)
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn extracts_block_and_preserves_line_numbers() {
        let src = "Mtx model = identity()\nupdate {\n  guRotate(model, time*90, 0, 0, 1)\n}\ngsSPEndDisplayList()\n";
        let (cleaned, gu, diags) = extract_update(src);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(gu.len(), 1);
        assert_eq!(gu[0].0, 3); // line number preserved
        assert!(matches!(gu[0].1, GuStmt::Rotate { .. }));
        // cleaned source keeps the same number of lines (update lines blanked)
        assert_eq!(cleaned.lines().count(), src.lines().count());
        // the Mtx decl and the EndDL survive into the cleaned source
        assert!(cleaned.contains("Mtx model = identity()"));
        assert!(cleaned.contains("gsSPEndDisplayList()"));
        assert!(!cleaned.contains("guRotate"));
    }

    #[test]
    fn parses_all_transform_builders() {
        let src = "update {\nguRotate(a, 90, 0, 0, 1)\nguTranslate(b, 1, 2, 3)\nguScale(c, 2, 2, 2)\nguMtxIdent(d)\n}\n";
        let (_c, gu, diags) = extract_update(src);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(gu.len(), 4);
        assert_eq!(gu[0].1.target(), "a");
        assert_eq!(gu[3].1.target(), "d");
    }

    #[test]
    fn diagnoses_errors() {
        // second update block
        let (_c, _g, d) = extract_update("update {\n}\nupdate {\n}\n");
        assert!(d.iter().any(|x| x.msg.contains("only one update")));
        // unterminated
        let (_c, _g, d) = extract_update("update {\nguMtxIdent(a)\n");
        assert!(d.iter().any(|x| x.msg.contains("unterminated")));
        // bad arity / unknown builder / bad expr
        let (_c, _g, d) = extract_update("update {\nguRotate(a, 90, 0, 0)\n}\n");
        assert!(d.iter().any(|x| x.msg.contains("guRotate expects 4")));
        let (_c, _g, d) = extract_update("update {\nguNope(a)\n}\n");
        assert!(d.iter().any(|x| x.msg.contains("unknown update statement")));
        let (_c, _g, d) = extract_update("update {\nguRotate(a, bogus, 0, 0, 1)\n}\n");
        assert!(!d.is_empty());
    }

    #[test]
    fn multiline_block_parses_stmt() {
        // (a) multiline block: update {\n  guMtxIdent(a)\n}
        let src = "update {\nguMtxIdent(a)\n}\n";
        let (_c, gu, diags) = extract_update(src);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(gu.len(), 1);
        assert!(matches!(gu[0].1, GuStmt::MtxIdent { .. }));
    }

    #[test]
    fn fully_inline_block_parses_and_closes() {
        // (b) fully inline: update { guMtxIdent(a) }
        let src = "update { guMtxIdent(a) }\n";
        let (_c, gu, diags) = extract_update(src);
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert_eq!(gu.len(), 1, "expected 1 stmt, got {}", gu.len());
        assert!(matches!(gu[0].1, GuStmt::MtxIdent { .. }));
    }

    #[test]
    fn junk_after_close_brace_produces_diag_not_unterminated() {
        // (c) update {\nguMtxIdent(a)\n} junk  → stmt parsed + "unexpected content" diag, NOT "unterminated"
        let src = "update {\nguMtxIdent(a)\n} junk\n";
        let (_c, gu, diags) = extract_update(src);
        assert_eq!(gu.len(), 1, "should have parsed the stmt");
        assert!(
            diags
                .iter()
                .any(|d| d.msg.contains("unexpected content after")),
            "expected unexpected-content diag, got: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.msg.contains("unterminated")),
            "must not produce unterminated diag"
        );
    }

    #[test]
    fn inline_bad_stmt_produces_parse_diag_not_silent() {
        // (d) update { bogus( — parse diag, not silent
        let src = "update { bogus(\n}\n";
        let (_c, gu, diags) = extract_update(src);
        assert!(gu.is_empty(), "should produce no valid stmts");
        assert!(!diags.is_empty(), "should have a parse diag");
    }

    #[test]
    fn no_update_block_is_empty() {
        let (cleaned, gu, diags) = extract_update("Mtx m = identity()\ngsSPEndDisplayList()\n");
        assert!(gu.is_empty());
        assert!(diags.is_empty());
        assert!(cleaned.contains("Mtx m = identity()"));
    }
}

#[cfg(test)]
mod slice2_grammar_tests {
    use super::*;

    #[test]
    fn parses_perspective_and_lookat_mtx_init() {
        let (stmts, diags) = parse(
            "Mtx p = perspective(45, 1.3333, 10, 1000, 1)\nMtx v = lookat(0, 0, 150, 0, 0, 0, 0, 1, 0)\n",
        );
        assert!(diags.is_empty(), "diags: {diags:?}");
        let inits: Vec<&MtxInit> = stmts
            .iter()
            .filter_map(|(_, s)| match s {
                Stmt::Mtx(d) => Some(&d.init),
                _ => None,
            })
            .collect();
        assert!(
            matches!(inits[0], MtxInit::Perspective { fovy, near, .. } if *fovy == 45.0 && *near == 10.0)
        );
        assert!(matches!(inits[1], MtxInit::LookAt(a) if a[2] == 150.0 && a[7] == 1.0));
    }

    #[test]
    fn parses_persp_normalize_command() {
        let (stmts, diags) = parse("gsSPPerspNormalize(proj)\n");
        assert!(diags.is_empty(), "diags: {diags:?}");
        assert!(matches!(&stmts[0].1, Stmt::SpPerspNormalize { name } if name == "proj"));
    }

    #[test]
    fn g_zbuffer_geometry_flag_recognized() {
        let (stmts, diags) = parse("gsSPSetGeometryMode(G_ZBUFFER, G_SHADE)\n");
        assert!(diags.is_empty(), "diags: {diags:?}");
        match stmts[0].1 {
            Stmt::SpSetGeometryMode(bits) => {
                assert_eq!(
                    bits & crate::hle::consts::G_ZBUFFER,
                    crate::hle::consts::G_ZBUFFER
                );
                assert_eq!(
                    bits & crate::hle::consts::G_SHADE,
                    crate::hle::consts::G_SHADE
                );
            }
            _ => panic!("expected SpSetGeometryMode"),
        }
    }

    #[test]
    fn bad_perspective_arity_diagnoses() {
        let (_s, diags) = parse("Mtx p = perspective(45, 1, 10)\n");
        assert!(diags.iter().any(|d| d.msg.contains("Mtx perspective")));
    }

    #[test]
    fn bad_lookat_arity_diagnoses() {
        let (_s, diags) = parse("Mtx v = lookat(0, 0, 0)\n");
        assert!(diags.iter().any(|d| d.msg.contains("Mtx lookat")));
    }
}

#[cfg(test)]
mod vtx_routing_tests {
    use super::*;

    /// Parser-level lock: `VtxN` must route to the normal branch, not the color branch.
    /// A VtxN→color misroute (or broken branch order) will produce `normal: None`, failing
    /// the `Some([-1, 2, 127])` assertion here — even when the emitted bytes happen to match.
    #[test]
    fn vtxn_parses_to_normal_some_not_color_path() {
        let src = "VtxN { 10, 20, 30, 0, 0, 0, -1, 2, 127, 255 }\n";
        let (stmts, diags) = parse(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(stmts.len(), 1, "expected 1 stmt");
        match &stmts[0].1 {
            Stmt::Vtx(def) => {
                assert_eq!(def.x, 10);
                assert_eq!(def.y, 20);
                assert_eq!(def.z, 30);
                assert_eq!(def.a, 255);
                assert_eq!(
                    def.normal,
                    Some([-1i8, 2, 127]),
                    "VtxN must set normal=Some([nx,ny,nz]), not None (color path)"
                );
                // Color fields must be zeroed for VtxN
                assert_eq!(def.r, 0, "VtxN: r must be 0");
                assert_eq!(def.g, 0, "VtxN: g must be 0");
                assert_eq!(def.b, 0, "VtxN: b must be 0");
            }
            other => panic!("expected Stmt::Vtx, got {other:?}"),
        }
    }

    /// Complementary: color `Vtx` must produce `normal: None`.
    #[test]
    fn color_vtx_parses_to_normal_none() {
        let src = "Vtx { 10, 20, 30, 0, 0, 0, 255, 2, 127, 200 }\n";
        let (stmts, diags) = parse(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(stmts.len(), 1, "expected 1 stmt");
        match &stmts[0].1 {
            Stmt::Vtx(def) => {
                assert_eq!(def.x, 10);
                assert_eq!(def.y, 20);
                assert_eq!(def.z, 30);
                assert_eq!(def.r, 255);
                assert_eq!(def.g, 2);
                assert_eq!(def.b, 127);
                assert_eq!(def.a, 200);
                assert_eq!(
                    def.normal, None,
                    "color Vtx must set normal=None, not Some (normal path)"
                );
            }
            other => panic!("expected Stmt::Vtx, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod geom_mode_pipe_tests {
    use super::*;

    #[test]
    fn geometry_mode_accepts_pipe_or_form() {
        use crate::hle::consts::{
            G_SHADE, G_SHADING_SMOOTH, G_TEXTURE_GEN, G_TEXTURE_GEN_LINEAR, G_ZBUFFER,
        };
        let (stmts, diags) = crate::asm::parser::parse(
            "gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH, G_ZBUFFER)\n",
        );
        assert!(diags.is_empty(), "diags: {diags:?}");
        let bits = stmts
            .iter()
            .find_map(|(_, s)| match s {
                Stmt::SpSetGeometryMode(b) => Some(*b),
                _ => None,
            })
            .unwrap();
        assert_eq!(bits, G_SHADE | G_SHADING_SMOOTH | G_ZBUFFER);
        // Overlap guard: substring matching would mis-handle this; exact-match per piece must NOT.
        let (s2, d2) = crate::asm::parser::parse(
            "gsSPSetGeometryMode(G_TEXTURE_GEN | G_TEXTURE_GEN_LINEAR)\n",
        );
        assert!(d2.is_empty());
        let b2 = s2
            .iter()
            .find_map(|(_, s)| match s {
                Stmt::SpSetGeometryMode(b) => Some(*b),
                _ => None,
            })
            .unwrap();
        assert_eq!(b2, G_TEXTURE_GEN | G_TEXTURE_GEN_LINEAR);
    }
}
