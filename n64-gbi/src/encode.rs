use crate::consts::*;

#[inline]
fn shiftl(value: u32, shift: u32, width: u32) -> u32 {
    (value & ((1u32 << width) - 1)) << shift
}

pub fn gdp_set_texture_image(fmt: u32, siz: u32, width: u32, addr: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETTIMG as u32, 24, 8)
        | shiftl(fmt, 21, 3)
        | shiftl(siz, 19, 2)
        | shiftl(width - 1, 0, 12);
    (w0, addr)
}

#[allow(clippy::too_many_arguments)]
pub fn gdp_set_tile(
    fmt: u32,
    siz: u32,
    line: u32,
    tmem: u32,
    tile: u32,
    palette: u32,
    cmt: u32,
    maskt: u32,
    shiftt: u32,
    cms: u32,
    masks: u32,
    shifts: u32,
) -> (u32, u32) {
    let w0 = shiftl(G_SETTILE as u32, 24, 8)
        | shiftl(fmt, 21, 3)
        | shiftl(siz, 19, 2)
        | shiftl(line, 9, 9)
        | shiftl(tmem, 0, 9);
    let w1 = shiftl(tile, 24, 3)
        | shiftl(palette, 20, 4)
        | shiftl(cmt, 18, 2)
        | shiftl(maskt, 14, 4)
        | shiftl(shiftt, 10, 4)
        | shiftl(cms, 8, 2)
        | shiftl(masks, 4, 4)
        | shiftl(shifts, 0, 4);
    (w0, w1)
}

pub fn gdp_set_tile_size(tile: u32, uls: u32, ult: u32, lrs: u32, lrt: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETTILESIZE as u32, 24, 8) | shiftl(uls, 12, 12) | shiftl(ult, 0, 12);
    let w1 = shiftl(tile, 24, 3) | shiftl(lrs, 12, 12) | shiftl(lrt, 0, 12);
    (w0, w1)
}

pub fn gdp_load_block(tile: u32, uls: u32, ult: u32, lrs: u32, dxt: u32) -> (u32, u32) {
    let w0 = shiftl(G_LOADBLOCK as u32, 24, 8) | shiftl(uls, 12, 12) | shiftl(ult, 0, 12);
    let w1 = shiftl(tile, 24, 3) | shiftl(lrs.min(2047), 12, 12) | shiftl(dxt, 0, 12);
    (w0, w1)
}

/// Load 1..1024 packed BE halfwords to the selected tile's TMEM word address.
/// `count_minus_one` occupies bits 14..23; `tile` occupies bits 24..26.
/// This replaces the old `(count - 1) << 2` argument and low-bit encoding.
pub fn gdp_load_tlut(tile: u32, count_minus_one: u32) -> (u32, u32) {
    let w0 = shiftl(G_LOADTLUT as u32, 24, 8);
    let w1 = shiftl(tile, 24, 3) | shiftl(count_minus_one, 14, 10);
    (w0, w1)
}

pub fn gdp_load_sync() -> (u32, u32) {
    (shiftl(G_RDPLOADSYNC as u32, 24, 8), 0)
}

pub fn gdp_pipe_sync() -> (u32, u32) {
    (shiftl(G_RDPPIPESYNC as u32, 24, 8), 0)
}

pub const ZERO_C: u32 = 31; // color ZERO mux index
pub const ZERO_A: u32 = 7; // alpha ZERO mux index

#[derive(Clone, Copy)]
pub struct CcPass {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
}

pub fn gdp_set_combine_lerp(c0: CcPass, a0: CcPass, c1: CcPass, a1: CcPass) -> (u32, u32) {
    let w0 = shiftl(G_SETCOMBINE as u32, 24, 8)
        | shiftl(c0.a, 20, 4)
        | shiftl(c0.c, 15, 5)
        | shiftl(a0.a, 12, 3)
        | shiftl(a0.c, 9, 3)
        | shiftl(c1.a, 5, 4)
        | shiftl(c1.c, 0, 5);
    let w1 = shiftl(c0.b, 28, 4)
        | shiftl(c0.d, 15, 3)
        | shiftl(a0.b, 12, 3)
        | shiftl(a0.d, 9, 3)
        | shiftl(c1.b, 24, 4)
        | shiftl(a1.a, 21, 3)
        | shiftl(a1.c, 18, 3)
        | shiftl(c1.d, 6, 3)
        | shiftl(a1.b, 3, 3)
        | shiftl(a1.d, 0, 3);
    (w0, w1) // full 32-bit w1; combine64 = (w1 << 32) | w0. NO shiftl(.., 0, 24) mask.
}

pub fn gdp_set_prim_color(minlevel: u32, lodfrac: u32, rgba: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETPRIMCOLOR as u32, 24, 8) | shiftl(minlevel, 8, 8) | shiftl(lodfrac, 0, 8);
    (w0, rgba)
}

pub fn gdp_set_env_color(rgba: u32) -> (u32, u32) {
    (shiftl(G_SETENVCOLOR as u32, 24, 8), rgba)
}

pub fn gdp_set_other_mode_h(shift: u32, length: u32, data: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETOTHERMODE_H as u32, 24, 8)
        | shiftl(32 - shift - length, 8, 8)
        | shiftl(length - 1, 0, 8);
    (w0, data)
}

pub fn gdp_set_other_mode_l(shift: u32, length: u32, data: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETOTHERMODE_L as u32, 24, 8)
        | shiftl(32 - shift - length, 8, 8)
        | shiftl(length - 1, 0, 8);
    (w0, data)
}

