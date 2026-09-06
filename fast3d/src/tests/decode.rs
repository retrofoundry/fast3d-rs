use crate::tests::common;

use crate::hle::interpret_rdram;

#[test]
fn g_tri2_emits_two_triangles_in_order() {
    use crate::hle::consts::{G_RM_OPA_SURF, G_RM_OPA_SURF2};
    use n64_gbi::encode::{
        gdp_set_combine_lerp, gdp_set_cycle_type, gdp_set_render_mode, gsp_2triangles, gsp_enddl,
        gsp_vertex, CcPass, VtxColored, ZERO_A, ZERO_C,
    };
    let mut rdram = vec![0u8; 0x100];
    // 4 vertices @0x00 (16 bytes each).
    for i in 0..4u8 {
        let v = VtxColored {
            x: i as i16,
            y: 0,
            z: 0,
            flag: 0,
            s: 0,
            t: 0,
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
        let o = i as usize * 16;
        rdram[o..o + 16].copy_from_slice(&v.to_bytes());
    }
    // Commands @0x40 (8-aligned, past the 64 bytes of vertex data).
    // snapshot_run requires a combiner before any triangle; add SHADE passthrough.
    let cc = CcPass {
        a: ZERO_C,
        b: ZERO_C,
        c: ZERO_C,
        d: 4,
    }; // d=4 → Shade
    let ca = CcPass {
        a: ZERO_A,
        b: ZERO_A,
        c: ZERO_A,
        d: 4,
    };
    let (cy0, cy1) = gdp_set_cycle_type(0); // 1-cycle
    rdram[0x40..0x44].copy_from_slice(&cy0.to_be_bytes());
    rdram[0x44..0x48].copy_from_slice(&cy1.to_be_bytes());
    let (rm0, rm1) = gdp_set_render_mode(G_RM_OPA_SURF, G_RM_OPA_SURF2);
    rdram[0x48..0x4C].copy_from_slice(&rm0.to_be_bytes());
    rdram[0x4C..0x50].copy_from_slice(&rm1.to_be_bytes());
    let (cl0, cl1) = gdp_set_combine_lerp(cc, ca, cc, ca);
    rdram[0x50..0x54].copy_from_slice(&cl0.to_be_bytes());
    rdram[0x54..0x58].copy_from_slice(&cl1.to_be_bytes());
    let (v0, v1) = gsp_vertex(0, 4, 0x00);
    rdram[0x58..0x5C].copy_from_slice(&v0.to_be_bytes());
    rdram[0x5C..0x60].copy_from_slice(&v1.to_be_bytes());
    let (t0, t1) = gsp_2triangles(0, 1, 2, 0, 2, 3);
    rdram[0x60..0x64].copy_from_slice(&t0.to_be_bytes());
    rdram[0x64..0x68].copy_from_slice(&t1.to_be_bytes());
    let (e0, e1) = gsp_enddl();
    rdram[0x68..0x6C].copy_from_slice(&e0.to_be_bytes());
    rdram[0x6C..0x70].copy_from_slice(&e1.to_be_bytes());

    let r = crate::hle::interpret_rdram(&rdram, 0x40);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    assert_eq!(r.scene.indices, vec![0, 1, 2, 0, 2, 3]);
}

#[test]
fn sample_dl_decodes_to_one_colored_triangle() {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("colored-triangle--white1");
    let res = interpret_rdram(rdram, entry_addr as u32);
    assert!(res.diags.is_empty(), "diags: {:?}", res.diags);

    // 3 vertices, one triangle (3 indices, in order).
    assert_eq!(res.scene.raw_pos.len(), 3);
    assert_eq!(res.scene.indices, vec![0, 1, 2]);

    // Colors survive the pipeline (authentic Vtx_t RGBA order).
    let c0 = res.scene.cn[0];
    assert_eq!(
        [
            (c0 & 0xff) as u8,
            ((c0 >> 8) & 0xff) as u8,
            ((c0 >> 16) & 0xff) as u8,
            ((c0 >> 24) & 0xff) as u8
        ],
        [255, 0, 0, 255]
    );
    let c1 = res.scene.cn[1];
    assert_eq!(
        [
            (c1 & 0xff) as u8,
            ((c1 >> 8) & 0xff) as u8,
            ((c1 >> 16) & 0xff) as u8,
            ((c1 >> 24) & 0xff) as u8
        ],
        [0, 255, 0, 255]
    );
    let c2 = res.scene.cn[2];
    assert_eq!(
        [
            (c2 & 0xff) as u8,
            ((c2 >> 8) & 0xff) as u8,
            ((c2 >> 16) & 0xff) as u8,
            ((c2 >> 24) & 0xff) as u8
        ],
        [0, 0, 255, 255]
    );

    // scale(1/64) projection gives clip (x/64, y/64, 0, 1); the full-screen viewport maps x,y
    // back to NDC unchanged (-0.75/0.75) and z to vp_trans.z = 511/1024 (mid-depth, no flip on x).
    let z = 511.0 / 1024.0;
    assert_eq!(common::ref_pos(&res.scene, 0), [-0.75, -0.75, z, 1.0]);
    assert_eq!(common::ref_pos(&res.scene, 1), [0.75, -0.75, z, 1.0]);
    assert_eq!(common::ref_pos(&res.scene, 2), [0.0, 0.75, z, 1.0]);
}

#[test]
fn tri_index_x2_convention_round_trips() {
    // gsSP1Triangle(0,1,2) must decode back to cache slots 0,1,2 (index*2 encode, /2 decode).
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("colored-triangle--white1");
    let res = interpret_rdram(rdram, entry_addr as u32);
    assert_eq!(res.scene.indices, vec![0, 1, 2]);
}

#[test]
fn geometry_mode_clear_then_set_is_asymmetric_masked() {
    // Clear(G_LIGHTING|G_CULL_BACK) then Set(G_SHADE|G_SHADING_SMOOTH), starting from the
    // initial G_CLIPPING (0x800000). Result preserves G_CLIPPING and adds the set bits:
    // 0x800000 (G_CLIPPING) | 0x4 (G_SHADE) | 0x200000 (G_SHADING_SMOOTH) = 0x00A0_0004.
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("colored-triangle--white1");
    let res = interpret_rdram(rdram, entry_addr as u32);
    assert_eq!(res.geometry_mode, 0x00A0_0004);
}

#[test]
fn i8_texel_decodes_intensity_and_alpha() {
    assert_eq!(
        crate::hle::texdec::decode_i8(&[128], 1, 1),
        vec![128, 128, 128, 128]
    );
}

#[test]
fn i4_pair_decodes_high_nibble_first() {
    assert_eq!(
        crate::hle::texdec::decode_i4(&[0xF0], 2, 1),
        vec![255, 255, 255, 255, 0, 0, 0, 0]
    );
}

#[test]
fn ia16_texel_decodes_intensity_and_alpha() {
    assert_eq!(
        crate::hle::texdec::decode_ia16(&[255, 128], 1, 1),
        vec![255, 255, 255, 128]
    );
}

#[test]
fn ia8_texel_decodes_intensity_and_alpha() {
    assert_eq!(
        crate::hle::texdec::decode_ia8(&[0x8C], 1, 1),
        vec![136, 136, 136, 204]
    );
}

#[test]
fn ia4_pair_decodes_high_nibble_first() {
    assert_eq!(
        crate::hle::texdec::decode_ia4(&[0xF0], 2, 1),
        vec![255, 255, 255, 255, 0, 0, 0, 0]
    );
}

#[test]
fn ci8_index_decodes_through_tlut() {
    let tlut = [
        0x00, 0x01, 0, 0, 0, 0, 0, 0, 0xF8, 0x01, 0, 0, 0, 0, 0, 0, 0x07, 0xC1, 0, 0, 0, 0, 0, 0,
    ];
    let out = crate::hle::texdec::decode_ci8(&[0, 1, 2], 3, 1, &tlut, 2);
    assert_eq!(
        &out[8..11],
        &[0, 255, 0],
        "index 2 must decode to green (RGBA16 5-bit expansion)"
    );
}

#[test]
fn missing_render_mode_diagnostic_has_command_address() {
    let (rdram, entry_addr) =
        crate::tests::fixtures::fixture("colored-triangle--missing-render-mode");
    let result = crate::hle::interpret_rdram(rdram, entry_addr as u32);
    assert_eq!(result.diags.len(), 1, "{:?}", result.diags);
    let diag = result
        .diags
        .iter()
        .find(|diag| diag.kind == crate::diag::DiagKind::RenderModeNeverSet)
        .expect("missing render mode diagnostic");
    assert_eq!(diag.at, entry_addr + 7 * 8);
}

#[test]
fn ci8_tlut_has_correct_count_and_content() {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("ci8--three-color-tlut");

    let palette_count: usize = 3;

    let result = crate::hle::interpret_rdram(rdram, entry_addr as u32);

    let fatal_diags: Vec<_> = result
        .diags
        .iter()
        .filter(|d| {
            !d.kind
                .to_string()
                .contains("combiner selector not implemented")
        })
        .collect();
    assert!(
        fatal_diags.is_empty(),
        "unexpected HLE diags: {:?}",
        fatal_diags
    );

    let tlut = result.rdp.tmem_bank.palette();
    assert!(
        tlut.len() >= palette_count * 8,
        "palette region must cover all {palette_count} stride-8 entries"
    );

    assert_eq!(tlut[0], 0x00, "entry 0 hi should be 0x00 (black)");
    assert_eq!(tlut[1], 0x01, "entry 0 lo should be 0x01 (alpha=1)");

    assert_eq!(tlut[8], 0xF8, "entry 1 hi should be 0xF8 (red)");
    assert_eq!(tlut[9], 0x01, "entry 1 lo should be 0x01 (alpha=1)");

    assert_eq!(tlut[16], 0x07, "entry 2 hi should be 0x07 (green)");
    assert_eq!(tlut[17], 0xC1, "entry 2 lo should be 0xC1 (green+alpha)");

    assert_eq!(
        &tlut[palette_count * 8..palette_count * 8 + 8],
        &[0u8; 8],
        "slot after last loaded entry must be zero"
    );
}
