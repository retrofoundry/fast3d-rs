//! Cross-backend faithfulness gate: the same *logical* display list, encoded two genuinely
//! incompatible ways, must produce the IDENTICAL `Scene` through both backends.
//!
//! - `RdramImage` backend: big-endian fixed-point data at RDRAM offsets, 8-byte command stride.
//! - `HostRam` backend (`GbiDataFormat::Fixed`): native-endian `#[repr(C)]` structs at host
//!   pointers, 16-byte stride — the same authentic layout, little-endian. This also exercises the
//!   fixed-point `HostRam::read_matrix`. (The float `HostRam` path is covered by `host_mem.rs`.)
//!
//! Both DLs open with a `gsSPSegment(1, base)` so both backends exercise segment-resolution
//! arithmetic.  Every resolve_masked target resolves to an 8-byte-aligned address so
//! `RdramImage`'s `& 0x00FFFFF8` mask is a provable no-op and both backends land at the same
//! physical bytes.
use crate::hle::{interpret, interpret_rdram, HostRam};

/// Encode a row-major float matrix to native-endian s15.16 split fixed-point (16 `i32` words:
/// `[0..8]` integer halves, `[8..16]` fraction halves) — the inverse of `HostRam`'s fixed
/// `read_matrix`, i.e. a native-endian `guMtxF2L`.
fn mtx_to_native_fixed(m: [[f32; 4]; 4]) -> [u8; 64] {
    let mut words = [0u32; 16];
    for r in 0..4 {
        for c in 0..2 {
            let int = (m[r][2 * c] * 65536.0) as i32 as u32;
            let frac = (m[r][2 * c + 1] * 65536.0) as i32 as u32;
            words[r * 2 + c] = (int & 0xFFFF_0000) | (frac >> 16);
            words[8 + r * 2 + c] = (int << 16) | (frac & 0xFFFF);
        }
    }
    let mut bytes = [0u8; 64];
    for (i, w) in words.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_ne_bytes());
    }
    bytes
}

// Host-side native data structs (`#[repr(C)]`, little-endian x86 — offsets match `rsp.rs`).

/// N64 colored vertex, 16 B. `set_vertex` reads x@+0 y@+2 z@+4 s@+8 t@+10 r@+12 g@+13 b@+14 a@+15.
#[repr(C)]
#[derive(Clone, Copy)]
struct HostVtx {
    x: i16,
    y: i16,
    z: i16,
    flag: u16,
    s: i16,
    t: i16,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// N64 Light_t, 16 B. `set_light` reads col u8 @ +0/+1/+2 and dir s8 @ +8/+9/+10.
#[repr(C)]
#[derive(Clone, Copy)]
struct HostLight {
    col: [u8; 3],
    colc_pad: u8,
    colc: [u8; 3],
    pad7: u8,
    dir: [i8; 3],
    pad11: u8,
    tail: u32,
}

/// N64 Vp_t viewport, 16 B. `set_viewport` reads i16 @ +0..+6 (vscale) and +8..+14 (vtrans).
#[repr(C)]
#[derive(Clone, Copy)]
struct HostVp {
    vscale: [i16; 4],
    vtrans: [i16; 4],
}

// Host command-word helpers (mirror `host_mem.rs` test).

#[inline]
fn cmd_imm(w0: u32, w1: u32) -> [usize; 2] {
    [w0 as usize, w1 as usize]
}

#[inline]
fn cmd_addr(w0: u32, ptr: u64) -> [usize; 2] {
    [w0 as usize, ptr as usize]
}

// Logical display list parameters (shared constants driving BOTH encoders).

/// Row-major translate(1,2,3) model matrix. Row 3 = [1,2,3,1]; identity elsewhere.
#[rustfmt::skip]
const MODEL_F32: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [1.0, 2.0, 3.0, 1.0],
];

/// Projection identity.
#[rustfmt::skip]
const PROJ_F32: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

// Segment-1 data layout (used by BOTH encoders at the SAME relative offsets).
//
//   Offset  Size   Content
//   0x000   64 B   proj matrix
//   0x040   64 B   model matrix
//   0x080   16 B   viewport (Vp_t)
//   0x090   16 B   directional light (Light_t)
//   0x0A0   16 B   ambient light (Light_t)
//   0x0B0   32 B   2 vertices (Vtx_t each 16 B)
//   0x0D0    8 B   texture (2 RGBA16 texels + 4 B pad, BE layout)

