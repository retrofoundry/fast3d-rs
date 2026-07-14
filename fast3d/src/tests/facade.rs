//! End-to-end equivalence tests for the `SceneRenderer` facade.
//!
//! These drive `SceneRenderer::new` + `render` against real test scenes and assert the SAME literal
//! pixel constants the toolbox scene tests in `render.rs` assert — proving the facade replicates
//! `web::render()`'s GPU flow (compute dispatch, content-keyed tex_cache, in-place uniform write,
//! z_buffer depth branch, and the clear-only path) faithfully.

use crate::tests::common;

use crate::render::{headless_device, SceneRenderer};
use common::{clear_color_rgb, pixel, render_to_pixels, scene_from_source, solid_env_texture};

const W: u32 = 64;
const H: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// 1. Textured + Linear sampler + depth branch: chrome-icosphere is the only Linear scene
///    and its render mode has z_test=true, exercising the facade's owned-depth
///    attachment. DECAL combiner → out = TEXEL0 = the sampled env texel ≈ (206, 99, 49).
#[test]
fn scene_renderer_textured_linear_depth_matches_toolbox() {
    let (device, queue, dual_source) = headless_device();
    let env = solid_env_texture([200, 100, 50]);
    let scene = scene_from_source("chrome-icosphere.n64", &env, 32, 32);
    assert!(
        scene.render_modes.iter().any(|r| r.z_test || r.z_write),
        "chrome-icosphere must have a depth render mode (exercises the owned-depth attachment)"
    );
    assert!(
        scene.materials.first().is_some_and(|m| m.tex_enable),
        "chrome-icosphere must be textured (DECAL uses TEXEL0)"
    );

    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);
    let buf = render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let [cr, cg, cb, _] = pixel(&buf, W, 32, 32);

    assert!(
        cr > 100,
        "DECAL center R must be > 100 (env ~206), got {cr}"
    );
    assert!(cg < 150, "DECAL center G must be < 150 (env ~99), got {cg}");
    assert!(cb < 100, "DECAL center B must be < 100 (env ~49), got {cb}");
}

/// 2. Untextured PRIM (no tex_cache rebuild path needed): flat-color → combiner = PRIMITIVE.
///    PRIM = gsDPSetPrimColor(64, 200, 255) → center pixel (64, 200, 255).
#[test]
fn scene_renderer_shade_only_matches_toolbox() {
    let (device, queue, dual_source) = headless_device();
    let white1x1 = vec![255u8; 4];
    let scene = scene_from_source("flat-color.n64", &white1x1, 1, 1);

    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);
    let buf = render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let [cr, cg, cb, _] = pixel(&buf, W, 32, 32);

    assert!((cr as i16 - 64).abs() <= 2, "PRIM R expected 64, got {cr}");
    assert!(
        (cg as i16 - 200).abs() <= 2,
        "PRIM G expected 200, got {cg}"
    );
    assert!(
        (cb as i16 - 255).abs() <= 2,
        "PRIM B expected 255, got {cb}"
    );
}

/// 3a. Clear-only via empty `raw_pos` (first disjunct of `raw_pos.is_empty() || indices.is_empty()`):
///     no draw, no depth attachment — every sampled pixel must be CLEAR_COLOR ≈ (13, 13, 20).
#[test]
fn scene_renderer_clear_only_empty_verts() {
    let (device, queue, dual_source) = headless_device();
    let mut scene = scene_from_source("flat-color.n64", &[255u8; 4], 1, 1);
    scene.raw_pos.clear(); // force the clear-only branch via the first disjunct

    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);
    let buf = render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let expect = clear_color_rgb();
    for &(x, y) in &[(0u32, 0u32), (32, 32), (63, 63), (10, 50)] {
        let [r, g, b, _] = pixel(&buf, W, x, y);
        assert_eq!(
            [r, g, b],
            expect,
            "clear-only pixel ({x},{y}) must be CLEAR_COLOR"
        );
    }
}

