use crate::tests::common;

#[test]
fn e2e_blend_color_macro_reaches_material() {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("textured-quad--blend-color");

    let res = crate::hle::interpret_rdram(rdram, entry_addr as u32);

    let m = res
        .scene
        .materials
        .first()
        .expect("material must be present");

    // gsDPSetBlendColor(18, 52, 86, 120) → 0x12345678 → RGBA bytes [18, 52, 86, 120].
    assert_eq!(
        m.blend_color,
        [18, 52, 86, 120],
        "blend_color must round-trip through assemble->interpret"
    );
}

#[test]
fn e2e_textured_quad_pipeline() {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("textured-quad--orange-blue");

    let res = crate::hle::interpret_rdram(rdram, entry_addr as u32);

    // No unwired-selector diagnostics expected for MODULATE (all wired).
    assert!(
        res.diags.is_empty(),
        "expected no diagnostics, got: {:?}",
        res.diags
    );

    assert_eq!(res.scene.raw_pos.len(), 4, "expected 4 vertices");
    assert_eq!(
        res.scene.indices.len(),
        6,
        "expected 6 indices (two triangles)"
    );

    let m = res
        .scene
        .materials
        .first()
        .expect("material must be present for a fully-wired MODULATE combine");

    // 1-cycle mode (othermode.H bit 20..21 == 0).
    assert_eq!(m.cycle_type, 0, "expected 1-cycle mode");

    // Texture should be enabled (gsSPTexture on=true + TEXEL0 in the combine).
    assert!(m.tex_enable, "expected tex_enable");

    assert_eq!(
        m.texture.len(),
        32 * 32 * 4,
        "decoded texture must be 32*32*4 bytes"
    );

    // Corner UVs: vertex 0 = (0,0), vertex 2 = (1,1). No V-flip.
    let uv = |i: usize| common::ref_uv(&res.scene, i);
    assert!(
        (uv(0)[0]).abs() < 1e-3 && (uv(2)[0] - 1.0).abs() < 2e-3 && (uv(2)[1] - 1.0).abs() < 2e-3
    );

    let u = crate::render::CombinerUniform::from_run(m, &crate::hle::RenderMode::default(), [0; 4]);
    assert_eq!(
        (u.combine_l, u.combine_h),
        (0xFC12_7E24, 0xFFFF_F9FC),
        "CombinerUniform must carry the raw MODULATE combine words"
    );

    assert_eq!(
        m.selectors.cyc1.ca,
        crate::hle::combiner::ColorIn::Texel0,
        "cyc1.ca must be TEXEL0"
    );
    assert_eq!(
        m.selectors.cyc1.cc,
        crate::hle::combiner::ColorIn::Shade,
        "cyc1.cc must be SHADE"
    );
    assert_eq!(
        m.selectors.cyc1.ad,
        crate::hle::combiner::AlphaIn::Shade,
        "cyc1.ad must be SHADE"
    );
}

#[test]
fn segmented_sub_dl_draws_two_culled_objects() {
    // 32x32 white RGBA8 texture — matches the shared segmented-sub-dl.n64's declared 32x32 dims.
    let (rdram, entry_addr) = crate::tests::fixtures::fixture("segmented-sub-dl");
    let r = crate::hle::interpret_rdram(rdram, entry_addr as u32);
    assert!(r.diags.is_empty(), "unexpected diag: {:?}", r.diags);
    // The quad sub-DL runs twice (one push/pop per object): 8 vertices, 12 indices (all front-facing).
    assert_eq!(r.scene.raw_pos.len(), 8, "two objects x 4 verts");
    assert_eq!(
        r.scene.indices.len(),
        12,
        "two objects x 2 tris x 3 (none culled under G_CULL_BACK)"
    );
    assert!(
        !r.scene.materials.is_empty(),
        "materials present -> renders in the web shell"
    );
    // Object 1 (left, tx=-64) is left of object 2 (right, tx=+64) on screen.
    assert!(
        common::ref_pos(&r.scene, 0)[0] < common::ref_pos(&r.scene, 4)[0],
        "left obj {} should be left of right obj {}",
        common::ref_pos(&r.scene, 0)[0],
        common::ref_pos(&r.scene, 4)[0],
    );
}