const OFF_PROJ: u32 = 0x000;
const OFF_MODEL: u32 = 0x040;
const OFF_VP: u32 = 0x080;
const OFF_DLIGHT: u32 = 0x090;
const OFF_AMBIENT: u32 = 0x0A0;
const OFF_VTX: u32 = 0x0B0;
const OFF_TEX: u32 = 0x0D0;
const DATA_SIZE: usize = 0x0D8; // 8-byte aligned total

/// Segment 1 base for RDRAM side: the RDRAM data region starts here (identity: base = 0).
const RDRAM_SEG1_BASE: u32 = 0;

/// Segmented address: segment 1 + offset (same bit pattern on both DL encodings).
fn seg1(off: u32) -> u32 {
    0x0100_0000 | off
}

// RDRAM-image encoder: big-endian RDRAM bytes + big-endian command stream.

/// Append a BE 8-byte command to the mutable buffer.
fn push(buf: &mut Vec<u8>, (w0, w1): (u32, u32)) {
    buf.extend_from_slice(&w0.to_be_bytes());
    buf.extend_from_slice(&w1.to_be_bytes());
}

fn encode_rdram_image() -> (Vec<u8>, u32) {
    use crate::asm::encode::*;

    // Pre-size with DATA_SIZE bytes of data, then append commands.
    let mut rdram = vec![0u8; DATA_SIZE];

    // Proj matrix at OFF_PROJ (BE fixed-point).
    let proj_bytes = mtx_to_bytes(PROJ_F32);
    rdram[OFF_PROJ as usize..OFF_PROJ as usize + 64].copy_from_slice(&proj_bytes);

    // Model matrix at OFF_MODEL (BE fixed-point, translate(1,2,3)).
    let model_bytes = mtx_to_bytes(MODEL_F32);
    rdram[OFF_MODEL as usize..OFF_MODEL as usize + 64].copy_from_slice(&model_bytes);

    // Viewport at OFF_VP: vscale/vtrans, BE i16.
    {
        let o = OFF_VP as usize;
        let vscale: [i16; 4] = [640, 480, 511, 0];
        let vtrans: [i16; 4] = [320, 240, 256, 0];
        for (i, &v) in vscale.iter().enumerate() {
            rdram[o + i * 2..o + i * 2 + 2].copy_from_slice(&v.to_be_bytes());
        }
        for (i, &v) in vtrans.iter().enumerate() {
            rdram[o + 8 + i * 2..o + 8 + i * 2 + 2].copy_from_slice(&v.to_be_bytes());
        }
    }

    // Directional light at OFF_DLIGHT: col@+0=[10,20,30], colc@+4=[10,20,30], dir@+8=[127,0,0].
    {
        let o = OFF_DLIGHT as usize;
        rdram[o] = 10;
        rdram[o + 1] = 20;
        rdram[o + 2] = 30;
        // +3 pad
        rdram[o + 4] = 10;
        rdram[o + 5] = 20;
        rdram[o + 6] = 30;
        // +7 pad
        rdram[o + 8] = 127u8; // dir[0] = +127 as s8
        rdram[o + 9] = 0;
        rdram[o + 10] = 0;
    }

    // Ambient light at OFF_AMBIENT: col@+0=[40,50,60], dir zeroed.
    {
        let o = OFF_AMBIENT as usize;
        rdram[o] = 40;
        rdram[o + 1] = 50;
        rdram[o + 2] = 60;
        rdram[o + 4] = 40;
        rdram[o + 5] = 50;
        rdram[o + 6] = 60;
    }

    // 2 vertices at OFF_VTX: BE i16 x,y,z,flag,s,t + u8 r,g,b,a.
    {
        #[allow(clippy::type_complexity)]
        let verts: [(i16, i16, i16, i16, i16, u8, u8, u8, u8); 2] = [
            (-48, 16, 7, 32, 64, 200, 100, 50, 255),
            (48, -16, -7, 96, 128, 1, 2, 3, 4),
        ];
        for (i, &(x, y, z, s, t, r, g, b, a)) in verts.iter().enumerate() {
            let o = OFF_VTX as usize + i * 16;
            rdram[o..o + 2].copy_from_slice(&x.to_be_bytes());
            rdram[o + 2..o + 4].copy_from_slice(&y.to_be_bytes());
            rdram[o + 4..o + 6].copy_from_slice(&z.to_be_bytes());
            // flag = 0 at [o+6..o+8]
            rdram[o + 8..o + 10].copy_from_slice(&s.to_be_bytes());
            rdram[o + 10..o + 12].copy_from_slice(&t.to_be_bytes());
            rdram[o + 12] = r;
            rdram[o + 13] = g;
            rdram[o + 14] = b;
            rdram[o + 15] = a;
        }
    }

    // Texture at OFF_TEX: 2 non-palindromic RGBA16 texels (BE) + 4 B pad.
    // 0xF801 = r=31,g=0,b=0,a=1  →  decode_rgba16 = (255,0,0,255)
    // 0x003F = r=0,g=0,b=31,a=1  →  decode_rgba16 = (0,0,255,255)
    {
        let o = OFF_TEX as usize;
        rdram[o] = 0xF8;
        rdram[o + 1] = 0x01;
        rdram[o + 2] = 0x00;
        rdram[o + 3] = 0x3F;
        // bytes [o+4..o+8] = 0x00 (pad, loaded but ignored by 2-texel asserts)
    }

    // Commands start here (8-byte aligned — DATA_SIZE is already aligned).
    let entry = rdram.len() as u32;

    // Combiner: MODULATE (TEXEL0 * SHADE).
    let crgb = crate::asm::encode::CcPass {
        a: 1,
        b: crate::asm::encode::ZERO_C,
        c: 4,
        d: crate::asm::encode::ZERO_C,
    };
    let calpha = crate::asm::encode::CcPass {
        a: crate::asm::encode::ZERO_A,
        b: crate::asm::encode::ZERO_A,
        c: crate::asm::encode::ZERO_A,
        d: 4,
    };
    let combine = gdp_set_combine_lerp(crgb, calpha, crgb, calpha);

    use crate::hle::consts::rsp_f3dex2::{G_MOVEMEM, G_MV_LIGHT, G_MW_NUMLIGHT};
    use crate::hle::consts::G_LIGHTING;

    let movemem_light_w0 = |off_words: u32| -> u32 {
        ((G_MOVEMEM as u32) << 24)
            | (((16u32 - 1) / 8) << 19)
            | (off_words << 8)
            | (G_MV_LIGHT as u32)
    };

    let (texon_w0, texon_w1) = gsp_texture(0xFFFF, 0xFFFF, 0, 0, true);
    let (setgeo_w0, setgeo_w1) = gsp_set_geometrymode(G_LIGHTING);
    let numlight_w1 = 24u32; // 1 directional light
    let numlight_w0 =
        ((crate::hle::consts::G_MOVEWORD as u32) << 24) | ((G_MW_NUMLIGHT as u32) << 16);
    let (settilesize_w0, settilesize_w1) = gdp_set_tile_size(0, 0, 0, (2 - 1) << 2, 0);
    // G_LOADBLOCK lrs=0 (→ 1 word = 8 bytes). Enough to cover 4 B texel data + 4 B pad.
    let (loadblock_w0, loadblock_w1) = gdp_load_block(7, 0, 0, 0, 0);
    let (settimg_w0, _) = gdp_set_texture_image(0, 2, 1, 0);

    let mut cmds: Vec<u8> = Vec::new();

    // gsSPSegment(1, RDRAM_SEG1_BASE=0): sets segment 1 to RDRAM offset 0.
    push(&mut cmds, gsp_segment(1, RDRAM_SEG1_BASE));

    // Proj matrix LOAD (segment-1 address).
    push(&mut cmds, gsp_matrix(seg1(OFF_PROJ), true, true, false));
    // Model matrix LOAD.
    push(&mut cmds, gsp_matrix(seg1(OFF_MODEL), false, true, false));

    // Viewport.
    push(&mut cmds, gsp_viewport(seg1(OFF_VP)));

    // Geometry mode.
    push(&mut cmds, (setgeo_w0, setgeo_w1));

    // NumLight = 1 directional.
    push(&mut cmds, (numlight_w0, numlight_w1));

    // Directional light (slot off_words=6 → light_idx 2).
    push(&mut cmds, (movemem_light_w0(6), seg1(OFF_DLIGHT)));
    // Ambient (slot off_words=9 → light_idx 3).
    push(&mut cmds, (movemem_light_w0(9), seg1(OFF_AMBIENT)));

    // Combiner.
    push(&mut cmds, combine);

    // SetTextureImage (unmasked resolve via `resolve`, not `resolve_masked`).
    push(&mut cmds, (settimg_w0, seg1(OFF_TEX)));

    // SetTileSize + LoadBlock.
    push(&mut cmds, (settilesize_w0, settilesize_w1));
    push(&mut cmds, (loadblock_w0, loadblock_w1));

    // SPTexture on.
    push(&mut cmds, (texon_w0, texon_w1));

    // Vertices.
    push(&mut cmds, gsp_vertex(0, 2, seg1(OFF_VTX)));

    // EndDL.
    push(&mut cmds, gsp_enddl());

    // Sanity: first command opcode is G_MOVEWORD (segment), last is G_ENDDL.
    assert_eq!(cmds[0], crate::hle::consts::G_MOVEWORD);
    assert_eq!(*cmds.last().unwrap(), 0u8); // G_ENDDL w1 tail

    rdram.extend_from_slice(&cmds);
    (rdram, entry)
}

