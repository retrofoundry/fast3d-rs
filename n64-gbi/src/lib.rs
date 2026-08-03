//! N64 GBI vocabulary, command encoders, packed records and texels, literal conformance vectors,
//! and libultra-compatible `gu` matrix math.
//!
//! A dependency leaf, shared by the interpreter that decodes display lists and anything that
//! produces them.

pub mod consts;
pub mod encode;
pub mod gu;
pub mod texel;
pub mod vectors;
