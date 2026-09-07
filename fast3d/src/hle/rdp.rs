use crate::diag::{DiagKind, Diagnostic};
use crate::hle::consts::rdp::{
    G_LOADBLOCK, G_LOADTILE, G_LOADTLUT, G_NOOP, G_RDPFULLSYNC, G_RDPHALF_1, G_RDPHALF_2,
    G_RDPLOADSYNC, G_RDPPIPESYNC, G_RDPSETOTHERMODE, G_RDPTILESYNC, G_SETBLENDCOLOR, G_SETCIMG,
    G_SETCOMBINE, G_SETCONVERT, G_SETENVCOLOR, G_SETFILLCOLOR, G_SETFOGCOLOR, G_SETKEYGB,
    G_SETKEYR, G_SETPRIMCOLOR, G_SETPRIMDEPTH, G_SETSCISSOR, G_SETTILE, G_SETTILESIZE, G_SETZIMG,
};
use crate::hle::interp::{checked_span, memory_try, read_bytes_exact};
use crate::hle::interp::{Cmd, Ctx, Handler};
use crate::hle::mem::Rdram;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyChannel {
    pub center: u8,
    pub scale: u8,
    pub width: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileDescriptor {
    pub uls: u16,
    pub ult: u16,
    pub lrs: u16,
    pub lrt: u16,
    pub width: u16,
    pub height: u16,
    // G_SETTILE fields (parsed in set_tile). cms/cmt: 0=WRAP 1=MIRROR 2=CLAMP.
    pub fmt: u8,
    pub siz: u8,
    pub palette: u8,
    pub cms: u8,
    pub cmt: u8,
    pub masks: u8,
    pub maskt: u8,
    pub shifts: u8,
    pub shiftt: u8,
    pub line: u16,
    pub tmem_addr: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rdp {
    pub tmem: Vec<u8>, // linear source bytes from LoadBlock (used by CI index decode + the load gate)
    /// Hardware-faithful, byte-addressable TMEM (populated by LoadBlock's `write_block` and LoadTLUT's
    /// `write_tlut`; sampled per tile). The single source of truth for the palette (TLUT) region:
    /// both the faithful CI sampler and the legacy linear `decode_ci*` fallback read palette bytes
    /// from its upper half via `tmem_bank.palette()`. `tmem.is_empty()` remains the "did a LoadBlock
    /// run" gate for the linear index bytes.
    pub tmem_bank: crate::hle::tmem::Tmem,
    /// True when `tmem_bank` was last populated by G_LOADTILE (`write_tile`), false after
    /// G_LOADBLOCK. LoadTile's genuine per-row stride lets the sampler read it faithfully for any
    /// width; LoadBlock's contiguous rows gate the faithful path on word-alignment. Read by
    /// `decode_tile_texture` to decide faithful-vs-legacy.
    pub load_via_tile: bool,
    pub tiles: [TileDescriptor; 8],
    pub tex_image: (u8, u8, u16, u64), // fmt, siz, width, addr (mirrored from RSP)
    pub combine_l: u32,                // = w0
    pub combine_h: u32,                // = w1
    pub other_mode_h: u32,             // authoritative; cycle type at (h>>20)&3
    // Zero encodes G_TC_CONV, but an unwritten TEXTCONV field keeps legacy lists renderable.
    pub texture_conversion_set: bool,
    pub other_mode_l: u32, // RDP othermode low word: blender mux + z-mode + render flags
    pub fog_color: [u8; 4], // G_SETFOGCOLOR RGBA8
    pub fog_mul: i16,      // gSPFogPosition fm
    pub fog_offset: i16,   // gSPFogPosition fo
    pub prim: [u8; 4],
    pub prim_depth: crate::scene::PrimitiveDepth,
    pub convert: [i16; 6],
    pub key: [KeyChannel; 3],
    /// LOD fraction captured from G_SETPRIMCOLOR w0 low byte. On N64 hardware the
    /// primitive LOD fraction = lodFrac / 256.0. Feeds the combiner PRIM_LOD_FRAC selector. Default 0.0.
    pub prim_lod_frac: f32,
    /// Primitive min LOD level from G_SETPRIMCOLOR w0 bits[8:12]. On N64 hardware the min LOD = lodMin / 32.0
    /// (the RDP uses only 5 of the 8 documented lodMin bits). Default 0.0.
    pub prim_min_level: f32,
    pub env: [u8; 4],
    /// Blend color RGBA8 — set by G_SETBLENDCOLOR (0xF9). Used by CLR_BL blender selector and
    /// THRESHOLD alpha-compare (alpha_threshold = blend_color[3] / 255). Default [0,0,0,255].
    pub blend_color: [u8; 4],
    /// TLUT entry format from othermode_H TT (0=NONE, 2=RGBA16, 3=IA16). Diagnostic; decode reads it inline.
    pub tlut_fmt: u8,
    // --- 2D / framebuffer state (set by G_SETCIMG / G_SETZIMG / G_SETSCISSOR / G_SETFILLCOLOR) ---
    /// Current color framebuffer target (decoded from G_SETCIMG; width = raw_field+1).
    pub color_image: crate::hle::rsp::ColorImage,
    /// True when `color_image` was updated since last pair-recording flush.
    pub color_changed: bool,
    /// Current depth buffer address (unmasked resolve of G_SETZIMG w1).
    pub depth_image: u64,
    /// True when `depth_image` was updated since last pair-recording flush.
    pub depth_changed: bool,
    /// Current scissor rectangle in pixels (decoded from G_SETSCISSOR 10.2 fields).
    pub scissor: crate::hle::rsp::Scissor,
    /// Raw G_SETFILLCOLOR word (stored verbatim; interpreted by fill-rect recording).
    pub fill_color_raw: u32,
}

impl Rdp {
    /// G_MDSFT_TEXTLOD (othermode_h bit 16): true = G_TL_LOD (LOD / mipmapping enabled). N64
    /// othermode_h layout.
    pub fn lod_enable(&self) -> bool {
        (self.other_mode_h >> 16) & 1 != 0
    }

    /// G_MDSFT_TEXTDETAIL (othermode_h bits [18:17], 2 bits): bit0 = sharpen, bit1 = detail. N64
    /// othermode_h layout.
    pub fn text_detail(&self) -> u8 {
        ((self.other_mode_h >> 17) & 3) as u8
    }
}

pub(crate) fn install_defaults<M: Rdram>(t: &mut [Handler<M>; 256]) {
    t[G_NOOP as usize] = no_op::<M>;
    t[G_SETCOMBINE as usize] = set_combine::<M>;
    t[G_SETTILE as usize] = set_tile::<M>;
    t[G_SETTILESIZE as usize] = set_tile_size::<M>;
    t[G_LOADBLOCK as usize] = load_block::<M>;
    t[G_LOADTILE as usize] = load_tile::<M>;
    t[G_LOADTLUT as usize] = load_tlut::<M>;
    t[G_SETPRIMCOLOR as usize] = set_prim_color::<M>;
    t[G_SETPRIMDEPTH as usize] = set_prim_depth::<M>;
    t[G_SETCONVERT as usize] = set_convert::<M>;
    t[G_SETKEYR as usize] = set_key_r::<M>;
    t[G_SETKEYGB as usize] = set_key_gb::<M>;
    t[G_SETENVCOLOR as usize] = set_env_color::<M>;
    t[G_SETFOGCOLOR as usize] = set_fog_color::<M>;
    t[G_SETBLENDCOLOR as usize] = set_blend_color::<M>;
    for op in [G_RDPLOADSYNC, G_RDPPIPESYNC, G_RDPTILESYNC, G_RDPFULLSYNC] {
        t[op as usize] = sync::<M>;
    }
    t[G_RDPSETOTHERMODE as usize] = rdp_set_other_mode::<M>;
    t[G_SETCIMG as usize] = set_color_image::<M>;
    t[G_SETZIMG as usize] = set_depth_image::<M>;
    t[G_SETSCISSOR as usize] = set_scissor::<M>;
    t[G_SETFILLCOLOR as usize] = set_fill_color::<M>;
    // RDPHALF continuation words are consumed INLINE by the walk loop's rect decode (never
    // table-dispatched in a well-formed DL). If one reaches the table, the rect word-count
    // desynced — surface it loudly instead of silently no-oping (Task 3, Step 2 canary).
    t[G_RDPHALF_1 as usize] = rdp_half::<M>;
    t[G_RDPHALF_2 as usize] = rdp_half::<M>;
}

fn no_op<M: Rdram>(_c: &Cmd, _cx: &mut Ctx<M>) {}
fn sync<M: Rdram>(_c: &Cmd, _cx: &mut Ctx<M>) {}

fn rdp_half<M: Rdram>(_c: &Cmd, cx: &mut Ctx<M>) {
    cx.diags.push(Diagnostic {
        at: cx.pc,
        kind: DiagKind::StrayRdphalf,
    });
}

fn rdp_set_other_mode<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // G_RDPSETOTHERMODE: w0 low 24 bits → other_mode_h field; w1 → other_mode_l.
    cx.rdp.other_mode_h = c.w0 & 0x00FF_FFFF;
    cx.rdp.texture_conversion_set = true;
    cx.rdp.other_mode_l = c.w1;
    cx.rsp.material_dirty = true;
}

fn set_combine<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.combine_l = c.w0;
    cx.rdp.combine_h = c.w1;
    cx.rsp.material_dirty = true;
}

