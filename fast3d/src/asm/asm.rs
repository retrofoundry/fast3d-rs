use crate::asm::encode::*;
use crate::asm::expr::EvalCtx;
use crate::asm::gu::{gu_look_at, gu_mtx_ident, gu_perspective, gu_rotate, gu_scale, gu_translate};
use crate::asm::parser::{extract_update, parse, AddrOperand, Diag, GuStmt, MtxInit, Stmt, VtxDef};
use std::collections::{HashMap, HashSet};

/// Encode a single RGBA8 texel to RGBA16 (5/5/5/1 big-endian, N64 RGBA16 format).
pub fn encode_rgba16_texel(r: u8, g: u8, b: u8, a: u8) -> [u8; 2] {
    let r5 = (r >> 3) as u32;
    let g5 = (g >> 3) as u32;
    let b5 = (b >> 3) as u32;
    let a1 = if a >= 128 { 1u32 } else { 0u32 };
    let v = (r5 << 11) | (g5 << 6) | (b5 << 1) | a1;
    [(v >> 8) as u8, (v & 0xFF) as u8]
}

/// Encode a single RGBA8 texel to I8 (8-bit luminance).
/// Luminance = (R + G + B) / 3. Alpha is discarded (I format has no alpha channel).
pub fn encode_i8_texel(r: u8, g: u8, b: u8, _a: u8) -> u8 {
    ((r as u16 + g as u16 + b as u16) / 3) as u8
}

/// Pack two 4-bit intensity nibbles into one byte.
/// **High nibble = even column (t0), low nibble = odd column (t1)** — matches `decode_i4`.
/// Image encoders must begin each odd-width row with a new pair.
pub fn encode_i4_pair(t0_i4: u8, t1_i4: u8) -> u8 {
    (t0_i4 << 4) | (t1_i4 & 0xF)
}

/// Encode a single RGBA8 texel to IA16 (big-endian: [intensity, alpha]).
/// Intensity = (R + G + B) / 3. Alpha passed through.
/// Matches decode_ia16: word = intensity<<8 | alpha.
pub fn encode_ia16_texel(r: u8, g: u8, b: u8, a: u8) -> [u8; 2] {
    let i = encode_i8_texel(r, g, b, a);
    [i, a]
}

/// Encode a single RGBA8 texel to IA8 (4-bit intensity + 4-bit alpha in one byte).
/// i4 = luma >> 4, a4 = a >> 4; packed high-nibble-first.
/// Matches decode_ia8: (v>>4)=i4, (v&0xF)=a4.
pub fn encode_ia8_texel(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let i4 = encode_i8_texel(r, g, b, a) >> 4;
    let a4 = a >> 4;
    (i4 << 4) | a4
}

/// Encode a single RGBA8 texel to a 4-bit IA4 nibble.
/// IA4 layout: bits [3:1] = 3-bit intensity, bit [0] = 1-bit alpha.
/// Matches decode_ia4 expansion.
pub fn encode_ia4_nibble(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let i3 = encode_i8_texel(r, g, b, a) >> 5;
    let a1 = a >> 7;
    (i3 << 1) | a1
}

/// Pack two 4-bit IA4 nibbles into one byte.
/// **High nibble = even column (t0), low nibble = odd column (t1)** — matches `decode_ia4`.
/// Image encoders must begin each odd-width row with a new pair.
pub fn encode_ia4_pair(t0_ia4: u8, t1_ia4: u8) -> u8 {
    (t0_ia4 << 4) | (t1_ia4 & 0xF)
}

fn encode_4bit_flat<F>(rgba8: &[u8], encode: F) -> Vec<u8>
where
    F: Fn(u8, u8, u8, u8) -> u8,
{
    let num_pixels = rgba8.len() / 4;
    let mut out = Vec::with_capacity(num_pixels.div_ceil(2));
    let mut i = 0;
    while i < num_pixels {
        let first = i * 4;
        let high = encode(
            rgba8[first],
            rgba8[first + 1],
            rgba8[first + 2],
            rgba8[first + 3],
        );
        let low = if i + 1 < num_pixels {
            let second = first + 4;
            encode(
                rgba8[second],
                rgba8[second + 1],
                rgba8[second + 2],
                rgba8[second + 3],
            )
        } else {
            0
        };
        out.push((high << 4) | (low & 0x0f));
        i += 2;
    }
    out
}

fn encode_4bit_rows<F>(rgba8: &[u8], width: u32, height: u32, encode: F) -> Vec<u8>
where
    F: Fn(u8, u8, u8, u8) -> u8,
{
    let width = width as usize;
    let height = height as usize;
    let mut out = Vec::with_capacity(width.div_ceil(2) * height);
    for row in 0..height {
        let row_start = row * width;
        for column in (0..width).step_by(2) {
            let first = (row_start + column) * 4;
            let high = encode(
                rgba8[first],
                rgba8[first + 1],
                rgba8[first + 2],
                rgba8[first + 3],
            );
            let low = if column + 1 < width {
                let second = first + 4;
                encode(
                    rgba8[second],
                    rgba8[second + 1],
                    rgba8[second + 2],
                    rgba8[second + 3],
                )
            } else {
                0
            };
            out.push((high << 4) | (low & 0x0f));
        }
    }
    out
}

/// Parse a `Texture { w, h, FMT }` format string to `(fmt_code, siz_code)`.
/// `"RGBA16"` → `(0, 2)`, `"I8"` → `(4, 1)`, `"I4"` → `(4, 0)`,
/// `"IA16"` → `(3, 2)`, `"IA8"` → `(3, 1)`, `"IA4"` → `(3, 0)`,
/// `"CI8"` → `(2, 1)`, `"CI4"` → `(2, 0)`.
fn try_parse_tex_fmt(s: &str) -> Option<(u32, u32)> {
    match s {
        "RGBA16" => Some((0, 2)),
        "I8" => Some((4, 1)),
        "I4" => Some((4, 0)),
        "IA16" => Some((3, 2)),
        "IA8" => Some((3, 1)),
        "IA4" => Some((3, 0)),
        "CI8" => Some((2, 1)),
        "CI4" => Some((2, 0)),
        _ => None,
    }
}

fn parse_tex_fmt(s: &str) -> (u32, u32) {
    try_parse_tex_fmt(s).unwrap_or_else(|| {
        eprintln!("asm: unknown texture format '{s}'; defaulting to RGBA16");
        (0, 2)
    })
}

/// Nearest-color Euclidean distance: find the closest palette entry for each RGBA8 pixel.
/// Returns one index byte per texel. Used by both CI8 and CI4.
pub fn quantize(rgba8: &[u8], palette: &[[u8; 4]]) -> Vec<u8> {
    let n = rgba8.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = rgba8[i * 4] as i32;
        let g = rgba8[i * 4 + 1] as i32;
        let b = rgba8[i * 4 + 2] as i32;
        let a = rgba8[i * 4 + 3] as i32;
        let best = palette
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| {
                let dr = r - c[0] as i32;
                let dg = g - c[1] as i32;
                let db = b - c[2] as i32;
                let da = a - c[3] as i32;
                dr * dr + dg * dg + db * db + da * da
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        out.push(best as u8);
    }
    out
}

/// Encode RGBA8 pixels as CI8 palette indices (one byte per texel) using nearest-color matching.
/// The palette is caller-supplied; use [`build_palette_ci8`] to derive it from the source.
pub fn encode_ci8(rgba8: &[u8], palette: &[[u8; 4]]) -> Vec<u8> {
    quantize(rgba8, palette)
}

/// Extract the set of distinct RGBA8 colors from source pixels (in order of first appearance).
/// Returns an error string if more than 256 distinct colors are found (CI8 limit).
fn build_palette_ci8(rgba8: &[u8]) -> Result<Vec<[u8; 4]>, String> {
    let n = rgba8.len() / 4;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    for i in 0..n {
        let c = [
            rgba8[i * 4],
            rgba8[i * 4 + 1],
            rgba8[i * 4 + 2],
            rgba8[i * 4 + 3],
        ];
        if !palette.contains(&c) {
            if palette.len() >= 256 {
                return Err("CI8 texture has more than 256 distinct colors".into());
            }
            palette.push(c);
        }
    }
    Ok(palette)
}

/// Encode RGBA8 pixels as CI4 packed nibbles (2 texels per byte, high nibble = even column).
/// The palette is caller-supplied (≤16 entries); use [`build_palette_ci4`] to derive it.
/// Panics clearly if `palette.len() > 16`.
pub fn encode_ci4(rgba8: &[u8], palette: &[[u8; 4]]) -> Vec<u8> {
    assert!(
        palette.len() <= 16,
        "CI4 palette must have at most 16 entries, got {}",
        palette.len()
    );
    let indices = quantize(rgba8, palette);
    let n = indices.len();
    let mut out = Vec::with_capacity(n.div_ceil(2));
    let mut i = 0;
    while i < n {
        let idx0 = indices[i] & 0xF;
        let idx1 = if i + 1 < n { indices[i + 1] & 0xF } else { 0 };
        out.push((idx0 << 4) | idx1);
        i += 2;
    }
    out
}

fn encode_ci4_rows(rgba8: &[u8], palette: &[[u8; 4]], width: u32, height: u32) -> Vec<u8> {
    assert!(
        palette.len() <= 16,
        "CI4 palette must have at most 16 entries, got {}",
        palette.len()
    );
    let indices = quantize(rgba8, palette);
    let width = width as usize;
    let height = height as usize;
    let mut out = Vec::with_capacity(width.div_ceil(2) * height);
    for row in 0..height {
        let row_start = row * width;
        for column in (0..width).step_by(2) {
            let high = indices[row_start + column] & 0x0f;
            let low = if column + 1 < width {
                indices[row_start + column + 1] & 0x0f
            } else {
                0
            };
            out.push((high << 4) | low);
        }
    }
    out
}

/// Extract the set of distinct RGBA8 colors from source pixels (in order of first appearance).
/// Returns an error string if more than 16 distinct colors are found (CI4 limit).
fn build_palette_ci4(rgba8: &[u8]) -> Result<Vec<[u8; 4]>, String> {
    let n = rgba8.len() / 4;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    for i in 0..n {
        let c = [
            rgba8[i * 4],
            rgba8[i * 4 + 1],
            rgba8[i * 4 + 2],
            rgba8[i * 4 + 3],
        ];
        if !palette.contains(&c) {
            if palette.len() >= 16 {
                return Err("CI4 texture has more than 16 distinct colors".into());
            }
            palette.push(c);
        }
    }
    Ok(palette)
}