/// gsDPSetRenderMode(mode1, mode2): write the render-mode field (bits[3..31]) of other_mode_l.
pub fn gdp_set_render_mode(mode1: u32, mode2: u32) -> (u32, u32) {
    gdp_set_other_mode_l(3, 29, mode1 | mode2)
}

pub fn gdp_set_cycle_type(cyc: u32) -> (u32, u32) {
    gdp_set_other_mode_h(20 /* G_MDSFT_CYCLETYPE */, 2, cyc << 20)
}

pub fn gsp_texture(sc: u16, tc: u16, level: u32, tile: u32, on: bool) -> (u32, u32) {
    let w0 = shiftl(G_TEXTURE as u32, 24, 8)
        | shiftl(0, 16, 8)
        | shiftl(level, 11, 3)
        | shiftl(tile, 8, 3)
        | shiftl(on as u32, 1, 7);
    let w1 = shiftl(sc as u32, 16, 16) | shiftl(tc as u32, 0, 16);
    (w0, w1)
}

/// libultra gsDPLoadTextureBlock expansion. Internal SetTextureImage uses width=1
/// (field 0) -- this corrects spec 5.1's "width field=31" (that applies only to a
/// standalone gsDPSetTextureImage).
///
/// Format-correct row geometry (generalized from the old RGBA16-only `width*2`): `line_bytes =
/// (width << siz) >> 1` per texel row (floor for the CALC_DXT word count, ceil for the render tile
/// stride so an odd 4-bit width still spans a whole byte); `dxt = ceil(2048 / max(1, floor/8))`.
/// `lrs = texels - 1` in texel units; `load_block` recovers the word count via `>> (4 - siz)`.
/// For 32x32 RGBA16 this reproduces the pinned lr_s=1023 / dxt=256 / render-line=8 / tmem=0.
#[allow(clippy::too_many_arguments)]
pub fn gdp_load_texture_block(
    fmt: u32,
    siz: u32,
    width: u32,
    height: u32,
    addr: u32,
    cmt: u32,
    maskt: u32,
    cms: u32,
    masks: u32,
) -> Vec<(u32, u32)> {
    // Per-row bytes: floor for the CALC_DXT word count, ceil for the render tile stride.
    let line_bytes_floor = (width << siz) >> 1;
    let line_bytes_ceil = ((width << siz) + 1) >> 1;
    let render_line = (line_bytes_ceil + 7) >> 3; // 8 for 32x32 RGBA16
    let texels = width * height;
    let lrs = texels - 1; // 1023 for 32x32
    let txl2words = core::cmp::max(1, line_bytes_floor / 8);
    let dxt = (1u32 << 11).div_ceil(txl2words); // CALC_DXT -> 256 for 32x32 RGBA16
    vec![
        gdp_set_texture_image(fmt, siz, 1, addr), // width field = 0
        gdp_set_tile(fmt, siz, 0, 0, 7, 0, cmt, maskt, 0, cms, masks, 0), // LOADTILE
        gdp_load_sync(),
        gdp_load_block(7, 0, 0, lrs, dxt),
        gdp_pipe_sync(),
        gdp_set_tile(fmt, siz, render_line, 0, 0, 0, cmt, maskt, 0, cms, masks, 0), // RENDERTILE
        gdp_set_tile_size(0, 0, 0, (width - 1) << 2, (height - 1) << 2),
    ]
}

/// gsSPVertex(v0, n, addr). bits[19:12]=n (count), bits[7:1]=(v0+n). NO *2 (the <<1 IS the *2).
/// Decode: count=p0(12,8)=n; dst=p0(1,7)-count=v0. For (0,3): w0 = 0x01003006.
pub fn gsp_vertex(v0: u8, n: u8, addr: u32) -> (u32, u32) {
    let w0 =
        ((G_VTX as u32) << 24) | ((n as u32 & 0xFF) << 12) | (((v0 as u32 + n as u32) & 0x7F) << 1);
    (w0, addr)
}

pub fn gsp_spnoop() -> (u32, u32) {
    ((G_SPNOOP as u32) << 24, 0)
}

pub fn gsp_line3d(v0: u8, v1: u8) -> (u32, u32) {
    gsp_linew3d(v0, v1, 0)
}

pub fn gsp_linew3d(v0: u8, v1: u8, width: u8) -> (u32, u32) {
    let w0 = ((G_LINE3D as u32) << 24)
        | shiftl(v0 as u32 * 2, 16, 8)
        | shiftl(v1 as u32 * 2, 8, 8)
        | width as u32;
    (w0, 0)
}

/// Encodes a DMEM transfer; `size` is 1..4096 bytes.
pub fn gsp_dma_io(write: bool, dmem: u32, dram: u32, size: u32) -> (u32, u32) {
    let w0 = ((G_DMA_IO as u32) << 24)
        | shiftl(write as u32, 23, 1)
        | shiftl(dmem / 8, 13, 10)
        | shiftl(size - 1, 0, 12);
    (w0, dram)
}

pub fn gsp_special_1(payload: u32, data: u32) -> (u32, u32) {
    (((G_SPECIAL_1 as u32) << 24) | (payload & 0x00ff_ffff), data)
}

pub fn gsp_special_2(payload: u32, data: u32) -> (u32, u32) {
    (((G_SPECIAL_2 as u32) << 24) | (payload & 0x00ff_ffff), data)
}