fn set_prim_color<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // w0 carries the primitive LOD fields: low byte = lodFrac (p0(0,8) → /256), bits
    // [8:12] = lodMin (p0(8,5); the RDP uses 5 of the 8 documented bits → /32). w1 = RGBA8 prim
    // color. Previously w0 was discarded; capture it faithfully.
    cx.rdp.prim_lod_frac = c.p0(0, 8) as f32 / 256.0;
    cx.rdp.prim_min_level = c.p0(8, 5) as f32 / 32.0;
    cx.rdp.prim = c.w1.to_be_bytes();
    cx.rsp.material_dirty = true;
}

fn set_env_color<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.env = c.w1.to_be_bytes();
    cx.rsp.material_dirty = true;
}

fn set_prim_depth<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.prim_depth = crate::scene::PrimitiveDepth {
        z: (c.w1 >> 16) as u16,
        dz: c.w1 as u16,
    };
}

fn set_convert<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.convert = [
        c.p0(13, 9),
        c.p0(4, 9),
        (c.p0(0, 4) << 5) | c.p1(27, 5),
        c.p1(18, 9),
        c.p1(9, 9),
        c.p1(0, 9),
    ]
    .map(|value| ((value as i16) << 7) >> 7);
    cx.rsp.material_dirty = true;
}

