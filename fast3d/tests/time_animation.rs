#![cfg(feature = "asm")]
use fast3d::asm::{assemble, assemble_at, source_is_time_variant};

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
    assert!(a0.is_time_variant);
    assert_ne!(
        a0.image.rdram, a1.image.rdram,
        "matrix should change with time"
    );
}

#[test]
fn static_source_is_time_invariant_and_stable() {
    let a0 = assemble_at(STATIC, 0.0, None).unwrap();
    let a1 = assemble_at(STATIC, 5.0, None).unwrap();
    assert!(!a0.is_time_variant);
    assert_eq!(a0.image.rdram, a1.image.rdram);
}

#[test]
fn assemble_equals_assemble_at_zero() {
    let legacy = assemble(STATIC).unwrap();
    let at0 = assemble_at(STATIC, 0.0, None).unwrap();
    assert_eq!(legacy.rdram, at0.image.rdram);
    assert_eq!(legacy.entry_addr, at0.image.entry_addr);
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
fn source_is_time_variant_works_on_animated_and_static() {
    // B4: source_is_time_variant should return true for animated, false for static.
    assert!(source_is_time_variant(SPIN));
    assert!(!source_is_time_variant(STATIC));
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