pub fn gsp_special_3(payload: u32, data: u32) -> (u32, u32) {
    (((G_SPECIAL_3 as u32) << 24) | (payload & 0x00ff_ffff), data)
}

/// Encodes RDPHALF_1 and LOAD_UCODE; `data_size` is 1..65536 bytes.
pub fn gsp_load_ucode(code_addr: u32, data_addr: u32, data_size: u32) -> [(u32, u32); 2] {
    [
        ((G_RDPHALF_1 as u32) << 24, data_addr),
        (
            ((G_LOAD_UCODE as u32) << 24) | shiftl(data_size - 1, 0, 16),
            code_addr,
        ),
    ]
}

/// gsSP1Triangle(v0,v1,v2). Each index*2 packed at BYTE shifts 16/8/0.
/// Decode p0(17,7)/p0(9,7)/p0(1,7) recovers the raw indices. For (0,1,2): w0 = 0x05000204.
pub fn gsp_1triangle(v0: u8, v1: u8, v2: u8) -> (u32, u32) {
    let w0 = ((G_TRI1 as u32) << 24)
        | (((v0 as u32 * 2) & 0xFF) << 16)
        | (((v1 as u32 * 2) & 0xFF) << 8)
        | ((v2 as u32 * 2) & 0xFF);
    (w0, 0)
}

/// gsSP2Triangles(v00,v01,v02,flag0, v10,v11,v12,flag1): two tris in one command (F3DEX2 tri2
/// decodes A=p0(17,7)/p0(9,7)/p0(1,7), B=p1(…)). Flags are unused.
pub fn gsp_2triangles(v0: u8, v1: u8, v2: u8, v3: u8, v4: u8, v5: u8) -> (u32, u32) {
    let w0 = ((G_TRI2 as u32) << 24)
        | (((v0 as u32 * 2) & 0xFF) << 16)
        | (((v1 as u32 * 2) & 0xFF) << 8)
        | ((v2 as u32 * 2) & 0xFF);
    let w1 = (((v3 as u32 * 2) & 0xFF) << 16)
        | (((v4 as u32 * 2) & 0xFF) << 8)
        | ((v5 as u32 * 2) & 0xFF);
    (w0, w1)
}

/// gsSPMatrix(addr, proj, load, push). DMA2P: w0 |= ((64-1)/8)<<19 length field; stream
/// byte = params XOR pushMask (F3DEX2 inverts the push bit). proj+load+nopush -> 0xDA380007.
pub fn gsp_matrix(addr: u32, proj: bool, load: bool, push: bool) -> (u32, u32) {
    let params = (if proj { G_MTX_PROJECTION } else { 0 })
        | (if load { G_MTX_LOAD } else { 0 })
        | (if push { G_MTX_PUSH } else { 0 });
    let w0 =
        ((G_MTX as u32) << 24) | (((64u32 - 1) / 8) << 19) | ((params ^ G_MTX_PUSH) as u32 & 0xFF);
    (w0, addr)
}

pub fn gsp_set_geometrymode(bits: u32) -> (u32, u32) {
    (((G_GEOMETRYMODE as u32) << 24) | 0x00FF_FFFF, bits)
}

pub fn gsp_clear_geometrymode(bits: u32) -> (u32, u32) {
    (((G_GEOMETRYMODE as u32) << 24) | (!bits & 0x00FF_FFFF), 0)
}

/// gsSPViewport(addr): MOVEMEM. DMA2P length field ((16-1)/8)<<19, index byte G_MV_VIEWPORT.
/// -> 0xDC080008. HLE decode reads only p0(0,8).
pub fn gsp_viewport(addr: u32) -> (u32, u32) {
    let w0 = ((G_MOVEMEM as u32) << 24) | (((16u32 - 1) / 8) << 19) | (G_MV_VIEWPORT as u32);
    (w0, addr)
}

pub fn gsp_enddl() -> (u32, u32) {
    (((G_ENDDL as u32) << 24), 0)
}

/// gsSPPopMatrix(num): pop `num` modelview matrices. F3DEX2 packs the count as `num << 6`;
/// the HLE decodes `count = w1 >> 6`. w0 carries only the opcode.
pub fn gsp_popmatrix(num: u32) -> (u32, u32) {
    ((G_POPMTX as u32) << 24, num << 6)
}

/// gsSPDisplayList(addr): nested call. Branch bit p0(16,1) = 0 (push return addr).
/// w0 = 0xDE000000, w1 = addr (segmented). Test-only in Plan 1 (no assembler hookup yet).
pub fn gsp_displaylist(addr: u32) -> (u32, u32) {
    ((G_DL as u32) << 24, addr)
}

/// gsSPBranchList(addr): tail branch. Branch bit p0(16,1) = 1 (no push).
/// w0 = 0xDE010000, w1 = addr (segmented). Test-only in Plan 1.
pub fn gsp_branchlist(addr: u32) -> (u32, u32) {
    (((G_DL as u32) << 24) | (1 << 16), addr)
}

/// gsSPSegment(seg, value): G_MOVEWORD/G_MW_SEGMENT.
/// type = p0(16,8) = 0x06, seg = p0(2,4), value = w1.
/// gsSPSegment(2, 0x09000000) -> (0xDB060008, 0x09000000).
pub fn gsp_segment(seg: u8, value: u32) -> (u32, u32) {
    let w0 =
        ((G_MOVEWORD as u32) << 24) | ((G_MW_SEGMENT as u32) << 16) | (((seg as u32) & 0xF) << 2);
    (w0, value)
}