fn set_key_r<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.key[0] = KeyChannel {
        center: c.p1(8, 8) as u8,
        scale: c.p1(0, 8) as u8,
        width: c.p1(16, 12) as u16,
    };
    cx.rsp.material_dirty = true;
}

fn set_key_gb<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.key[1] = KeyChannel {
        center: c.p1(24, 8) as u8,
        scale: c.p1(16, 8) as u8,
        width: c.p0(12, 12) as u16,
    };
    cx.rdp.key[2] = KeyChannel {
        center: c.p1(8, 8) as u8,
        scale: c.p1(0, 8) as u8,
        width: c.p0(0, 12) as u16,
    };
    cx.rsp.material_dirty = true;
}

fn set_fog_color<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // Fog color has no texture dependency, so material_dirty stays unchanged.
    cx.rdp.fog_color = c.w1.to_be_bytes();
}

fn set_blend_color<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // G_SETBLENDCOLOR (0xF9): w1 is the RGBA8 blend color packed as a big-endian u32.
    // Used by the CLR_BL blender selector and THRESHOLD alpha-compare (Phase D).
    cx.rdp.blend_color = c.w1.to_be_bytes();
    cx.rsp.material_dirty = true;
}

fn set_tile<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let tile = c.p1(24, 3) as usize;
    let t = &mut cx.rdp.tiles[tile];
    t.siz = c.p0(19, 2) as u8;
    t.fmt = c.p0(21, 3) as u8;
    t.line = c.p0(9, 9) as u16;
    t.tmem_addr = c.p0(0, 9) as u16;
    t.palette = c.p1(20, 4) as u8;
    t.cmt = c.p1(18, 2) as u8;
    t.maskt = c.p1(14, 4) as u8;
    t.shiftt = c.p1(10, 4) as u8;
    t.cms = c.p1(8, 2) as u8;
    t.masks = c.p1(4, 4) as u8;
    t.shifts = c.p1(0, 4) as u8;
    cx.rsp.material_dirty = true;
}

fn set_tile_size<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let tile = c.p1(24, 3) as usize;
    let t = &mut cx.rdp.tiles[tile];
    t.uls = c.p0(12, 12) as u16;
    t.ult = c.p0(0, 12) as u16;
    t.lrs = c.p1(12, 12) as u16;
    t.lrt = c.p1(0, 12) as u16;
    t.width = ((i32::from(t.lrs) - i32::from(t.uls)).div_euclid(4) + 1).max(1) as u16;
    t.height = ((i32::from(t.lrt) - i32::from(t.ult)).div_euclid(4) + 1).max(1) as u16;
    cx.rsp.material_dirty = true;
}

fn load_block<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // contiguous, linear: N64 RDP hardware word count.
    let (_fmt, siz, _w, addr) = cx.rdp.tex_image;
    let tile_idx = c.p1(24, 3) as usize; // load tile (7 for a well-formed LoadTextureBlock)
    let uls = c.p0(12, 12);
    let lrs = c.p1(12, 12);
    let dxt = c.p1(0, 12); // CALC_DXT accumulator increment
                           // saturating_sub guards malformed input (uls > lrs) against u32 underflow/panic.
    let words = (lrs.saturating_sub(uls) >> (4 - siz as u32)) + 1; // RGBA16 siz=2
    let bytes = (words as usize) << 3; // 8 bytes/word (siz<=2)
    let src = memory_try!(cx, Texture, read_bytes_exact(cx.mem, addr, bytes)).into_owned();

    // The load tile's `tmem`/`line` set the faithful write's destination base and DXT row stride
    // (both 0 for a well-formed LoadTextureBlock); the linear copy still feeds the CI decode path
    // and the `tmem.is_empty()` gate.
    let dst_words = cx.rdp.tiles[tile_idx].tmem_addr as usize;
    let line_words = cx.rdp.tiles[tile_idx].line as usize;
    cx.rdp
        .tmem_bank
        .write_block(&src, dst_words, line_words, dxt, words as usize, siz);
    cx.rdp.tmem = src;
    cx.rdp.load_via_tile = false; // contiguous rows: faithful path gated on word-aligned rows.
    cx.rsp.material_dirty = true;
}

