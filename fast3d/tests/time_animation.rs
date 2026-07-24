#![cfg(feature = "asm")]
use fast3d::asm::{analyze, assemble, assemble_at};

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
fn animated_source_differs_over_time_and_is_flagged() {
    let a0 = assemble_at(SPIN, 0.0, None).unwrap();
    let a1 = assemble_at(SPIN, 1.0, None).unwrap();
    assert_ne!(a0.rdram, a1.rdram, "matrix should change with time");
}

#[test]
fn static_source_is_time_invariant_and_stable() {
    let a0 = assemble_at(STATIC, 0.0, None).unwrap();
    let a1 = assemble_at(STATIC, 5.0, None).unwrap();
    assert_eq!(a0.rdram, a1.rdram);
}

#[test]
fn assemble_equals_assemble_at_zero() {
    let legacy = assemble(STATIC).unwrap();
    let at0 = assemble_at(STATIC, 0.0, None).unwrap();
    assert_eq!(legacy.rdram, at0.rdram);
    assert_eq!(legacy.entry_addr, at0.entry_addr);
}

#[test]
fn update_targeting_undeclared_matrix_is_error() {
    // B2: `update` with no `Mtx model` declaration must return Err with a diag about unknown matrix.
    let src = "update { guRotate(model, time*90, 0,0,1) }\ngsSPEndDisplayList()\n";
    let err = assemble_at(src, 0.0, None).unwrap_err();
    assert!(
        err.iter().any(|d| d.msg.contains("unknown matrix")),
        "expected 'unknown matrix' diag, got: {err:?}"
    );
}

#[test]
fn analyze_reports_time_variance_for_animated_and_static() {
    assert!(analyze(SPIN).references_time);
    assert!(!analyze(STATIC).references_time);
}

#[test]
fn nonfinite_matrix_is_diagnosed_not_baked() {
    // 1/(time-2) is +inf at time=2
    let src = "Mtx m = identity()\nupdate {\n  guTranslate(m, 1 / (time - 2), 0, 0)\n}\ngsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)\ngsSPEndDisplayList()\n";
    assert!(assemble_at(src, 0.0, None).is_ok());
    let err = assemble_at(src, 2.0, None).unwrap_err();
    assert!(
        err.iter().any(|d| d.msg.contains("non-finite")),
        "diags: {err:?}"
    );
}
