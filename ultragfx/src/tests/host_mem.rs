//! Synthetic in-process proof of the `HostMemory` backend (native-endian host pointers).
//!
//! We build a REAL F3DEX2 display list in the test process: a `Vec<[usize; 2]>` of `Gfx` words
//! whose stride is 16 bytes (`command_stride()`), where each command is `[w0, w1]`. For address
//! operands `w1` carries a live host pointer into one of the backing `Vec`s; for immediates it
//! carries the packed bits. The referenced structs are `#[repr(C)]` with byte offsets that exactly
//! match what `rsp.rs` reads (`set_vertex`/`set_light`/`set_viewport`/`read_matrix`). Host data is
//! NATIVE-endian, so a `repr(C)` struct's fields land where `read_unaligned` expects them — the
//! whole reason `HostMemory` exists (no byteswap, unlike `RdramImage`).
//!
//! Backing allocations remain alive and stable until interpretation returns.
//! The asserts are discriminating: a transposed matrix, a swapped vp scale/trans, or a wrong
//! field offset (col vs dir) all make a specific assert fail.
use crate::hle::HostRam;

// Native data structs. `#[repr(C)]` field offsets match what `rsp.rs` reads via `read_unaligned`.
// The Float layout (float matrices + 24-byte float vertices) is driven by the `DataFormat::Float`
// arg passed to `interpret` below, not a backend default — the sm64 PC-port path.

/// `GBI_FLOATS` colored vertex (24 B). Offsets MUST match the `HostMemory` float `read_vertex`:
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

