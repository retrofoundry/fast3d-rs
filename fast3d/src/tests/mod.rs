//! In-crate integration tests (relocated from `tests/` in P5.4a so they can reach the `pub(crate)`
//! `hle`/`render`/`scene` internals demoted in P5.4b). The ENTIRE tree is asm-only and is gated once
//! at the `lib.rs` root: `#[cfg(all(test, feature = "asm"))] mod tests;` (step 5). So no `mod` line
//! below needs a `feature = "asm"` cfg. The 4 pure-asm suites stay external in `tests/`.

#[allow(dead_code)]
mod common;

mod asm_tests;
mod culling;
mod decode;
mod dl_plumbing;
mod e2e;
mod facade;
mod fb_store;
mod goldens;
mod hud_power_meter;
mod interp_tests;
mod lookat_roundtrip;
mod math_rdram;
mod matrix_stack;
mod render;
mod renderer_hooks;
mod renderer_present;
mod renderer_process_dl;
mod rsp_tests;
mod run_split;
mod scene_walk;

// `HostRam`-dependent → 64-bit non-wasm only (preserves host_mem.rs:14 / dlmemory_equivalence.rs:13).
// `feature = "asm"` already comes from the root tree gate; only the EXTRA conditions survive here.
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod dlmemory_equivalence;
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod host_mem;
