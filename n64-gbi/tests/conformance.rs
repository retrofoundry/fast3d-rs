//! Conformance vectors: expected command words derived from the libultra `gs*` macros.
//!
//! Keep every expectation a literal. Encoder-vs-interpreter round trips prove nothing about
//! opcode identity — both sides read the same `consts`.

use n64_gbi::encode::*;

#[test]
fn f3dex2_stub_words_match_libultra() {
    assert_eq!(gsp_spnoop(), (0xe000_0000, 0));
    assert_eq!(gsp_line3d(1, 3), (0x0802_0600, 0));
    assert_eq!(gsp_linew3d(3, 1, 5), (0x0806_0205, 0));
    assert_eq!(
        gsp_dma_io(false, 0x120, 0x1234_5678, 32),
        (0xd604_801f, 0x1234_5678)
    );
    assert_eq!(
        gsp_dma_io(true, 0x120, 0x1234_5678, 32),
        (0xd684_801f, 0x1234_5678)
    );
    assert_eq!(
        gsp_special_1(0xab12_3456, 0x9876_5432),
        (0xd512_3456, 0x9876_5432)
    );
    assert_eq!(
        gsp_special_2(0xab12_3456, 0x9876_5432),
        (0xd412_3456, 0x9876_5432)
    );
    assert_eq!(
        gsp_special_3(0xab12_3456, 0x9876_5432),
        (0xd312_3456, 0x9876_5432)
    );
    assert_eq!(
        gsp_load_ucode(0x1234_5678, 0x8765_4321, 2048),
        [(0xe100_0000, 0x8765_4321), (0xdd00_07ff, 0x1234_5678)]
    );
    assert_eq!(
        gsp_load_ucode(1, 2, 1),
        [(0xe100_0000, 2), (0xdd00_0000, 1)]
    );
    assert_eq!(
        gsp_load_ucode(1, 2, 65536),
        [(0xe100_0000, 2), (0xdd00_ffff, 1)]
    );
}

#[test]
fn tlut_words_match_libultra() {
    for (tile, count_minus_one, expected) in [
        (0, 0, (0xF000_0000, 0x0000_0000)),
        (7, 0, (0xF000_0000, 0x0700_0000)),
        (3, 2, (0xF000_0000, 0x0300_8000)),
        (7, 3, (0xF000_0000, 0x0700_C000)),
        (7, 15, (0xF000_0000, 0x0703_C000)),
        (5, 255, (0xF000_0000, 0x053F_C000)),
        (7, 1023, (0xF000_0000, 0x07FF_C000)),
    ] {
        assert_eq!(gdp_load_tlut(tile, count_minus_one), expected);
    }
}

#[test]
fn vtx_words_match_libultra() {
    // gsSPVertex(v0=0, n=3, addr): w0 bits[19:12]=3 (count), bits[7:1]=(0+3)=3. No *2.
    let (w0, w1) = gsp_vertex(0, 3, 0x0010_0000);
    assert_eq!(w0, 0x0100_3006);
    assert_eq!(w1, 0x0010_0000);
}

#[test]
fn tri1_words_match_libultra() {
    // gsSP1Triangle(0,1,2): index*2 at BYTE shifts 16/8/0. Decode p0(17,7)/p0(9,7)/p0(1,7)=0,1,2.
    let (w0, w1) = gsp_1triangle(0, 1, 2);
    assert_eq!(w0, 0x0500_0204);
    assert_eq!(w1, 0);
}

#[test]
fn enddl_words() {
    assert_eq!(gsp_enddl(), (0xDFu32 << 24, 0));
}

#[test]
fn geometrymode_set_and_clear() {
    // SetGeometryMode(G_SHADE): offMask keeps all (0xFFFFFF), onMask = bits.
    assert_eq!(
        gsp_set_geometrymode(0x0000_0004),
        ((0xD9u32 << 24) | 0x00FF_FFFF, 0x0000_0004)
    );
    // ClearGeometryMode(G_LIGHTING): offMask = ~bits & 0xFFFFFF, onMask = 0.
    assert_eq!(
        gsp_clear_geometrymode(0x0002_0000),
        ((0xD9u32 << 24) | (!0x0002_0000u32 & 0x00FF_FFFF), 0)
    );
}

