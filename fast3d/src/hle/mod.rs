//! HLE interpreter: binary GBI image -> draw calls.
pub mod blender;
pub mod combiner;
pub use n64_gbi::consts;
pub mod gbi;
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
pub mod host_mem;
pub mod interp;
pub mod math;
pub mod mem;
pub mod rdp;
pub mod rsp;
pub mod rsp_f3d;
pub mod rsp_f3dex2;
pub mod texdec;
pub mod tmem;

#[cfg_attr(not(test), allow(unused_imports))]
pub use blender::{decode_render_mode, AlphaCompare, BlendClass, RenderMode, ZMode};
pub use combiner::{decode_rgba16, Material, MipLevel, MAX_LOD_LEVELS};
#[cfg_attr(any(not(test), not(fast3d_repository_tests)), allow(unused_imports))]
pub use gbi::GbiUcode;
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[cfg_attr(any(not(test), not(fast3d_repository_tests)), allow(unused_imports))]
pub use host_mem::HostRam;
#[cfg_attr(any(not(test), not(fast3d_repository_tests)), allow(unused_imports))]
pub use interp::{interpret, interpret_rdram, InterpResult};
#[cfg_attr(any(not(test), not(fast3d_repository_tests)), allow(unused_imports))]
pub use mem::Rdram;
#[cfg_attr(any(not(test), not(fast3d_repository_tests)), allow(unused_imports))]
pub use rsp::{ColorImage, CullKind, DrawRun, FramebufferPair, Rect, Scene, SceneOp, Scissor};