/// The assembled artifact: a unified RDRAM image. The data section (Vp/Vtx/Mtx/Texture) is laid
/// first; the command stream(s) follow. With named `Gfx[]` blocks, the implicit/explicit `main`
/// block is laid first (so `entry_addr` is the byte offset just past the data section), then the
/// named sub-DL blocks, each 8-byte aligned. All addresses are physical offsets into `rdram`.
#[derive(Clone, Debug)]
pub struct Image {
    pub rdram: Vec<u8>,
    /// Byte offset of the first command in `rdram` (8-aligned). The DL entry point.
    pub entry_addr: u32,
    pub vtx_addr: u32,
    pub vp_addr: u32,
    /// Address of the texture data in `rdram` (0 if no texture was assembled).
    pub tex_addr: u32,
    /// Address of the first named Lights block in `rdram` (0 if no Lights was assembled).
    pub light_addr: u32,
    /// Address of the first named LookAt block in `rdram` (0 if no LookAt was assembled).
    pub lookat_addr: u32,
}

fn push_word(buf: &mut Vec<u8>, w0: u32, w1: u32) {
    buf.extend_from_slice(&w0.to_be_bytes());
    buf.extend_from_slice(&w1.to_be_bytes());
}

fn scale_mtx(s: f32) -> [[f32; 4]; 4] {
    [
        [s, 0.0, 0.0, 0.0],
        [0.0, s, 0.0, 0.0],
        [0.0, 0.0, s, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Row-vector translate: `[x,y,z,1] * M = [x+tx, y+ty, z+tz, 1]` (translation in the last row).
fn translate_mtx(t: [f32; 3]) -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

/// Words emitted by one command statement (data decls + block markers emit none). Used to
/// pre-size blocks so block addresses are known before any block is emitted (forward references).
fn stmt_word_count(s: &Stmt) -> usize {
    match s {
        // CI8 (fmt=2,siz=1) and CI4 (fmt=2,siz=0): 4 TLUT-load commands + 7 standard texture-block
        // commands = 11. All other formats: gdp_load_texture_block returns 7 words.
        Stmt::DpLoadTextureBlock { fmt, siz, .. } => {
            if *fmt == 2 && (*siz == 0 || *siz == 1) {
                11
            } else {
                7
            }
        }
        Stmt::Vtx(_)
        | Stmt::Viewport(_)
        | Stmt::Mtx(_)
        | Stmt::Texture(_)
        | Stmt::Lights(_)
        | Stmt::LookAt(_)
        | Stmt::VtxSet { .. }
        | Stmt::Morph(_)
        | Stmt::GfxBlockStart(_)
        | Stmt::GfxBlockEnd => 0,
        // gSPNumLights(1 word) + N directional G_MOVEMEM + 1 ambient G_MOVEMEM
        Stmt::SpSetLights { num_dir, .. } => *num_dir as usize + 2,
        // gsSPLookAt: two G_MOVEMEM(G_MV_LIGHT) commands (S/T basis axes to DMEM slots 0/1).
        Stmt::SpLookAt { .. } => 2,
        Stmt::DpSetFogColor { .. } | Stmt::DpSetBlendColor { .. } | Stmt::SpFogPosition { .. } => 1,
        Stmt::SpTextureRectangle { .. } => 3,
        Stmt::DpSetColorImage { .. }
        | Stmt::DpSetDepthImage { .. }
        | Stmt::DpSetScissor { .. }
        | Stmt::DpSetFillColor { .. }
        | Stmt::DpFillRectangle { .. }
        | Stmt::DpSetTextureImage { .. }
        | Stmt::DpSetTile { .. }
        | Stmt::DpSetTileSize { .. } => 1,
        _ => 1,
    }
}

struct EmitCtx<'a> {
    mtx_addr: &'a HashMap<String, u32>,
    light_addr: &'a HashMap<String, u32>,
    lookat_addr: &'a HashMap<String, u32>,
    vtx_addr: u32,
    vp_addr: u32,
    block_addr: &'a HashMap<String, u32>,
    tex: Option<&'a HashMap<String, u32>>,
    persp_norm: &'a HashMap<String, u16>,
    /// For CI8 textures: maps texture name → (pal_addr, palette_entry_count).
    /// Used by emit_stmt to emit the G_LOADTLUT sequence before the standard LoadBlock.
    ci_pal: &'a HashMap<String, (u32, u32)>,
}

/// Resolve an address operand: a `seg(N,off)` segmented address, a raw literal, or a named symbol
/// (a `Gfx[]` block, `Mtx`, or `Texture`) to its physical rdram address.
fn resolve_addr(
    op: &AddrOperand,
    ctx: &EmitCtx,
    line: usize,
    diags: &mut Vec<Diag>,
) -> Option<u32> {
    match op {
        AddrOperand::Raw(v) => Some(*v),
        AddrOperand::Segmented { seg, off } => Some(((*seg as u32) << 24) | (off & 0x00FF_FFFF)),
        AddrOperand::Symbol(name) => {
            if let Some(&a) = ctx.block_addr.get(name) {
                Some(a)
            } else if let Some(&a) = ctx.mtx_addr.get(name) {
                Some(a)
            } else if let Some(&a) = ctx.tex.and_then(|t| t.get(name)) {
                Some(a)
            } else {
                diags.push(Diag {
                    line,
                    msg: format!("unknown symbol: {name}"),
                });
                None
            }
        }
    }
}

/// Group command statements into blocks: top-level commands form the implicit `main`; each
/// `Gfx <name>[]` is its own block. Output is `main` first, then the named blocks in source order.
fn group_blocks<'a>(
    stmts: &'a [(usize, Stmt)],
    diags: &mut Vec<Diag>,
) -> Vec<(String, Vec<(usize, &'a Stmt)>)> {
    let mut main: Vec<(usize, &Stmt)> = Vec::new();
    let mut named: Vec<(String, Vec<(usize, &Stmt)>)> = Vec::new();
    let mut cur: Option<usize> = None;
    for (line, s) in stmts {
        match s {
            Stmt::Vtx(_)
            | Stmt::Viewport(_)
            | Stmt::Mtx(_)
            | Stmt::Texture(_)
            | Stmt::Lights(_)
            | Stmt::LookAt(_)
            | Stmt::VtxSet { .. }
            | Stmt::Morph(_) => {}
            Stmt::GfxBlockStart(name) => {
                if name == "main" && !main.is_empty() {
                    diags.push(Diag {
                        line: *line,
                        msg: "cannot mix top-level commands with an explicit `Gfx main[]`".into(),
                    });
                }
                named.push((name.clone(), Vec::new()));
                cur = Some(named.len() - 1);
            }
            Stmt::GfxBlockEnd => cur = None,
            cmd => match cur {
                Some(i) => named[i].1.push((*line, cmd)),
                None => main.push((*line, cmd)),
            },
        }
    }
    let mut out: Vec<(String, Vec<(usize, &Stmt)>)> = Vec::new();
    if let Some(pos) = named.iter().position(|(n, _)| n == "main") {
        out.push(named.remove(pos));
    } else {
        out.push(("main".to_string(), main));
    }
    out.extend(named);
    out
}

#[derive(Clone, Copy, Debug)]
pub struct TextureInput<'a> {
    pub name: &'a str,
    pub rgba8: &'a [u8],
    pub width: u32,
    pub height: u32,
}

enum TextureInputs<'a> {
    None,
    Legacy(&'a [u8]),
    Named(&'a [TextureInput<'a>]),
}

struct EncodedTextures {
    first_addr: u32,
    addresses: HashMap<String, u32>,
    ci_palettes: HashMap<String, (u32, u32)>,
}

fn checked_rgba_len(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)
}

fn align_to_8(rdram: &mut Vec<u8>) {
    while !rdram.len().is_multiple_of(8) {
        rdram.push(0);
    }
}

#[derive(Clone, Copy)]
enum TextureEncodingLayout {
    Flat,
    Rows { width: u32, height: u32 },
}

/// Append one RGBA8 input in the selected N64 texture format. For CI formats, the returned tuple
/// identifies the separately aligned packed RGBA16 palette block.
fn encode_texture_data(
    rdram: &mut Vec<u8>,
    rgba8: &[u8],
    layout: TextureEncodingLayout,
    tex_fmt: u32,
    tex_siz: u32,
    line: usize,
    diags: &mut Vec<Diag>,
) -> Option<(u32, u32)> {
    match (tex_fmt, tex_siz) {
        (2, 1) => match build_palette_ci8(rgba8) {
            Ok(palette) => {
                let count = palette.len() as u32;
                rdram.extend_from_slice(&encode_ci8(rgba8, &palette));
                align_to_8(rdram);
                let palette_addr = rdram.len() as u32;
                for color in &palette {
                    rdram.extend_from_slice(&encode_rgba16_texel(
                        color[0], color[1], color[2], color[3],
                    ));
                }
                Some((palette_addr, count))
            }
            Err(msg) => {
                diags.push(Diag { line, msg });
                None
            }
        },
        (2, 0) => match build_palette_ci4(rgba8) {
            Ok(palette) => {
                let count = palette.len() as u32;
                let encoded = match layout {
                    TextureEncodingLayout::Flat => encode_ci4(rgba8, &palette),
                    TextureEncodingLayout::Rows { width, height } => {
                        encode_ci4_rows(rgba8, &palette, width, height)
                    }
                };
                rdram.extend_from_slice(&encoded);
                align_to_8(rdram);
                let palette_addr = rdram.len() as u32;
                for color in &palette {
                    rdram.extend_from_slice(&encode_rgba16_texel(
                        color[0], color[1], color[2], color[3],
                    ));
                }
                Some((palette_addr, count))
            }
            Err(msg) => {
                diags.push(Diag { line, msg });
                None
            }
        },
        (4, 1) => {
            for pixel in rgba8.chunks_exact(4) {
                rdram.push(encode_i8_texel(pixel[0], pixel[1], pixel[2], pixel[3]));
            }
            None
        }
        (4, 0) => {
            let encode = |r, g, b, a| encode_i8_texel(r, g, b, a) >> 4;
            let encoded = match layout {
                TextureEncodingLayout::Flat => encode_4bit_flat(rgba8, encode),
                TextureEncodingLayout::Rows { width, height } => {
                    encode_4bit_rows(rgba8, width, height, encode)
                }
            };
            rdram.extend_from_slice(&encoded);
            None
        }
        (3, 2) => {
            for pixel in rgba8.chunks_exact(4) {
                rdram.extend_from_slice(&encode_ia16_texel(pixel[0], pixel[1], pixel[2], pixel[3]));
            }
            None
        }
        (3, 1) => {
            for pixel in rgba8.chunks_exact(4) {
                rdram.push(encode_ia8_texel(pixel[0], pixel[1], pixel[2], pixel[3]));
            }
            None
        }
        (3, 0) => {
            let encoded = match layout {
                TextureEncodingLayout::Flat => encode_4bit_flat(rgba8, encode_ia4_nibble),
                TextureEncodingLayout::Rows { width, height } => {
                    encode_4bit_rows(rgba8, width, height, encode_ia4_nibble)
                }
            };
            rdram.extend_from_slice(&encoded);
            None
        }
        _ => {
            for pixel in rgba8.chunks_exact(4) {
                rdram.extend_from_slice(&encode_rgba16_texel(
                    pixel[0], pixel[1], pixel[2], pixel[3],
                ));
            }
            None
        }
    }
}