/// 3b. Clear-only via empty `indices` (second disjunct): same clear-only guarantee.
#[test]
fn scene_renderer_clear_only_empty_indices() {
    let (device, queue, dual_source) = headless_device();
    let mut scene = scene_from_source("flat-color.n64", &[255u8; 4], 1, 1);
    scene.indices.clear(); // force the clear-only branch via the second disjunct

    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);
    let buf = render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let expect = clear_color_rgb();
    for &(x, y) in &[(0u32, 0u32), (32, 32), (63, 63), (10, 50)] {
        let [r, g, b, _] = pixel(&buf, W, x, y);
        assert_eq!(
            [r, g, b],
            expect,
            "clear-only pixel ({x},{y}) must be CLEAR_COLOR"
        );
    }
}

/// 4. PERSISTENT content-keyed tex_cache + in-place uniform write: render TWICE on the SAME
///    SceneRenderer with two materials whose texture bytes DIFFER (same w,h). Frame 2's center
///    pixel must match the SECOND texture, not the first — the only test that catches a tex_cache
///    keying bug (e.g. keying on size only, or never invalidating).
#[test]
fn scene_renderer_tex_cache_rebuilds_on_content_change() {
    let (device, queue, dual_source) = headless_device();
    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);

    // Frame 1: orange-ish env (≈206, 99, 49 after RGBA16 round-trip).
    let env1 = solid_env_texture([200, 100, 50]);
    let scene1 = scene_from_source("chrome-icosphere.n64", &env1, 32, 32);
    let buf1 = render_to_pixels(&device, &queue, &mut sr, &scene1, W, H);
    let [r1, _, b1, _] = pixel(&buf1, W, 32, 32);

    // Frame 2: a CLEARLY different env (blue-dominant: low R, high B), same 32×32 size, same scene.
    // RGBA16 round-trip of (40, 60, 220): r≈41, g≈57, b≈222 — disjoint from frame 1's (206,_,49).
    let env2 = solid_env_texture([40, 60, 220]);
    let scene2 = scene_from_source("chrome-icosphere.n64", &env2, 32, 32);
    assert_ne!(
        scene1.materials.first().unwrap().texture,
        scene2.materials.first().unwrap().texture,
        "frame-2 texture bytes must differ from frame-1 (same w,h) to exercise content keying"
    );
    let buf2 = render_to_pixels(&device, &queue, &mut sr, &scene2, W, H);
    let [r2, _, b2, _] = pixel(&buf2, W, 32, 32);

    // Frame 1 is red-dominant (R high, B low); frame 2 is blue-dominant (R low, B high). If the
    // tex_cache failed to rebuild on the content change, frame 2 would still show frame 1's texel.
    assert!(
        r1 > 100 && b1 < 100,
        "frame 1 center must be orange env (R>100, B<100), got R={r1} B={b1}"
    );
    assert!(
        r2 < 100 && b2 > 100,
        "frame 2 center must be the SECOND (blue) env (R<100, B>100), got R={r2} B={b2} \
         — tex_cache did not rebuild on content change"
    );
}

