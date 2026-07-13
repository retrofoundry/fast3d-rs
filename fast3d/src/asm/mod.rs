//! GBI assembler: C-like gbi-macro source -> bit-accurate big-endian GBI image.
// The assembler lives in the `asm` submodule of the (feature-gated) `asm` module — the intended
// `fast3d::asm::asm::…` path after the P2 crate merge. Suppress the resulting module-inception lint.
#[allow(clippy::module_inception)]
pub mod asm;
pub mod encode;
pub mod expr;
pub mod gu;
pub mod parser;

pub use asm::{assemble, assemble_at, assemble_with_texture, Assembled, Image};
pub use parser::Diag;

/// Returns true if the source animates over time: an `update` builder OR a `morph` weight that
/// references `time`/`frame`. Cheap pre-flight check — does not assemble. Used to carry
/// `is_time_variant` on the error path so the transport is always shown for animated sources.
pub fn source_is_time_variant(source: &str) -> bool {
    let (cleaned, gu, _diags) = crate::asm::parser::extract_update(source);
    if gu.iter().any(|(_, s)| s.references_time()) {
        return true;
    }
    crate::asm::parser::parse(&cleaned)
        .0
        .iter()
        .any(|(_, s)| matches!(s, crate::asm::parser::Stmt::Morph(m) if m.weight.references_time()))
}