/// gsSPPerspNormalize(s): G_MOVEWORD / G_MW_PERSPNORM. type = p0(16,8) = 0x0E, offset = 0,
/// value = w1 = the perspNorm coefficient. gsSPPerspNormalize(129) -> (0xDB0E0000, 0x00000081).
pub fn gsp_persp_normalize(pn: u16) -> (u32, u32) {
    let w0 = ((G_MOVEWORD as u32) << 24) | ((G_MW_PERSPNORM as u32) << 16);
    (w0, pn as u32)
}

/// gsDPSetFogColor(rgba): set the scene-global fog color. Opcode G_SETFOGCOLOR=0xF8; w1=rgba.
pub fn gdp_set_fog_color(rgba: u32) -> (u32, u32) {
    (shiftl(G_SETFOGCOLOR as u32, 24, 8), rgba)
}

/// gsDPSetBlendColor(rgba): set the blend-color register (CLR_BL blender selector + THRESHOLD
/// alpha-compare). Opcode G_SETBLENDCOLOR=0xF9; w1=rgba.
pub fn gdp_set_blend_color(rgba: u32) -> (u32, u32) {
    (shiftl(G_SETBLENDCOLOR as u32, 24, 8), rgba)
}

/// gsSPFogPosition(min, max): G_MOVEWORD / G_MW_FOG. Computes fog multiplier fm and offset fo
/// from view-space z range [min, max], packs them as two int16s into w1, and encodes the
/// MOVEWORD with index G_MW_FOG=0x08 at bits[16:23] (matching the move_word handler's
/// `c.p0(16,8)` read). BLO2: index at <<16, offset at <<0 — NOT swapped.
pub fn gsp_fog_position(min: i32, max: i32) -> (u32, u32) {
    let span = (max - min).max(1);
    let fm = (128000 / span).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let fo = (((500 - min) * 256) / span).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let data = ((fm as u16 as u32) << 16) | (fo as u16 as u32);
    let w0 = shiftl(G_MOVEWORD as u32, 24, 8)
        | shiftl(G_MW_FOG as u32, 16, 8)
        | shiftl(G_MWO_FOG as u32, 0, 16);
    (w0, data)
}

pub fn gsp_vertex_f3d(v0: u8, n: u8, addr: u32) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_VTX as u32, 24, 8)
        | shiftl(n as u32 - 1, 20, 4)
        | shiftl(v0 as u32, 16, 4);
    (w0, addr)
}

pub fn gsp_1triangle_f3d(v0: u8, v1: u8, v2: u8) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_TRI1 as u32, 24, 8);
    let w1 =
        shiftl(v0 as u32 * 10, 16, 8) | shiftl(v1 as u32 * 10, 8, 8) | shiftl(v2 as u32 * 10, 0, 8);
    (w0, w1)
}

pub fn gsp_quad_f3d(v0: u8, v1: u8, v2: u8, v3: u8) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_QUAD as u32, 24, 8);
    let w1 = shiftl(v0 as u32 * 10, 24, 8)
        | shiftl(v1 as u32 * 10, 16, 8)
        | shiftl(v2 as u32 * 10, 8, 8)
        | shiftl(v3 as u32 * 10, 0, 8);
    (w0, w1)
}

pub fn gsp_matrix_f3d(addr: u32, proj: bool, load: bool, push: bool) -> (u32, u32) {
    let params = (if proj {
        rsp_f3d::G_MTX_PROJECTION
    } else {
        rsp_f3d::G_MTX_MODELVIEW
    }) | (if load {
        rsp_f3d::G_MTX_LOAD
    } else {
        rsp_f3d::G_MTX_MUL
    }) | (if push {
        rsp_f3d::G_MTX_PUSH
    } else {
        rsp_f3d::G_MTX_NOPUSH
    });
    let w0 = shiftl(rsp_f3d::G_MTX as u32, 24, 8) | shiftl(params as u32, 16, 8);
    (w0, addr)
}

pub fn gsp_popmatrix_f3d() -> (u32, u32) {
    (shiftl(rsp_f3d::G_POPMTX as u32, 24, 8), 0)
}

pub fn gsp_set_geometrymode_f3d(bits: u32) -> (u32, u32) {
    (shiftl(rsp_f3d::G_SETGEOMETRYMODE as u32, 24, 8), bits)
}

pub fn gsp_clear_geometrymode_f3d(bits: u32) -> (u32, u32) {
    (shiftl(rsp_f3d::G_CLEARGEOMETRYMODE as u32, 24, 8), bits)
}

pub fn gsp_texture_f3d(sc: u16, tc: u16, level: u32, tile: u32, on: bool) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_TEXTURE as u32, 24, 8)
        | shiftl(level, 11, 3)
        | shiftl(tile, 8, 3)
        | shiftl(on as u32, 0, 1);
    let w1 = shiftl(sc as u32, 16, 16) | shiftl(tc as u32, 0, 16);
    (w0, w1)
}

pub fn gsp_viewport_f3d(addr: u32) -> (u32, u32) {
    let w0 =
        shiftl(rsp_f3d::G_MOVEMEM as u32, 24, 8) | shiftl(rsp_f3d::G_MV_VIEWPORT as u32, 16, 8);
    (w0, addr)
}