#[test]
fn matrix_dma_length_and_param_bits() {
    // projection + load + nopush: params=(0x04|0x02|0x00), stream byte = params ^ 0x01 = 0x07.
    // DMA length field: ((64-1)/8)<<19 = 7<<19 = 0x0038_0000. w0 = 0xDA380007.
    let (w0, w1) = gsp_matrix(0x0020_0000, true, true, false);
    assert_eq!(w0, 0xDA38_0007);
    assert_eq!(w1, 0x0020_0000);
    // modelview + load + push: params=(0x00|0x02|0x01), stream byte = 0x03 ^ 0x01 = 0x02. w0 = 0xDA380002.
    let (w0b, _) = gsp_matrix(0x0020_0040, false, true, true);
    assert_eq!(w0b, 0xDA38_0002);
}

#[test]
fn viewport_dma_length_and_index_byte() {
    // DMA length field: ((16-1)/8)<<19 = 1<<19 = 0x0008_0000; index byte 0x08. w0 = 0xDC080008.
    let (w0, w1) = gsp_viewport(0x0020_0080);
    assert_eq!(w0, 0xDC08_0008);
    assert_eq!(w1, 0x0020_0080);
}

#[test]
fn vtx_bytes_authentic_libultra_order() {
    // Authentic Vtx_t order: x,y,z (s16), flag (u16), s,t (s16), r,g,b,a (u8). Big-endian. No swaps.
    let v = VtxColored {
        x: 0x0102,
        y: 0x0304,
        z: 0x0506,
        flag: 0x0708,
        s: 0x090A,
        t: 0x0B0C,
        r: 0xAA,
        g: 0xBB,
        b: 0xCC,
        a: 0xDD,
    };
    let b = v.to_bytes();
    assert_eq!(
        b,
        [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xAA, 0xBB,
            0xCC, 0xDD
        ]
    );
    assert_eq!(b.len(), 16);
}

#[test]
fn vp_bytes_big_endian() {
    // Vp_t: s16 vscale[4]; s16 vtrans[4]; big-endian, 16 bytes.
    let vp = Vp {
        vscale: [480, 640, 511, 511],
        vtrans: [480, 640, 0, 511],
    };
    let b = vp.to_bytes();
    assert_eq!(&b[0..2], &480i16.to_be_bytes());
    assert_eq!(&b[2..4], &640i16.to_be_bytes());
    assert_eq!(&b[8..10], &480i16.to_be_bytes());
    assert_eq!(b.len(), 16);
}

#[test]
fn identity_matrix_split_int_frac_be_no_swap() {
    // 64 bytes: [16 s16 integer at k*2][16 u16 frac at 32+k*2], k=i*4+j, big-endian, NO j^1.
    // Identity: integer at k for row==col is 1; frac all 0.
    let b = mtx_identity_bytes();
    assert_eq!(b.len(), 64);
    assert_eq!(&b[0..2], &1i16.to_be_bytes()); // element[0][0], k=0
    assert_eq!(&b[10..12], &1i16.to_be_bytes()); // element[1][1], k=5 -> off 10
    assert_eq!(&b[2..4], &0i16.to_be_bytes()); // element[0][1], k=1 -> off 2
    assert!(b[32..64].iter().all(|&x| x == 0)); // frac block all zero
}

