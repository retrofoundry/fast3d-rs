//! End-to-end native integration test: crate::asm::assemble_with_texture -> crate::hle::interpret -> crate::render::CombinerUniform::from_run.
use crate::tests::common;

/// Textured-quad source — reads from the shared tests/scenes/ file (single source of truth).
const SAMPLE_SOURCE_RUST: &str = include_str!("../../tests/scenes/textured-quad.n64");

/// Build a non-symmetric 32x32 RGBA8 default texture.
/// Top half: warm orange (200, 100, 50, 255); bottom half: cool blue (50, 100, 200, 255).
/// The asymmetry (top != bottom) catches any stray V-flip in the render pipeline.
fn default_rgba() -> Vec<u8> {
    let mut data = Vec::with_capacity(32 * 32 * 4);
    for row in 0..32usize {
        for _col in 0..32usize {
            if row < 16 {
                data.extend_from_slice(&[200u8, 100, 50, 255]);
            } else {
                data.extend_from_slice(&[50u8, 100, 200, 255]);
            }
        }
    }
    data
}

/// Inline source exercising the gsDPSetBlendColor DSL macro (G_SETBLENDCOLOR=0xF9): assemble it
/// through the gbi assembler, interpret with the HLE, and assert the blend-color RGBA reaches the
/// emitted Material (via the RDP blend_color register). Without the assembler/encoder/interpreter
/// wiring this round-trips to [0,0,0,0]; with it, the per-byte RGBA must survive intact.
const BLEND_COLOR_SOURCE: &str = r#"
Texture tex = { 32, 32, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetBlendColor(18, 52, 86, 120)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

#[test]
fn e2e_blend_color_macro_reaches_material() {
    let rgba = default_rgba();
    let img = crate::asm::assemble_with_texture(BLEND_COLOR_SOURCE, &rgba, 32, 32)
        .expect("assemble_with_texture must succeed with gsDPSetBlendColor present");

    let res = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);

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
    let rgba = default_rgba();
    let img = crate::asm::assemble_with_texture(SAMPLE_SOURCE_RUST, &rgba, 32, 32)
        .expect("assemble_with_texture must succeed for the Milestone A sample");

    let res = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);

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

    let u =
        crate::render::CombinerUniform::from_run(m, &crate::hle::RenderMode::default(), [0.0; 4]);
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

/// Segmented sub-DL source — reads from the shared tests/scenes/ file (single source of truth).
const SEGMENTED_SUB_DL_SAMPLE: &str = include_str!("../../tests/scenes/segmented-sub-dl.n64");

#[test]
fn segmented_sub_dl_draws_two_culled_objects() {
    // 32x32 white RGBA8 texture — matches the shared segmented-sub-dl.n64's declared 32x32 dims.
    let rgba = vec![255u8; 32 * 32 * 4];
    let img = crate::asm::assemble_with_texture(SEGMENTED_SUB_DL_SAMPLE, &rgba, 32, 32)
        .expect("assemble");
    let r = crate::hle::interpret_rdram(&img.rdram, img.entry_addr);
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