/// Full native-DL walk through the default (`GBI_FLOATS`) `HostMemory`: float matrices + float
/// vertices, the sm64 PC-port path.
#[test]
fn hostptr_walks_native_dl_and_decodes_scene() {
    use crate::hle::consts::rsp_f3dex2::{G_MOVEMEM, G_MOVEWORD, G_MV_LIGHT, G_MW_NUMLIGHT};
    use crate::hle::consts::{G_ENDDL, G_LIGHTING, G_MTX as G_MTX_OP};
    use crate::tests::inspect::equivalent as interpret;

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
    let combine = n64_gbi::encode::gdp_set_combine_lerp(
        n64_gbi::encode::CcPass {
            a: 1,
            b: n64_gbi::encode::ZERO_C,
            c: 4,
            d: n64_gbi::encode::ZERO_C,
        },
        n64_gbi::encode::CcPass {
            a: n64_gbi::encode::ZERO_A,
            b: n64_gbi::encode::ZERO_A,
            c: n64_gbi::encode::ZERO_A,
            d: 4,
        },
        n64_gbi::encode::CcPass {
            a: 1,
            b: n64_gbi::encode::ZERO_C,
            c: 4,
            d: n64_gbi::encode::ZERO_C,
        },
        n64_gbi::encode::CcPass {
            a: n64_gbi::encode::ZERO_A,
            b: n64_gbi::encode::ZERO_A,
            c: n64_gbi::encode::ZERO_A,
            d: 4,
        },
    );
    assert_eq!(
        combine,
        (0xFC12_7E24, 0xFFFF_F9FC),
        "MODULATE golden combine words"
    );

    let (mtx_proj_w0, _) = n64_gbi::encode::gsp_matrix(0, true, true, false);
    let (mtx_model_w0, _) = n64_gbi::encode::gsp_matrix(0, false, true, false);
    let (vtx_w0, _) = n64_gbi::encode::gsp_vertex(0, 2, 0);
    // One (degenerate) triangle so a per-run material is snapshotted during the walk; it needs a
    // render mode set first (otherwise the walk emits the "render mode never set" diagnostic).
    let (tri_w0, tri_w1) = n64_gbi::encode::gsp_1triangle(0, 1, 0);
    let (rm_w0, rm_w1) = n64_gbi::encode::gdp_set_render_mode(
        crate::hle::consts::G_RM_AA_ZB_OPA_SURF,
        crate::hle::consts::G_RM_AA_ZB_OPA_SURF2,
    );
    let (vp_w0, _) = n64_gbi::encode::gsp_viewport(0);
    // gsSPTexture(on): required for tex_enable (build_material gates on rsp.texture_state.on).
    let (texon_w0, texon_w1) = n64_gbi::encode::gsp_texture(0xFFFF, 0xFFFF, 0, 0, true);
    let (setgeo_w0, setgeo_w1) = n64_gbi::encode::gsp_set_geometrymode(G_LIGHTING);
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
        n64_gbi::encode::gdp_set_tile(0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    let (settilesize_w0, settilesize_w1) =
        n64_gbi::encode::gdp_set_tile_size(0, 0, 0, (2 - 1) << 2, 0);
    // lrs=0 -> words = (0>>2)+1 = 1 -> 8 B (covers 4-byte blob + pad).
    let (loadblock_w0, loadblock_w1) = n64_gbi::encode::gdp_load_block(7, 0, 0, 0, 0);
    let (settimg_w0, _) = n64_gbi::encode::gdp_set_texture_image(0, 2, 1, 0);

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
    let res = interpret(
        || unsafe { crate::hle::host_mem::HostMemory::new(HostRam::new(frame)) },
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
    assert_eq!(
        &mat.texture.decode()[0..4],
        &[0xFF, 0x00, 0x00, 0xFF],
        "texel 0 red"
    );
    assert_eq!(
        &mat.texture.decode()[4..8],
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
/// address whose segment is UNSET resolves to itself. Full-width pointers at or above
/// `SEGMENT_SPACE_END` pass through untouched whatever segments are set, which is the branch the
/// data pointers above take.
#[test]
fn hostptr_segment_resolve_vs_passthrough() {
    use crate::hle::Rdram;
    let backing = [0u8; 16];
    let mut host = unsafe { crate::hle::host_mem::HostMemory::new(HostRam::new(&backing)) };
    // Unset segment -> passthrough.
    assert_eq!(host.resolve(0x0512_3456).unwrap(), 0x0512_3456);
    // Set segment 5 to a base; a 0x05xxxxxx address rebases to base + (a & 0x00FFFFFF).
    host.set_segment(5, 0x1_0000_0000);
    assert_eq!(
        host.resolve(0x0512_3456).unwrap(),
        0x1_0000_0000 + 0x0012_3456
    );
    assert_eq!(host.resolve(0x1_0512_3456).unwrap(), 0x1_0512_3456);
    // resolve_masked must NOT mask a pointer (no &0x00FFFFF8).
    assert_eq!(
        host.resolve_masked(0x0512_3457).unwrap(),
        0x1_0000_0000 + 0x0012_3457
    );
}

fn span<T>(values: &[T]) -> std::ops::Range<u64> {
    let start = values.as_ptr() as u64;
    start..start + std::mem::size_of_val(values) as u64
}

#[test]
fn checked_host_spans_stay_inside_one_range() {
    use crate::{hle::host_mem::HostMemory, Rdram};
    let bytes = [0u8; 128];
    let start = bytes.as_ptr() as u64;
    let ranges = [start..start + 64, start + 64..start + 128];
    let memory = unsafe { HostMemory::new(HostRam::with_ranges(&bytes, &ranges)) };
    assert!(memory.in_bounds(start, 64));
    assert!(memory.in_bounds(start + 64, 64));
    assert!(!memory.in_bounds(start + 63, 2));
    assert!(!memory.in_bounds(start - 1, 1));
    assert!(!memory.in_bounds(start + 128, 1));
    assert!(!memory.in_bounds(u64::MAX, 2));
    assert!(!unsafe { HostMemory::new(HostRam::with_ranges(&bytes, &[])) }.in_bounds(start, 1));
    assert!(unsafe { HostMemory::new(HostRam::new(&bytes)) }.in_bounds(start + 128, 1));
}

#[test]
fn checked_host_walk_rejects_garbage_vertex_and_draws_registered_vertices() {
    use crate::{hle::host_mem::HostMemory, DiagKind, MemoryAccess, MemoryErrorKind};
    let vertices = [[-1i16, -1i16], [1, -1], [0, 1]].map(|[x, y]| {
        let mut vertex = [0u8; 16];
        vertex[..2].copy_from_slice(&x.to_ne_bytes());
        vertex[2..4].copy_from_slice(&y.to_ne_bytes());
        vertex[12..].fill(255);
        vertex
    });
    for pointer in [0x0000_fed0_0029_0003, vertices.as_ptr() as u64] {
        let mode = n64_gbi::encode::gdp_set_render_mode(
            n64_gbi::consts::G_RM_OPA_SURF,
            n64_gbi::consts::G_RM_OPA_SURF2,
        );
        let dl = [
            [u64::from(mode.0), u64::from(mode.1)],
            [0xfcff_ffff, 0xfffe_793c],
            [0x0100_3006u64, pointer],
            [0x0500_0204, 0],
            [0xdf00_0000, 0],
        ];
        let ranges = [span(&dl), span(&vertices)];
        let result = crate::hle::interp::interpret(
            unsafe { HostMemory::new(HostRam::with_ranges(&[], &ranges)) },
            dl.as_ptr() as u64,
            crate::hle::GbiUcode::F3dex2,
            crate::DataFormat::Fixed,
            None,
        );
        if pointer == vertices.as_ptr() as u64 {
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            assert_eq!(result.scene.indices.len(), 3);
        } else {
            assert!(matches!(result.diags.as_slice(), [crate::Diagnostic {
                kind: DiagKind::MemoryRead { access: MemoryAccess::Vertex, error }, ..
            }] if error.address == pointer && error.kind == MemoryErrorKind::Unavailable));
            assert!(result.scene.raw_pos.is_empty());
            assert!(result.scene.indices.is_empty());
        }
    }
}

#[test]
fn checked_host_walk_rejects_command_entry_and_branch_outside_ranges() {
    use crate::{hle::host_mem::HostMemory, DiagKind, MemoryAccess, MemoryErrorKind};
    let invalid = 0x0000_fed0_0029_0003;
    let dl = [[0xde01_0000u64, invalid]];
    let ranges = [span(&dl)];
    for entry in [invalid, dl.as_ptr() as u64] {
        let result = crate::hle::interp::interpret(
            unsafe { HostMemory::new(HostRam::with_ranges(&[], &ranges)) },
            entry,
            crate::hle::GbiUcode::F3dex2,
            crate::DataFormat::Fixed,
            None,
        );
        assert!(matches!(result.diags.as_slice(), [crate::Diagnostic {
            kind: DiagKind::MemoryRead { access: MemoryAccess::Command, error }, ..
        }] if error.address == invalid && error.kind == MemoryErrorKind::Unavailable));
        assert!(result.scene.indices.is_empty());
    }
}

#[test]
fn checked_host_reads_and_writeback_reject_unregistered_spans() {
    use crate::{hle::host_mem::HostMemory, MemoryErrorKind, Rdram};
    let bytes = [0u8; 128];
    let start = bytes.as_ptr() as u64;
    let range = start..start + 64;
    let ranges = [range];
    let memory = unsafe { HostMemory::new(HostRam::with_ranges(&bytes, &ranges)) };
    let outside = start + 64;
    assert_eq!(
        memory.check_write(outside, 1).unwrap_err().kind,
        MemoryErrorKind::Unavailable
    );
    assert!(memory.check_write(start, 64).is_ok());
    assert!(unsafe { HostMemory::new(HostRam::new(&bytes)) }
        .check_write(outside, 1)
        .is_ok());
    for error in [
        memory.read_command(start + 49).unwrap_err(),
        memory.read_u8(outside).unwrap_err(),
        memory.read_i8(outside).unwrap_err(),
        memory.read_u16(start + 63).unwrap_err(),
        memory.read_i16(start + 63).unwrap_err(),
        memory.read_bytes(start + 63, 2).unwrap_err(),
        memory
            .read_matrix(start + 1, crate::DataFormat::Fixed)
            .unwrap_err(),
        memory
            .read_matrix(start + 1, crate::DataFormat::Float)
            .unwrap_err(),
        memory
            .read_vertex(start + 49, crate::DataFormat::Fixed)
            .unwrap_err(),
        memory
            .read_vertex(start + 41, crate::DataFormat::Float)
            .unwrap_err(),
    ] {
        assert_eq!(error.kind, MemoryErrorKind::Unavailable);
    }
}

fn host_fill(address: u64, word: u32) -> Vec<[u64; 2]> {
    use n64_gbi::encode::*;
    [
        gdp_set_cycle_type(3),
        gdp_set_render_mode(0, 0),
        gdp_set_color_image(0, 2, 8, 0),
        gdp_set_scissor(0, 0, 0, 32, 16),
        gdp_set_fill_color(word),
        gdp_fill_rectangle(0, 0, 28, 12),
        gsp_enddl(),
    ]
    .map(|(w0, w1)| {
        [
            u64::from(w0),
            if w0 >> 24 == 0xff {
                address
            } else {
                u64::from(w1)
            },
        ]
    })
    .to_vec()
}

#[test]
fn checked_host_writeback_validates_color_and_depth_extents() {
    use crate::{hle::host_mem::HostMemory, render::workload::Workload, MemoryErrorKind};
    let pixels = [0u8; 64];
    let address = pixels.as_ptr() as u64;
    for depth in [false, true] {
        for (target, available, valid) in [
            (address, 64, true),
            (address, 63, false),
            (0x0000_fed0_0029_0003, 64, false),
        ] {
            let mut dl = host_fill(target, 0xf801_f801);
            if depth {
                dl.insert(0, [0xfe00_0000, target]);
            }
            let ranges = [span(&dl), address..address + available];
            let memory = || unsafe { HostMemory::new(HostRam::with_ranges(&[], &ranges)) };
            let result = crate::hle::interp::interpret(
                memory(),
                dl.as_ptr() as u64,
                crate::hle::GbiUcode::F3dex2,
                crate::DataFormat::Fixed,
                None,
            );
            assert!(result.diags.is_empty(), "{:?}", result.diags);
            let workload = Workload::new(&result.scene);
            let checked = workload.check_writeback(&result.scene, &memory());
            if valid {
                checked.unwrap();
            } else {
                let error = checked.unwrap_err();
                assert_eq!(
                    (error.address, error.length, error.kind),
                    (target, 64, MemoryErrorKind::Unavailable)
                );
            }
            workload
                .check_writeback(&result.scene, &crate::RdramImage::new(&[]))
                .unwrap();
        }
    }
}

#[test]
fn checked_host_writeback_checks_separate_depth_only_when_drawn() {
    use crate::{
        hle::{
            consts::{G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2, G_RM_OPA_SURF, G_RM_OPA_SURF2},
            host_mem::HostMemory,
        },
        render::workload::Workload,
        MemoryErrorKind,
    };
    let pixels = [0u8; 64];
    let address = pixels.as_ptr() as u64;
    let invalid = 0x0000_fed0_0029_0003;
    let vertices = [[-1i16, -1i16], [1, -1], [0, 1]].map(|[x, y]| {
        let mut vertex = [0u8; 16];
        vertex[..2].copy_from_slice(&x.to_ne_bytes());
        vertex[2..4].copy_from_slice(&y.to_ne_bytes());
        vertex[12..].fill(255);
        vertex
    });
    for (modes, valid) in [
        ((G_RM_OPA_SURF, G_RM_OPA_SURF2), true),
        ((G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2), false),
    ] {
        let mode = n64_gbi::encode::gdp_set_render_mode(modes.0, modes.1);
        let dl = [
            [0xff10_0007, address],
            [0xfe00_0000, invalid],
            [0xed00_0000, 0x0002_0010],
            [u64::from(mode.0), u64::from(mode.1)],
            [0xfcff_ffff, 0xfffe_793c],
            [0x0100_3006, vertices.as_ptr() as u64],
            [0x0500_0204, 0],
            [0xdf00_0000, 0],
        ];
        let ranges = [span(&dl), span(&vertices), span(&pixels)];
        let memory = || unsafe { HostMemory::new(HostRam::with_ranges(&[], &ranges)) };
        let result = crate::hle::interp::interpret(
            memory(),
            dl.as_ptr() as u64,
            crate::hle::GbiUcode::F3dex2,
            crate::DataFormat::Fixed,
            None,
        );
        assert!(result.diags.is_empty(), "{:?}", result.diags);
        assert_eq!(result.scene.indices.len(), 3);
        let checked = Workload::new(&result.scene).check_writeback(&result.scene, &memory());
        if valid {
            checked.unwrap();
        } else {
            let error = checked.unwrap_err();
            assert_eq!(
                (error.address, error.length, error.kind),
                (invalid, 64, MemoryErrorKind::Unavailable)
            );
        }
    }
}

#[test]
fn checked_host_writeback_rejects_task_and_recovers() {
    let (device, queue) = crate::render::headless_device_forced_fallback();
    let mut renderer = crate::Renderer::with_device(
        device,
        queue,
        crate::PresentTarget::Headless {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 8,
            height: 4,
        },
        crate::RendererConfig {
            clear_policy: crate::ClearPolicy::Persist,
            resolution_multiplier: 1,
            sample_count: 1,
            present_mode: wgpu::PresentMode::Fifo,
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            power_preference: wgpu::PowerPreference::LowPower,
        },
    );
    renderer.set_guest_writeback(true);
    let pixels = [0u8; 64];
    let address = pixels.as_ptr() as u64;
    let invalid = 0x0000_fed0_0029_0003;
    for depth in [false, true] {
        renderer.reset();
        let mut bad = host_fill(address, 0x07c1_07c1);
        bad.pop();
        if depth {
            bad.push([0xfe00_0000, invalid]);
        }
        bad.extend(host_fill(invalid, 0xf801_f801));
        let good = host_fill(address, 0xf801_f801);
        for (dl, accepted) in [(&bad, false), (&good, true)] {
            let ranges = [span(dl), span(&pixels)];
            let mut diags = Vec::new();
            let rdp = renderer.rdp.clone();
            let epoch = renderer.color_image_epoch;
            let result = unsafe {
                renderer.process_dl_host(
                    HostRam::with_ranges(&[], &ranges),
                    dl.as_ptr() as u64,
                    crate::Microcode::F3dex2,
                    &mut diags,
                )
            };
            let patches = renderer.take_guest_writes();
            if accepted {
                assert!(diags.is_empty(), "{diags:?}");
                assert!(result.renderable);
                assert_eq!(
                    patches,
                    [crate::GuestWrite {
                        address,
                        bytes: [0xf801u16; 32]
                            .into_iter()
                            .flat_map(u16::to_ne_bytes)
                            .collect(),
                    }]
                );
                let (device, queue) = (renderer.device().clone(), renderer.queue().clone());
                let rgba = super::common::pixels_from_render(
                    &device,
                    &queue,
                    8,
                    4,
                    wgpu::TextureFormat::Rgba8Unorm,
                    |view| renderer.present_last_to(view),
                );
                assert!(rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [255, 0, 0, 255]));
            } else {
                assert_eq!(
                    result.termination,
                    crate::inspect::WalkTermination::Rejected
                );
                assert!(!result.renderable);
                assert!(patches.is_empty());
                assert!(matches!(diags.as_slice(), [crate::Diagnostic {
                    kind: crate::DiagKind::MemoryRead { access: crate::MemoryAccess::Framebuffer, error }, ..
                }] if error.address == invalid && error.kind == crate::MemoryErrorKind::Unavailable));
                assert!(renderer
                    .inner
                    .coherence
                    .as_ref()
                    .unwrap()
                    .owners
                    .ranges
                    .is_empty());
                assert!(renderer.frame_scenes.is_empty());
                assert_eq!(renderer.rdp, rdp);
                assert_eq!(renderer.color_image_epoch, epoch);
            }
        }
    }
}