#[test]
fn scale_matrix_fixed_point_exact() {
    // scale(1/64): 1/64 = 0.015625; fixed = round(0.015625*65536) = 1024; intgr=0, frac=1024=0x0400.
    let b = mtx_to_bytes([
        [0.015625, 0.0, 0.0, 0.0],
        [0.0, 0.015625, 0.0, 0.0],
        [0.0, 0.0, 0.015625, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    // element[0][0] k=0: integer 0x0000 at off 0, frac 0x0400 at off 32.
    assert_eq!(&b[0..2], &0i16.to_be_bytes());
    assert_eq!(&b[32..34], &1024u16.to_be_bytes());
    // element[3][3] k=15: integer 0x0001 at off 30, frac 0x0000 at off 62.
    assert_eq!(&b[30..32], &1i16.to_be_bytes());
    assert_eq!(&b[62..64], &0u16.to_be_bytes());
}

#[test]
fn nonsymmetric_matrix_encode_places_translation_in_last_row() {
    // Translation row [2.0, 3.0, 0.0, 1.0] at i=3; diagonal otherwise 1. NO j^1: stored at k=i*4+j.
    let m = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [2.0, 3.0, 0.0, 1.0],
    ];
    let b = mtx_to_bytes(m);
    // element[3][0]=2.0 -> k=12, integer 0x0002 at off 24, frac 0 at off 56.
    assert_eq!(&b[24..26], &2i16.to_be_bytes());
    assert_eq!(&b[56..58], &0u16.to_be_bytes());
    // element[3][1]=3.0 -> k=13, integer 0x0003 at off 26.
    assert_eq!(&b[26..28], &3i16.to_be_bytes());
}

#[test]
fn gsp_displaylist_is_call_with_branch_bit_clear() {
    // gsSPDisplayList(addr): w0 = 0xDE000000 (branch bit p0(16,1) = 0 = call), w1 = addr.
    let (w0, w1) = gsp_displaylist(0x0900_0010);
    assert_eq!(w0, 0xDE00_0000);
    assert_eq!(w1, 0x0900_0010);
}

#[test]
fn gsp_branchlist_sets_branch_bit() {
    // gsSPBranchList(addr): w0 = 0xDE010000 (branch bit p0(16,1) = 1 = branch), w1 = addr.
    let (w0, w1) = gsp_branchlist(0x0900_0020);
    assert_eq!(w0, 0xDE01_0000);
    assert_eq!(w1, 0x0900_0020);
}

#[test]
fn gsp_segment_packs_type_and_segment_index() {
    // gsSPSegment(2, 0x09000000): G_MOVEWORD/G_MW_SEGMENT.
    // type = p0(16,8) = 0x06, seg = p0(2,4), value = w1.
    let (w0, w1) = gsp_segment(2, 0x0900_0000);
    assert_eq!(w0, 0xDB06_0008);
    assert_eq!(w1, 0x0900_0000);
}

#[test]
fn golden_sp_popmatrix() {
    use n64_gbi::encode::gsp_popmatrix;
    // F3DEX2 decodes count = w1 >> 6; w0 carries only the opcode.
    assert_eq!(gsp_popmatrix(1), (0xD800_0000, 0x0000_0040));
    assert_eq!(gsp_popmatrix(2), (0xD800_0000, 0x0000_0080));
}

#[test]
fn golden_sp_2triangles() {
    use n64_gbi::encode::gsp_2triangles;
    // libultra arg order (v00,v01,v02, v10,v11,v12); index*2 byte-packed like G_TRI1.
    // A=(0,1,2) -> w0=0x06000204 ; B=(0,2,3) -> w1=0x00000406 ; decodes to [0,1,2,0,2,3].
    assert_eq!(gsp_2triangles(0, 1, 2, 0, 2, 3), (0x0600_0204, 0x0000_0406));
}

#[test]
fn sm64_seed_macro_words_match_gbi() {
    // Expanded with sm64 4a9dcf0d0a82a637b19b401f969639c9f4e0c83a include/PR/gbi.h (F3D).
    // Source symbols identify prefixes or selected commands; these are not complete fixtures.
    let cc = |a, b, c, d| CcPass { a, b, c, d };
    let decal_rgb = cc(31, 31, 31, 1);
    let decal_alpha = cc(7, 7, 7, 1);
    let env_alpha = cc(7, 7, 7, 5);
    let shade_rgb = cc(31, 31, 31, 4);
    let shade_alpha = cc(7, 7, 7, 4);

    let mut mario_metal_butt = vec![
        gdp_pipe_sync(),
        gsp_set_geometrymode_f3d(0x0004_0000),
        gdp_set_combine_lerp(decal_rgb, env_alpha, decal_rgb, env_alpha),
    ];
    mario_metal_butt.extend(gdp_load_texture_block(
        0,
        2,
        64,
        32,
        0x0400_0090,
        0,
        5,
        0,
        6,
    ));
    mario_metal_butt.push(gsp_texture_f3d(0x0F80, 0x07C0, 0, 0, true));
    mario_metal_butt.extend([
        gsp_light_f3d(0, 0x0400_1120),
        gsp_light_f3d(1, 0x0400_1130),
        gsp_displaylist_f3d(0x0400_4000),
        gsp_enddl_f3d(),
    ]);
    assert_eq!(
        mario_metal_butt,
        [
            (0xE700_0000, 0x0000_0000),
            (0xB700_0000, 0x0004_0000),
            (0xFCFF_FFFF, 0xFFFC_FA7D),
            (0xFD10_0000, 0x0400_0090),
            (0xF510_0000, 0x0701_4060),
            (0xE600_0000, 0x0000_0000),
            (0xF300_0000, 0x077F_F080),
            (0xE700_0000, 0x0000_0000),
            (0xF510_2000, 0x0001_4060),
            (0xF200_0000, 0x000F_C07C),
            (0xBB00_0001, 0x0F80_07C0),
            (0x0386_0000, 0x0400_1120),
            (0x0388_0000, 0x0400_1130),
            (0x0600_0000, 0x0400_4000),
            (0xB800_0000, 0x0000_0000),
        ],
        "actors/mario/model.inc.c: mario_metal_butt with synthetic light/list addresses"
    );

    assert_eq!(
        [
            gdp_pipe_sync(),
            gsp_clear_geometrymode_f3d(0x0002_0000),
            gdp_set_combine_lerp(decal_rgb, decal_alpha, decal_rgb, decal_alpha),
            gdp_set_render_mode_f3d(0x0C08_7008, 0x0302_7008),
            gsp_setothermode_h_f3d(12, 2, 0),
            gsp_texture_f3d(0xFFFF, 0xFFFF, 0, 0, true),
        ],
        [
            (0xE700_0000, 0x0000_0000),
            (0xB600_0000, 0x0002_0000),
            (0xFCFF_FFFF, 0xFFFC_F279),
            (0xB900_031D, 0x0F0A_7008),
            (0xBA00_0C02, 0x0000_0000),
            (0xBB00_0001, 0xFFFF_FFFF),
        ],
        "actors/power_meter/model.inc.c: dl_power_meter_base prefix"
    );
    assert_eq!(
        [
            gdp_set_tile(0, 2, 8, 0, 0, 0, 2, 6, 0, 2, 5, 0),
            gdp_set_tile_size(0, 0, 0, 124, 252),
            gdp_set_texture_image(0, 2, 1, 0x0302_33E0),
            gdp_load_block(7, 0, 0, 2047, 256),
            gsp_1triangle_f3d(0, 1, 2),
            gsp_1triangle_f3d(0, 2, 3),
            gdp_set_texture_image(0, 2, 1, 0x0302_43E0),
            gsp_1triangle_f3d(4, 5, 6),
            gsp_1triangle_f3d(4, 6, 7),
        ],
        [
            (0xF510_1000, 0x0009_8250),
            (0xF200_0000, 0x0007_C0FC),
            (0xFD10_0000, 0x0302_33E0),
            (0xF300_0000, 0x077F_F100),
            (0xBF00_0000, 0x0000_0A14),
            (0xBF00_0000, 0x0000_141E),
            (0xFD10_0000, 0x0302_43E0),
            (0xBF00_0000, 0x0028_323C),
            (0xBF00_0000, 0x0028_3C46),
        ],
        "actors/power_meter/model.inc.c: dl_power_meter_base selected commands"
    );
    assert_eq!(
        [
            gdp_set_tile(0, 2, 8, 0, 0, 0, 2, 5, 0, 2, 5, 0),
            gdp_set_tile_size(0, 0, 0, 124, 124),
        ],
        [(0xF510_1000, 0x0009_4250), (0xF200_0000, 0x0007_C07C),],
        "actors/power_meter/model.inc.c: dl_power_meter_health_segments_begin tile commands"
    );
    assert_eq!(
        [
            gdp_pipe_sync(),
            gsp_texture_f3d(0xFFFF, 0xFFFF, 0, 0, false),
            gsp_set_geometrymode_f3d(0x0002_0000),
            gdp_set_render_mode_f3d(0x0C08_4000, 0x0302_4000),
            gdp_set_combine_lerp(shade_rgb, shade_alpha, shade_rgb, shade_alpha),
            gsp_setothermode_h_f3d(12, 2, 0x0000_2000),
            gsp_enddl_f3d(),
        ],
        [
            (0xE700_0000, 0x0000_0000),
            (0xBB00_0000, 0xFFFF_FFFF),
            (0xB700_0000, 0x0002_0000),
            (0xB900_031D, 0x0F0A_4000),
            (0xFCFF_FFFF, 0xFFFE_793C),
            (0xBA00_0C02, 0x0000_2000),
            (0xB800_0000, 0x0000_0000),
        ],
        "actors/power_meter/model.inc.c: dl_power_meter_health_segments_end"
    );

    assert_eq!(
        [
            gdp_pipe_sync(),
            gdp_set_cycle_type_f3d(2),
            gsp_setothermode_h_f3d(19, 1, 0),
            gsp_setothermode_l_f3d(0, 2, 1),
            gdp_set_blend_color(0xFFFF_FFFF),
            gdp_set_render_mode_f3d(0x0040_41C8, 0x0010_41C8),
            gsp_enddl_f3d(),
        ],
        [
            (0xE700_0000, 0x0000_0000),
            (0xBA00_1402, 0x0020_0000),
            (0xBA00_1301, 0x0000_0000),
            (0xB900_0002, 0x0000_0001),
            (0xF900_0000, 0xFFFF_FFFF),
            (0xB900_031D, 0x0050_41C8),
            (0xB800_0000, 0x0000_0000),
        ],
        "bin/segment2.c: dl_hud_img_begin VERSION_US"
    );
    assert_eq!(
        [
            gdp_pipe_sync(),
            gdp_set_cycle_type_f3d(2),
            gsp_setothermode_h_f3d(19, 1, 0),
            gsp_setothermode_l_f3d(0, 2, 1),
            gdp_set_blend_color(0xFFFF_FFFF),
            gdp_set_render_mode_f3d(0, 0),
            gsp_setothermode_h_f3d(12, 2, 0),
            gsp_enddl_f3d(),
        ],
        [
            (0xE700_0000, 0x0000_0000),
            (0xBA00_1402, 0x0020_0000),
            (0xBA00_1301, 0x0000_0000),
            (0xB900_0002, 0x0000_0001),
            (0xF900_0000, 0xFFFF_FFFF),
            (0xB900_031D, 0x0000_0000),
            (0xBA00_0C02, 0x0000_0000),
            (0xB800_0000, 0x0000_0000),
        ],
        "bin/segment2.c: dl_hud_img_begin VERSION_EU"
    );

    // jrb_seg7_dl_070069B0 uses raw FogFactor(0x0724, 0xF9DC), which has no encoder.
    assert_eq!(
        gdp_set_fog_color(0x0F41_64FF),
        (0xF800_0000, 0x0F41_64FF),
        "levels/jrb/areas/1/5/model.inc.c: jrb_seg7_dl_070069B0 fog color"
    );
    assert_eq!(
        [
            gdp_pipe_sync(),
            gdp_set_cycle_type_f3d(1),
            gdp_set_render_mode_f3d(0xC800_0000, 0x0011_2078),
            gsp_setothermode_l_f3d(2, 1, 0),
            gdp_set_fog_color(0x0550_4BFF),
            gsp_fog_position_f3d(900, 1000),
            gsp_set_geometrymode_f3d(0x0001_0000),
            gdp_set_combine_lerp(
                cc(1, 31, 4, 31),
                shade_alpha,
                cc(31, 31, 31, 0),
                cc(7, 7, 7, 0),
            ),
        ],
        [
            (0xE700_0000, 0x0000_0000),
            (0xBA00_1402, 0x0010_0000),
            (0xB900_031D, 0xC811_2078),
            (0xB900_0201, 0x0000_0000),
            (0xF800_0000, 0x0550_4BFF),
            (0xBC00_0008, 0x0500_FC00),
            (0xB700_0000, 0x0001_0000),
            (0xFC12_7FFF, 0xFFFF_F838),
        ],
        "levels/jrb/areas/1/2/model.inc.c: jrb_seg7_dl_07004940 prefix"
    );

    assert_eq!(
        [
            gsp_setothermode_l_f3d(0, 2, 3),
            gdp_set_env_color(0xFFFF_FF80),
            gsp_enddl_f3d(),
        ],
        [
            (0xB900_0002, 0x0000_0003),
            (0xFB00_0000, 0xFFFF_FF80),
            (0xB800_0000, 0x0000_0000),
        ],
        "src/game/mario_misc.c: make_gfx_mario_alpha(alpha=128)"
    );

    assert_eq!(
        [
            gdp_set_combine_lerp(
                cc(2, 1, 13, 1),
                cc(2, 1, 0, 1),
                cc(31, 31, 31, 0),
                shade_alpha,
            ),
            gdp_set_render_mode_f3d(0x0C08_0000, 0x0011_2078),
            gsp_setothermode_h_f3d(16, 1, 0x0001_0000),
            gsp_clear_geometrymode_f3d(0x0002_0200),
            gdp_set_tile(0, 2, 8, 0, 0, 0, 2, 5, 0, 2, 5, 0),
            gdp_set_tile_size(0, 0, 0, 124, 124),
            gdp_set_tile(0, 2, 8, 256, 1, 0, 2, 5, 0, 2, 5, 0),
            gdp_set_tile_size(1, 0, 0, 124, 124),
            gsp_texture_f3d(0xFFFF, 0xFFFF, 1, 0, true),
        ],
        [
            (0xFC26_A1FF, 0x1FFC_923C),
            (0xB900_031D, 0x0C19_2078),
            (0xBA00_1001, 0x0001_0000),
            (0xB600_0000, 0x0002_0200),
            (0xF510_1000, 0x0009_4250),
            (0xF200_0000, 0x0007_C07C),
            (0xF510_1100, 0x0109_4250),
            (0xF200_0000, 0x0107_C07C),
            (0xBB00_0801, 0xFFFF_FFFF),
        ],
        "levels/castle_inside/areas/1/1/model.inc.c: inside_castle_seg7_dl_07023DB0 selected commands"
    );
}
#[test]
fn texture_filter_words_match_gbi() {
    use n64_gbi::encode::{gdp_set_other_mode_h, gsp_setothermode_h_f3d};
    for (mode, data) in [(0, 0x0000_0000), (2, 0x0000_2000), (3, 0x0000_3000)] {
        assert_eq!(
            gsp_setothermode_h_f3d(12, 2, mode << 12),
            (0xba00_0c02, data)
        );
        assert_eq!(gdp_set_other_mode_h(12, 2, mode << 12), (0xe300_1201, data));
    }
}

#[test]
fn texture_rectangle_words_match_gbi() {
    assert_eq!(
        gsp_texture_rectangle(5, 11, 38, 45, 3, 11, 0xfff3, 0xefff, 0xfdff, false),
        [
            (0xe402_602d, 0x0300_500b),
            (0xe100_0000, 0x000b_fff3),
            (0xf100_0000, 0xefff_fdff)
        ]
    );
    assert_eq!(
        gsp_texture_rectangle(0, 0, 36, 36, 7, 0, 0, 4096, 1024, true),
        [
            (0xe502_4024, 0x0700_0000),
            (0xe100_0000, 0),
            (0xf100_0000, 0x1000_0400)
        ]
    );
}

#[test]
fn modifyvertex_words_match_libultra() {
    assert_eq!(
        gsp_modifyvertex(0, 0x10, 0x1234_5678),
        (0x0210_0000, 0x1234_5678)
    );
    assert_eq!(
        gsp_modifyvertex(3, 0x14, 0xffe0_0040),
        (0x0214_0006, 0xffe0_0040)
    );
    assert_eq!(
        gsp_modifyvertex(31, 0x18, 0xfffc_0081),
        (0x0218_003e, 0xfffc_0081)
    );
    assert_eq!(
        gsp_modifyvertex(127, 0x1c, 0x03ff_8000),
        (0x021c_00fe, 0x03ff_8000)
    );
    assert_eq!(gsp_modifyvertex(0x1234, 0x10, 1), (0x0210_2468, 1));
    assert_eq!(gsp_modifyvertex(0x7fff, 0xff, 0), (0x02ff_fffe, 0));
}

#[test]
fn quad_words_match_libultra() {
    assert_eq!(gsp_quad(0, 1, 2, 3), (0x0700_0204, 0x0000_0406));
    assert_eq!(gsp_quad(3, 7, 12, 31), (0x0706_0e18, 0x0006_183e));
    assert_eq!(gsp_quad(127, 126, 125, 124), (0x07fe_fcfa, 0x00fe_faf8));
}

#[test]
fn conditional_control_words_match_libultra() {
    assert_eq!(gsp_culldisplaylist(0, 31), (0x0300_0000, 0x0000_003e));
    assert_eq!(gsp_culldisplaylist(3, 7), (0x0300_0006, 0x0000_000e));
    assert_eq!(
        gsp_culldisplaylist(0x7ffe, 0x7fff),
        (0x0300_fffc, 0x0000_fffe)
    );
    assert_eq!(
        gsp_branch_less_z_raw(0x0700_1238, 3, 0x01ff_0000),
        [(0xe100_0000, 0x0700_1238), (0x0400_f006, 0x01ff_0000)]
    );
    assert_eq!(
        gsp_branch_less_z_raw(0x1234_5678, 31, u32::MAX),
        [(0xe100_0000, 0x1234_5678), (0x0409_b03e, 0xffff_ffff)]
    );
}