fn encode_textures(
    stmts: &[(usize, Stmt)],
    inputs: TextureInputs<'_>,
    rdram: &mut Vec<u8>,
    diags: &mut Vec<Diag>,
) -> EncodedTextures {
    match inputs {
        TextureInputs::None => EncodedTextures {
            first_addr: 0,
            addresses: HashMap::new(),
            ci_palettes: HashMap::new(),
        },
        TextureInputs::Legacy(rgba8) => {
            // Apply the first declared format to the shared input, preserving the historical API.
            let format = stmts
                .iter()
                .find_map(|(_, statement)| match statement {
                    Stmt::Texture(definition) => Some(definition.fmt.as_str()),
                    _ => None,
                })
                .unwrap_or("RGBA16");
            let (tex_fmt, tex_siz) = parse_tex_fmt(format);
            let first_addr = rdram.len() as u32;
            let palette = encode_texture_data(
                rdram,
                rgba8,
                TextureEncodingLayout::Flat,
                tex_fmt,
                tex_siz,
                0,
                diags,
            );
            align_to_8(rdram);

            let mut addresses = HashMap::new();
            let mut ci_palettes = HashMap::new();
            for (_, statement) in stmts {
                if let Stmt::Texture(definition) = statement {
                    addresses.insert(definition.name.clone(), first_addr);
                    if let Some(palette) = palette {
                        ci_palettes.insert(definition.name.clone(), palette);
                    }
                }
            }
            EncodedTextures {
                first_addr,
                addresses,
                ci_palettes,
            }
        }
        TextureInputs::Named(inputs) => {
            let declarations: Vec<_> = stmts
                .iter()
                .filter_map(|(line, statement)| match statement {
                    Stmt::Texture(definition) => Some((*line, definition)),
                    _ => None,
                })
                .collect();
            let mut declaration_names = HashSet::new();
            for (line, declaration) in &declarations {
                if !declaration_names.insert(declaration.name.as_str()) {
                    diags.push(Diag {
                        line: *line,
                        msg: format!("duplicate texture declaration: {}", declaration.name),
                    });
                }
            }

            let mut inputs_by_name = HashMap::new();
            for input in inputs {
                if inputs_by_name.insert(input.name, input).is_some() {
                    diags.push(Diag {
                        line: 0,
                        msg: format!("duplicate texture input: {}", input.name),
                    });
                }
                if !declaration_names.contains(input.name) {
                    diags.push(Diag {
                        line: 0,
                        msg: format!("undeclared texture input: {}", input.name),
                    });
                }
            }

            for (line, declaration) in &declarations {
                let Some(input) = inputs_by_name.get(declaration.name.as_str()) else {
                    diags.push(Diag {
                        line: *line,
                        msg: format!("missing texture input: {}", declaration.name),
                    });
                    continue;
                };
                if declaration.width == 0
                    || declaration.height == 0
                    || input.width == 0
                    || input.height == 0
                {
                    diags.push(Diag {
                        line: *line,
                        msg: format!("texture `{}` has zero dimensions", declaration.name),
                    });
                }
                if declaration.width != input.width || declaration.height != input.height {
                    diags.push(Diag {
                        line: *line,
                        msg: format!(
                            "texture `{}` declares {}x{}, but input is {}x{}",
                            declaration.name,
                            declaration.width,
                            declaration.height,
                            input.width,
                            input.height
                        ),
                    });
                }
                match checked_rgba_len(declaration.width, declaration.height) {
                    Some(expected) if input.rgba8.len() != expected => diags.push(Diag {
                        line: *line,
                        msg: format!(
                            "texture `{}` expected {expected} RGBA8 bytes, got {}",
                            declaration.name,
                            input.rgba8.len()
                        ),
                    }),
                    None => diags.push(Diag {
                        line: *line,
                        msg: format!("texture `{}` dimensions are too large", declaration.name),
                    }),
                    _ => {}
                }
                if try_parse_tex_fmt(&declaration.fmt).is_none() {
                    diags.push(Diag {
                        line: *line,
                        msg: format!("unknown texture format: {}", declaration.fmt),
                    });
                }
            }

            if !diags.is_empty() {
                return EncodedTextures {
                    first_addr: 0,
                    addresses: HashMap::new(),
                    ci_palettes: HashMap::new(),
                };
            }

            let mut encoded = EncodedTextures {
                first_addr: 0,
                addresses: HashMap::new(),
                ci_palettes: HashMap::new(),
            };
            for (line, declaration) in declarations {
                align_to_8(rdram);
                let address = rdram.len() as u32;
                if encoded.addresses.is_empty() {
                    encoded.first_addr = address;
                }
                let input = inputs_by_name[declaration.name.as_str()];
                let (tex_fmt, tex_siz) = try_parse_tex_fmt(&declaration.fmt)
                    .expect("named texture formats were validated before encoding");
                let palette = encode_texture_data(
                    rdram,
                    input.rgba8,
                    TextureEncodingLayout::Rows {
                        width: input.width,
                        height: input.height,
                    },
                    tex_fmt,
                    tex_siz,
                    line,
                    diags,
                );
                encoded.addresses.insert(declaration.name.clone(), address);
                if let Some(palette) = palette {
                    encoded
                        .ci_palettes
                        .insert(declaration.name.clone(), palette);
                }
                align_to_8(rdram);
            }
            encoded
        }
    }
}

