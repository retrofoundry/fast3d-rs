//! Synthetic in-process proof of the `HostRam` backend (native-endian host pointers).
//!
//! We build a REAL F3DEX2 display list in the test process: a `Vec<[usize; 2]>` of `Gfx` words
//! whose stride is 16 bytes (`command_stride()`), where each command is `[w0, w1]`. For address
//! operands `w1` carries a live host pointer into one of the backing `Vec`s; for immediates it
//! carries the packed bits. The referenced structs are `#[repr(C)]` with byte offsets that exactly
//! match what `rsp.rs` reads (`set_vertex`/`set_light`/`set_viewport`/`read_matrix`). Host data is
//! NATIVE-endian, so a `repr(C)` struct's fields land where `read_unaligned` expects them — the
//! whole reason `HostRam` exists (no byteswap, unlike `RdramImage`).
//!
//! Every backing `Vec`/struct is held in scope: it is the `'a` frame witness for `HostRam::new`.
//! The asserts are discriminating: a transposed matrix, a swapped vp scale/trans, or a wrong
//! field offset (col vs dir) all make a specific assert fail.
use crate::hle::HostRam;

// Native data structs. `#[repr(C)]` field offsets match what `rsp.rs` reads via `read_unaligned`.
// The Float layout (float matrices + 24-byte float vertices) is driven by the `DataFormat::Float`
// arg passed to `interpret` below, not a backend default — the sm64 PC-port path.