// Host-native encoder: native `Gfx[]` + `#[repr(C)]` structs + 64-bit pointers.

/// Build the host DL and return `(dl_vec, dl_entry_ptr, backing_data)` where `backing_data`
/// keeps all the pointed-to structs alive for the duration of the interpret call.
///
/// Host data is laid out in a single `Vec<u8>` at the SAME relative offsets as RDRAM data,
/// so the segment-1 + offset arithmetic produces consistent results across backends.
#[allow(clippy::type_complexity)]
fn build_host_dl() -> (Vec<[usize; 2]>, u64, Vec<u8>) {
    use crate::asm::encode::*;
    use crate::hle::consts::rsp_f3dex2::{G_MOVEMEM, G_MV_LIGHT, G_MW_NUMLIGHT};
    use crate::hle::consts::{G_LIGHTING, G_MTX as G_MTX_OP};

    // Build the host data buffer at the same relative offsets as RDRAM.
    let mut hdata = vec![0u8; DATA_SIZE];

    // Proj matrix at OFF_PROJ (native-endian fixed-point — the HostRam Fixed path).
    hdata[OFF_PROJ as usize..OFF_PROJ as usize + 64]
        .copy_from_slice(&mtx_to_native_fixed(PROJ_F32));

    // Model matrix at OFF_MODEL (native-endian fixed-point, translate(1,2,3)).
    hdata[OFF_MODEL as usize..OFF_MODEL as usize + 64]
        .copy_from_slice(&mtx_to_native_fixed(MODEL_F32));

    // Viewport at OFF_VP: native-endian i16 (little-endian on x86).
    {
        let vp = HostVp {
            vscale: [640, 480, 511, 0],
            vtrans: [320, 240, 256, 0],
        };
        unsafe {
            let src = &vp as *const HostVp as *const u8;
            hdata[OFF_VP as usize..OFF_VP as usize + 16]
                .copy_from_slice(std::slice::from_raw_parts(src, 16));
        }
    }

    // Directional light at OFF_DLIGHT: native-endian (#[repr(C)]).
    {
        let dl = HostLight {
            col: [10, 20, 30],
            colc_pad: 0,
            colc: [10, 20, 30],
            pad7: 0,
            dir: [127, 0, 0],
            pad11: 0,
            tail: 0,
        };
        unsafe {
            let src = &dl as *const HostLight as *const u8;
            hdata[OFF_DLIGHT as usize..OFF_DLIGHT as usize + 16]
                .copy_from_slice(std::slice::from_raw_parts(src, 16));
        }
    }

    // Ambient light at OFF_AMBIENT.
    {
        let al = HostLight {
            col: [40, 50, 60],
            colc_pad: 0,
            colc: [40, 50, 60],
            pad7: 0,
            dir: [0, 0, 0],
            pad11: 0,
            tail: 0,
        };
        unsafe {
            let src = &al as *const HostLight as *const u8;
            hdata[OFF_AMBIENT as usize..OFF_AMBIENT as usize + 16]
                .copy_from_slice(std::slice::from_raw_parts(src, 16));
        }
    }

    // 2 vertices at OFF_VTX: native-endian #[repr(C)].
    {
        let verts: [HostVtx; 2] = [
            HostVtx {
                x: -48,
                y: 16,
                z: 7,
                flag: 0,
                s: 32,
                t: 64,
                r: 200,
                g: 100,
                b: 50,
                a: 255,
            },
            HostVtx {
                x: 48,
                y: -16,
                z: -7,
                flag: 0,
                s: 96,
                t: 128,
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            },
        ];
        unsafe {
            let src = verts.as_ptr() as *const u8;
            hdata[OFF_VTX as usize..OFF_VTX as usize + 32]
                .copy_from_slice(std::slice::from_raw_parts(src, 32));
        }
    }

    // Texture at OFF_TEX: SAME BE bytes as RDRAM side (raw blob, decoded BE by decode_rgba16).
    // 0xF801 = r=31,g=0,b=0,a=1  →  decode_rgba16 = (255,0,0,255)
    // 0x003F = r=0,g=0,b=31,a=1  →  decode_rgba16 = (0,0,255,255)
    // Texture bytes are raw RAM (big-endian layout) on BOTH backends — read_bytes is unswapped.
    {
        let o = OFF_TEX as usize;
        hdata[o] = 0xF8;
        hdata[o + 1] = 0x01;
        hdata[o + 2] = 0x00;
        hdata[o + 3] = 0x3F;
        // bytes [o+4..o+8] = 0x00 (pad, loaded but ignored by 2-texel asserts)
    }

    // Capture AFTER the buffer is fully built; no further pushes, so the pointer is stable.
    let seg1_base = hdata.as_ptr() as u64;

    // Combiner: MODULATE.
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
    let combine = gdp_set_combine_lerp(crgb, calpha, crgb, calpha);

    let movemem_light_w0 = |off_words: u32| -> u32 {
        ((G_MOVEMEM as u32) << 24)
            | (((16u32 - 1) / 8) << 19)
            | (off_words << 8)
            | (G_MV_LIGHT as u32)
    };

    let (texon_w0, texon_w1) = gsp_texture(0xFFFF, 0xFFFF, 0, 0, true);
    let (setgeo_w0, setgeo_w1) = gsp_set_geometrymode(G_LIGHTING);
    let numlight_w0 =
        ((crate::hle::consts::G_MOVEWORD as u32) << 24) | ((G_MW_NUMLIGHT as u32) << 16);
    let numlight_w1 = 24u32;
    let (settilesize_w0, settilesize_w1) = gdp_set_tile_size(0, 0, 0, (2 - 1) << 2, 0);
    let (loadblock_w0, loadblock_w1) = gdp_load_block(7, 0, 0, 0, 0);
    let (settimg_w0, _) = gdp_set_texture_image(0, 2, 1, 0);

    let seg_cmd_w0 = gsp_segment(1, 0).0;

    let (mtx_proj_w0, _) = gsp_matrix(0, true, true, false);
    let (mtx_model_w0, _) = gsp_matrix(0, false, true, false);
    let (vtx_w0, _) = gsp_vertex(0, 2, 0);
    let (vp_w0, _) = gsp_viewport(0);
    let g_enddl_w0 = (crate::hle::consts::G_ENDDL as u32) << 24;

    // Data addresses are SEGMENTED (`seg1(off)` = `0x01000000 | off`), exactly like the RDRAM side, so
    // the host backend resolves them through segment 1 (= seg1_base) → `seg1_base + off`,
    // DETERMINISTICALLY, and this actually exercises host-side segment resolution (the point of the
    // `gsSPSegment(1, seg1_base)` above).
    //
    // Passing the raw `seg1_base + off` directly here was UNSOUND: `HostRam::resolve` keys the segment
    // index on bits 24-27 of the address, so whenever `seg1_base`'s segment nibble happened to equal 1
    // (segment 1 being the one we set), it wrongly re-resolved the direct pointer through segment 1 and
    // corrupted every data read — an address-dependent (~1/6 of heap placements) intermittent failure.
    let dl: Vec<[usize; 2]> = vec![
        cmd_addr(seg_cmd_w0, seg1_base),
        cmd_addr(mtx_proj_w0, seg1(OFF_PROJ) as u64),
        cmd_addr(mtx_model_w0, seg1(OFF_MODEL) as u64),
        cmd_addr(vp_w0, seg1(OFF_VP) as u64),
        cmd_imm(setgeo_w0, setgeo_w1),
        cmd_imm(numlight_w0, numlight_w1),
        cmd_addr(movemem_light_w0(6), seg1(OFF_DLIGHT) as u64),
        cmd_addr(movemem_light_w0(9), seg1(OFF_AMBIENT) as u64),
        cmd_imm(combine.0, combine.1),
        cmd_addr(settimg_w0, seg1(OFF_TEX) as u64),
        cmd_imm(settilesize_w0, settilesize_w1),
        cmd_imm(loadblock_w0, loadblock_w1),
        cmd_imm(texon_w0, texon_w1),
        cmd_addr(vtx_w0, seg1(OFF_VTX) as u64),
        cmd_imm(g_enddl_w0, 0),
    ];

    // Sanity guards.
    assert_eq!(
        (dl[0][0] as u32) >> 24,
        crate::hle::consts::G_MOVEWORD as u32,
        "first cmd must be segment"
    );
    assert_eq!(
        (dl[dl.len() - 1][0] as u32) >> 24,
        crate::hle::consts::G_ENDDL as u32,
        "last cmd must be ENDDL"
    );
    assert_eq!(
        (dl[1][0] as u32) >> 24,
        G_MTX_OP as u32,
        "second cmd must be G_MTX"
    );

    let entry_ptr = dl.as_ptr() as u64;
    (dl, entry_ptr, hdata)
}