fn assemble_inner(
    source: &str,
    textures: TextureInputs<'_>,
    overrides: &HashMap<String, [[f32; 4]; 4]>,
    vtx_overrides: &HashMap<String, Vec<u8>>,
) -> Result<Image, Vec<Diag>> {
    let (stmts, mut diags) = parse(source);
    let mut rdram: Vec<u8> = Vec::new();

    // --- data: viewport ---
    let vp = stmts.iter().find_map(|(_l, s)| match s {
        Stmt::Viewport(v) => Some(Vp {
            vscale: v.vscale,
            vtrans: v.vtrans,
        }),
        _ => None,
    });
    let vp_addr = rdram.len() as u32;
    let vp = vp.unwrap_or(Vp {
        vscale: [480, 640, 511, 511],
        vtrans: [480, 640, 0, 511],
    });
    rdram.extend_from_slice(&vp.to_bytes());

    // --- data: vertex pool ---
    // Loose `Vtx`/`VtxN` lines lay out positionally. `VtxSet` blocks are morph operands (skip-armed,
    // never written here). The morph OUTPUT pools — interpolated per frame in `assemble_at` and
    // passed in via `vtx_overrides` — are materialized after the static pool. Since at most one
    // pool reaches RDRAM in practice, `vtx_addr` is the single base `gsSPVertex(pool, n, 0)` loads.
    let vtx_addr = rdram.len() as u32;
    for (_l, s) in &stmts {
        if let Stmt::Vtx(v) = s {
            let vc = VtxColored {
                x: v.x,
                y: v.y,
                z: v.z,
                flag: v.flag,
                s: v.s,
                t: v.t,
                r: v.r,
                g: v.g,
                b: v.b,
                a: v.a,
            };
            let mut bytes = vc.to_bytes();
            // VtxN form: overwrite bytes 12/13/14 with s8 normals (byte 15 = alpha already set).
            if let Some([nx, ny, nz]) = v.normal {
                bytes[12] = nx as u8;
                bytes[13] = ny as u8;
                bytes[14] = nz as u8;
                // byte 15 is alpha, already written from vc.a
            }
            rdram.extend_from_slice(&bytes);
        }
    }
    // Materialize any morph output pools (pre-baked 16-byte vertices), in declared order.
    for (_l, s) in &stmts {
        if let Stmt::Morph(m) = s {
            if let Some(bytes) = vtx_overrides.get(&m.pool) {
                rdram.extend_from_slice(bytes);
            }
        }
    }

    // --- data: matrices ---
    let mut mtx_addr: HashMap<String, u32> = HashMap::new();
    let mut mtx_persp_norm: HashMap<String, u16> = HashMap::new();
    for (l, s) in &stmts {
        if let Stmt::Mtx(def) = s {
            let addr = rdram.len() as u32;
            let bytes = if let Some(m) = overrides.get(&def.name) {
                mtx_to_bytes(*m)
            } else {
                match def.init {
                    MtxInit::Identity => mtx_identity_bytes(),
                    MtxInit::Scale(scale) => mtx_to_bytes(scale_mtx(scale)),
                    MtxInit::Translate(t) => mtx_to_bytes(translate_mtx(t)),
                    MtxInit::Perspective {
                        fovy,
                        aspect,
                        near,
                        far,
                        scale,
                    } => {
                        let (m, pn) = gu_perspective(fovy, aspect, near, far, scale);
                        if m.iter().flatten().any(|v| !v.is_finite()) {
                            diags.push(Diag {
                                line: *l,
                                msg: format!(
                                    "perspective `{}` produced a non-finite matrix (check near != far, aspect != 0)",
                                    def.name
                                ),
                            });
                        }
                        mtx_persp_norm.insert(def.name.clone(), pn);
                        mtx_to_bytes(m)
                    }
                    MtxInit::LookAt(a) => mtx_to_bytes(gu_look_at(
                        a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8],
                    )),
                }
            };
            rdram.extend_from_slice(&bytes);
            mtx_addr.insert(def.name.clone(), addr);
        }
    }

    while !rdram.len().is_multiple_of(8) {
        rdram.push(0);
    }

    // --- data: lights ---
    // Track the first lights base address for the Image struct (0 if none).
    let mut light_addr: HashMap<String, u32> = HashMap::new();
    let mut first_light_addr: u32 = 0;
    for (_l, s) in &stmts {
        if let Stmt::Lights(def) = s {
            // Align to 8 bytes before each block (matrices may not be 8-aligned themselves).
            while !rdram.len().is_multiple_of(8) {
                rdram.push(0);
            }
            let base = rdram.len() as u32;
            if light_addr.is_empty() {
                first_light_addr = base;
            }
            // Each directional Light_t: exactly 16 bytes.
            // Layout: col[0..2] @0..2, pad @3, colc[0..2] @4..6, pad @7,
            //         dir[0..2] as u8 @8..10, 5 pad bytes @11..15.
            for dir_light in &def.dirs {
                let mut b = [0u8; 16];
                b[0] = dir_light.col[0];
                b[1] = dir_light.col[1];
                b[2] = dir_light.col[2];
                // b[3] = 0 (pad)
                b[4] = dir_light.col[0]; // colc copy
                b[5] = dir_light.col[1];
                b[6] = dir_light.col[2];
                // b[7] = 0 (pad)
                b[8] = dir_light.dir[0] as u8;
                b[9] = dir_light.dir[1] as u8;
                b[10] = dir_light.dir[2] as u8;
                // b[11..15] = 0 (5 pad bytes)
                rdram.extend_from_slice(&b);
            }
            // Ambient_t: exactly 8 bytes.
            // Layout: col[0..2] @0..2, pad @3, colc[0..2] @4..6, pad @7.
            let mut b = [0u8; 8];
            b[0] = def.ambient[0];
            b[1] = def.ambient[1];
            b[2] = def.ambient[2];
            // b[3] = 0 (pad)
            b[4] = def.ambient[0]; // colc copy
            b[5] = def.ambient[1];
            b[6] = def.ambient[2];
            // b[7] = 0 (pad)
            rdram.extend_from_slice(&b);
            light_addr.insert(def.name.clone(), base);
        }
    }

    // --- data: look-at-reflect basis blocks ---
    // Each LookAt emits a 2×16-byte Light_t-shaped block: entry 0 = S-axis, entry 1 = T-axis.
    // The s8 axis components live in bytes 8..11 (mirroring a directional Light_t's dir field);
    // every other byte is zero. The two MOVEMEM commands load entry 0 → DMEM slot 0 and entry 1
    // → slot 1 (the two LookAt slots that precede the directional lights).
    let mut lookat_addr: HashMap<String, u32> = HashMap::new();
    let mut first_lookat_addr: u32 = 0;
    for (_l, s) in &stmts {
        if let Stmt::LookAt(def) = s {
            // Align to 8 bytes before each block.
            while !rdram.len().is_multiple_of(8) {
                rdram.push(0);
            }
            let base = rdram.len() as u32;
            if lookat_addr.is_empty() {
                first_lookat_addr = base;
            }
            for axis in [def.s_axis, def.t_axis] {
                let mut b = [0u8; 16];
                b[8] = axis[0] as u8;
                b[9] = axis[1] as u8;
                b[10] = axis[2] as u8;
                // all other bytes = 0
                rdram.extend_from_slice(&b);
            }
            lookat_addr.insert(def.name.clone(), base);
        }
    }

    // --- data: optional textures ---
    let has_texture_inputs = !matches!(textures, TextureInputs::None);
    let encoded_textures = encode_textures(&stmts, textures, &mut rdram, &mut diags);

    // --- command blocks: group, pre-size + assign addresses (main first), then emit ---
    let blocks = group_blocks(&stmts, &mut diags);
    let mut block_addr: HashMap<String, u32> = HashMap::new();
    let mut cursor = rdram.len() as u32; // 8-aligned (data padded above)
    for (name, blk) in &blocks {
        block_addr.insert(name.clone(), cursor);
        let words: usize = blk.iter().map(|(_l, s)| stmt_word_count(s)).sum();
        cursor += (words * 8) as u32; // each block is a whole number of 8-byte words
    }
    // group_blocks always produces a "main" block (implicit from top-level commands, or explicit).
    let entry_addr = *block_addr
        .get("main")
        .expect("group_blocks always produces a main block");
    debug_assert!(entry_addr.is_multiple_of(8), "entry_addr must be 8-aligned"); // spec A5

    let ctx = EmitCtx {
        mtx_addr: &mtx_addr,
        light_addr: &light_addr,
        lookat_addr: &lookat_addr,
        vtx_addr,
        vp_addr,
        block_addr: &block_addr,
        tex: has_texture_inputs.then_some(&encoded_textures.addresses),
        persp_norm: &mtx_persp_norm,
        ci_pal: &encoded_textures.ci_palettes,
    };
    for (name, blk) in &blocks {
        // The pre-sized layout only holds when nothing has been diagnosed: a diagnosed command
        // (unknown symbol / texture) emits 0 words instead of its pre-sized count, desyncing the
        // cursor for later blocks. That is fine — we discard rdram and return Err below — but the
        // assert must not fire on that path. Guard it on `diags.is_empty()`.
        debug_assert!(
            !diags.is_empty() || rdram.len() as u32 == block_addr[name],
            "block layout mismatch"
        );
        for (line, s) in blk {
            emit_stmt(&mut rdram, s, *line, &ctx, &mut diags);
        }
    }

    if !diags.is_empty() {
        return Err(diags);
    }
    Ok(Image {
        rdram,
        entry_addr,
        vtx_addr,
        vp_addr,
        tex_addr: encoded_textures.first_addr,
        light_addr: first_light_addr,
        lookat_addr: first_lookat_addr,
    })
}