/// `GBI_FLOATS` colored vertex (24 B). Offsets MUST match the `HostRam` float `read_vertex`:
/// ob[3] f32 @+0/+4/+8, flag@+12, s@+14, t@+16, r@+18 g@+19 b@+20 a@+21.
#[repr(C)]
#[derive(Clone, Copy)]
struct HostVtx {
    x: f32,
    y: f32,
    z: f32,
    flag: u16,
    s: i16,
    t: i16,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// N64 Light_t (16 B). `set_light` reads col u8 @ +0/+1/+2 and dir s8 @ +8/+9/+10.
/// We give col and dir DISTINCT bytes so a col/dir offset mixup is caught.
#[repr(C)]
#[derive(Clone, Copy)]
struct HostLight {
    col: [u8; 3],
    colc_pad: u8, // +3 (Light_t has a duplicated colc; we never read it)
    colc: [u8; 3],
    pad7: u8,
    dir: [i8; 3], // +8/+9/+10
    pad11: u8,
    tail: u32, // pad to 16 B
}

/// N64 Vp_t viewport (16 B). `set_viewport` reads i16 @ +0..+6 (vscale) and +8..+14 (vtrans).
#[repr(C)]
#[derive(Clone, Copy)]
struct HostVp {
    vscale: [i16; 4],
    vtrans: [i16; 4],
}

#[inline]
fn cmd_imm(w0: u32, w1: u32) -> [usize; 2] {
    [w0 as usize, w1 as usize]
}

#[inline]
fn cmd_addr(w0: u32, ptr: u64) -> [usize; 2] {
    [w0 as usize, ptr as usize]
}

/// Full native-DL walk through the default (`GBI_FLOATS`) `HostRam`: float matrices + float
/// vertices, the sm64 PC-port path.
#[test]
fn hostptr_walks_native_dl_and_decodes_scene() {
    use crate::hle::consts::rsp_f3dex2::{G_MOVEMEM, G_MOVEWORD, G_MV_LIGHT, G_MW_NUMLIGHT};
    use crate::hle::consts::{G_ENDDL, G_LIGHTING, G_MTX as G_MTX_OP};
    use crate::hle::interpret;

    let g_enddl_w0: u32 = (G_ENDDL as u32) << 24;

    // Model matrix: row-major translate(1, 2, 3). `read_matrix` (gbifloats) lands m[i][j]=src[i*4+j],
    // so m[3] = [1,2,3,1]. (A transposing decode — from_cols_array — would instead put 1,2,3 in the
    // last COLUMN; we assert the row, so a transpose fails.)
    #[rustfmt::skip]
    let model: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        1.0, 2.0, 3.0, 1.0,
    ];
    #[rustfmt::skip]
    let proj: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];

    // Viewport: distinct scale/trans so a scale<->trans swap is caught. Authentic libultra order:
    // vscale[0]=X-scale. `set_viewport` divides X/Y by 4.0, Z by DEPTH_RANGE (1024).
    let vp = HostVp {
        vscale: [640, 480, 511, 0], // -> [160.0, 120.0, 511/1024]
        vtrans: [320, 240, 256, 0], // -> [80.0, 60.0, 256/1024]
    };

    // One directional light: col bytes (10,20,30), dir s8 (+127,0,0) = +X. Ambient col (40,50,60).
    let dlight = HostLight {
        col: [10, 20, 30],
        colc_pad: 0,
        colc: [10, 20, 30],
        pad7: 0,
        dir: [127, 0, 0],
        pad11: 0,
        tail: 0,
    };
    // Ambient_t is 8 B (col@+0..2); we still read it through the 16-B light struct (only +0..+2).
    let ambient = HostLight {
        col: [40, 50, 60],
        colc_pad: 0,
        colc: [40, 50, 60],
        pad7: 0,
        dir: [0, 0, 0],
        pad11: 0,
        tail: 0,
    };

    // Two vertices with distinct positions / texcoords / colors.
    let verts: [HostVtx; 2] = [
        HostVtx {
            x: -48.0,
            y: 16.0,
            z: 7.0,
            flag: 0,
            s: 32,
            t: 64,
            r: 200,
            g: 100,
            b: 50,
            a: 255,
        },
        HostVtx {
            x: 48.0,
            y: -16.0,
            z: -7.0,
            flag: 0,
            s: 96,
            t: 128,
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        },
    ];

    // BE-laid-out RGBA16 texture blob: 2 texels. red = 0xF801 (r5=31,g=0,b=0,a=1), then
    // blue = 0x003F (r=0,g=0,b5=31,a=1). decode_rgba16 reads these big-endian regardless of host.
    let tex: Vec<u8> = vec![0xF8, 0x01, 0x00, 0x3F];

    // Pointers (resolved at build time; segments left unset so `resolve` passes them through).
    let model_ptr = model.as_ptr() as u64;
    let proj_ptr = proj.as_ptr() as u64;
    let vp_ptr = (&vp as *const HostVp) as u64;
    let dlight_ptr = (&dlight as *const HostLight) as u64;
    let ambient_ptr = (&ambient as *const HostLight) as u64;
    let vtx_ptr = verts.as_ptr() as u64;

    // Combiner: MODULATE (TEXEL0 * SHADE) — build_material keeps tmem only when combiner samples TEXEL0.
    let combine = crate::asm::encode::gdp_set_combine_lerp(
        crate::asm::encode::CcPass {
            a: 1,
            b: crate::asm::encode::ZERO_C,
            c: 4,
            d: crate::asm::encode::ZERO_C,
        },
        crate::asm::encode::CcPass {
            a: crate::asm::encode::ZERO_A,
            b: crate::asm::encode::ZERO_A,
            c: crate::asm::encode::ZERO_A,
            d: 4,
        },
        crate::asm::encode::CcPass {
            a: 1,
            b: crate::asm::encode::ZERO_C,
            c: 4,
            d: crate::asm::encode::ZERO_C,
        },
        crate::asm::encode::CcPass {
            a: crate::asm::encode::ZERO_A,
            b: crate::asm::encode::ZERO_A,
            c: crate::asm::encode::ZERO_A,
            d: 4,
        },
    );
    assert_eq!(
        combine,
        (0xFC12_7E24, 0xFFFF_F9FC),
        "MODULATE golden combine words"
    );

    let (mtx_proj_w0, _) = crate::asm::encode::gsp_matrix(0, true, true, false);
    let (mtx_model_w0, _) = crate::asm::encode::gsp_matrix(0, false, true, false);
    let (vtx_w0, _) = crate::asm::encode::gsp_vertex(0, 2, 0);
    // One (degenerate) triangle so a per-run material is snapshotted during the walk; it needs a
    // render mode set first (otherwise the walk emits the "render mode never set" diagnostic).
    let (tri_w0, tri_w1) = crate::asm::encode::gsp_1triangle(0, 1, 0);
    let (rm_w0, rm_w1) = crate::asm::encode::gdp_set_render_mode(
        crate::hle::consts::G_RM_AA_ZB_OPA_SURF,
        crate::hle::consts::G_RM_AA_ZB_OPA_SURF2,
    );
    let (vp_w0, _) = crate::asm::encode::gsp_viewport(0);
    // gsSPTexture(on): required for tex_enable (build_material gates on rsp.texture_state.on).
    let (texon_w0, texon_w1) = crate::asm::encode::gsp_texture(0xFFFF, 0xFFFF, 0, 0, true);
    let (setgeo_w0, setgeo_w1) = crate::asm::encode::gsp_set_geometrymode(G_LIGHTING);
    let (numlight_w0, numlight_w1) = (
        ((G_MOVEWORD as u32) << 24) | ((G_MW_NUMLIGHT as u32) << 16),
        24u32, // 1 directional light (n*24)
    );
    // G_MV_LIGHT: byte_off = p0(8,8) * 8; light_idx = byte_off / 24. Slots 0/1 are LookAt, so the
    // first real directional light is light_idx 2 (offset 24*2/8 = 6). Ambient is light_idx 3.
    let movemem_light_w0 = |off_words: u32| {
        ((G_MOVEMEM as u32) << 24)
            | (((16u32 - 1) / 8) << 19)
            | (off_words << 8)
            | (G_MV_LIGHT as u32)
    };
    // Render tile 0: RGBA (fmt 0) / 16-bit (siz 2) — what build_material reads to decode the texture.
    let (settile_w0, settile_w1) =
        crate::asm::encode::gdp_set_tile(0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let (settilesize_w0, settilesize_w1) =
        crate::asm::encode::gdp_set_tile_size(0, 0, 0, (2 - 1) << 2, 0);
    // lrs=0 -> words = (0>>2)+1 = 1 -> 8 B (covers 4-byte blob + pad).
    let (loadblock_w0, loadblock_w1) = crate::asm::encode::gdp_load_block(7, 0, 0, 0, 0);
    let (settimg_w0, _) = crate::asm::encode::gdp_set_texture_image(0, 2, 1, 0);

    let tex_padded: Vec<u8> = {
        let mut v = tex.clone();
        v.resize(8, 0);
        v
    };
    let tex_padded_ptr = tex_padded.as_ptr() as u64;

    let dl: Vec<[usize; 2]> = vec![
        cmd_addr(mtx_proj_w0, proj_ptr),
        cmd_addr(mtx_model_w0, model_ptr),
        cmd_addr(vp_w0, vp_ptr),
        cmd_imm(setgeo_w0, setgeo_w1),
        cmd_imm(numlight_w0, numlight_w1),
        cmd_addr(movemem_light_w0(6), dlight_ptr), // light_idx 2 -> directional slot 0
        cmd_addr(movemem_light_w0(9), ambient_ptr), // light_idx 3 -> ambient (num_dir == 1)
        cmd_imm(combine.0, combine.1),
        cmd_imm(rm_w0, rm_w1),
        cmd_addr(settimg_w0, tex_padded_ptr),
        cmd_imm(settile_w0, settile_w1),
        cmd_imm(settilesize_w0, settilesize_w1),
        cmd_imm(loadblock_w0, loadblock_w1),
        cmd_imm(texon_w0, texon_w1),
        cmd_addr(vtx_w0, vtx_ptr),
        cmd_imm(tri_w0, tri_w1),
        cmd_imm(g_enddl_w0, 0),
    ];

    // Sanity: the first/last opcode bytes are what we think (guards a bad const / bad index).
    let last = dl.len() - 1;
    assert_eq!((dl[0][0] as u32) >> 24, G_MTX_OP as u32);
    assert_eq!((dl[last][0] as u32) >> 24, G_ENDDL as u32);

    let frame: &[u8] = unsafe {
        std::slice::from_raw_parts(
            dl.as_ptr() as *const u8,
            dl.len() * core::mem::size_of::<[usize; 2]>(),
        )
    };
    let dl_ptr = dl.as_ptr() as u64;
    let host = unsafe { HostRam::new(frame) };
    let res = interpret(
        host,
        dl_ptr,
        crate::hle::GbiUcode::F3dex2,
        crate::DataFormat::Float,
    );

    assert!(res.diags.is_empty(), "unexpected diags: {:?}", res.diags);

    // === Matrix / MVP =========================================================================
    // mvp_table[0] is the seeded identity; entry 1 is recompute after the model LOAD.
    // mvp = mul4(model, viewproj) = mul4(translate(1,2,3), identity) = translate(1,2,3).
    // The translation lives in ROW 3 of our row-vector matrices.
    let mvp = res.scene.mvp_table[1];
    assert_eq!(
        mvp[3],
        [1.0, 2.0, 3.0, 1.0],
        "mvp row 3 must be the translation"
    );
    // Upper-left is identity (a transpose would smear 1,2,3 into the last COLUMN instead).
    assert_eq!(mvp[0], [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(mvp[1], [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(mvp[2], [0.0, 0.0, 1.0, 0.0]);
    // Discriminating transpose guard: column 3 of rows 0..2 must be 0 (NOT 1,2,3).
    assert_eq!(
        [mvp[0][3], mvp[1][3], mvp[2][3]],
        [0.0, 0.0, 0.0],
        "translation must be in the row, not the column"
    );

    // The two vertices both load under this MVP (index 1).
    assert_eq!(res.scene.mtx_index, vec![1, 1]);
    let _ = proj_ptr; // proj loaded but identity; referenced to keep it alive

    // === Viewport =============================================================================
    // viewport_table[0] is the default; entry 1 is our Vp. X/Y scale /4, Z /1024.
    let (vp_scale, vp_trans) = res.scene.viewport_table[1];
    assert_eq!(vp_scale, [640.0 / 4.0, 480.0 / 4.0, 511.0 / 1024.0]);
    assert_eq!(vp_trans, [320.0 / 4.0, 240.0 / 4.0, 256.0 / 1024.0]);
    assert_eq!(res.scene.viewport_index, vec![1, 1]);

    // === Lights ===============================================================================
    // One directional + ambient -> light_count 2. Identity modelview component of the light prefold
    // keeps dir == normalize(eye dir). dir s8 +127 = +X -> normalize([1,0,0]) = [1,0,0].
    assert_eq!(res.scene.light_count, vec![2, 2]);
    let li = res.scene.light_index[0] as usize;
    let (dir_obj, dir_col) = res.scene.lights_table[li];
    // col @ +0..2 = (10,20,30)/255. dir @ +8..10 = +127 -> [1,0,0]. DISTINCT bytes prove the
    // col/dir offsets (col@+0 vs dir@+8) are not crossed.
    assert_eq!(
        dir_col,
        [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0],
        "light col @ +0"
    );
    assert_eq!(dir_obj, [1.0, 0.0, 0.0], "light dir s8 @ +8 (+127 -> +X)");
    // Ambient is the LAST entry of the set; col (40,50,60)/255, dir zeroed.
    let (amb_dir, amb_col) = res.scene.lights_table[li + 1];
    assert_eq!(amb_dir, [0.0, 0.0, 0.0]);
    assert_eq!(
        amb_col,
        [40.0 / 255.0, 50.0 / 255.0, 60.0 / 255.0],
        "ambient col @ +0"
    );

    // === Vertices =============================================================================
    // raw_pos is the object-space position read as f32 ob[3] @ +0/+4/+8 (GBI_FLOATS layout).
    assert_eq!(
        res.scene.raw_pos,
        vec![[-48.0, 16.0, 7.0], [48.0, -16.0, -7.0]]
    );
    // raw_st: s@+14, t@+16 (i16 -> f32).
    assert_eq!(res.scene.raw_st, vec![[32.0, 64.0], [96.0, 128.0]]);
    // cn = from_le_bytes([r,g,b,a]) (bytes +18..+21). v0 = (200,100,50,255).
    assert_eq!(res.scene.cn[0], u32::from_le_bytes([200, 100, 50, 255]));
    assert_eq!(res.scene.cn[1], u32::from_le_bytes([1, 2, 3, 4]));

    // === Texture (read_bytes / G_LOADBLOCK) ===================================================
    // The combiner samples TEXEL0, so the material retains the decoded texture. First texel red,
    // second blue (RGBA16 BE decode). 8 bytes loaded; first 4 are our blob, last 4 zero-padded.
    let mat = &res.scene.materials[0];
    assert!(mat.tex_enable, "MODULATE combiner must enable texturing");
    // decode_rgba16(0xF801) = R255 G0 B0 A255 ; decode_rgba16(0x003F) = R0 G0 B255 A255.
    assert_eq!(&mat.texture[0..4], &[0xFF, 0x00, 0x00, 0xFF], "texel 0 red");
    assert_eq!(
        &mat.texture[4..8],
        &[0x00, 0x00, 0xFF, 0xFF],
        "texel 1 blue"
    );

    // === Geometry mode ========================================================================
    assert_eq!(res.geometry_mode & G_LIGHTING, G_LIGHTING, "G_LIGHTING set");

    // Keep witnesses provably alive past the asserts (and silence unused-warnings on the structs).
    std::hint::black_box((
        &model,
        &proj,
        &vp,
        &dlight,
        &ambient,
        &verts,
        &tex,
        &tex_padded,
        &dl,
    ));
}

/// `set_segment` must store the raw value and `resolve` then rebases segmented addresses; an
/// address whose segment is UNSET passes through untouched (pointer passthrough). This is the
/// branch the data pointers above rely on (their high byte selects an unset segment).
#[test]
fn hostptr_segment_resolve_vs_passthrough() {
    use crate::hle::Rdram;
    let backing = [0u8; 16];
    let mut host = unsafe { HostRam::new(&backing) };
    // Unset segment -> passthrough.
    assert_eq!(host.resolve(0x0512_3456), 0x0512_3456);
    // Set segment 5 to a base; a 0x05xxxxxx address rebases to base + (a & 0x00FFFFFF).
    host.set_segment(5, 0x1_0000_0000);
    assert_eq!(host.resolve(0x0512_3456), 0x1_0000_0000 + 0x0012_3456);
    // resolve_masked must NOT mask a pointer (no &0x00FFFFF8).
    assert_eq!(
        host.resolve_masked(0x0512_3457),
        0x1_0000_0000 + 0x0012_3457
    );
}
