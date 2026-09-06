#[allow(dead_code)]
pub(crate) mod common;

mod alpha_dither;
#[cfg(feature = "capture")]
mod alpha_dither_fixture;
#[cfg(feature = "capture")]
mod browser_fixtures;
#[cfg(feature = "capture")]
#[path = "../../../tools/rt64-oracle/fixture.rs"]
mod capture_fixture;
mod culling;
mod decode;
mod dl_plumbing;
mod e2e;
mod facade;
mod fb_store;
mod filter;
#[cfg(feature = "capture")]
mod filter_fixtures;
mod fog;
mod framebuffer;
mod gbi_roundtrip;
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
#[cfg(feature = "capture")]
mod sm64_corpus;
#[cfg(feature = "capture")]
#[path = "../../tests/common/sm64_semantics.rs"]
mod sm64_semantics;
#[cfg(feature = "capture")]
mod sm64_surface_fixtures;
mod stubs;
mod texgen;
mod texrect;
mod tile_sampling;
#[cfg(feature = "capture")]
mod tlut_fixtures;
mod unsupported_formats;

// `HostRam`-dependent → 64-bit non-wasm only (preserves host_mem.rs:14 / dlmemory_equivalence.rs:13).
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod dlmemory_equivalence;
#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
mod host_mem;

mod dl_builder;
pub(crate) mod fixtures;
mod scene_builders;