fn load_tile<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    // G_LOADTILE (0xF4): row-by-row load of a strided sub-rectangle of the current tex_image into
    // TMEM. Params (w0: uls/ult, w1: tile/lrs/lrt) are 10.2
    // fixed point; integer texel coords are the fields >> 2.
    let (_fmt, siz, width, addr) = cx.rdp.tex_image;
    let tile_idx = c.p1(24, 3) as usize; // load tile (7 for a well-formed LoadTextureTile)
    let uls = c.p0(12, 12) >> 2;
    let ult = c.p0(0, 12) >> 2;
    let lrs = c.p1(12, 12) >> 2;
    let lrt = c.p1(0, 12) >> 2;

    // saturating_sub guards malformed input (uls > lrs / ult > lrt) against u32 underflow/panic.
    let tile_width = lrs.saturating_sub(uls);
    let row_count = 1 + lrt.saturating_sub(ult);
    let words_per_row = (tile_width >> (4 - siz as u32)) + 1;

    // Source geometry: bytesPerRow is the tex-image row stride; textureStart offsets into it by the
    // upper-left texel. LoadTile reads a sub-rectangle, so the source region spans row_count rows,
    // each `bytes_per_row` apart, but only `words_per_row` words are consumed per row.
    // `tex_image.width` is the RAW G_SETTIMG field (actual width - 1); the load path needs
    // the ACTUAL width (`p0(0,12) + 1`), so add 1 before deriving the byte stride.
    let actual_width = width as u32 + 1;
    let bytes_per_row = (actual_width << siz) >> 1;
    let bytes_offset = (uls << siz) >> 1;
    let offset = u64::from(bytes_offset) + u64::from(bytes_per_row) * u64::from(ult);
    let texture_start = memory_try!(cx, Texture, checked_span(addr, offset));
    let src_len = (row_count as usize - 1) * bytes_per_row as usize + words_per_row as usize * 8;
    let src = memory_try!(
        cx,
        Texture,
        read_bytes_exact(cx.mem, texture_start, src_len)
    )
    .into_owned();

    // Dest: the load tile's `tmem`/`line` set the destination base and the padded per-row stride.
    let dst_words = cx.rdp.tiles[tile_idx].tmem_addr as usize;
    let line_words = cx.rdp.tiles[tile_idx].line as usize;
    cx.rdp.tmem_bank.write_tile(
        &src,
        dst_words,
        line_words,
        row_count as usize,
        words_per_row as usize,
        bytes_per_row as usize,
        siz,
    );
    // Keep the linear source for the `tmem.is_empty()` gate; mark the padded-row (LoadTile) path so
    // `decode_tile_texture` samples via the faithful bank even for sub-word widths.
    cx.rdp.tmem = src;
    cx.rdp.load_via_tile = true;
    cx.rsp.material_dirty = true;
}

fn load_tlut<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    if c.w0 & 0x00ff_ffff != 0 || c.w1 & 0x3fff != 0 {
        cx.diags.push(Diagnostic {
            at: cx.pc,
            kind: DiagKind::UnsupportedCommandParameters { opcode: G_LOADTLUT },
        });
        return;
    }
    let addr = cx.rdp.tex_image.3;
    let count = c.p1(14, 10) as usize + 1;
    let packed_bytes = count * 2;
    let packed = memory_try!(cx, Tlut, read_bytes_exact(cx.mem, addr, packed_bytes));
    let tile = c.p1(24, 3) as usize;
    let dst_word = usize::from(cx.rdp.tiles[tile].tmem_addr);
    cx.rdp.tmem_bank.write_tlut(&packed, count, dst_word);
    cx.rsp.material_dirty = true;
}

fn set_color_image<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let fmt = c.p0(21, 3) as u8;
    let siz = c.p0(19, 2) as u8;
    let width = (c.p0(0, 12) as u16) + 1;
    let addr = memory_try!(cx, Command, cx.mem.resolve(c.w1_addr));
    let new = crate::hle::rsp::ColorImage {
        fmt,
        siz,
        width,
        addr,
    };
    if cx.rdp.color_image != new {
        cx.rdp.color_image = new;
        cx.rdp.color_changed = true;
    }
}

fn set_depth_image<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    let addr = memory_try!(cx, Command, cx.mem.resolve(c.w1_addr));
    if addr != cx.rdp.depth_image {
        cx.rdp.depth_image = addr;
        cx.rdp.depth_changed = true;
    }
}

fn set_scissor<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.scissor = crate::hle::rsp::Scissor {
        ulx: (c.p0(12, 12) as i32) >> 2,
        uly: (c.p0(0, 12) as i32) >> 2,
        mode: c.p1(24, 2) as u8,
        lrx: (c.p1(12, 12) as i32) >> 2,
        lry: (c.p1(0, 12) as i32) >> 2,
    };
}