/// 5. Per-pair FB-pool roundtrip: render a TRIS-ONLY scene two ways and assert pixel equality
///    within `TOL=2`.
///
///    Path A (pair-less): the flat `draw_runs` scene renders straight to the target.
///    Path B (paired): the SAME triangles are moved into a single `FramebufferPair` whose CIMG
///    width == facade W (64) and `active_scissor.lry` == H (64), so the FB pool allocates a 64×64
///    offscreen color target, the per-pair pass draws the tris into it, and the scanout blit copies
///    it 1:1 to the target (no resample). Equality (within ±2 for linear-sample float rounding)
///    proves the FB-pool + per-pair-pass + blit roundtrip is lossless. depth_image=None exercises
///    the color-only branch.
#[test]
fn scene_renderer_paired_tris_matches_pair_less() {
    const TOL: i16 = 2;
    let (device, queue, dual_source) = headless_device();

    // Path A: the unmodified pair-less scene (flat-color → one draw_run, no depth, PRIM combine).
    let pair_less = scene_from_source("flat-color.n64", &[255u8; 4], 1, 1);
    assert!(
        !pair_less.draw_runs.is_empty() && pair_less.framebuffer_pairs.is_empty(),
        "flat-color must be a pair-less draw_runs scene"
    );

    // Path B: clone it, move every draw_run into a single FramebufferPair as a Tris op (CIMG
    // width 64, scissor lry 64 → 64×64 FB, 1:1 scanout blit), and clear draw_runs.
    let mut paired = pair_less.clone();
    let ops: Vec<crate::hle::SceneOp> = paired
        .draw_runs
        .drain(..)
        .map(crate::hle::SceneOp::Tris)
        .collect();
    paired.framebuffer_pairs = vec![crate::hle::FramebufferPair {
        color_image: crate::hle::ColorImage {
            fmt: 0,
            siz: 2, // G_IM_SIZ_16b
            width: W as u16,
            addr: 0x0010_0000,
        },
        depth_image: None,
        ops,
        active_scissor: crate::hle::Scissor {
            ulx: 0,
            uly: 0,
            lrx: W as i32,
            lry: H as i32,
            mode: 0,
        },
        size_extent: (W, H),
        is_depth_clear: false,
    }];

    let mut sr = SceneRenderer::new(&device, FORMAT, W, H, dual_source);
    let buf_a = render_to_pixels(&device, &queue, &mut sr, &pair_less, W, H);
    let buf_b = render_to_pixels(&device, &queue, &mut sr, &paired, W, H);

    assert_eq!(buf_a.len(), buf_b.len(), "buffers must be the same length");
    let mut max_diff = 0i16;
    for (i, (&a, &b)) in buf_a.iter().zip(buf_b.iter()).enumerate() {
        let d = (a as i16 - b as i16).abs();
        max_diff = max_diff.max(d);
        assert!(
            d <= TOL,
            "byte {i} differs beyond TOL={TOL}: pair-less={a} paired={b} (Δ={d})"
        );
    }
    // Sanity: the paired path must have actually drawn the quad (not just a clear), so the center
    // is the PRIM color, distinct from the background clear.
    let [cr, cg, cb, _] = pixel(&buf_b, W, 32, 32);
    assert!(
        (cr as i16 - 64).abs() <= TOL
            && (cg as i16 - 200).abs() <= TOL
            && (cb as i16 - 255).abs() <= TOL,
        "paired center must be PRIM (64,200,255), got ({cr},{cg},{cb}) — max Δ vs pair-less {max_diff}"
    );
}

/// Phase 1 (spec §4): the pair-less flat-3D path renders into an INTERNAL framebuffer sized to the
/// SceneRenderer's surface dims (from `new`), NOT the caller's `target`. Proof: render a depth-using
/// pair-less scene into a target SMALLER than the FB. Before the rework, the 32×32 color target and
/// the owned 64×64 depth buffer are both attached to one pass → a wgpu attachment-size-mismatch
/// validation error. After the rework, geometry rasterizes into the 64×64 internal FB (matching the
/// 64×64 depth) and is blitted (down-scaled) to the 32×32 target — no error.
#[test]
fn pair_less_depth_decouples_from_target_size() {
    let (device, queue, dual_source) = headless_device();
    let env = solid_env_texture([200, 100, 50]);
    let scene = scene_from_source("chrome-icosphere.n64", &env, 32, 32);
    assert!(
        scene.framebuffer_pairs.is_empty()
            && scene.render_modes.iter().any(|r| r.z_test || r.z_write),
        "chrome-icosphere must be a pair-less depth scene"
    );
    let mut sr = SceneRenderer::new(&device, FORMAT, 64, 64, dual_source);

    // A target SMALLER than the internal FB (32×32 vs new(64, 64)).
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("small-target"),
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    sr.render(&device, &queue, &scene, &view);
    let err = pollster::block_on(scope.pop());
    assert!(
        err.is_none(),
        "pair-less depth render into a smaller target must not raise a validation error \
         (internal FB is decoupled from target size); got: {err:?}"
    );
}