pub fn gsp_light_f3d(slot: u8, addr: u32) -> (u32, u32) {
    let selector = rsp_f3d::G_MV_LIGHT as u32 + slot as u32 * 2;
    let w0 = shiftl(rsp_f3d::G_MOVEMEM as u32, 24, 8) | shiftl(selector, 16, 8);
    (w0, addr)
}

pub fn gsp_lookat_f3d(axis: u8, addr: u32) -> (u32, u32) {
    let selector = if axis == 0 {
        rsp_f3d::G_MV_LOOKATX
    } else {
        rsp_f3d::G_MV_LOOKATY
    };
    let w0 = shiftl(rsp_f3d::G_MOVEMEM as u32, 24, 8) | shiftl(selector as u32, 16, 8);
    (w0, addr)
}

pub fn gsp_setothermode_h_f3d(shift: u32, length: u32, data: u32) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_SETOTHERMODE_H as u32, 24, 8)
        | shiftl(shift, 8, 8)
        | shiftl(length, 0, 8);
    (w0, data)
}

pub fn gsp_setothermode_l_f3d(shift: u32, length: u32, data: u32) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_SETOTHERMODE_L as u32, 24, 8)
        | shiftl(shift, 8, 8)
        | shiftl(length, 0, 8);
    (w0, data)
}

pub fn gdp_set_cycle_type_f3d(cyc: u32) -> (u32, u32) {
    gsp_setothermode_h_f3d(20, 2, cyc << 20)
}

pub fn gdp_set_render_mode_f3d(mode1: u32, mode2: u32) -> (u32, u32) {
    gsp_setothermode_l_f3d(3, 29, mode1 | mode2)
}

pub fn gsp_segment_f3d(seg: u8, value: u32) -> (u32, u32) {
    let w0 = shiftl(rsp_f3d::G_MOVEWORD as u32, 24, 8)
        | shiftl(seg as u32, 10, 4)
        | shiftl(rsp_f3d::G_MW_SEGMENT as u32, 0, 8);
    (w0, value)
}

pub fn gsp_numlights_f3d(n: u8) -> (u32, u32) {
    let w0 =
        shiftl(rsp_f3d::G_MOVEWORD as u32, 24, 8) | shiftl(rsp_f3d::G_MW_NUMLIGHT as u32, 0, 8);
    let w1 = 0x8000_0000 + (n as u32 + 1) * 32;
    (w0, w1)
}

pub fn gsp_fog_position_f3d(min: i32, max: i32) -> (u32, u32) {
    let span = (max - min).max(1);
    let fm = (128000 / span).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let fo = (((500 - min) * 256) / span).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    let data = ((fm as u16 as u32) << 16) | (fo as u16 as u32);
    let w0 = shiftl(rsp_f3d::G_MOVEWORD as u32, 24, 8) | shiftl(rsp_f3d::G_MW_FOG as u32, 0, 8);
    (w0, data)
}

pub fn gsp_enddl_f3d() -> (u32, u32) {
    (shiftl(rsp_f3d::G_ENDDL as u32, 24, 8), 0)
}

pub fn gsp_displaylist_f3d(addr: u32) -> (u32, u32) {
    (shiftl(rsp_f3d::G_DL as u32, 24, 8), addr)
}

pub fn gsp_branchlist_f3d(addr: u32) -> (u32, u32) {
    (shiftl(rsp_f3d::G_DL as u32, 24, 8) | shiftl(1, 16, 1), addr)
}

pub fn gsp_forcematrix_f3d(addr: u32) -> (u32, u32) {
    let w0 =
        shiftl(rsp_f3d::G_MOVEMEM as u32, 24, 8) | shiftl(rsp_f3d::G_MV_MATRIX_1 as u32, 16, 8);
    (w0, addr)
}

pub fn gsp_lightcolor_f3d(idx: u8, rgba: u32) -> (u32, u32) {
    let offset = idx as u32 * 32;
    let w0 = shiftl(rsp_f3d::G_MOVEWORD as u32, 24, 8)
        | shiftl(offset, 8, 16)
        | shiftl(rsp_f3d::G_MW_LIGHTCOL as u32, 0, 8);
    (w0, rgba)
}

pub fn gsp_modifyvertex_f3d(vtx: u8, r#where: u16, val: u32) -> (u32, u32) {
    let offset = vtx as u32 * 40 + r#where as u32;
    let w0 = shiftl(rsp_f3d::G_MOVEWORD as u32, 24, 8)
        | shiftl(offset, 8, 16)
        | shiftl(rsp_f3d::G_MW_POINTS as u32, 0, 8);
    (w0, val)
}

/// On-disk N64 colored vertex (authentic libultra Vtx_t), 16 bytes, big-endian. No field swaps.
#[derive(Clone, Copy, Debug)]
pub struct VtxColored {
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
}

impl VtxColored {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..2].copy_from_slice(&self.x.to_be_bytes());
        b[2..4].copy_from_slice(&self.y.to_be_bytes());
        b[4..6].copy_from_slice(&self.z.to_be_bytes());
        b[6..8].copy_from_slice(&self.flag.to_be_bytes());
        b[8..10].copy_from_slice(&self.s.to_be_bytes());
        b[10..12].copy_from_slice(&self.t.to_be_bytes());
        b[12] = self.r;
        b[13] = self.g;
        b[14] = self.b;
        b[15] = self.a;
        b
    }
}

/// On-disk N64 viewport (Vp_t), 16 bytes, big-endian.
#[derive(Clone, Copy, Debug)]
pub struct Vp {
    pub vscale: [i16; 4],
    pub vtrans: [i16; 4],
}