/// Assemble result carrying the RDRAM image plus whether the source animates over time
/// (an `update` builder reads `time`/`frame`). The host loops iff `is_time_variant`.
#[derive(Clone, Debug)]
pub struct Assembled {
    pub image: Image,
    pub is_time_variant: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureDecl {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureDeclarations {
    pub declarations: Vec<TextureDecl>,
    pub diagnostics: Vec<Diag>,
}

pub fn texture_declarations(source: &str) -> TextureDeclarations {
    let (cleaned, _, mut diagnostics) = extract_update(source);
    let (statements, mut parser_diagnostics) = parse(&cleaned);
    diagnostics.append(&mut parser_diagnostics);

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    for (line, statement) in statements {
        if let Stmt::Texture(definition) = statement {
            if !seen.insert(definition.name.clone()) {
                diagnostics.push(Diag {
                    line,
                    msg: format!("duplicate texture declaration: {}", definition.name),
                });
            }
            declarations.push(TextureDecl {
                name: definition.name,
                width: definition.width,
                height: definition.height,
                format: definition.fmt,
                line,
            });
        }
    }
    TextureDeclarations {
        declarations,
        diagnostics,
    }
}

fn eval_gu(s: &GuStmt, ctx: &EvalCtx) -> [[f32; 4]; 4] {
    match s {
        GuStmt::Rotate { deg, x, y, z, .. } => {
            gu_rotate(deg.eval(ctx), x.eval(ctx), y.eval(ctx), z.eval(ctx))
        }
        GuStmt::Translate { x, y, z, .. } => gu_translate(x.eval(ctx), y.eval(ctx), z.eval(ctx)),
        GuStmt::Scale { x, y, z, .. } => gu_scale(x.eval(ctx), y.eval(ctx), z.eval(ctx)),
        GuStmt::MtxIdent { .. } => gu_mtx_ident(),
    }
}

/// Interpolate one morph vertex from operands `a`/`b` at weight `w` (already clamped to 0..1) and
/// return its 16-byte on-disk vertex. Positions/UVs lerp linearly (rounded); the s8 normal is the
/// renormalized lerp of the unit-scaled operand normals (`* 127`, clamped to s8). RGBA take the
/// nearest endpoint (`a` for w<0.5, else `b`) — colored morphs are not interpolated.
fn morph_vertex_bytes(a: &VtxDef, b: &VtxDef, w: f32) -> [u8; 16] {
    let lerp = |x: f32, y: f32| x + (y - x) * w;
    let lerp_i16 = |x: i16, y: i16| (lerp(x as f32, y as f32)).round() as i16;
    let pick = if w < 0.5 { a } else { b };

    let vc = VtxColored {
        x: lerp_i16(a.x, b.x),
        y: lerp_i16(a.y, b.y),
        z: lerp_i16(a.z, b.z),
        flag: pick.flag,
        s: lerp_i16(a.s, b.s),
        t: lerp_i16(a.t, b.t),
        r: pick.r,
        g: pick.g,
        b: pick.b,
        a: pick.a,
    };
    let mut bytes = vc.to_bytes();

    // Normal: lerp the unit-scaled operand normals, renormalize, re-scale to s8. A `VtxDef` with no
    // normal contributes a zero vector (so a missing operand normal degrades gracefully).
    let na = a.normal.unwrap_or([0, 0, 0]);
    let nb = b.normal.unwrap_or([0, 0, 0]);
    if a.normal.is_some() || b.normal.is_some() {
        let nx = lerp(na[0] as f32 / 127.0, nb[0] as f32 / 127.0);
        let ny = lerp(na[1] as f32 / 127.0, nb[1] as f32 / 127.0);
        let nz = lerp(na[2] as f32 / 127.0, nb[2] as f32 / 127.0);
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        let to_s8 = |v: f32| (v * 127.0).round().clamp(-128.0, 127.0) as i8;
        let (sx, sy, sz) = if len > 1e-6 {
            (to_s8(nx / len), to_s8(ny / len), to_s8(nz / len))
        } else {
            (0, 0, 0)
        };
        bytes[12] = sx as u8;
        bytes[13] = sy as u8;
        bytes[14] = sz as u8;
        // byte 15 (alpha) already written from vc.a.
    }
    bytes
}

fn assemble_at_internal(
    source: &str,
    time: f32,
    textures: TextureInputs<'_>,
) -> Result<Assembled, Vec<Diag>> {
    let (cleaned, gu_stmts, mut diags) = extract_update(source);
    let ctx = EvalCtx {
        time,
        frame: (time * 60.0).floor(),
    };

    // Parse the cleaned source once to collect Mtx names (for update-target validation), the named
    // `VtxSet` blocks, and the `morph` declarations (for per-frame interpolation).
    let parsed = crate::asm::parser::parse(&cleaned).0;
    let declared: std::collections::HashSet<String> = parsed
        .iter()
        .filter_map(|(_, s)| match s {
            crate::asm::parser::Stmt::Mtx(d) => Some(d.name.clone()),
            _ => None,
        })
        .collect();
    let vtx_sets: HashMap<&str, &Vec<crate::asm::parser::VtxDef>> = parsed
        .iter()
        .filter_map(|(_, s)| match s {
            crate::asm::parser::Stmt::VtxSet { name, verts } => Some((name.as_str(), verts)),
            _ => None,
        })
        .collect();
    let morphs: Vec<(usize, &crate::asm::parser::MorphDef)> = parsed
        .iter()
        .filter_map(|(l, s)| match s {
            crate::asm::parser::Stmt::Morph(m) => Some((*l, m)),
            _ => None,
        })
        .collect();

    // Time-variance: an update builder OR any morph weight reading `time`/`frame`.
    let is_time_variant = gu_stmts.iter().any(|(_, s)| s.references_time())
        || morphs.iter().any(|(_, m)| m.weight.references_time());

    let mut overrides: HashMap<String, [[f32; 4]; 4]> = HashMap::new();
    for (line, s) in &gu_stmts {
        if !declared.contains(s.target()) {
            diags.push(Diag {
                line: *line,
                msg: format!("update targets unknown matrix `{}`", s.target()),
            });
            continue;
        }
        let m = eval_gu(s, &ctx);
        if m.iter().flatten().any(|v| !v.is_finite()) {
            diags.push(Diag {
                line: *line,
                msg: format!(
                    "update produced a non-finite matrix for `{}` at time={time}",
                    s.target()
                ),
            });
        } else {
            overrides.insert(s.target().to_string(), m);
        }
    }

    // Evaluate each morph into pre-baked vertex bytes for its output pool.
    let mut vtx_overrides: HashMap<String, Vec<u8>> = HashMap::new();
    for (line, m) in &morphs {
        let (a, b) = match (vtx_sets.get(m.a.as_str()), vtx_sets.get(m.b.as_str())) {
            (Some(a), Some(b)) => (*a, *b),
            _ => {
                diags.push(Diag {
                    line: *line,
                    msg: format!(
                        "morph `{}` references unknown VtxSet (a=`{}`, b=`{}`)",
                        m.pool, m.a, m.b
                    ),
                });
                continue;
            }
        };
        if a.len() != b.len() {
            diags.push(Diag {
                line: *line,
                msg: format!(
                    "morph `{}`: VtxSet `{}` ({}) and `{}` ({}) must have equal length",
                    m.pool,
                    m.a,
                    a.len(),
                    m.b,
                    b.len()
                ),
            });
            continue;
        }
        let w = m.weight.eval(&ctx);
        if !w.is_finite() {
            diags.push(Diag {
                line: *line,
                msg: format!("morph `{}`: weight is non-finite at time={time}", m.pool),
            });
            continue;
        }
        let w = w.clamp(0.0, 1.0);
        let mut bytes = Vec::with_capacity(a.len() * 16);
        for (va, vb) in a.iter().zip(b.iter()) {
            bytes.extend_from_slice(&morph_vertex_bytes(va, vb, w));
        }
        vtx_overrides.insert(m.pool.clone(), bytes);
    }

    match assemble_inner(&cleaned, textures, &overrides, &vtx_overrides) {
        Ok(image) if diags.is_empty() => Ok(Assembled {
            image,
            is_time_variant,
        }),
        Ok(_) => Err(diags),
        Err(mut e) => {
            diags.append(&mut e);
            Err(diags)
        }
    }
}

/// Assemble `source` at playback `time` (seconds). Evaluates the `update` block, baking the
/// resulting matrices over their declared initializers. `frame = floor(time*60)`.
pub fn assemble_at(
    source: &str,
    time: f32,
    tex: Option<(&[u8], u32, u32)>,
) -> Result<Assembled, Vec<Diag>> {
    let inputs = match tex {
        Some((rgba8, _, _)) => TextureInputs::Legacy(rgba8),
        None => TextureInputs::None,
    };
    assemble_at_internal(source, time, inputs)
}

/// Assemble `source` with one exact RGBA8 input for each named `Texture` declaration.
pub fn assemble_at_with_textures(
    source: &str,
    time: f32,
    textures: &[TextureInput<'_>],
) -> Result<Assembled, Vec<Diag>> {
    assemble_at_internal(source, time, TextureInputs::Named(textures))
}

/// Assemble a display-list source into a unified RDRAM image. Texture statements are diagnosed
/// (use [`assemble_with_texture`] for those).
pub fn assemble(source: &str) -> Result<Image, Vec<Diag>> {
    assemble_at(source, 0.0, None).map(|a| a.image)
}

/// Assemble a display list source that references a texture, embedding the RGBA8 pixel data into
/// RDRAM as big-endian RGBA16 (5/5/5/1). The `tex_addr` field of the returned `Image` points to
/// the start of the texture data in `rdram`.
pub fn assemble_with_texture(
    source: &str,
    rgba8: &[u8],
    tex_w: u32,
    tex_h: u32,
) -> Result<Image, Vec<Diag>> {
    assemble_at(source, 0.0, Some((rgba8, tex_w, tex_h))).map(|a| a.image)
}

/// Emit one command statement's word(s) into `rdram`, resolving symbol/segment operands. Texture
/// statements require a texture context (`ctx.tex`); without one they reproduce the plain-path
/// diagnostic that `assemble()` has always emitted.
fn emit_stmt(rdram: &mut Vec<u8>, s: &Stmt, line: usize, ctx: &EmitCtx, diags: &mut Vec<Diag>) {
    match s {
        Stmt::Vtx(_)
        | Stmt::Viewport(_)
        | Stmt::Mtx(_)
        | Stmt::Texture(_)
        | Stmt::Lights(_)
        | Stmt::LookAt(_)
        | Stmt::VtxSet { .. }
        | Stmt::Morph(_)
        | Stmt::GfxBlockStart(_)
        | Stmt::GfxBlockEnd => {}
        Stmt::SpMatrix { name, flags } => match ctx.mtx_addr.get(name) {
            Some(&addr) => {
                let (w0, w1) = gsp_matrix(addr, flags.proj, flags.load, flags.push);
                push_word(rdram, w0, w1);
            }
            None => diags.push(Diag {
                line,
                msg: format!("unknown matrix name: {name}"),
            }),
        },
        Stmt::SpViewport => {
            let (w0, w1) = gsp_viewport(ctx.vp_addr);
            push_word(rdram, w0, w1);
        }
        Stmt::SpPerspNormalize { name } => match ctx.persp_norm.get(name) {
            Some(&pn) => {
                let (w0, w1) = gsp_persp_normalize(pn);
                push_word(rdram, w0, w1);
            }
            None => {
                let msg = if ctx.mtx_addr.contains_key(name) {
                    format!("gsSPPerspNormalize: `{name}` is not a perspective matrix")
                } else {
                    format!("gsSPPerspNormalize: unknown matrix name: {name}")
                };
                diags.push(Diag { line, msg });
            }
        },
        Stmt::SpSetGeometryMode(bits) => {
            let (w0, w1) = gsp_set_geometrymode(*bits);
            push_word(rdram, w0, w1);
        }
        Stmt::SpClearGeometryMode(bits) => {
            let (w0, w1) = gsp_clear_geometrymode(*bits);
            push_word(rdram, w0, w1);
        }
        Stmt::SpVertex { n, v0 } => {
            let (w0, w1) = gsp_vertex(*v0, *n, ctx.vtx_addr);
            push_word(rdram, w0, w1);
        }
        Stmt::Sp1Triangle { v0, v1, v2 } => {
            let (w0, w1) = gsp_1triangle(*v0, *v1, *v2);
            push_word(rdram, w0, w1);
        }
        Stmt::Sp2Triangles {
            v0,
            v1,
            v2,
            v3,
            v4,
            v5,
        } => {
            let (w0, w1) = gsp_2triangles(*v0, *v1, *v2, *v3, *v4, *v5);
            push_word(rdram, w0, w1);
        }
        Stmt::SpPopMatrix { num } => {
            let (w0, w1) = gsp_popmatrix(*num);
            push_word(rdram, w0, w1);
        }
        Stmt::SpDisplayList { target } => {
            if let Some(addr) = resolve_addr(target, ctx, line, diags) {
                let (w0, w1) = gsp_displaylist(addr);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::SpBranchList { target } => {
            if let Some(addr) = resolve_addr(target, ctx, line, diags) {
                let (w0, w1) = gsp_branchlist(addr);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::SpSegment { seg, base } => {
            if let Some(addr) = resolve_addr(base, ctx, line, diags) {
                let (w0, w1) = gsp_segment(*seg, addr);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::SpSetLights { name, num_dir } => {
            use crate::hle::consts::rsp_f3dex2::{
                G_MOVEMEM, G_MOVEWORD, G_MV_LIGHT, G_MWO_NUMLIGHT, G_MW_NUMLIGHT,
            };
            match ctx.light_addr.get(name) {
                Some(&base) => {
                    let n = *num_dir as usize;
                    // 1. gSPNumLights: MOVEWORD G_MW_NUMLIGHT, w1 = n*24
                    let w0_num = ((G_MOVEWORD as u32) << 24)
                        | ((G_MW_NUMLIGHT as u32) << 16)
                        | (G_MWO_NUMLIGHT as u32);
                    push_word(rdram, w0_num, (n as u32) * 24);
                    // 2. Directional lights: N MOVEMEM commands
                    for k in 0..n {
                        let slot = (k as u32 + 2) * 3;
                        let w0 = ((G_MOVEMEM as u32) << 24) | (slot << 8) | (G_MV_LIGHT as u32);
                        push_word(rdram, w0, base + (k as u32) * 16);
                    }
                    // 3. Ambient light MOVEMEM
                    let amb_slot = (n as u32 + 2) * 3;
                    let w0_amb = ((G_MOVEMEM as u32) << 24) | (amb_slot << 8) | (G_MV_LIGHT as u32);
                    push_word(rdram, w0_amb, base + (n as u32) * 16);
                }
                None => diags.push(Diag {
                    line,
                    msg: format!("unknown lights name: {name}"),
                }),
            }
        }
        Stmt::SpLookAt { name } => {
            use crate::hle::consts::rsp_f3dex2::{G_MOVEMEM, G_MV_LIGHT};
            match ctx.lookat_addr.get(name) {
                Some(&base) => {
                    // slot 0 (S): p0(8,8)=0, w1=base; slot 1 (T): p0(8,8)=3 (=(1*24)>>3), w1=base+16.
                    // slot 0 contributes `0 << 8` (a no-op, elided here to satisfy clippy::identity_op).
                    let w0_s = ((G_MOVEMEM as u32) << 24) | (G_MV_LIGHT as u32);
                    push_word(rdram, w0_s, base);
                    let w0_t = ((G_MOVEMEM as u32) << 24) | (3u32 << 8) | (G_MV_LIGHT as u32);
                    push_word(rdram, w0_t, base + 16);
                }
                None => diags.push(Diag {
                    line,
                    msg: format!("unknown lookat name: {name}"),
                }),
            }
        }
        Stmt::DpSetRenderMode { mode1, mode2 } => {
            let (w0, w1) = gdp_set_render_mode(*mode1, *mode2);
            push_word(rdram, w0, w1);
        }
        Stmt::DpSetOtherModeL {
            shift,
            length,
            data,
        } => {
            let (w0, w1) = gdp_set_other_mode_l(*shift, *length, *data);
            push_word(rdram, w0, w1);
        }
        Stmt::DpSetFogColor { rgba } => {
            let (w0, w1) = gdp_set_fog_color(*rgba);
            push_word(rdram, w0, w1);
        }
        Stmt::DpSetBlendColor { rgba } => {
            let (w0, w1) = gdp_set_blend_color(*rgba);
            push_word(rdram, w0, w1);
        }
        Stmt::SpFogPosition { min, max } => {
            let (w0, w1) = gsp_fog_position(*min, *max);
            push_word(rdram, w0, w1);
        }
        Stmt::SpEndDisplayList => {
            let (w0, w1) = gsp_enddl();
            push_word(rdram, w0, w1);
        }
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
        } => match ctx.tex.and_then(|t| t.get(tex_name)) {
            Some(&addr) => {
                // CI8 (fmt=2, siz=1) and CI4 (fmt=2, siz=0): prepend 4 TLUT-load commands before
                // the standard 7. SetTextureImage (palette addr) → LoadSync → LoadTLUT → PipeSync.
                if *fmt == 2 && (*siz == 0 || *siz == 1) {
                    match ctx.ci_pal.get(tex_name) {
                        Some(&(pal_addr, pal_count)) => {
                            // lrt encodes (count-1) in 10.2 fixed-point units (<<2); lands in bits[11:0].
                            // HLE recovers: count = (lrt>>2)+1 = pal_count. ✓
                            let lrt = (pal_count - 1) << 2;
                            let (w0, w1) = gdp_set_texture_image(0, 2, 1, pal_addr);
                            push_word(rdram, w0, w1);
                            let (w0, w1) = gdp_load_sync();
                            push_word(rdram, w0, w1);
                            let (w0, w1) = gdp_load_tlut(7, lrt);
                            push_word(rdram, w0, w1);
                            let (w0, w1) = gdp_pipe_sync();
                            push_word(rdram, w0, w1);
                        }
                        None => {
                            diags.push(Diag {
                                line,
                                msg: format!("CI palette data missing for texture: {tex_name}"),
                            });
                            return;
                        }
                    }
                }
                // Standard 7-command texture block (SetTextureImage + SetTile + Load* etc.).
                for (w0, w1) in gdp_load_texture_block(
                    *fmt, *siz, *width, *height, addr, *cmt, *maskt, *cms, *masks,
                ) {
                    push_word(rdram, w0, w1);
                }
            }
            None if ctx.tex.is_none() => diags.push(Diag {
                line,
                msg: "texture statements require assemble_with_texture()".into(),
            }),
            None => diags.push(Diag {
                line,
                msg: format!("unknown texture name: {tex_name}"),
            }),
        },
        Stmt::SpTexture {
            sc,
            tc,
            level,
            tile,
            on,
        } => {
            if ctx.tex.is_none() {
                diags.push(Diag {
                    line,
                    msg: "texture statements require assemble_with_texture()".into(),
                });
            } else {
                let (w0, w1) = gsp_texture(*sc, *tc, *level, *tile, *on);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetOtherModeH {
            shift,
            length,
            data,
        } => {
            if ctx.tex.is_none() {
                diags.push(Diag {
                    line,
                    msg: "texture statements require assemble_with_texture()".into(),
                });
            } else {
                let (w0, w1) = gdp_set_other_mode_h(*shift, *length, *data);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetCombineLerp {
            c0a,
            c0b,
            c0c,
            c0d,
            a0a,
            a0b,
            a0c,
            a0d,
            c1a,
            c1b,
            c1c,
            c1d,
            a1a,
            a1b,
            a1c,
            a1d,
        } => {
            if ctx.tex.is_none() {
                diags.push(Diag {
                    line,
                    msg: "texture statements require assemble_with_texture()".into(),
                });
            } else {
                let c0 = CcPass {
                    a: *c0a,
                    b: *c0b,
                    c: *c0c,
                    d: *c0d,
                };
                let a0 = CcPass {
                    a: *a0a,
                    b: *a0b,
                    c: *a0c,
                    d: *a0d,
                };
                let c1 = CcPass {
                    a: *c1a,
                    b: *c1b,
                    c: *c1c,
                    d: *c1d,
                };
                let a1 = CcPass {
                    a: *a1a,
                    b: *a1b,
                    c: *a1c,
                    d: *a1d,
                };
                let (w0, w1) = gdp_set_combine_lerp(c0, a0, c1, a1);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetPrimColor {
            minlevel,
            lodfrac,
            rgba,
        } => {
            if ctx.tex.is_none() {
                diags.push(Diag {
                    line,
                    msg: "texture statements require assemble_with_texture()".into(),
                });
            } else {
                let (w0, w1) = gdp_set_prim_color(*minlevel, *lodfrac, *rgba);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetEnvColor { rgba } => {
            if ctx.tex.is_none() {
                diags.push(Diag {
                    line,
                    msg: "texture statements require assemble_with_texture()".into(),
                });
            } else {
                let (w0, w1) = gdp_set_env_color(*rgba);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetColorImage {
            fmt,
            siz,
            width,
            addr,
        } => {
            if let Some(a) = resolve_addr(addr, ctx, line, diags) {
                let (w0, w1) = gdp_set_color_image(*fmt, *siz, *width, a);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetDepthImage { addr } => {
            if let Some(a) = resolve_addr(addr, ctx, line, diags) {
                let (w0, w1) = gdp_set_depth_image(a);
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetScissor {
            mode,
            ulx,
            uly,
            lrx,
            lry,
        } => {
            let (w0, w1) = gdp_set_scissor(*mode, *ulx, *uly, *lrx, *lry);
            push_word(rdram, w0, w1);
        }
        Stmt::DpSetFillColor { rgba } => {
            let (w0, w1) = gdp_set_fill_color(*rgba);
            push_word(rdram, w0, w1);
        }
        Stmt::DpFillRectangle { ulx, uly, lrx, lry } => {
            let (w0, w1) = gdp_fill_rectangle(*ulx, *uly, *lrx, *lry);
            push_word(rdram, w0, w1);
        }
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
            flip,
        } => {
            for (w0, w1) in gsp_texture_rectangle(
                *ulx, *uly, *lrx, *lry, *tile, *uls, *ult, *dsdx, *dtdy, *flip,
            ) {
                push_word(rdram, w0, w1);
            }
        }
        Stmt::DpSetTextureImage {
            fmt,
            siz,
            width,
            addr,
        } => {
            if let Some(a) = resolve_addr(addr, ctx, line, diags) {
                let (w0, w1) = gdp_set_texture_image_std(*fmt, *siz, *width, a);
                push_word(rdram, w0, w1);
            }
        }
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
        } => {
            let (w0, w1) = gdp_set_tile(
                *fmt, *siz, *rd_line, *tmem, *tile, *palette, *cmt, *maskt, *shiftt, *cms, *masks,
                *shifts,
            );
            push_word(rdram, w0, w1);
        }
        Stmt::DpSetTileSize {
            tile,
            uls,
            ult,
            lrs,
            lrt,
        } => {
            let (w0, w1) = gdp_set_tile_size(*tile, *uls, *ult, *lrs, *lrt);
            push_word(rdram, w0, w1);
        }
    }
}

#[cfg(test)]
mod asm_tests {
    use super::*;

    #[test]
    fn assemble_with_texture_embeds_rgba16_and_emits_block() {
        let src = "Texture tex = { 2, 1, RGBA16 }\ngsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 2, 1)\ngsSPEndDisplayList()\n";
        // 2x1 RGBA8 input: red, blue
        let rgba = [255, 0, 0, 255, 0, 0, 255, 255];
        let img = assemble_with_texture(src, &rgba, 2, 1).unwrap();
        // RGBA16 big-endian: red=0xF801, blue=0x003F (5/5/5/1, a=1)
        let a = img.tex_addr as usize;
        assert_eq!(&img.rdram[a..a + 4], &[0xF8, 0x01, 0x00, 0x3F]);
        // first emitted block command is SetTextureImage(width=1) pointing at tex_addr
        let e = img.entry_addr as usize;
        let w0 = u32::from_be_bytes(img.rdram[e..e + 4].try_into().unwrap());
        assert_eq!(w0, 0xFD10_0000);
    }

    #[test]
    fn perspective_mtx_bakes_and_persp_normalize_emits_coefficient() {
        // proj declared via perspective(); gsSPPerspNormalize(proj) must emit the matrix's perspNorm.
        let src = "Mtx proj = perspective(90, 1, 1, 2, 1)\n\
                   gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)\n\
                   gsSPPerspNormalize(proj)\n\
                   gsSPEndDisplayList()\n";
        let img = assemble(src).unwrap();
        let (m, pn) = crate::asm::gu::gu_perspective(90.0, 1.0, 1.0, 2.0, 1.0);
        assert_eq!(pn, 43690);
        // perspNorm command is the SECOND emitted word-pair (after gsSPMatrix). entry_addr points at
        // gsSPMatrix; +8 is gsSPPerspNormalize.
        let e = img.entry_addr as usize;
        let w0 = u32::from_be_bytes(img.rdram[e + 8..e + 12].try_into().unwrap());
        let w1 = u32::from_be_bytes(img.rdram[e + 12..e + 16].try_into().unwrap());
        assert_eq!((w0, w1), crate::asm::encode::gsp_persp_normalize(pn));
        // sanity: the matrix is the gu_perspective bytes at proj_addr (data: default Vp(16) then Mtx).
        let want = crate::asm::encode::mtx_to_bytes(m);
        let proj_addr = 16usize;
        assert_eq!(&img.rdram[proj_addr..proj_addr + 64], &want[..]);
    }

    #[test]
    fn persp_normalize_on_non_perspective_matrix_diagnoses() {
        let src = "Mtx m = identity()\n\
                   gsSPPerspNormalize(m)\n\
                   gsSPEndDisplayList()\n";
        let err = assemble(src).unwrap_err();
        assert!(err
            .iter()
            .any(|d| d.msg.contains("not a perspective matrix")));
    }

    #[test]
    fn perspective_with_near_equal_far_diagnoses_non_finite() {
        // near == far => (near+far)/(near-far) divides by zero => non-finite matrix. The declared
        // initializer must diagnose this (like the update-override path), not silently bake garbage.
        let src = "Mtx p = perspective(45, 1.333, 10, 10, 1)\n\
                   gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)\n\
                   gsSPEndDisplayList()\n";
        let err = assemble(src).unwrap_err();
        assert!(
            err.iter()
                .any(|d| d.msg.contains("non-finite") && d.msg.contains("p")),
            "expected a non-finite-matrix diagnostic for `p`, got: {err:?}"
        );
    }

    #[test]
    fn well_formed_perspective_still_assembles_ok() {
        // Regression guard: a valid perspective must still assemble cleanly.
        let src = "Mtx p = perspective(45, 1.3333, 10, 1000, 1)\n\
                   gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)\n\
                   gsSPEndDisplayList()\n";
        assert!(
            assemble(src).is_ok(),
            "well-formed perspective must assemble"
        );
    }

    #[test]
    fn lights_dir_negative_component_emits_correct_s8_byte() {
        // A light with dir.x = -100 must encode byte @8 as i8 == -100 (0x9C as u8).
        let src = "\
Lights lneg = { dir(-100, 0, 0) col(255, 255, 255); ambient(0, 0, 0) }
Gfx main[] = {
  gsSPSetLights(lneg)
  gsSPEndDisplayList()
}
";
        let img = crate::asm::assemble(src).unwrap();
        let la = img.light_addr as usize;
        // dir bytes @8..10: dir.x = -100 → byte value 0x9C (as i8 == -100).
        assert_eq!(img.rdram[la + 8] as i8, -100, "dir.x s8 round-trip");
        assert_eq!(img.rdram[la + 9] as i8, 0, "dir.y zero");
        assert_eq!(img.rdram[la + 10] as i8, 0, "dir.z zero");
    }

    #[test]
    fn vtx_normal_form_emits_s8_normal_in_bytes_12_13_14() {
        // `VtxN { x,y,z, flag, s,t, nx,ny,nz, a }` — last 4 bytes = s8 normal + u8 alpha.
        let src = "VtxN { 10, 20, 30, 0, 0, 0, -1, 2, 127, 255 }\ngsSPEndDisplayList()\n";
        let img = crate::asm::assemble(src).unwrap();
        // The single Vtx data block sits at vtx_addr; read its 16 bytes.
        let o = img.vtx_addr as usize;
        let b = &img.rdram[o..o + 16];
        assert_eq!(&b[0..2], &10i16.to_be_bytes()); // x
        assert_eq!(&b[4..6], &30i16.to_be_bytes()); // z
        assert_eq!(b[12] as i8, -1); // nx (s8)
        assert_eq!(b[13] as i8, 2); // ny
        assert_eq!(b[14] as i8, 127); // nz
        assert_eq!(b[15], 255); // alpha
    }

    #[test]
    fn sp_set_lights_emits_block_and_command_stream() {
        let src = "\
Lights l1 = { dir(127, 0, 0) col(255, 128, 64); ambient(16, 32, 48) }
Gfx main[] = {
  gsSPSetLights(l1)
  gsSPEndDisplayList()
}
";
        let img = crate::asm::assemble(src).unwrap();
        // Directional Light_t: col@0..2, pad@3, colc@4..6 (copy), pad@7, dir s8 @8..10, 5 pad bytes @11..15.
        let la = img.light_addr as usize;
        assert_eq!(&img.rdram[la..la + 3], &[255, 128, 64]); // col
        assert_eq!(&img.rdram[la + 4..la + 7], &[255, 128, 64]); // colc (copy)
        assert_eq!(img.rdram[la + 8] as i8, 127); // dir.x
        assert_eq!(img.rdram[la + 9] as i8, 0);
        assert_eq!(img.rdram[la + 10] as i8, 0);
        // Ambient_t at la+16: col@0..2, pad@3, colc@4..6.
        assert_eq!(&img.rdram[la + 16..la + 19], &[16, 32, 48]);
        assert_eq!(&img.rdram[la + 20..la + 23], &[16, 32, 48]); // colc

        // Command stream at entry_addr: decode into (w0,w1) pairs.
        let e = img.entry_addr as usize;
        // The block has 3 words: NUMLIGHT + MOVEMEM dir + MOVEMEM ambient + ENDDL
        // Read all words up to and including ENDDL.
        let mut words: Vec<(u32, u32)> = Vec::new();
        let mut off = e;
        loop {
            let w0 = u32::from_be_bytes(img.rdram[off..off + 4].try_into().unwrap());
            let w1 = u32::from_be_bytes(img.rdram[off + 4..off + 8].try_into().unwrap());
            words.push((w0, w1));
            off += 8;
            if (w0 >> 24) as u8 == crate::hle::consts::rsp_f3dex2::G_ENDDL {
                break;
            }
        }

        // gSPNumLights(1): MOVEWORD G_MW_NUMLIGHT, w1 = 1*24 = 24.
        assert!(
            words.iter().any(|(w0, w1)| (w0 >> 24) as u8
                == crate::hle::consts::rsp_f3dex2::G_MOVEWORD
                && ((w0 >> 16) & 0xff) as u8 == crate::hle::consts::rsp_f3dex2::G_MW_NUMLIGHT
                && *w1 == 24),
            "NUMLIGHT command not found in stream: {words:?}"
        );
        // Directional MOVEMEM: index G_MV_LIGHT in p0(0,8), slot offset (0+2)*3=6 in p0(8,8),
        // and w1 = the physical light-block base (slot 0 → la + 0*16).
        assert!(
            words.iter().any(|(w0, w1)| (w0 >> 24) as u8
                == crate::hle::consts::rsp_f3dex2::G_MOVEMEM
                && (w0 & 0xff) as u8 == crate::hle::consts::rsp_f3dex2::G_MV_LIGHT
                && ((w0 >> 8) & 0xff) == 6
                && *w1 == la as u32),
            "directional MOVEMEM not found: {words:?}"
        );
        // Ambient MOVEMEM: slot offset (1+2)*3=9, w1 = la + n*16 = la + 16 (n=1 directional).
        assert!(
            words.iter().any(|(w0, w1)| (w0 >> 24) as u8
                == crate::hle::consts::rsp_f3dex2::G_MOVEMEM
                && (w0 & 0xff) as u8 == crate::hle::consts::rsp_f3dex2::G_MV_LIGHT
                && ((w0 >> 8) & 0xff) == 9
                && *w1 == la as u32 + 16),
            "ambient MOVEMEM not found: {words:?}"
        );
    }

    #[test]
    fn sp_lookat_emits_block_and_two_movemem() {
        let src = "\
LookAt la = lookat_reflect(0, 0, 100, 0, 0, 0, 0, 1, 0)
Gfx main[] = {
  gsSPLookAt(la)
  gsSPEndDisplayList()
}
";
        let img = crate::asm::assemble(src).unwrap();
        // Eye on +Z looking at origin, up +Y: Look=norm(eye-at)=(0,0,1); Right=norm(up×Look)=(1,0,0);
        // Up'=Look×Right=(0,1,0). S-axis=Right→s8 (127,0,0); T-axis=Up'→s8 (0,127,0).
        let la = img.lookat_addr as usize;
        assert_eq!(img.rdram[la + 8] as i8, 127); // S dir.x
        assert_eq!(img.rdram[la + 9] as i8, 0);
        assert_eq!(img.rdram[la + 10] as i8, 0);
        assert_eq!(img.rdram[la + 16 + 8] as i8, 0); // T dir.x
        assert_eq!(img.rdram[la + 16 + 9] as i8, 127); // T dir.y
                                                       // Command stream: 2 MOVEMEM(G_MV_LIGHT), slot0 (p0(8,8)=0, w1=la) + slot1 (p0(8,8)=3, w1=la+16).
                                                       // Decode (w0,w1) from entry to G_ENDDL — inline loop copied verbatim from
                                                       // sp_set_lights_emits_block_and_command_stream (asm.rs:881-891); there is NO test_words helper.
        let e = img.entry_addr as usize;
        let mut words: Vec<(u32, u32)> = Vec::new();
        let mut off = e;
        loop {
            let w0 = u32::from_be_bytes(img.rdram[off..off + 4].try_into().unwrap());
            let w1 = u32::from_be_bytes(img.rdram[off + 4..off + 8].try_into().unwrap());
            words.push((w0, w1));
            off += 8;
            if (w0 >> 24) as u8 == crate::hle::consts::rsp_f3dex2::G_ENDDL {
                break;
            }
        }
        let mv = crate::hle::consts::rsp_f3dex2::G_MOVEMEM as u32;
        let ml = crate::hle::consts::rsp_f3dex2::G_MV_LIGHT as u32;
        assert!(words.iter().any(|(w0, w1)| (w0 >> 24) == mv
            && (w0 & 0xff) == ml
            && ((w0 >> 8) & 0xff) == 0
            && *w1 == la as u32));
        assert!(words.iter().any(|(w0, w1)| (w0 >> 24) == mv
            && (w0 & 0xff) == ml
            && ((w0 >> 8) & 0xff) == 3
            && *w1 == la as u32 + 16));
    }

    #[test]
    fn morph_lerps_positions_and_renormalizes_at_t_half() {
        // cube vert A=(40,0,0) normal +X ; sphere vert B=(0,40,0) normal +Y. weight=(1-cos(time))/2.
        // time = PI/2 -> cos=0 -> w=0.5 -> pos=(20,20,0); normal = normalize(lerp(+X,+Y)) ~ (0.707,0.707,0).
        let src = "\
VtxSet a = { VtxN { 40, 0, 0, 0, 0, 0, 127, 0, 0, 255 } }
VtxSet b = { VtxN { 0, 40, 0, 0, 0, 0, 0, 127, 0, 255 } }
morph verts = lerp(a, b, (1 - cos(time)) / 2)
Gfx main[] = { gsSPVertex(verts, 1, 0) gsSP1Triangle(0,0,0,0) gsSPEndDisplayList() }
";
        let asm = crate::asm::assemble_at(src, std::f32::consts::FRAC_PI_2, None).unwrap();
        assert!(
            asm.is_time_variant,
            "morph weight reads time -> must be time-variant"
        );
        let v = asm.image.vtx_addr as usize;
        let x = i16::from_be_bytes([asm.image.rdram[v], asm.image.rdram[v + 1]]);
        let y = i16::from_be_bytes([asm.image.rdram[v + 2], asm.image.rdram[v + 3]]);
        assert!(
            (x - 20).abs() <= 1 && (y - 20).abs() <= 1,
            "pos lerp: got ({x},{y})"
        );
        let nx = asm.image.rdram[v + 12] as i8;
        let ny = asm.image.rdram[v + 13] as i8;
        assert!(
            (nx as i32 - 90).abs() <= 3 && (ny as i32 - 90).abs() <= 3,
            "normal renorm ~0.707*127=90: got ({nx},{ny})"
        );
    }

    #[test]
    fn source_with_morph_is_time_variant() {
        let src = "VtxSet a = { VtxN { 0,0,0,0,0,0,127,0,0,255 } }\nVtxSet b = { VtxN { 1,0,0,0,0,0,127,0,0,255 } }\nmorph v = lerp(a, b, (1 - cos(time)) / 2)\n";
        assert!(crate::asm::source_is_time_variant(src));
    }

    // ---- 2D / framebuffer round-trip tests (Task 5) ----

    #[test]
    fn roundtrip_fill_rectangle() {
        // CIMG-first DL: SetColorImage + SetFillColor + FillRectangle
        let src = "Gfx main[] = {\n\
            gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 320, 0x00100000)\n\
            gsDPSetFillColor(0xCAFECAFE)\n\
            gsDPFillRectangle(0, 0, 1280, 960)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        let pairs = &r.scene.framebuffer_pairs;
        assert_eq!(pairs.len(), 1, "expected 1 framebuffer pair");
        let pair = &pairs[0];
        // Check color image fields from SETCIMG
        assert_eq!(pair.color_image.fmt, 0, "fmt should be RGBA(0)");
        assert_eq!(pair.color_image.siz, 2, "siz should be 16b(2)");
        assert_eq!(pair.color_image.width, 320);
        assert_eq!(pair.color_image.addr, 0x00100000);
        // Check FillRect op
        assert_eq!(pair.ops.len(), 1);
        match &pair.ops[0] {
            crate::hle::SceneOp::FillRect { rect, color_raw } => {
                assert_eq!(*color_raw, 0xCAFECAFE);
                assert_eq!(rect.ulx, 0);
                assert_eq!(rect.uly, 0);
                assert_eq!(rect.lrx, 320); // 1280 >> 2
                assert_eq!(rect.lry, 240); // 960 >> 2
            }
            other => panic!("expected FillRect, got {:?}", other),
        }
        // stmt_word_count for DpFillRectangle must be 1
        let fill_stmt = crate::asm::parser::Stmt::DpFillRectangle {
            ulx: 0,
            uly: 0,
            lrx: 1280,
            lry: 960,
        };
        assert_eq!(stmt_word_count(&fill_stmt), 1);
    }

    #[test]
    fn roundtrip_texture_rectangle() {
        // CIMG-first DL: SetColorImage + TextureRectangle (3 GBI words)
        let src = "Gfx main[] = {\n\
            gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 320, 0x00100000)\n\
            gsSPTextureRectangle(0, 0, 1280, 960, 0, 44, 52, 1024, 512)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        // stmt_word_count for SpTextureRectangle must be 3 (the layout-desync guard)
        let tex_rect_stmt = crate::asm::parser::Stmt::SpTextureRectangle {
            ulx: 0,
            uly: 0,
            lrx: 1280,
            lry: 960,
            tile: 0,
            uls: 44,
            ult: 52,
            dsdx: 1024,
            dtdy: 512,
            flip: false,
        };
        assert_eq!(stmt_word_count(&tex_rect_stmt), 3);
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        let pairs = &r.scene.framebuffer_pairs;
        assert_eq!(pairs.len(), 1);
        match &pairs[0].ops[0] {
            crate::hle::SceneOp::TexRect {
                rect,
                uls,
                ult,
                dsdx,
                dtdy,
                flip,
                ..
            } => {
                assert_eq!(rect.lrx, 320); // 1280 >> 2
                assert_eq!(rect.lry, 240); // 960 >> 2
                assert_eq!(rect.ulx, 0);
                assert_eq!(rect.uly, 0);
                assert_eq!(*uls, 44);
                assert_eq!(*ult, 52);
                assert_eq!(*dsdx, 1024);
                assert_eq!(*dtdy, 512);
                assert!(!*flip);
            }
            other => panic!("expected TexRect, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_texture_rectangle_flip() {
        // gsSPTextureRectangleFlip: same as TexRect but flip=true
        let src = "Gfx main[] = {\n\
            gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 320, 0x00100000)\n\
            gsSPTextureRectangleFlip(0, 0, 1280, 960, 0, 11, 13, 1024, 512)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        match &r.scene.framebuffer_pairs[0].ops[0] {
            crate::hle::SceneOp::TexRect {
                flip,
                uls,
                ult,
                dsdx,
                dtdy,
                ..
            } => {
                assert!(*flip, "flip should be true for TextureRectangleFlip");
                assert_eq!(*uls, 11);
                assert_eq!(*ult, 13);
                assert_eq!(*dsdx, 1024);
                assert_eq!(*dtdy, 512);
            }
            other => panic!("expected TexRect (flip), got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_set_color_and_depth_image() {
        // SETCIMG + SETZIMG + FILLRECT → pair.color_image and pair.depth_image
        let src = "Gfx main[] = {\n\
            gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 320, 0x00100000)\n\
            gsDPSetDepthImage(0x00200000)\n\
            gsDPSetFillColor(0xFFFFFFFF)\n\
            gsDPFillRectangle(0, 0, 1280, 960)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        let pair = &r.scene.framebuffer_pairs[0];
        assert_eq!(pair.color_image.addr, 0x00100000);
        assert_eq!(pair.depth_image, Some(0x00200000));
    }

    #[test]
    fn roundtrip_set_scissor() {
        // SETCIMG + SETSCISSOR (before first draw) → active_scissor on pair
        let src = "Gfx main[] = {\n\
            gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 320, 0x00100000)\n\
            gsDPSetScissor(0, 0, 0, 1280, 960)\n\
            gsDPSetFillColor(0xCAFECAFE)\n\
            gsDPFillRectangle(0, 0, 1280, 960)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
        assert!(r.diags.is_empty(), "diags: {:?}", r.diags);
        let pair = &r.scene.framebuffer_pairs[0];
        // SETSCISSOR before the pair opens → captured in active_scissor (no SetScissor op pushed)
        assert_eq!(pair.active_scissor.lrx, 320); // 1280 >> 2
        assert_eq!(pair.active_scissor.lry, 240); // 960 >> 2
        assert_eq!(pair.active_scissor.mode, 0);
        // Only the FillRect op (no SetScissor op because scissor was set before pair opened)
        assert_eq!(pair.ops.len(), 1);
        assert!(matches!(pair.ops[0], crate::hle::SceneOp::FillRect { .. }));
    }

    #[test]
    fn roundtrip_standalone_timg_tile_tilesize() {
        // Standalone gsDPSetTextureImage, gsDPSetTile, gsDPSetTileSize: check emitted opcode bytes.
        let src = "Gfx main[] = {\n\
            gsDPSetTextureImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 32, 0x00050000)\n\
            gsDPSetTile(G_IM_FMT_RGBA, G_IM_SIZ_16b, 8, 0, G_TX_RENDERTILE, 0, 2, 5, 0, 2, 5, 0)\n\
            gsDPSetTileSize(G_TX_RENDERTILE, 0, 0, 124, 124)\n\
            gsSPEndDisplayList()\n\
        }";
        let img = crate::asm::assemble(src).unwrap();
        assert!(
            img.rdram.len() >= 32,
            "must emit at least 4 commands (3 + ENDDL)"
        );
        let e = img.entry_addr as usize;
        // Word 0: SetTextureImage RGBA16 width=32 → (0xFD10_001F, 0x00050000)
        let w0 = u32::from_be_bytes(img.rdram[e..e + 4].try_into().unwrap());
        assert_eq!(w0 >> 24, 0xFD, "SetTextureImage opcode");
        // Word 1: SetTile RGBA16 render → (0xF510_1000, 0x0009_4250)
        let w0b = u32::from_be_bytes(img.rdram[e + 8..e + 12].try_into().unwrap());
        assert_eq!(w0b >> 24, 0xF5, "SetTile opcode");
        // Word 2: SetTileSize → (0xF200_0000, 0x0007_C07C)
        let w0c = u32::from_be_bytes(img.rdram[e + 16..e + 20].try_into().unwrap());
        assert_eq!(w0c >> 24, 0xF2, "SetTileSize opcode");
    }
}
