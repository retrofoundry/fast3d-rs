//! In-crate integration tests (relocated from `tests/` in P5.4a so they can reach the `pub(crate)`
//! `hle`/`render`/`scene` internals).
//! Only modules and individual tests that depend on the compiler are gated on `asm`; four pure-asm
//! suites remain external in `tests/`.

#[allow(dead_code)]
mod common;

#[cfg(feature = "asm")]
mod asm_tests;
mod culling;
mod decode;
mod dl_plumbing;
#[cfg(feature = "asm")]
mod e2e;
#[cfg(feature = "asm")]
mod facade;
mod fb_store;
mod fixtures;
mod goldens;
mod hud_power_meter;
mod interp_tests;
#[cfg(feature = "asm")]
mod lookat_roundtrip;
mod math_rdram;
mod matrix_stack;
mod render;
mod renderer_hooks;
#[cfg(feature = "asm")]
mod renderer_present;
mod renderer_process_dl;
mod rsp_tests;
mod run_split;
#[cfg(feature = "asm")]
mod scene_walk;

// `HostRam`-dependent → 64-bit non-wasm only (preserves host_mem.rs:14 / dlmemory_equivalence.rs:13).
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod dlmemory_equivalence;
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod host_mem;