impl Vp {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        for i in 0..4 {
            b[i * 2..i * 2 + 2].copy_from_slice(&self.vscale[i].to_be_bytes());
        }
        for i in 0..4 {
            b[8 + i * 2..8 + i * 2 + 2].copy_from_slice(&self.vtrans[i].to_be_bytes());
        }
        b
    }
}

/// Encode a 4x4 float matrix into the N64 fixed-point split form: 32 bytes of s16 integer parts
/// (at k*2) then 32 bytes of u16 frac parts (at 32+k*2), where k = i*4 + j (NO j^1 swap),
/// big-endian. Matches guMtxF2L; the HLE decodes element[i][j] at the same k. 1.0 = int 0x0001 frac 0.
/// Quantization truncates toward zero (matches guMtxF2L's `(long)` C cast), not rounds.
pub fn mtx_to_bytes(m: [[f32; 4]; 4]) -> [u8; 64] {
    let mut b = [0u8; 64];
    for (i, row) in m.iter().enumerate() {
        for (j, &val) in row.iter().enumerate() {
            let k = i * 4 + j;
            let fixed = (val * 65536.0) as i32;
            let intgr = (fixed >> 16) as i16;
            let frac = (fixed & 0xFFFF) as u16;
            b[k * 2..k * 2 + 2].copy_from_slice(&intgr.to_be_bytes());
            b[32 + k * 2..32 + k * 2 + 2].copy_from_slice(&frac.to_be_bytes());
        }
    }
    b
}

