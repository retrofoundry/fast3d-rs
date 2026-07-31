//! GBI assembler: C-like gbi-macro source -> bit-accurate big-endian GBI image.
// The implementation lives in an internal `asm` submodule of the feature-gated public `asm`
// module. Suppress the resulting module-inception lint.
#[allow(clippy::module_inception)]
pub(crate) mod asm;
mod expr;
mod parser;

pub use asm::{
    analyze, assemble, assemble_at, assemble_at_with_textures, assemble_with_texture, Analysis,
    Image, TextureDecl, TextureInput,
};
pub use parser::Diag;