// The faithfulness gate test.

#[test]
fn rdram_image_and_host_ptr_produce_identical_scene() {
    let (rdram, entry_off) = encode_rdram_image();
    let res_img = interpret_rdram(&rdram, entry_off);
    assert!(
        res_img.diags.is_empty(),
        "RDRAM side unexpected diags: {:?}",
        res_img.diags
    );

    let (dl, entry_ptr, hdata) = build_host_dl();
    let frame_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            dl.as_ptr() as *const u8,
            dl.len() * core::mem::size_of::<[usize; 2]>(),
        )
    };
    let host = unsafe { HostRam::new(frame_bytes) };
    let res_host = interpret(
        host,
        entry_ptr,
        crate::hle::GbiUcode::F3dex2,
        crate::DataFormat::Fixed,
    );
    assert!(
        res_host.diags.is_empty(),
        "host side unexpected diags: {:?}",
        res_host.diags
    );

    // mvp_table[1] = after model LOAD. MVP = translate(1,2,3) × identity.
    // Row 3 of the row-major MVP = [1,2,3,1]; a transpose would scatter 1,2,3 into column 3.
    let mvp_img = res_img.scene.mvp_table[1];
    let mvp_host = res_host.scene.mvp_table[1];

    // Transpose discriminator: assert row 3 BEFORE the whole-scene assert.
    assert_eq!(
        mvp_img[3],
        [1.0, 2.0, 3.0, 1.0],
        "RDRAM mvp row 3 must be the translation"
    );
    assert_eq!(
        mvp_host[3],
        [1.0, 2.0, 3.0, 1.0],
        "host mvp row 3 must be the translation (catches HostRam transpose)"
    );
    // Cross-backend field equality on row 3.
    assert_eq!(
        res_img.scene.mvp_table[1], res_host.scene.mvp_table[1],
        "mvp_table[1] must match across backends"
    );

    assert_eq!(
        res_img.scene, res_host.scene,
        "Scene must be identical across RdramImage and HostRam backends"
    );

    // Keep all backing buffers alive past the asserts.
    std::hint::black_box((&dl, &hdata, &rdram));
}
