# Oracle validation, 2026-09-05

Implemented on `rt64-oracle-local`, based on `728380504af2ec7b382e9022414573ddeeffed24`.
The rt64 source and static build were read only. No rt64 executable was run, and no
reference PNG, fast3d fixture replay PNG, cross-renderer pixel count or acceptance
budget has been established. Claude needs to run the GPU steps in [README.md](README.md).
That file contains the full build, fixture generation, export, render, replay and
comparison sequence for both fixtures.

Files:

- `fast3d/examples/export_capture_rdram.rs`: IMAGE-only 8 MiB RDRAM export and JSON metadata.
- `fast3d/examples/compare_rgba8.rs`: channel differences, threshold count, bounds, PNG mask and optional budget.
- `fast3d/src/hle/capture.rs` and `capture/tests.rs`: capture-only CPU final-colour inspection, using the existing interpreter; nested-list and segment test.
- `fast3d/src/tests/{texgen,fog,mod}.rs` and `tools/rt64-oracle/fixture.rs`: shared raw scene construction and ignored fixture writers.
- `fast3d/Cargo.toml`: capture feature gate for the exporter example.
- `tools/rt64-oracle/{CMakeLists.txt,main.cpp}`: standalone macOS C++17 harness, direct GBI selection, native RDRAM readback, PNG/RGBA8/log outputs.
- `tools/rt64-oracle/README.md` and `docs/ROADMAP.md`: commands, scope and limitations.

No renderer or interpreter behavior changed. The small public capture helper is
needed because examples cannot access the crate-private CPU interpreter. The C++
project is not a Cargo workspace member and has no CI build hook.

All Rust commands below ran through `XDG_CACHE_HOME=/tmp/fast3d-oracle-cache devenv shell --`.
The initial plain `devenv shell` failed before Cargo started because the default
Nix SQLite cache was outside the writable sandbox. Moving the cache to `/tmp`
resolved it; no toolchain installation or repository configuration change was needed.