fn set_fill_color<M: Rdram>(c: &Cmd, cx: &mut Ctx<M>) {
    cx.rdp.fill_color_raw = c.w1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Diagnostic;
    use crate::hle::mem::RdramImage;
    use crate::hle::rsp::{Rsp, Scene};

    fn default_handler<M: Rdram>(_c: &Cmd, _cx: &mut Ctx<M>) {}

    fn run_cmd(rdram_bytes: &[u8], rdp_in: Rdp, w0: u32, w1: u32) -> (Rdp, Vec<Diagnostic>) {
        let mut t: [Handler<RdramImage<'_>>; 256] =
            [default_handler::<RdramImage<'_>> as Handler<RdramImage<'_>>; 256];
        install_defaults(&mut t);
        let mut rdram = RdramImage::new(rdram_bytes);
        let mut rsp = Rsp::default();
        let mut rdp = rdp_in;
        let mut scene = Scene::default();
        let mut diags = Vec::new();
        let mut rec = crate::hle::rsp::PairRec::default();
        let mut dropped = 0u32;
        let mut seen = [false; 256];
        let cmd = Cmd {
            w0,
            w1,
            w1_addr: w1 as u64,
        };
        let mut cx = Ctx {
            rsp: &mut rsp,
            rdp: &mut rdp,
            mem: &mut rdram,
            scene: &mut scene,
            diags: &mut diags,
            pc: 0,
            gbi_consts: crate::hle::gbi::GbiUcode::F3dex2.constants(),
            rec: &mut rec,
            dropped_runs: &mut dropped,
            unknown_seen: &mut seen,
        };
        t[cmd.opcode() as usize](&cmd, &mut cx);
        (rdp, diags)
    }

    #[test]
    fn tile_size_subtracts_origin() {
        for (ul, lr, expected) in [
            ([12, 20], [24, 28], [4, 3]),
            ([3, 7], [6, 14], [1, 2]),
            ([20, 40], [4, 0], [1, 1]),
        ] {
            let (rdp, diags) = run_cmd(
                &[],
                Rdp::default(),
                0xf200_0000 | (ul[0] << 12) | ul[1],
                (lr[0] << 12) | lr[1],
            );
            assert!(diags.is_empty());
            let tile = &rdp.tiles[0];
            assert_eq!([tile.width, tile.height], expected);
            assert_eq!(
                [tile.uls, tile.ult, tile.lrs, tile.lrt],
                [ul[0] as u16, ul[1] as u16, lr[0] as u16, lr[1] as u16]
            );
        }
    }

    #[test]
    fn set_combine_stores_l_equals_w0_h_equals_w1() {
        let (rdp, diags) = run_cmd(&[], Rdp::default(), 0xFC12_7E24, 0xFFFF_F9FC);
        assert!(diags.is_empty());
        assert_eq!(rdp.combine_l, 0xFC12_7E24); // L = w0
        assert_eq!(rdp.combine_h, 0xFFFF_F9FC); // H = w1
    }

    #[test]
    fn prim_env_decode_be() {
        // SetPrimColor white (0xFA000000 / 0xFFFFFFFF)
        let (rdp, _) = run_cmd(&[], Rdp::default(), 0xFA00_0000, 0xFFFF_FFFF);
        assert_eq!(rdp.prim, [255, 255, 255, 255]);
        // SetEnvColor opaque black (0xFB000000 / 0x000000FF)
        let (rdp2, _) = run_cmd(&[], Rdp::default(), 0xFB00_0000, 0x0000_00FF);
        assert_eq!(rdp2.env, [0, 0, 0, 255]);
    }

    #[test]
    fn set_prim_color_captures_lod_frac_and_min_level() {
        // G_SETPRIMCOLOR: lodFrac = p0(0,8) → prim LOD fraction = lodFrac/256; lodMin = p0(8,5) (5 bits)
        // → prim min LOD = lodMin/32. w0 = G_SETPRIMCOLOR<<24 | lodMin<<8 | lodFrac.
        // lodFrac = 128 (→ 0.5), lodMin = 5 (→ 5/32 = 0.15625).
        let w0 = (0xFAu32 << 24) | (5u32 << 8) | 128u32;
        let (rdp, _) = run_cmd(&[], Rdp::default(), w0, 0xFFFF_FFFF);
        assert_eq!(rdp.prim_lod_frac, 128.0 / 256.0);
        assert_eq!(rdp.prim_min_level, 5.0 / 32.0);
        // The upper 3 bits of the documented 8-bit lodMin field are ignored (RDP uses 5).
        let w0_hi = (0xFAu32 << 24) | (0xFFu32 << 8); // lodMin field all-ones, lodFrac 0
        let (rdp2, _) = run_cmd(&[], Rdp::default(), w0_hi, 0);
        assert_eq!(
            rdp2.prim_min_level,
            31.0 / 32.0,
            "only low 5 lodMin bits used"
        );
    }

    #[test]
    fn othermode_lod_helpers_read_textlod_and_textdetail() {
        // G_MDSFT_TEXTLOD at bit16 (G_TL_LOD); G_MDSFT_TEXTDETAIL at bits[18:17] (2 bits).
        let rdp = Rdp {
            other_mode_h: (1 << 16) | (0b10 << 17),
            ..Rdp::default()
        };
        assert!(rdp.lod_enable());
        assert_eq!(rdp.text_detail(), 0b10);
        // Both off by default.
        assert!(!Rdp::default().lod_enable());
        assert_eq!(Rdp::default().text_detail(), 0);
    }

    #[test]
    fn set_tile_size_fills_render_tile() {
        // gdp_set_tile_size(0, 0, 0, 124, 124) = (0xF200_0000, 0x0007_C07C)
        let (rdp, _) = run_cmd(&[], Rdp::default(), 0xF200_0000, 0x0007_C07C);
        assert_eq!(rdp.tiles[0].lrs, 124);
        assert_eq!(rdp.tiles[0].lrt, 124);
    }

    #[test]
    fn load_block_byte_count_matches_hardware_formula() {
        // siz=2 (RGBA16), lrs=1021 (NOT a multiple of 4 after +1) -> hardware words=256, bytes=2048.
        // The naive (lrs+1)*2 = 2044 would diverge; assert the hardware count.
        let rdp = Rdp {
            tex_image: (0, 2, 1, 0),
            ..Rdp::default()
        }; // fmt=0, siz=2 (RGBA16), addr=0
        let rdram_bytes = vec![0u8; 4096];
        // LoadBlock: tile=7, uls=0, ult=0, lrs=1021, dxt=0
        // w0 = shiftl(G_LOADBLOCK, 24, 8) | shiftl(0, 12, 12) | shiftl(0, 0, 12)
        //    = 0xF3000000
        // w1 = shiftl(7, 24, 3) | shiftl(1021, 12, 12) | shiftl(0, 0, 12)
        //    = (7 << 24) | (1021 << 12)
        let w0 = 0xF300_0000u32;
        let w1 = (7u32 << 24) | (1021u32 << 12);
        let (rdp, diags) = run_cmd(&rdram_bytes, rdp, w0, w1);
        assert!(diags.is_empty());
        assert_eq!(rdp.tmem.len(), 2048);
    }

    #[test]
    fn set_tile_parses_all_fields() {
        // fmt=3(IA), siz=1(8b), line=8, tmem=0, tile=0, palette=5,
        // cmt=WRAP(0) maskt=5 shiftt=0, cms=MIRROR(1) masks=5 shifts=0
        // w0 = G_SETTILE<<24 | fmt<<21 | siz<<19 | line<<9 | tmem(0)
        let w0 = (0xF5u32 << 24) | (3 << 21) | (1 << 19) | (8 << 9);
        // w1 = tile(0)<<24 | palette<<20 | cmt(0)<<18 | maskt<<14 | shiftt(0)<<10 | cms<<8 | masks<<4 | shifts(0)
        let w1 = (5u32 << 20) | (5 << 14) | (1 << 8) | (5 << 4);
        let (rdp, diags) = run_cmd(&[], Rdp::default(), w0, w1);
        assert!(diags.is_empty());
        let t = &rdp.tiles[0];
        assert_eq!((t.fmt, t.siz), (3, 1));
        assert_eq!(t.palette, 5);
        assert_eq!((t.cms, t.cmt), (1, 0));
        assert_eq!((t.masks, t.maskt), (5, 5));
        assert_eq!((t.shifts, t.shiftt), (0, 0));
        assert_eq!((t.line, t.tmem_addr), (8, 0));
    }

    fn palette_state(tile: usize, destination: u16) -> Rdp {
        let mut rdp = Rdp {
            tex_image: (0, 2, 1, 0),
            ..Rdp::default()
        };
        rdp.tiles[tile].tmem_addr = destination;
        rdp.tmem_bank.write_block(&[0x55; 4096], 0, 0, 0, 512, 1);
        rdp
    }

    #[test]
    fn tlut_replicates_four_halfwords() {
        let (rdp, diags) = run_cmd(
            &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22],
            palette_state(7, 0x100),
            0xf000_0000,
            0x0700_c000,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            &rdp.tmem_bank.palette()[..32],
            &[
                0xaa, 0xbb, 0xaa, 0xbb, 0xaa, 0xbb, 0xaa, 0xbb, 0xcc, 0xdd, 0xcc, 0xdd, 0xcc, 0xdd,
                0xcc, 0xdd, 0xee, 0xff, 0xee, 0xff, 0xee, 0xff, 0xee, 0xff, 0x11, 0x22, 0x11, 0x22,
                0x11, 0x22, 0x11, 0x22,
            ],
        );
        assert!(rdp.tmem_bank.palette()[32..].iter().all(|&b| b == 0x55));
        let alias = TileDescriptor {
            fmt: 3,
            siz: 2,
            tmem_addr: 0x100,
            width: 4,
            height: 1,
            ..TileDescriptor::default()
        };
        assert_eq!(
            rdp.tmem_bank.sample_tile(&alias, 0).unwrap(),
            [0xaa, 0xaa, 0xaa, 0xbb].repeat(4)
        );
    }

    #[test]
    fn tlut_ci4_bank15_partial_update() {
        let (mut rdp, diags) = run_cmd(
            &[0xf8, 0x01].repeat(16),
            palette_state(6, 0x1f0),
            0xf000_0000,
            0x0603_c000,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let before = rdp.tmem_bank.palette().to_vec();
        rdp.tiles[3].tmem_addr = 0x1f5;
        let (mut rdp, diags) = run_cmd(&[0x07, 0xc1, 0x00, 0x3f], rdp, 0xf000_0000, 0x0300_4000);
        assert!(diags.is_empty(), "{diags:?}");
        let palette = rdp.tmem_bank.palette();
        assert_eq!(&palette[..0x7a8], &before[..0x7a8]);
        assert_eq!(&palette[0x7b8..], &before[0x7b8..]);
        assert_eq!(
            &palette[0x7a8..0x7b8],
            &[
                0x07, 0xc1, 0x07, 0xc1, 0x07, 0xc1, 0x07, 0xc1, 0x00, 0x3f, 0x00, 0x3f, 0x00, 0x3f,
                0x00, 0x3f
            ]
        );
        rdp.tmem_bank
            .write_block(&[0x45, 0x6f, 0, 0, 0, 0, 0, 0], 0, 0, 0, 1, 1);
        let tile = TileDescriptor {
            fmt: 2,
            siz: 0,
            palette: 15,
            width: 4,
            height: 1,
            ..TileDescriptor::default()
        };
        assert_eq!(
            rdp.tmem_bank.sample_tile(&tile, 2).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255,]
        );
    }

    #[test]
    fn tlut_ci8_256_entries() {
        let source: Vec<_> = (0..256u16).flat_map(|i| [i as u8, 255 - i as u8]).collect();
        let (mut rdp, diags) = run_cmd(&source, palette_state(5, 0x100), 0xf000_0000, 0x053f_c000);
        assert!(diags.is_empty(), "{diags:?}");
        for (i, word) in rdp
            .tmem_bank
            .palette()
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
        {
            assert_eq!(word, &[i as u8, 255 - i as u8].repeat(4)[..]);
        }
        let indices: Vec<_> = (0..=255).collect();
        rdp.tmem_bank.write_block(&indices, 0, 0, 0, 32, 1);
        let tile = TileDescriptor {
            fmt: 2,
            siz: 1,
            width: 256,
            height: 1,
            ..TileDescriptor::default()
        };
        let pixels = rdp.tmem_bank.sample_tile(&tile, 3).unwrap();
        for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(*pixel, [i as u8, i as u8, i as u8, 255 - i as u8]);
        }
    }

    #[test]
    fn tlut_rgba16_and_ia16() {
        let (rdp, diags) = run_cmd(
            &[0x00, 0x01, 0xf8, 0x01, 0x07, 0xc1],
            palette_state(7, 0x100),
            0xf000_0000,
            0x0700_8000,
        );
        assert!(diags.is_empty(), "{diags:?}");
        let palette = rdp.tmem_bank.palette();
        assert_eq!(
            crate::hle::texdec::decode_ci8(&[0, 1, 2], 3, 1, palette, 2),
            [0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255]
        );
        assert_eq!(
            crate::hle::texdec::decode_ci8(&[0, 1, 2], 3, 1, palette, 3),
            [0, 0, 0, 1, 248, 248, 248, 1, 7, 7, 7, 193]
        );
    }

    #[test]
    fn tlut_wraps_tmem_without_clamping_count() {
        let source: Vec<_> = (0..1024u16).flat_map(u16::to_be_bytes).collect();
        for (word, count) in [(0x07ff_c000, 1024), (0x0740_0000, 257)] {
            let (rdp, diags) = run_cmd(&source, palette_state(7, 0x1ff), 0xf000_0000, word);
            assert!(diags.is_empty(), "{diags:?}");
            let tile = TileDescriptor {
                fmt: 3,
                siz: 2,
                width: 2048,
                height: 1,
                ..TileDescriptor::default()
            };
            let pixels = rdp.tmem_bank.sample_tile(&tile, 0).unwrap();
            for i in 0..512 {
                let value = if count == 1024 {
                    Some(if i == 511 { 512 } else { i + 513 })
                } else if i == 511 {
                    Some(0)
                } else if i < 256 {
                    Some(i + 1)
                } else {
                    None
                };
                let [hi, lo] = value.map(|v| (v as u16).to_be_bytes()).unwrap_or([0x55; 2]);
                assert_eq!(
                    &pixels[i * 16..(i + 1) * 16],
                    [hi, hi, hi, lo].repeat(4),
                    "word {i}, count {count}"
                );
            }
        }
    }

    #[test]
    fn tlut_legacy_row_fields_diagnosed() {
        for (w0, w1) in [
            (0xf000_0001, 0x0700_0000),
            (0xf000_1000, 0x0700_0000),
            (0xf000_0000, 0x0700_0001),
            (0xf000_0000, 0x0700_000c),
            (0xf000_0000, 0x0700_1000),
            (0xf000_0000, 0x0700_2000),
        ] {
            let before = palette_state(7, 0x100);
            let (after, diags) = run_cmd(&[], before.clone(), w0, w1);
            assert_eq!(after, before);
            assert_eq!(
                diags,
                [Diagnostic {
                    at: 0,
                    kind: DiagKind::UnsupportedCommandParameters { opcode: 0xf0 }
                }]
            );
            assert_eq!(diags[0].kind.severity(), crate::diag::Severity::Error);
        }
        let (rdp, diags) = run_cmd(
            &[0x12, 0x34],
            palette_state(7, 0x100),
            0xf000_0000,
            0x0700_0000,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(&rdp.tmem_bank.palette()[..8], [0x12, 0x34].repeat(4));
        assert!(rdp.tmem_bank.palette()[8..].iter().all(|&b| b == 0x55));
    }

    #[test]
    fn set_other_mode_l_writes_rendermode_field() {
        // gsDPSetRenderMode-style write encoded as command FIELDS (BLO1): logical shift=3, length=29
        // pack to shift_field = 32-3-29 = 0, len_field = 29-1 = 28. Passing the LOGICAL (3, 29) here
        // would make `off = 32 - 3 - 30` u32-underflow-panic before the assert. data = 0x00442078.
        let mut rsp = Rsp::default();
        let mut rdp = Rdp::default();
        rsp.set_other_mode_l(0, 28, 0x0044_2078, &mut rdp);
        assert_eq!(rdp.other_mode_l, 0x0044_2078);
    }

    #[test]
    fn rdp_set_other_mode_writes_both_words() {
        // G_RDPSETOTHERMODE (0xEF): w0[23:0]→other_mode_h, w1→other_mode_l.
        let (rdp, diags) = run_cmd(&[], Rdp::default(), 0xEF00_0A01, 0x0044_2078);
        assert!(diags.is_empty());
        assert_eq!(rdp.other_mode_l, 0x0044_2078);
        assert_eq!(rdp.other_mode_h & 0x00FF_FFFF, 0x0000_0A01);
    }

    #[test]
    fn load_block_linear_decode_two_rows() {
        // 32x32 RGBA16 LoadBlock -> linear RGBA8 decode.
        // Guards the conscious linear-decode / swizzle-cancellation decision.
        // Row 0 texel 0: red RGBA16 = 0xF801 (r=31, g=0, b=0, a=1)
        // Row 1 texel 0: blue RGBA16 = 0x003F (r=0, g=0, b=31, a=1)
        let mut src = vec![0u8; 2048]; // 32*32*2 bytes
        src[0] = 0xF8;
        src[1] = 0x01;
        // row 1 texel 0 at offset 32*2 = 64
        src[64] = 0x00;
        src[65] = 0x3F;

        let rdp = Rdp {
            tex_image: (0, 2, 1, 0),
            ..Rdp::default()
        }; // siz=2, addr=0
           // LoadBlock: lrs=1023, dxt=256 (standard 32x32 RGBA16)
           // w0 = 0xF3000000, w1 = (7 << 24) | (1023 << 12) | 256
        let w0 = 0xF300_0000u32;
        let w1 = (7u32 << 24) | (1023u32 << 12) | 256u32;
        let (rdp, diags) = run_cmd(&src, rdp, w0, w1);
        assert!(diags.is_empty());
        assert_eq!(rdp.tmem.len(), 2048);
        let decoded = crate::hle::combiner::decode_rgba16(&rdp.tmem);

        // row 0 texel 0: 0xF8,0x01 -> r5=31,g5=0,b5=0,a1=1 -> R=255,G=0,B=0,A=255
        // 5-bit expand: (31<<3)|(31>>2) = 248|7 = 255
        assert_eq!(&decoded[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
        // row 1 texel 0 at offset 32*4: 0x00,0x3F -> r5=0,g5=0,b5=31,a1=1 -> R=0,G=0,B=255,A=255
        // b5=31: (31<<3)|(31>>2) = 248|7 = 255
        assert_eq!(&decoded[32 * 4..32 * 4 + 4], &[0x00, 0x00, 0xFF, 0xFF]);
    }

    // --- 2D state handler tests (Task 2, Step 3) ---

    #[test]
    fn set_color_image_stores_fields_and_sets_changed() {
        // G_SETCIMG (0xFF): fmt=2, siz=1, width_field=9 (actual width=10), addr=0.
        // w0 = opcode<<24 | fmt<<21 | siz<<19 | width_field
        let w0 = (0xFFu32 << 24) | (2u32 << 21) | (1u32 << 19) | 9u32;
        let (rdp, diags) = run_cmd(&[], Rdp::default(), w0, 0);
        assert!(diags.is_empty());
        assert_eq!(
            rdp.color_image,
            crate::hle::rsp::ColorImage {
                fmt: 2,
                siz: 1,
                width: 10,
                addr: 0
            }
        );
        assert!(
            rdp.color_changed,
            "color_changed must be set on first write"
        );
    }

    #[test]
    fn set_color_image_diff_guard_no_change_on_same() {
        // Dispatch the same G_SETCIMG twice; second dispatch must NOT re-set color_changed
        // (the diff-guard prevents spurious pair boundaries).
        let w0 = (0xFFu32 << 24) | (2u32 << 21) | (1u32 << 19) | 9u32;
        let (mut rdp, _) = run_cmd(&[], Rdp::default(), w0, 0);
        rdp.color_changed = false; // reset after first write
        let (rdp2, _) = run_cmd(&[], rdp, w0, 0);
        assert!(
            !rdp2.color_changed,
            "diff-guard: color_changed must stay false on identical write"
        );
    }

    #[test]
    fn set_depth_image_stores_addr_and_sets_changed() {
        // G_SETZIMG (0xFE): addr=0x0010_0000.
        let w0 = 0xFE00_0000u32;
        let w1 = 0x0010_0000u32;
        let (rdp, diags) = run_cmd(&[], Rdp::default(), w0, w1);
        assert!(diags.is_empty());
        assert_eq!(rdp.depth_image, 0x0010_0000u64);
        assert!(
            rdp.depth_changed,
            "depth_changed must be set on first write"
        );
    }

    #[test]
    fn set_scissor_decodes_10p2_fields() {
        // G_SETSCISSOR (0xED): ulx=4 (raw=16), uly=8 (raw=32), mode=1,
        //   lrx=316 (raw=1264), lry=236 (raw=944).
        // w0 = opcode<<24 | ulx_raw<<12 | uly_raw<<0
        // w1 = mode<<24 | lrx_raw<<12 | lry_raw<<0
        let ulx_raw: u32 = 4 << 2; // 16
        let uly_raw: u32 = 8 << 2; // 32
        let lrx_raw: u32 = 316 << 2; // 1264
        let lry_raw: u32 = 236 << 2; // 944
        let mode: u32 = 1;
        let w0 = (0xEDu32 << 24) | (ulx_raw << 12) | uly_raw;
        let w1 = (mode << 24) | (lrx_raw << 12) | lry_raw;
        let (rdp, diags) = run_cmd(&[], Rdp::default(), w0, w1);
        assert!(diags.is_empty());
        assert_eq!(
            rdp.scissor,
            crate::hle::rsp::Scissor {
                ulx: 4,
                uly: 8,
                lrx: 316,
                lry: 236,
                mode: 1
            }
        );
    }

    #[test]
    fn set_fill_color_stores_raw_w1() {
        // G_SETFILLCOLOR (0xF7): w1 = 0xDEAD_BEEF.
        let w0 = 0xF700_0000u32;
        let w1 = 0xDEAD_BEEFu32;
        let (rdp, diags) = run_cmd(&[], Rdp::default(), w0, w1);
        assert!(diags.is_empty());
        assert_eq!(rdp.fill_color_raw, 0xDEAD_BEEF);
    }
}
