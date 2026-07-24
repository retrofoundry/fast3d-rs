#![cfg(feature = "asm")]
use fast3d::asm::analyze;

const SPIN: &str = "\
Mtx model = identity()
update {
  guRotate(model, time * 90, 0, 0, 1)
}
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPEndDisplayList()
";

const STATIC: &str = "\
Mtx model = identity()
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPEndDisplayList()
";

#[test]
fn analyze_detects_time_reference_in_update_block() {
    let out = analyze(SPIN);
    assert!(out.references_time, "guRotate reads `time`");
    assert!(
        out.diagnostics.is_empty(),
        "clean source: {:?}",
        out.diagnostics
    );
}

#[test]
fn analyze_reports_static_source_as_time_invariant() {
    let out = analyze(STATIC);
    assert!(!out.references_time);
    assert!(
        out.diagnostics.is_empty(),
        "clean source: {:?}",
        out.diagnostics
    );
}

#[test]
fn analyze_detects_time_reference_in_morph_weight() {
    let src = "\
VtxSet a = { VtxN { 0,0,0,0,0,0,127,0,0,255 } }
VtxSet b = { VtxN { 1,0,0,0,0,0,127,0,0,255 } }
morph v = lerp(a, b, (1 - cos(time)) / 2)
";
    assert!(analyze(src).references_time);
}

#[test]
fn analyze_reports_texture_declarations() {
    let src = "Texture tex = { 8, 8, RGBA16 }\n";
    let out = analyze(src);
    assert_eq!(out.textures.len(), 1);
    assert_eq!(out.textures[0].name, "tex");
    assert_eq!(out.textures[0].width, 8);
    assert_eq!(out.textures[0].height, 8);
    assert_eq!(out.textures[0].format, "RGBA16");
}

#[test]
fn analyze_diagnoses_duplicate_texture_declarations() {
    let out = analyze("Texture tex = { 1, 1, RGBA16 }\nTexture tex = { 1, 1, I8 }\n");
    assert_eq!(
        out.textures.len(),
        2,
        "both declarations are still reported"
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.msg.contains("duplicate texture declaration")),
        "got {:?}",
        out.diagnostics
    );
}

#[test]
fn analyze_reports_both_axes_from_one_pass() {
    let src = "\
Texture tex = { 8, 8, RGBA16 }
Mtx model = identity()
update {
    guRotate(model, time * 90, 0, 1, 0)
}
";
    let out = analyze(src);
    assert_eq!(out.textures.len(), 1);
    assert!(out.references_time);
}