| Check | Result |
| --- | --- |
| `cargo fmt --all` | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed, default features. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --workspace --all-features --no-fail-fast` | Exit 101: 428 passed, 113 failed for unavailable GPU, 5 ignored. Names below. |
| `cargo build -p fast3d --target wasm32-unknown-unknown --features 'capture'` | Passed. |
| `cargo test -p fast3d --features capture --lib hle::capture::tests` | 14 passed. |
| `cargo test -p fast3d --features capture --examples` | 6 passed: 4 exporter tests and 2 comparison tests. |
| Mario state and fog CPU tests | 6 passed; original command-derived states preserved. |
| Wrapper identity matrix regression | Passed after adding the missing projection load; failed before it. |
| `FAST3D_WRITE_FIXTURES=/tmp/fast3d-oracle cargo test -p fast3d --features 'capture' --lib write_rt64_ -- --ignored` | 2 passed; both captures written and decoded back. |
| Export both generated fixtures | Passed. Independent JSON/size check: each one F3D task, 8 MiB image, RGBA16 320x240 at `0x00100000`. |
| Export `fast3d/tests/fixtures/host64-fill.f3dcap` | Correctly rejected with explicit HOST64/IMAGE error, exit 1. |
| Compare hand-checked 3x2 RGBA8 inputs | Max 255, RGBA maxima `[9,10,255,11]`, 3 differing pixels, bounds `(0,0)..(2,1)`. |
| Same comparison, `--max-diff-pixels 2` / `3` | Correctly returned exit 1 / 0, respectively. Both wrote masks. |
| Same comparison, wrong dimensions 2x2 | Correctly rejected 24-byte files where 16 bytes were required, exit 1. |
| Independent diff PNG decode | Passed: grayscale 3x2, rows `[0,255,0]` and `[255,0,255]`. |
| CMake configure and compile/link in the requested devenv/SDL2/zlib shell and xcrun-shim PATH | Passed with Release, C++17, `-include cstdlib`, xcrun-selected clang/clang++; executable at `/tmp/rt64-oracle-build/rt64-oracle`. Not run. |
| `git diff --check` | Passed. |
| Independent code review and comment audit | Projection initialization and Release unknown-opcode handling fixed; scoped re-review approved. No comment deletions needed. |

The CMake commands are in the README verbatim. They link the existing rt64,
re-spirv, nfd, zstd and plume archives plus SDL2 and Apple frameworks. The harness
compiles the reference's `xxhash.c` because rt64's archive requires external XXH
symbols. No archive or microcode binary was copied into this repository.

Source corrections and limits:

- The supplied rt64 checkout does contain SM64 F3D hashes: `src/gbi/rt64_gbi.cpp:58`,
  `:172`, `:271`. Direct GBI selection still avoids a microcode binary. Both F3D
  and F3DEX2 tables use the initialization sequence at `:462` and flags at `:509`.
- `src/hle/rt64_vi.cpp:81` subtracts one row from origin. The harness therefore sets
  origin to colour address plus row bytes; its decoded framebuffer points at the
  colour target. `:115` explains the height adjustment. `--scale` changes only
  window size, not the native oracle.
- `src/hle/rt64_rsp.cpp:48` starts matrix stacks at zero. Both fixture wrappers load
  identity projection and modelview explicitly. Both use 320x240 to avoid the known
  fast3d viewport-fold limitation; the original Mario 256x256 test is unchanged.
- Ordered tasks share one exported image. Conflicting captured bytes are rejected;
  arbitrary changing snapshots, RAM feedback, prior GPU contents, and host-pointer
  captures are outside this tool. Standard F3D/F3DEX2 flags are supported.
- Release rt64 removes debug logging and some internal assertions
  (`src/common/rt64_common.h:37`). The harness rejects unmapped opcodes, but cannot
  restore checks compiled out of supported handlers in the supplied archive.
- FullSync's native RAM path waits for the graphics worker before copyback
  (`src/hle/rt64_state.cpp:1471`, `:1479`), then the harness waits for workload and
  presentation completion following `src/hle/rt64_application.cpp:528`.
  SDL creation, GPU rendering, queue completion at runtime, actual RGBA16/32 GPU
  writeback and cross-renderer fidelity remain unverified here.

The final workspace test run failed in 108 library tests and all 5 capture-facade
tests. Every failure originated in missing adapter availability; some facade tests
then compared that GPU error against the error they had intended to exercise.
The five ignored tests were the two fixture writers, two dual-source GPU tests,
and the ROM-dependent `hle::rsp_f3d::phase6_tests::sm64_us_bob_rom_display_list`.
No golden was updated.

Failed tests, listed in full:

```text
begin_frame_tests::begin_frame_clears_retained_frame_scenes
begin_frame_tests::process_dl_draw_nothing_keeps_last_good_scanout_addr
capture_checked_in_high_address_fixture_renders
capture_public_replay_rejects_missing_span
capture_public_replay_rejects_prior_framebuffer_dependence
config_construction_tests::reconfigure_headless_rebuilds_and_stores_config
config_construction_tests::resize_headless_is_noop_and_keeps_surface_format
config_construction_tests::shutdown_headless_drains_without_panic
config_construction_tests::with_device_headless_builds_and_exposes_handles
debug::render_tests::debugger_draw_runs_headless_and_counts_all_scenes
debug::render_tests::debugger_repaints_after_surface_format_change
debugger_present_tests::debugger_composites_over_scanout_without_erasing_the_game
debugger_toggle_tests::debugger_is_off_by_default_and_toggles
fixture_image_vi_selects_captured_framebuffer
hle::combiner::fidelity_tests::pixels::combiner_alpha_color_inputs_render
hle::combiner::fidelity_tests::pixels::combiner_selector_pixels_dualsrc_and_fallback
hook_lifecycle_tests::drop_without_shutdown_fires_deinit
hook_lifecycle_tests::set_draw_hook_accepts_a_non_send_closure
hook_lifecycle_tests::set_render_hook_fires_init_once_and_replace_orders_deinit_before_init
hook_lifecycle_tests::shutdown_fires_deinit_before_device_drops
hook_lifecycle_tests::take_render_hook_fires_deinit_and_removes
hooks::tests::hook_frame_releases_the_encoder_borrow_at_draw_return
host::fixture_public_facade_matches_live_backend
present_headless_tests::present_on_a_headless_target_is_a_noop_ok
render::tests::headless_device_reports_dual_source_flag
render::tests::scene_renderer_new_at_rgba8_and_bgra8
tests::facade::pair_less_depth_decouples_from_target_size
tests::facade::scene_renderer_clear_only_empty_indices
tests::facade::scene_renderer_clear_only_empty_verts
tests::facade::scene_renderer_paired_tris_matches_pair_less
tests::facade::scene_renderer_shade_only_matches_toolbox
tests::facade::scene_renderer_tex_cache_rebuilds_on_content_change
tests::facade::scene_renderer_textured_linear_depth_matches_toolbox
tests::fb_store::clear_policy_persist_keeps_prior_frame_perframe_clears
tests::fb_store::draw_nothing_walk_returns_none_and_touches_nothing
tests::fb_store::paired_walk_stores_by_cimg_addr_and_scans_out
tests::fb_store::pairless_walk_stores_and_scans_out
tests::fb_store::store_fb_recreates_on_resize
tests::fog::fixture_sm64_jrb_mixed_fog
tests::fog::fog_factors_gpu_readback
tests::fog::fog_modify_rgba_gpu_readback
tests::fog::fog_reused_vertex_gpu_readback
tests::fog::fog_texrect_color_gpu_readback
tests::goldens::alphaover_pipeline_blends_translucent_over_background
tests::goldens::decal_coplanar_tolerance_boundary
tests::goldens::golden_2d_alpha_texrect_over_bg
tests::goldens::golden_2d_bgra8_present_cover
tests::goldens::golden_2d_copy_alpha_keyed_over_bg
tests::goldens::golden_2d_fill_texrect
tests::goldens::golden_2d_hud_over_3d
tests::goldens::golden_2d_offscreen_then_sample
tests::goldens::golden_2d_rect_geometry_exact
tests::goldens::golden_2d_texrectflip
tests::goldens::golden_alpha_threshold
tests::goldens::golden_ci4_canary
tests::goldens::golden_ci4_grid
tests::goldens::golden_ci8_canary
tests::goldens::golden_ci8_ramp
tests::goldens::golden_decal
tests::goldens::golden_fogworld
tests::goldens::golden_high_poly
tests::goldens::golden_i4_ramp
tests::goldens::golden_i8_ramp
tests::goldens::golden_ia16_ramp
tests::goldens::golden_ia4_ramp
tests::goldens::golden_ia8_ramp
tests::goldens::golden_mirror_repeat
tests::goldens::golden_multi_material
tests::goldens::golden_multi_material_cutout_shows_hole
tests::goldens::golden_multi_material_forced_fallback
tests::goldens::golden_paired_decal_matches_pair_less
tests::goldens::golden_paired_decal_respects_op_order
tests::goldens::golden_pairless_chrome_icosphere
tests::goldens::golden_pairless_flat_color
tests::goldens::golden_rgba16_quad
tests::goldens::golden_tron
tests::goldens::golden_tron_forced_fallback
tests::goldens::golden_wrap_repeat
tests::render::chrome_icosphere_decal_pixel_is_env_texel_not_black
tests::render::compute_outputs_match_oracle_for_every_scene
tests::render::cull_back_mode_keeps_n64_front_drops_n64_back
tests::render::cycle_type_1_two_cycle_combiner_pixel
tests::render::decal_pass_structure_does_not_panic_and_depth_is_sampleable
tests::render::depth_test_hides_the_farther_triangle
tests::render::detail_mode_samples_detail_tap_at_magnification
tests::render::flat_color_prim_pixel_equals_gsdpsetprimcolor
tests::render::fog_factor_uses_raw_clip_z
tests::render::kernel_lit_color_front_facing_and_back_facing
tests::render::kernel_lit_color_two_directional_lights_plus_ambient
tests::render::modulate_is_texel_times_shade_and_decal_is_texel
tests::render::nonhalving_lod_blends_two_same_size_levels_at_mid_lod
tests::render::nonhalving_lod_magnify_minify_select_level0_and_coarsest
tests::render::renders_red_triangle_center_and_clear_corner
tests::render::sharpen_mode_extrapolates_below_plain_level0_at_magnification
tests::render::single_run_path_renders_textured_center
tests::render::trilinear_lod_magnify_minify_and_blend_match_hand_computed_pixel
tests::render::two_cycle_role_swap_cycle1_texel0_reads_tex1
tests::render::two_cycle_two_texture_blend_matches_hand_computed_pixel
tests::render::two_material_scene_renders_two_distinct_regions
tests::render::two_material_scene_routes_per_material_texture
tests::renderer_hooks::hook_extra_command_buffers_run_before_frame_encoder
tests::renderer_hooks::hook_full_lifecycle_end_to_end
tests::renderer_hooks::hook_overlay_composites_over_scanout
tests::renderer_hooks::present_to_reports_target_dimensions
tests::renderer_hooks::present_without_hook_is_unchanged
tests::renderer_present::present_to_scans_out_the_last_rendered_framebuffer
tests::renderer_process_dl::process_dl_of_flat_quad_is_renderable
tests::renderer_process_dl::process_dl_of_out_of_bounds_entry_reports_error_without_panic
tests::rsp_tests::phase4_modify_vertex_screen_override_renders_requested_pixel_and_depth
tests::texgen::fixture_sm64_mario_metal_butt
tests::texgen::texgen_linear_matches_acos
tests::texgen::texgen_metal_scale_yields_62_by_31
tests::texgen::texgen_mixed_vertices_share_texel_units
```