/// Identity matrix in the fixed-point split form.
pub fn mtx_identity_bytes() -> [u8; 64] {
    mtx_to_bytes([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

pub fn gdp_set_color_image(fmt: u32, siz: u32, width: u32, addr: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETCIMG as u32, 24, 8)
        | shiftl(fmt, 21, 3)
        | shiftl(siz, 19, 2)
        | shiftl(width - 1, 0, 12);
    (w0, addr)
}

pub fn gdp_set_depth_image(addr: u32) -> (u32, u32) {
    (shiftl(G_SETZIMG as u32, 24, 8), addr)
}

pub fn gdp_set_scissor(mode: u32, ulx: u32, uly: u32, lrx: u32, lry: u32) -> (u32, u32) {
    let w0 = shiftl(G_SETSCISSOR as u32, 24, 8) | shiftl(ulx, 12, 12) | shiftl(uly, 0, 12);
    let w1 = shiftl(mode, 24, 2) | shiftl(lrx, 12, 12) | shiftl(lry, 0, 12);
    (w0, w1)
}

pub fn gdp_set_fill_color(raw: u32) -> (u32, u32) {
    (shiftl(G_SETFILLCOLOR as u32, 24, 8), raw)
}

pub fn gdp_fill_rectangle(ulx: u32, uly: u32, lrx: u32, lry: u32) -> (u32, u32) {
    let w0 = shiftl(G_FILLRECT as u32, 24, 8) | shiftl(lrx, 12, 12) | shiftl(lry, 0, 12);
    let w1 = shiftl(ulx, 12, 12) | shiftl(uly, 0, 12);
    (w0, w1)
}

#[allow(clippy::too_many_arguments)]
pub fn gsp_texture_rectangle(
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
) -> [(u32, u32); 3] {
    let op = if flip {
        G_TEXRECTFLIP as u32
    } else {
        G_TEXRECT as u32
    };
    let cmd0 = (
        shiftl(op, 24, 8) | shiftl(lrx, 12, 12) | shiftl(lry, 0, 12),
        shiftl(tile, 24, 3) | shiftl(ulx, 12, 12) | shiftl(uly, 0, 12),
    );
    let cmd1 = (
        shiftl(G_RDPHALF_1 as u32, 24, 8),
        shiftl(uls, 16, 16) | shiftl(ult, 0, 16),
    );
    let cmd2 = (
        shiftl(G_RDPHALF_2 as u32, 24, 8),
        shiftl(dsdx, 16, 16) | shiftl(dtdy, 0, 16),
    );
    [cmd0, cmd1, cmd2]
}

/// Standalone gsDPSetTextureImage encoder (same layout as gdp_set_texture_image; exposed
/// separately so the standalone stmt is clearly distinct from the embedded-texture form).
pub fn gdp_set_texture_image_std(fmt: u32, siz: u32, width: u32, addr: u32) -> (u32, u32) {
    gdp_set_texture_image(fmt, siz, width, addr)
}

#[cfg(test)]
mod encode_tests {
    use super::*;

    #[test]
    fn golden_combine_modulate_unmasked() {
        // RGB = (TEXEL0-ZERO)*SHADE+ZERO ; ALPHA = (ZERO-ZERO)*ZERO+SHADE (SHADE passthrough)
        // ZERO_C(31): in color-b 4-bit slot &0xF=15 -> colorInputB(15)=C_ZERO
        //             in color-d 3-bit slot &0x7=7  -> colorInputD(7)=C_ZERO
        // ZERO_A(7):  alpha a/b/c slots = 7         -> alphaInputABD(7)/alphaInputC(7)=A_ZERO
        let crgb = CcPass {
            a: 1,
            b: ZERO_C,
            c: 4,
            d: ZERO_C,
        };
        let calpha = CcPass {
            a: ZERO_A,
            b: ZERO_A,
            c: ZERO_A,
            d: 4,
        };
        // identical both cycles (so 1-cycle index-1 read is the same combine)
        let (w0, w1) = gdp_set_combine_lerp(crgb, calpha, crgb, calpha);
        assert_eq!((w0, w1), (0xFC12_7E24, 0xFFFF_F9FC)); // computed + round-trips (see Task 7)
    }
    #[test]
    fn golden_prim_env() {
        assert_eq!(
            gdp_set_prim_color(0, 0, 0xFFFF_FFFF),
            (0xFA00_0000, 0xFFFF_FFFF)
        );
        assert_eq!(gdp_set_env_color(0x0000_00FF), (0xFB00_0000, 0x0000_00FF));
    }
    #[test]
    fn golden_cycle_type() {
        assert_eq!(gdp_set_cycle_type(0), (0xE300_0A01, 0x0000_0000)); // 1CYCLE
        assert_eq!(gdp_set_cycle_type(1), (0xE300_0A01, 0x0010_0000)); // 2CYCLE
    }
    #[test]
    fn golden_sp_texture() {
        assert_eq!(
            gsp_texture(0xFFFF, 0xFFFF, 0, 0, true),
            (0xD700_0002, 0xFFFF_FFFF)
        );
    }

    #[test]
    fn golden_set_texture_image_rgba16_w32() {
        // STANDALONE SetTextureImage: width=32 -> field 31 (0x1F)
        assert_eq!(
            gdp_set_texture_image(0, 2, 32, 0x1234_5678),
            (0xFD10_001F, 0x1234_5678)
        );
    }
    #[test]
    fn golden_set_tile_rendertile() {
        // fmt=0,siz=2,line=8,tmem=0,tile=0,pal=0,cmT=CLAMP(2),maskT=5,shiftT=0,cmS=CLAMP(2),maskS=5,shiftS=0
        assert_eq!(
            gdp_set_tile(0, 2, 8, 0, 0, 0, 2, 5, 0, 2, 5, 0),
            (0xF510_1000, 0x0009_4250)
        );
    }
    #[test]
    fn golden_set_tile_loadtile() {
        assert_eq!(
            gdp_set_tile(0, 2, 0, 0, 7, 0, 2, 5, 0, 2, 5, 0),
            (0xF510_0000, 0x0709_4250)
        );
    }
    #[test]
    fn golden_set_tile_size_32() {
        assert_eq!(
            gdp_set_tile_size(0, 0, 0, 124, 124),
            (0xF200_0000, 0x0007_C07C)
        );
    }
    #[test]
    fn golden_load_block_32x32_rgba16() {
        assert_eq!(
            gdp_load_block(7, 0, 0, 1023, 256),
            (0xF300_0000, 0x073F_F100)
        );
    }
    #[test]
    fn golden_syncs() {
        assert_eq!(gdp_load_sync(), (0xE600_0000, 0));
        assert_eq!(gdp_pipe_sync(), (0xE700_0000, 0));
    }

    #[test]
    fn mtx_truncates_toward_zero_like_gu_mtx_f2l() {
        // cos(45°) ≈ 0.70710677; ×65536 ≈ 46340.95 → truncated to 46340, NOT 46341.
        let val = 45_f32.to_radians().cos(); // ≈ 0.70710677
        let expected_fixed = (val * 65536.0) as i32; // truncation via Rust `as i32`
                                                     // expected_fixed == 46340 (truncated), not 46341 (rounded)
        assert_eq!(expected_fixed, 46340);
        let mut m = [[0.0f32; 4]; 4];
        m[0][0] = val;
        let bytes = mtx_to_bytes(m);
        // integer part is at bytes[0..2] (k=0)
        let int_part = i16::from_be_bytes([bytes[0], bytes[1]]) as i32;
        // frac part is at bytes[32..34] (k=0)
        let frac_part = u16::from_be_bytes([bytes[32], bytes[33]]) as i32;
        let baked = (int_part << 16) | frac_part;
        assert_eq!(
            baked, expected_fixed,
            "mtx_to_bytes must truncate (guMtxF2L), not round"
        );
    }

    #[test]
    fn render_mode_encodes_to_set_other_mode_l() {
        use crate::consts::rdp::*;
        use crate::consts::rsp_f3dex2::G_SETOTHERMODE_L;
        let (w0, w1) = gdp_set_other_mode_l(3, 29, G_RM_AA_ZB_OPA_SURF | G_RM_AA_ZB_OPA_SURF2);
        assert_eq!((w0 >> 24) as u8, G_SETOTHERMODE_L);
        // shift field = 32 - 3 - 29 = 0; length-1 = 28.
        assert_eq!((w0 >> 8) & 0xFF, 0);
        assert_eq!(w0 & 0xFF, 28);
        assert_eq!(w1, G_RM_AA_ZB_OPA_SURF | G_RM_AA_ZB_OPA_SURF2);
    }

    #[test]
    fn golden_persp_normalize() {
        assert_eq!(gsp_persp_normalize(129), (0xDB0E_0000, 0x0000_0081));
        assert_eq!(gsp_persp_normalize(0xFFFF), (0xDB0E_0000, 0x0000_FFFF));
    }

    #[test]
    fn golden_load_texture_block_32x32_rgba16() {
        let addr = 0x0010_0000u32;
        let cmds = gdp_load_texture_block(
            /*fmt*/ 0, /*siz*/ 2, /*w*/ 32, /*h*/ 32, addr, /*cmt*/ 2,
            /*maskt*/ 5, /*cms*/ 2, /*masks*/ 5,
        );
        assert_eq!(
            cmds,
            vec![
                (0xFD10_0000, addr),        // SetTextureImage width=1 (field 0!) -- NOT 32
                (0xF510_0000, 0x0709_4250), // SetTile LOADTILE
                (0xE600_0000, 0x0000_0000), // LoadSync
                (0xF300_0000, 0x073F_F100), // LoadBlock lrs=1023 dxt=256
                (0xE700_0000, 0x0000_0000), // PipeSync
                (0xF510_1000, 0x0009_4250), // SetTile RENDERTILE line=8
                (0xF200_0000, 0x0007_C07C), // SetTileSize lrs=lrt=124
            ]
        );
    }

    #[test]
    fn golden_vertex_f3d() {
        assert_eq!(
            gsp_vertex_f3d(5, 4, 0x0123_4567),
            (0x0435_0000, 0x0123_4567)
        );
    }

    #[test]
    fn golden_1triangle_f3d() {
        assert_eq!(gsp_1triangle_f3d(0, 1, 2), (0xBF00_0000, 0x0000_0A14));
    }

    #[test]
    fn golden_quad_f3d() {
        assert_eq!(gsp_quad_f3d(0, 1, 2, 3), (0xB500_0000, 0x000A_141E));
    }

    #[test]
    fn golden_matrix_f3d() {
        assert_eq!(
            gsp_matrix_f3d(0x0123_4567, true, true, false),
            (0x0103_0000, 0x0123_4567)
        );
    }

    #[test]
    fn golden_popmatrix_f3d() {
        assert_eq!(gsp_popmatrix_f3d(), (0xBD00_0000, 0x0000_0000));
    }

    #[test]
    fn golden_set_geometrymode_f3d() {
        assert_eq!(
            gsp_set_geometrymode_f3d(0x0082_1005),
            (0xB700_0000, 0x0082_1005)
        );
    }

    #[test]
    fn golden_clear_geometrymode_f3d() {
        assert_eq!(
            gsp_clear_geometrymode_f3d(0x0001_2000),
            (0xB600_0000, 0x0001_2000)
        );
    }

    #[test]
    fn golden_texture_f3d() {
        assert_eq!(
            gsp_texture_f3d(0xFFFF, 0xFFFF, 0, 0, true),
            (0xBB00_0001, 0xFFFF_FFFF)
        );
    }

    #[test]
    fn golden_viewport_f3d() {
        assert_eq!(gsp_viewport_f3d(0x0123_4567), (0x0380_0000, 0x0123_4567));
    }

    #[test]
    fn golden_light_f3d() {
        assert_eq!(gsp_light_f3d(3, 0x0123_4567), (0x038C_0000, 0x0123_4567));
    }

    #[test]
    fn golden_lookat_f3d() {
        assert_eq!(gsp_lookat_f3d(1, 0x0123_4567), (0x0382_0000, 0x0123_4567));
    }

    #[test]
    fn golden_setothermode_h_f3d() {
        assert_eq!(
            gsp_setothermode_h_f3d(20, 2, 0x0010_0000),
            (0xBA00_1402, 0x0010_0000)
        );
    }

    #[test]
    fn golden_setothermode_l_f3d() {
        assert_eq!(
            gsp_setothermode_l_f3d(3, 29, 0x4411_2233),
            (0xB900_031D, 0x4411_2233)
        );
    }

    #[test]
    fn golden_cycle_type_f3d() {
        assert_eq!(gdp_set_cycle_type_f3d(1), (0xBA00_1402, 0x0010_0000));
    }

    #[test]
    fn golden_render_mode_f3d() {
        assert_eq!(
            gdp_set_render_mode_f3d(0x0011_2233, 0x4400_0000),
            (0xB900_031D, 0x4411_2233)
        );
    }

    #[test]
    fn golden_segment_f3d() {
        assert_eq!(gsp_segment_f3d(2, 0x0900_0000), (0xBC00_0806, 0x0900_0000));
    }

    #[test]
    fn golden_numlights_f3d() {
        assert_eq!(gsp_numlights_f3d(2), (0xBC00_0002, 0x8000_0060));
    }

    #[test]
    fn golden_fog_position_f3d() {
        assert_eq!(gsp_fog_position_f3d(900, 1000), (0xBC00_0008, 0x0500_FC00));
    }

    #[test]
    fn golden_enddl_f3d() {
        assert_eq!(gsp_enddl_f3d(), (0xB800_0000, 0x0000_0000));
    }

    #[test]
    fn golden_displaylist_f3d() {
        assert_eq!(gsp_displaylist_f3d(0x0123_4567), (0x0600_0000, 0x0123_4567));
    }

    #[test]
    fn golden_branchlist_f3d() {
        assert_eq!(gsp_branchlist_f3d(0x0123_4567), (0x0601_0000, 0x0123_4567));
    }

    #[test]
    fn golden_forcematrix_f3d() {
        assert_eq!(gsp_forcematrix_f3d(0x0123_4567), (0x039E_0000, 0x0123_4567));
    }

    #[test]
    fn golden_lightcolor_f3d() {
        assert_eq!(
            gsp_lightcolor_f3d(3, 0x1122_33FF),
            (0xBC00_600A, 0x1122_33FF)
        );
    }

    #[test]
    fn golden_modifyvertex_f3d() {
        assert_eq!(
            gsp_modifyvertex_f3d(3, 0x14, 0xFFE0_0020),
            (0xBC00_8C0C, 0xFFE0_0020)
        );
    }
}
