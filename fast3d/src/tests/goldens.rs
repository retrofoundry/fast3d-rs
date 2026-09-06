//! Headless golden-image harness for the renderer.
//!
//! Each test renders a test scene offscreen and compares the pixel output to a committed golden
//! (`.bin` raw-RGBA8 file).  Running with `UPDATE_GOLDENS=1` writes a new golden instead of
//! comparing.
//!
use crate::render::{headless_device, headless_device_forced_fallback, CLEAR_COLOR};

use crate::tests::common;

/// Maps an N64 wrap mode (cms/cmt: 0=WRAP, 1=MIRROR, 2+=CLAMP) to a wgpu `AddressMode`.
/// Mirrors `crate::render::address_mode` (private) so golden tests can select the correct sampler.
fn address_mode(wrap: u8) -> wgpu::AddressMode {
    match wrap {
        0 => wgpu::AddressMode::Repeat,
        1 => wgpu::AddressMode::MirrorRepeat,
        _ => wgpu::AddressMode::ClampToEdge,
    }
}

fn set_address_modes(
    tile: &mut crate::hle::tile_sampling::TileSampling,
    addr_u: wgpu::AddressMode,
    addr_v: wgpu::AddressMode,
) {
    for (axis, mode) in [addr_u, addr_v].into_iter().enumerate() {
        let (mode, mask) = match mode {
            wgpu::AddressMode::ClampToEdge => (2, 0),
            wgpu::AddressMode::Repeat | wgpu::AddressMode::MirrorRepeat => {
                let extent = tile.image[axis];
                assert!(extent.is_power_of_two() && extent <= 1 << 15);
                (
                    u32::from(mode == wgpu::AddressMode::MirrorRepeat),
                    extent.ilog2(),
                )
            }
            _ => panic!("unsupported golden address mode: {mode:?}"),
        };
        tile.modes[axis] = mode;
        tile.shift_mask[axis + 2] = mask;
    }
}

/// Maximum per-channel absolute difference allowed in golden comparisons.
const TOL: u8 = 2;

// ── helpers ──────────────────────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)] // test helper: all 8 params are logically distinct
fn render_scene_with_device(
    name: &str,
    w: u32,
    h: u32,
    addr_u: wgpu::AddressMode,
    addr_v: wgpu::AddressMode,
    device: wgpu::Device,
    queue: wgpu::Queue,
) -> Vec<u8> {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture(name);
    let mut result = crate::hle::interpret_rdram(rdram, entry_addr as u32);
    assert!(result.diags.is_empty(), "HLE diags: {:?}", result.diags);
    for material in &mut result.scene.materials {
        set_address_modes(&mut material.sampling, addr_u, addr_v);
    }
    let mut renderer = crate::render::SceneRenderer::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        w,
        h,
        device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING),
    );
    common::render_to_pixels(&device, &queue, &mut renderer, &result.scene, w, h)
}

/// Render a `.n64` test scene source using the primary headless device (dual-source when available)
/// and explicit sampler address modes.
///
/// Thin wrapper around `render_scene_with_device` that creates the primary `(device, queue)`.
/// Called by `render_scene_to_rgba8` (ClampToEdge) and the wrap/mirror golden tests.
fn render_scene_to_rgba8_addr(
    name: &str,
    w: u32,
    h: u32,
    addr_u: wgpu::AddressMode,
    addr_v: wgpu::AddressMode,
) -> Vec<u8> {
    let (device, queue, _dual_source) = headless_device();
    render_scene_with_device(name, w, h, addr_u, addr_v, device, queue)
}

/// Render a `.n64` test scene source through the **forced-fallback device** (dual-source disabled),
/// exercising the B3 AlphaOver/Replace pipelines deterministically.
///
/// Uses `headless_device_forced_fallback()` which requests `Features::empty()` even when the
/// adapter supports `DUAL_SOURCE_BLENDING`.  `TexturedPipeline::new` then builds only the
/// AlphaOver/Replace fallback pipelines — the dual-source WGSL module is never compiled.
///
/// [IMP13] Called for every AlphaOver-expressible scene each CI run so the fallback module +
/// pipelines are compiled and rendered (a web-only fallback break cannot ship green).
fn render_scene_to_rgba8_forced_fallback(name: &str, w: u32, h: u32) -> Vec<u8> {
    let (device, queue) = headless_device_forced_fallback();
    render_scene_with_device(
        name,
        w,
        h,
        wgpu::AddressMode::ClampToEdge,
        wgpu::AddressMode::ClampToEdge,
        device,
        queue,
    )
}

fn render_scene_to_rgba8(name: &str, w: u32, h: u32) -> Vec<u8> {
    render_scene_to_rgba8_addr(
        name,
        w,
        h,
        wgpu::AddressMode::ClampToEdge,
        wgpu::AddressMode::ClampToEdge,
    )
}

/// Compare `actual` RGBA8 pixels against the committed golden `.bin` file, or write the golden
/// when `UPDATE_GOLDENS=1` is set.
///
/// Goldens live in `crates/renderer/goldens/<name>.bin` (raw RGBA8, `w × h × 4` bytes).
/// The comparison tolerates a max per-channel absolute difference of `TOL` to absorb
/// platform-specific rounding in GPU rasterisation.
fn compare_or_write(name: &str, actual: &[u8], w: u32, h: u32) {
    if let Ok(dir) = std::env::var("FAST3D_GOLDEN_OUTPUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.bin")),
            actual,
        )
        .unwrap();
    }
    let path = format!("{}/goldens/{name}.bin", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_GOLDENS").is_ok() {
        std::fs::write(&path, actual)
            .unwrap_or_else(|e| panic!("failed to write golden {path}: {e}"));
        eprintln!("golden written: {path} ({} bytes, {w}×{h})", actual.len());
        return;
    }
    let golden = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "golden '{name}' missing ({path}): {e}\n\
             Run `UPDATE_GOLDENS=1 cargo test -p renderer golden_{name}` to generate it."
        )
    });
    assert_eq!(
        golden.len(),
        actual.len(),
        "{name}: golden size {} ≠ actual size {} (expected {w}×{h}×4={})",
        golden.len(),
        actual.len(),
        w * h * 4
    );
    let max = actual
        .iter()
        .zip(golden.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(max <= TOL, "{name}: max per-channel diff {max} > {TOL}");
}

// ── Tier-1 texture format golden seeds ───────────────────────────────────────────────────────────

// ── tests ─────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn golden_rgba16_quad() {
    let px = render_scene_to_rgba8("textured-quad--rgba16-4x4", 64, 64);
    compare_or_write("rgba16-quad", &px, 64, 64);
}

#[test]
fn golden_i8_ramp() {
    let px = render_scene_to_rgba8("i8-ramp--4x4", 64, 64);
    compare_or_write("i8-ramp", &px, 64, 64);
}

/// Golden test for I4 intensity format — same vertical ramp through 4-bit encode/decode.
///
/// I4 quantises to 4 bits (>> 4) then replicates ((v4 << 4) | v4). The seed values 0, 85,
/// 170, 255 are all exact I4 round-trips, so the I4 golden should be byte-identical to I8.
/// Row-scramble or banding differences indicate a nibble-order or TMEM layout bug.
#[test]
fn golden_i4_ramp() {
    let px = render_scene_to_rgba8("i4-ramp--4x4", 64, 64);
    compare_or_write("i4-ramp", &px, 64, 64);
}

/// Golden test for IA16 intensity+alpha format — vertical ramp (black→white).
///
/// Alpha validation: the combiner uses SHADE alpha (vertex alpha=255), so texel alpha does NOT
/// reach the framebuffer. The golden validates the INTENSITY/color channel only.
/// Alpha decode correctness is validated by unit tests (`ia16_splits_intensity_and_alpha`, etc.).
/// The swizzle bet: IA16 is a 2-byte format (siz=2); the linear-TMEM decoder iterates two bytes
/// per texel, so nibble-order is irrelevant — no swizzle risk.
#[test]
fn golden_ia16_ramp() {
    let px = render_scene_to_rgba8("ia16-ramp--4x4", 64, 64);
    compare_or_write("ia16-ramp", &px, 64, 64);
}

/// Golden test for IA8 intensity+alpha format.
///
/// Alpha validation: same as IA16 — combiner outputs SHADE alpha, not texel alpha.
/// Unit tests cover alpha decode. Swizzle: IA8 is 1-byte/texel, no nibble order.
#[test]
fn golden_ia8_ramp() {
    let px = render_scene_to_rgba8("ia8-ramp--4x4", 64, 64);
    compare_or_write("ia8-ramp", &px, 64, 64);
}

/// Golden test for IA4 intensity+alpha format — 4-bit format, 2 texels/byte.
///
/// Alpha validation: combiner outputs SHADE alpha; texel alpha validated by unit tests.
/// Swizzle: IA4 uses the same nibble order as I4 (even col = high nibble). The 2×2 multi-row
/// unit test (`ia4_multirow_swizzle_canary`) is the primary swizzle check; the golden confirms
/// the intensity bands are visible and row-distinct.
#[test]
fn golden_ia4_ramp() {
    let px = render_scene_to_rgba8("ia4-ramp--4x4", 64, 64);
    compare_or_write("ia4-ramp", &px, 64, 64);
}

// ── Wrap / mirror sampler golden seeds ───────────────────────────────────────────────────────────

#[test]
fn golden_address_modes_reach_tile_sampling() {
    for (mode, scene) in [(0, "wrap-repeat"), (1, "mirror-repeat")] {
        for (name, extent, mask) in [
            (format!("{scene}--white4"), 4usize, 0),
            (scene.to_owned(), 32, 5),
        ] {
            let (rdram, entry_addr) = crate::tests::fixtures::fixture(&name);
            let interp = crate::hle::interpret_rdram(rdram, entry_addr as u32);
            assert!(interp.diags.is_empty(), "{:?}", interp.diags);
            let mat = &interp.scene.materials[0];
            let mut sampling = crate::render::material_sampling(mat);
            assert_eq!(sampling[0].image, [extent as u32, extent as u32, 0, 0]);
            assert_eq!(sampling[0].shift_mask, [0, 0, mask, mask]);
            assert_eq!(sampling[0].modes[..2], [mode.into(); 2]);
            let inv_size = crate::render::triangle_inv_tex_size(mat);
            assert_eq!(inv_size[0] * extent as f32, 1.0);
            assert_eq!(inv_size[1] * extent as f32, 1.0);
            set_address_modes(&mut sampling[0], address_mode(mode), address_mode(mode));
            let expected_mask = extent.ilog2();
            assert_eq!(sampling[0].shift_mask, [0, 0, expected_mask, expected_mask]);
            assert_eq!(sampling[0].modes[..2], [mode.into(); 2]);
            set_address_modes(
                &mut sampling[0],
                wgpu::AddressMode::ClampToEdge,
                wgpu::AddressMode::ClampToEdge,
            );
            assert_eq!(sampling[0].shift_mask, [0; 4]);
            assert_eq!(sampling[0].modes[..2], [2; 2]);
        }
    }
}

/// Golden test for WRAP (Repeat) sampler — 4×4 two-colour-block texture tiled 2×2.
///
/// The quad UVs span [0,2] (S/T vertices at 256 on a 4-texel tile with sc=0xFFFF).
/// `cms=cmt=0` routes to `samplers[0][0]` (wgpu Repeat).  With Repeat the rendered image shows
/// four quadrants of the texture (R/G/B/Y repeated 2×2), visibly different from a ClampToEdge
/// render that would fill the outer ~50 % of the quad with the clamped edge colour.
#[test]
fn golden_wrap_repeat() {
    let px =
        render_scene_to_rgba8_addr("wrap-repeat--4x4", 64, 64, address_mode(0), address_mode(0));
    compare_or_write("wrap-repeat", &px, 64, 64);
}

/// Golden test for MIRROR (MirrorRepeat) sampler — same geometry, `cms=cmt=1`.
///
/// With MirrorRepeat the second tile (UV 1..2) is the horizontal/vertical reflection of the
/// first, so the rendered image shows R|G||G|R (top) and B|Y||Y|B (bottom) — distinct from
/// both WRAP (which repeats R|G||R|G) and CLAMP.
#[test]
fn golden_mirror_repeat() {
    let px = render_scene_to_rgba8_addr(
        "mirror-repeat--4x4",
        64,
        64,
        address_mode(1),
        address_mode(1),
    );
    compare_or_write("mirror-repeat", &px, 64, 64);
}

/// Golden test for CI8 color-indexed format — vertical ramp via RGBA16 palette.
///
/// The MODULATE combiner with white shade passes through the palette colors; each of the 32
/// rows should render as a distinct horizontal band of grayscale. Clean gradient confirms both
/// the CI8 encoder, TLUT loading, and palette decode are correct.
/// Row-distinct flat regions also serve as the swizzle canary: a TMEM row-swap would produce
/// bands out of order, immediately visible.
#[test]
fn golden_ci8_ramp() {
    let px = render_scene_to_rgba8("ci8-ramp--palette", 64, 64);
    compare_or_write("ci8-ramp", &px, 64, 64);
}

/// Golden test for CI8 combine-route canary — TEXEL0_ALPHA routed into RGB output.
///
/// Combiner: (ONE−0)×TEXEL0_ALPHA+0 = texel.alpha as grayscale.
/// CI8_TEX palette has alternating a1 (0/1): even-row entries → alpha=255 (white output),
/// odd-row entries → alpha=0 (black output). The golden MUST show alternating bands,
/// NOT solid white. Solid white means tex_enable is false (TEXEL0_ALPHA not wired) — a
/// false pass matching the broken IA state; do not bake if white.
#[test]
fn golden_ci8_canary() {
    let px = render_scene_to_rgba8("ci8-ramp--canary", 64, 64);
    // Verify the canary output is NOT solid white (which would indicate tex_enable=false bug).
    let max_r = px.chunks(4).map(|p| p[0]).max().unwrap_or(0);
    let min_r = px.chunks(4).map(|p| p[0]).min().unwrap_or(255);
    assert!(
        max_r > 200 && min_r < 50,
        "ci8-canary rendered solid color (max_r={max_r}, min_r={min_r}); \
         expected alternating white/black bands (TEXEL0_ALPHA path broken?)"
    );
    compare_or_write("ci8-canary", &px, 64, 64);
}

// ── CI4 golden seeds ─────────────────────────────────────────────────────────────────────────────

/// Golden test for CI4 color-indexed format — 4×4 rainbow grid via RGBA16 palette.
///
/// The MODULATE combiner with white shade passes through the palette colors; the 4×4 grid
/// of 8×8 solid-color cells should each render as a distinct flat region. Flat-cell distinct
/// regions make palette-index scrambles (nibble-order or TMEM layout bugs) immediately visible.
/// SHADE alpha (vertex alpha=255) is used as the alpha output, so even transparent-palette cells
/// (odd indices, a1=0) render fully opaque with their palette RGB.
#[test]
fn golden_ci4_grid() {
    let px = render_scene_to_rgba8("ci4-grid--palette", 64, 64);
    compare_or_write("ci4-grid", &px, 64, 64);
}

/// Golden test for CI4 combine-route canary — TEXEL0_ALPHA routed into RGB output.
///
/// Combiner: (ONE−0)×TEXEL0_ALPHA+0 = texel.alpha as grayscale.
/// CI4_TEX palette has alternating a1 (0/1): even-index cells → alpha=255 (white output),
/// odd-index cells → alpha=0 (black output). The golden MUST show a non-uniform alternating
/// chequerboard of 8×8 bright and dark cells — NOT solid white. Solid white means
/// TEXEL0_ALPHA is not wired for CI4+TLUT — a false pass; do not bake if white.
#[test]
fn golden_ci4_canary() {
    let px = render_scene_to_rgba8("ci4-grid--canary", 64, 64);
    // Verify the canary output is NOT solid white (which would indicate tex_enable=false bug).
    let max_r = px.chunks(4).map(|p| p[0]).max().unwrap_or(0);
    let min_r = px.chunks(4).map(|p| p[0]).min().unwrap_or(255);
    assert!(
        max_r > 200 && min_r < 50,
        "ci4-canary rendered solid color (max_r={max_r}, min_r={min_r}); \
         expected alternating white/black cells (TEXEL0_ALPHA path broken for CI4?)"
    );
    compare_or_write("ci4-canary", &px, 64, 64);
}

// ── B3: AlphaOver fallback-blend smoke test ───────────────────────────────────────────────────────

/// B3 smoke test: AlphaOver pipeline blends a translucent XLU quad over the background.
///
/// A full-screen red quad with prim_alpha=128/255≈0.502 and G_RM_AA_ZB_XLU_SURF must BLEND
/// over the clear color (not Replace it). With AlphaOver:
///   result.R ≈ 0.502 * 255 + 0.498 * clear(13) ≈ 134 → strictly between clear(13) and 255.
/// With the old Replace pipeline:
///   result.R = 255 → fails `px[c] < 220`.
///
/// Placed before golden_multi_material so a failing blend assertion stops the run early.
#[test]
fn alphaover_pipeline_blends_translucent_over_background() {
    // 1×1 white placeholder; the XLU scene has no gsDPLoadTextureBlock so tex_enable=false.
    let px = render_scene_to_rgba8("flat-color--translucent", 32, 32);
    // Center pixel of the 32×32 render: row 16, col 16.
    let c = ((16 * 32 + 16) * 4) as usize;
    // AlphaOver result ≈ 134; Replace result = 255.
    // CLEAR_COLOR.r=0.05 → ~13 in u8; prim.r=1.0 → 255 in u8.
    assert!(
        px[c] > 60 && px[c] < 220,
        "expected AlphaOver blend (R≈134), got R={}; Replace pipeline still active?",
        px[c]
    );
}

// ── Multi-material golden ─────────────────────────────────────────────────────────────────────────

/// Golden test for multi-material per-run binding — Phase A gate.
///
/// One display list with THREE quads, each preceded by its own `gsDPSetCombineLERP` +
/// `gsDPSetRenderMode`:
/// - Left (pixels 0–32): opaque textured (TEXEL0 × white-SHADE, G_RM_OPA_SURF).
/// - Center (pixels 32–64): flat-primitive blue (PRIMITIVE combiner, G_RM_AA_ZB_XLU_SURF);
///   renders OPAQUE in Phase A (blender wired in Phase B).
/// - Right (pixels 64–96): textured × orange-SHADE (G_RM_AA_ZB_TEX_EDGE); renders OPAQUE
///   in Phase A (alpha-test wired in Phase D).
///
/// PASS: three visually distinct regions (not a single flat colour — the old collapse).
/// BLOCKED if the centre equals the left (dedup collapsed three materials into one).
#[test]
fn golden_multi_material() {
    // 3 regions render 3 distinct materials (not the old flat collapse).
    let px = render_scene_to_rgba8("multi-material--rgba16-4x4", 96, 96);
    // Canary: a left-third pixel vs. a centre-third pixel must differ. Row 60 and columns 12/76
    // sit mid-texel (texel row 1, column 1: green) so the sample does not straddle a texel edge.
    let row = 60usize;
    let stride = 96usize;
    let left_r = px[(row * stride + 12) * 4];
    let left_g = px[(row * stride + 12) * 4 + 1];
    let left_b = px[(row * stride + 12) * 4 + 2];
    let centre_r = px[(row * stride + 48) * 4];
    let centre_g = px[(row * stride + 48) * 4 + 1];
    let centre_b = px[(row * stride + 48) * 4 + 2];
    let right_r = px[(row * stride + 76) * 4];
    let right_g = px[(row * stride + 76) * 4 + 1];
    let right_b = px[(row * stride + 76) * 4 + 2];
    // Centre quad is flat blue (PRIMITIVE = 0,0,255) — assert blue channel dominant.
    assert!(
        centre_b > 200 && centre_b > centre_r + 100,
        "multi-material: centre quad not blue (r={centre_r},g={centre_g},b={centre_b}); \
         expected flat PRIMITIVE blue — per-material binding broken?"
    );
    // Left and right must differ from centre (proves distinct material binding, not collapse).
    let left_diff = (left_r as i32 - centre_r as i32).unsigned_abs()
        + (left_g as i32 - centre_g as i32).unsigned_abs()
        + (left_b as i32 - centre_b as i32).unsigned_abs();
    let right_diff = (right_r as i32 - centre_r as i32).unsigned_abs()
        + (right_g as i32 - centre_g as i32).unsigned_abs()
        + (right_b as i32 - centre_b as i32).unsigned_abs();
    assert!(
        left_diff > 60,
        "multi-material: left region too similar to centre \
         (L=[{left_r},{left_g},{left_b}] C=[{centre_r},{centre_g},{centre_b}] diff={left_diff}); \
         expected distinct textures — per-material binding collapsed?"
    );
    assert!(
        right_diff > 60,
        "multi-material: right region too similar to centre \
         (R=[{right_r},{right_g},{right_b}] C=[{centre_r},{centre_g},{centre_b}] diff={right_diff}); \
         expected distinct textures — per-material binding collapsed?"
    );
    compare_or_write("multi-material", &px, 96, 96);
}

/// Phase D cutout gate — the TEX_EDGE (CVG_X_ALPHA) quad must show a BACKGROUND HOLE.
///
/// The cutout quad (right third, G_RM_AA_ZB_TEX_EDGE) uses alpha = TEXEL0.a.  The shared
/// texture has rows 0-1 with alpha=255 (opaque, kept) and rows 2-3 with alpha=0 (sub-threshold,
/// discarded → background shows through).
///
/// UV 128 maps the 4×4 texture exactly once: V=0 at world_y=-128 (screen bottom) → row 0;
/// V=1 at world_y=+128 (screen top) → row 3.  The alpha=0 rows (2-3) occupy the UPPER half
/// of the screen (y=0..48) and the alpha=255 rows (0-1) the LOWER half (y=48..96).
///
/// Pixel (x=80, y=20) → V ≈ 0.79 → rows 2-3 (alpha=0) → discard → clear color.
/// Pixel (x=80, y=70) → V ≈ 0.27 → rows 0-1 (alpha=255) → kept → orange-tinted texture.
/// CLEAR_COLOR = (0.05, 0.05, 0.08) ≈ RGBA8 (13, 13, 20): R<20, G<20, B<40.
///
/// BLOCKED if: hole pixel is non-background (alpha-test not wired), threshold is wrong (not 0.125),
/// or alpha_mode leaks into non-cutout runs (texture-format/tron/fogworld goldens must be unchanged).
#[test]
fn golden_multi_material_cutout_shows_hole() {
    // The cutout region must show BACKGROUND through a sub-threshold hole (not opaque texels).
    let px = render_scene_to_rgba8("multi-material--rgba16-4x4", 96, 96);
    // Sample a pixel inside the cutout region's hole (right quad, upper area → texture rows 2-3, α=0).
    let hole = (20 * 96 + 80) * 4usize;
    assert!(
        px[hole] < 20 && px[hole + 1] < 20 && px[hole + 2] < 40,
        "cutout hole must show background (CLEAR_COLOR ≈ R<20,G<20,B<40); \
         got R={} G={} B={} — alpha-test discard not firing?",
        px[hole],
        px[hole + 1],
        px[hole + 2]
    );
    // ...but an opaque texel (right quad, lower area → texture rows 0-1, α=255) MUST survive the
    // discard. Without this, a fully-discarded region would still pass the hole assert above.
    let opaque = (70 * 96 + 80) * 4usize;
    assert!(
        px[opaque] > 50 || px[opaque + 1] > 50,
        "cutout opaque texel must survive; got R={} G={} B={}",
        px[opaque],
        px[opaque + 1],
        px[opaque + 2]
    );
    compare_or_write("multi-material", &px, 96, 96); // regenerate after wiring the cutout
}

// ── Tron scene + Phase B forced-fallback CI gate ─────────────────────────────────────────────────

/// Golden test for the `tron` scene — overlapping translucent neon panels.
///
/// Two semi-transparent quads (cyan + magenta, SHADE alpha=128/255≈0.5, G_RM_AA_ZB_XLU_SURF)
/// overlap in the center band.  The overlap must show a BLENDED MIX of both panel colors:
///
/// - Non-overlap cyan region  ≈ (6,  116, 134) — blue-green tinted clear
/// - Non-overlap magenta region ≈ (134, 6, 116) — reddish-purple tinted clear
/// - Overlap region           ≈ (131, 58, 177) — R from magenta, G from cyan, B from both
///
/// BLOCKED if overlap shows a single opaque color (Replace pipeline, not AlphaOver/DualSrc)
/// or shows the clear color (panels didn't render).
///
/// Rendered via the PRIMARY path (`headless_device` — dual-source when available).
#[test]
fn golden_tron() {
    let px = render_scene_to_rgba8("tron--empty-texture", 96, 96);
    // Inspect the overlap region: pixel at (row=48, col=48) is inside the cyan+magenta overlap
    // band (x=-43..43 → pixels≈32..64).  Expected ≈ (131, 58, 177).
    let row = 48usize;
    let stride = 96usize;
    let c = (row * stride + 48) * 4;
    let r = px[c];
    let g = px[c + 1];
    let b = px[c + 2];
    // R>50 proves magenta component reached the framebuffer.
    // G>20 proves cyan component survived blending.
    // B>80 proves both panels contributed (both cyan and magenta have high blue).
    // If Replace pipeline: only last-drawn panel shows (pure magenta: R≈134, G≈6 → fails G>20
    // check but would pass R>50 — so G>20 is the key discriminator for Replace vs blend).
    assert!(
        r > 50 && g > 20 && b > 80,
        "tron overlap region must show blended mix of cyan+magenta \
         (R={r},G={g},B={b}); expected ≈(131,58,177); \
         XLU blending broken? (Replace pipeline would show R≈134,G≈6)"
    );
    compare_or_write("tron", &px, 96, 96);
}

/// Forced-fallback re-render of `tron` — proves B3 AlphaOver pipelines compile + render.
///
/// [IMP13] Re-renders `tron` through `headless_device_forced_fallback()` (dual-source disabled),
/// exercising the AlphaOver/Replace fallback blender pipelines.  `tron` uses the canonical XLU
/// lerp (B=1MA, A=A_IN) which is losslessly expressible as AlphaOver.
///
/// RGB channels match the primary golden within TOL=2; alpha differs by design (dual-source
/// preserves dst.a=255 from clear; AlphaOver writes src.a≈128).  The inline RGB cross-check
/// against the PRIMARY `tron.bin` is the IMP13 assertion (fallback RGB == primary RGB); the
/// `tron-fallback.bin` golden is an additional regression guard for the fallback alpha too.
#[test]
fn golden_tron_forced_fallback() {
    let px = render_scene_to_rgba8_forced_fallback("tron--empty-texture", 96, 96);
    // Cross-check RGB against the PRIMARY golden (IMP13). Alpha intentionally differs:
    // dual-source preserves dst.a=255 from clear; AlphaOver writes src.a≈128.
    let primary_path = format!("{}/goldens/tron.bin", env!("CARGO_MANIFEST_DIR"));
    let primary =
        std::fs::read(&primary_path).unwrap_or_else(|e| panic!("primary tron.bin missing: {e}"));
    let max_rgb_diff = px
        .chunks(4)
        .zip(primary.chunks(4))
        .flat_map(|(a, p)| {
            [
                a[0].abs_diff(p[0]),
                a[1].abs_diff(p[1]),
                a[2].abs_diff(p[2]),
            ]
        })
        .max()
        .unwrap_or(0);
    assert!(
        max_rgb_diff <= TOL,
        "tron fallback RGB diverges from primary (max diff={max_rgb_diff} > TOL={TOL}); \
         XLU AlphaOver pipeline producing wrong RGB?"
    );
    compare_or_write("tron-fallback", &px, 96, 96);
}

/// Forced-fallback re-render of `multi-material` — [IMP13] second AlphaOver-expressible scene.
///
/// Re-renders `multi-material` through `headless_device_forced_fallback()` and asserts the
/// output is IDENTICAL (within TOL=2) to the primary `multi-material` golden.  The XLU center
/// quad (PRIMITIVE combiner, B=1MA, A=A_IN) is canonically expressible as AlphaOver so the
/// fallback is lossless.  If this test passes, the fallback module + pipelines compiled and
/// produced a correct result — a web-only regression cannot ship green.
#[test]
fn golden_multi_material_forced_fallback() {
    let px = render_scene_to_rgba8_forced_fallback("multi-material--rgba16-4x4", 96, 96);
    // Compare against the PRIMARY multi-material golden (not a separate fallback file).
    compare_or_write("multi-material", &px, 96, 96);
}

// ── Fog golden ───────────────────────────────────────────────────────────────────────────────────

/// Golden test for the fogworld fog demo — proves the G_RM_FOG_SHADE_A + G_CYC_2CYCLE pipeline.
///
/// Two quads at different z depths: far (z=110) is heavily fogged → pixel ≈ fog_color [128,128,128];
/// near (z=0) has no fog → pixel = crisp surface [200,50,50].
///
/// MIN6: two pixel assertions verify the fog gradient is real:
///   far  (y=30, x=30): ≈ fog_color [0x80,0x80,0x80] within ±24 per channel
///   near (y=60, x=70): ≥ 1 channel differs from fog_color by > 40 (crisp surface)
#[test]
fn golden_fogworld() {
    let px = render_scene_to_rgba8("fogworld--empty-texture", 96, 96);
    compare_or_write("fogworld", &px, 96, 96);
    // MIN6: assert the fog gradient at two sample points.
    let far = ((30 * 96 + 30) * 4) as usize; // distant quad — heavily fogged
    let near = ((60 * 96 + 70) * 4) as usize; // near quad — crisp surface
    let fog = [0x80u8, 0x80, 0x80]; // == gsDPSetFogColor in fogworld.n64
    for k in 0..3 {
        assert!(
            (px[far + k] as i32 - fog[k] as i32).abs() < 24,
            "far quad pixel channel {k}: {} is not within 24 of fog_color {} (far ≈ fog failed)",
            px[far + k],
            fog[k]
        );
    }
    // Near quad must be visibly LESS fogged — at least one channel must differ from fog by > 40.
    assert!(
        (0..3).any(|k| (px[near + k] as i32 - fog[k] as i32).abs() > 40),
        "near quad pixel [{},{},{}] is too close to fog_color — fog not cleared for near geometry",
        px[near],
        px[near + 1],
        px[near + 2]
    );
}

// ── Alpha-threshold scene (Phase D Task D2) ──────────────────────────────────────────────────────

/// Golden test for the `alpha-threshold` scene — G_AC_THRESHOLD alpha-compare gate (Phase D, D2).
///
/// A full-screen textured quad with Gouraud vertex alpha varying left→right:
///   left (x=0..31)  vertex alpha ≈ 0..127 → combiner alpha < 0.502 → DISCARDED (background)
///   right (x=32..63) vertex alpha ≈ 128..255 → combiner alpha ≥ 0.502 → KEPT (textured surface)
///
/// Threshold = `gsDPSetBlendColor(0,0,0,128)` → blendColor.a = 128/255 ≈ 0.502.
/// This is DISTINCT from CVG_X_ALPHA (threshold fixed at 0.125): the THRESHOLD path reads
/// blendColor.a from the material, not a hardcoded constant.
///
/// MIN7: assert BOTH — sub-threshold sample shows BACKGROUND, supra-threshold shows SURFACE.
/// BLOCKED if whole quad shows (THRESHOLD path broken) or none shows (discard always fires).
#[test]
fn golden_alpha_threshold() {
    let px = render_scene_to_rgba8("alpha-threshold--rgba16-4x4", 64, 64);
    compare_or_write("alpha-threshold", &px, 64, 64);
    // MIN7: assert the THRESHOLD gate (combiner-α < blendColor.a). A sub-threshold texel must show
    // BACKGROUND (discarded); a supra-threshold texel must show the texel — mirrors D1's cutout hole.
    let sub = ((/* y */50 * 64 + /* x */ 16) * 4) as usize; // alpha < 0.5 region → discarded
    let supra = ((/* y */50 * 64 + /* x */ 48) * 4) as usize; // alpha > 0.5 region → kept
    assert!(
        px[sub] < 20 && px[sub + 1] < 20 && px[sub + 2] < 40,
        "sub-threshold (x=16,y=50) must show background CLEAR_COLOR (R<20,G<20,B<40); \
         got R={} G={} B={} — G_AC_THRESHOLD discard not firing or threshold wrong?",
        px[sub],
        px[sub + 1],
        px[sub + 2]
    );
    assert!(
        px[supra] > 40 || px[supra + 1] > 40 || px[supra + 2] > 40,
        "supra-threshold (x=48,y=50) must show texel (at least one channel > 40); \
         got R={} G={} B={} — whole quad discarded? Check blendColor.a vs threshold.",
        px[supra],
        px[supra + 1],
        px[supra + 2]
    );
}

// ── Decal scene (Phase E Task E2) ─────────────────────────────────────────────────────────────

const DECAL_BASE_RGB: [u8; 3] = [40, 40, 200]; // blue base quad
const DECAL_DECAL_RGB: [u8; 3] = [240, 220, 40]; // yellow coplanar decal
const OCCLUDER_RGB: [u8; 3] = [220, 40, 40]; // red nearer quad

/// Golden test for the `decal` scene — in-shader ZMODE_DEC occlusion + coplanar discard (Phase E, E2).
///
/// A blue base quad fills the screen; a coplanar yellow decal covers the top half; a NEARER red
/// quad covers the upper-right quadrant. The decal fragment samples the depth the opaque pass
/// wrote and (a) shows coplanar on the base WITHOUT z-fighting, (b) is OCCLUDED (Z_CMP discard)
/// where the red quad is in front.
///
/// MIN8: assert BOTH — a pixel where the red quad covers the decal shows the OCCLUDER color (decal
/// discarded by Z_CMP), and a pixel where the decal sits coplanar on the base shows the DECAL color
/// (no z-fight). BLOCKED if the decal shows THROUGH the red quad (occlusion broken) or is missing on
/// the base (coplanar/z-fight broken).
#[test]
fn golden_decal() {
    let px = render_scene_to_rgba8("decal--empty-texture", 96, 96);
    compare_or_write("decal", &px, 96, 96);

    // (1) Occlusion: a pixel under the nearer red quad (upper-right) where the decal would be must
    // show the OCCLUDER color — the decal is discarded by the in-shader Z_CMP.
    let occluded = ((/* y */20 * 96 + /* x */ 70) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[occluded + k] as i32 - OCCLUDER_RGB[k] as i32).abs() < 24,
            "occluded pixel (x=70,y=20) must show the nearer quad (occluder) color {OCCLUDER_RGB:?}, \
             got [{},{},{}] — decal showing THROUGH the nearer quad (Z_CMP occlusion broken)?",
            px[occluded],
            px[occluded + 1],
            px[occluded + 2]
        );
    }

    // (2) Coplanar: a pixel where the decal sits ON the base (top-left, no occluder) must show the
    // DECAL color, NOT the base — proving the decal binds coplanar without z-fighting.
    let coplanar = ((/* y */20 * 96 + /* x */ 20) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[coplanar + k] as i32 - DECAL_DECAL_RGB[k] as i32).abs() < 24,
            "coplanar pixel (x=20,y=20) must show the DECAL color {DECAL_DECAL_RGB:?}, \
             got [{},{},{}] — decal missing on the base (z-fight / coplanar discard too strict)?",
            px[coplanar],
            px[coplanar + 1],
            px[coplanar + 2]
        );
    }

    // (3) Bottom half (no decal) shows the base color — sanity that the decal is bounded.
    let base = ((/* y */76 * 96 + /* x */ 20) * 4) as usize;
    for k in 0..3 {
        assert!(
            (px[base + k] as i32 - DECAL_BASE_RGB[k] as i32).abs() < 24,
            "base-only pixel (x=20,y=76) must show the base color {DECAL_BASE_RGB:?}, got [{},{},{}]",
            px[base],
            px[base + 1],
            px[base + 2]
        );
    }
}

/// Coplanar tolerance boundary: a decal slightly IN FRONT of the (tilted) surface shows; the same
/// decal slightly BEHIND is occluded (shows base). Asserts the two center pixels differ — the
/// in-shader tolerance must distinguish front (within dz) from behind (beyond epsilon).
#[test]
fn decal_coplanar_tolerance_boundary() {
    let shown = render_scene_to_rgba8("decal--in-front", 48, 48);
    let hidden = render_scene_to_rgba8("decal--behind", 48, 48);
    let c = ((24 * 48 + 24) * 4) as usize;
    assert_ne!(
        &shown[c..c + 3],
        &hidden[c..c + 3],
        "tolerance must distinguish front (decal shown) from behind (decal occluded): \
         front=[{},{},{}] behind=[{},{},{}]",
        shown[c],
        shown[c + 1],
        shown[c + 2],
        hidden[c],
        hidden[c + 1],
        hidden[c + 2]
    );
}

// ── High-poly scene (Phase F Task F1) ────────────────────────────────────────────────────────────

/// Golden test for the `high-poly` scene — multi-batch vertex-loading guard (Phase F, F1).
///
/// 5 × `gsSPVertex(verts,28,0)` reloads accumulate 140 global entries (indices 0-139, > 127) of
/// the blue 4×7 grid mesh; the 6th batch (`gsSPVertex(verts,31,0)`) loads the red marker verts at
/// slots 28-30 — slots the mesh batches (count=28) NEVER touch — placing them at global indices
/// 168-170 (post-127). The marker triangle covers the top-left corner (pixel 10,10); the blue mesh
/// fills the right portion (x≥0 → screen x≥48), so pixel (10,10) is background unless the marker
/// renders there.
///
/// The marker is UNIQUELY tied to the post-127 batch: global indices 0-2 hold BLUE mesh corners,
/// not red. So a slot-reuse / wrong-global-index regression in the reload path (e.g. batch-6 slot
/// 28 resolving to a LOWER global) would point the marker at a blue mesh vertex (or off-screen),
/// drawing blue/background at (10,10) — NOT red — and FAILING the assertion. A marker authored
/// from low slots (also loaded by earlier batches) would be blind to this; this one is not.
///
/// MIN9: assert BOTH — the overall image matches the golden (whole-mesh regression), AND pixel
/// (10,10) is red (the post-127 batch resolved correctly). Red requires G<60; both blue
/// (0,50,200) and the dark background (≈13,13,20) fail `R>200`, so either regression is caught.
#[test]
fn golden_high_poly() {
    let px = render_scene_to_rgba8("high-poly--empty-texture", 96, 96);
    compare_or_write("high-poly", &px, 96, 96);
    // Marker triangle (red, from the post-127 batch — global 168-170) must appear at (10,10).
    // A slot-resolution regression would resolve the marker slots to a lower global (a BLUE mesh
    // vertex) or off-screen, drawing blue (0,50,200) / background (≈13,13,20) here — both fail R>200.
    let marker = ((/* y */10 * 96 + /* x */ 10) * 4) as usize;
    assert!(
        px[marker] > 200 && px[marker + 1] < 60,
        "marker triangle (red) must render at pixel (10,10); got R={} G={} B={} — post-127 batch \
         mis-resolved (drew the mesh's blue or background) or the marker slot-mapping regressed?",
        px[marker],
        px[marker + 1],
        px[marker + 2]
    );
}

// ── 2D / framebuffer-pipeline goldens (Slice 9) ──────────────────────────────────────────────────
//
// These route through `SceneRenderer::render` (the facade paired path) via `common::render_to_pixels`
// — the FB-pool + per-pair-pass + scanout-blit pipeline — unlike the older goldens, which use the
// manual `render_scene_with_device` raster path. The 2D scenes are 64×64 (`w*4 = 256`, readback-aligned).

/// Build a hand-crafted single-`FramebufferPair` scene with one `FillRect` op (no materials, no
/// triangles) over an `fb_w × fb_h` CIMG. The fill resolves through `CombinerUniform::fill_rect`.
fn fill_rect_scene(
    fb_w: u32,
    fb_h: u32,
    rect: crate::hle::Rect,
    color_raw: u32,
) -> crate::hle::Scene {
    crate::hle::Scene {
        framebuffer_pairs: vec![crate::hle::FramebufferPair {
            color_image: crate::hle::ColorImage {
                fmt: 0,
                siz: 2, // G_IM_SIZ_16b
                width: fb_w as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![crate::hle::SceneOp::FillRect {
                rect,
                color_raw,
                convert: Default::default(),
                key: Default::default(),
            }],
            active_scissor: crate::hle::Scissor {
                ulx: 0,
                uly: 0,
                lrx: fb_w as i32,
                lry: fb_h as i32,
                mode: 0,
            },
            size_extent: (fb_w, fb_h),
            is_depth_clear: false,
        }],
        ..Default::default()
    }
}

/// A 1×1-texel material whose decoded RGBA8 is exactly `rgba` (so any sampled point returns it).
fn tex1x1_material(rgba: [u8; 4]) -> crate::hle::Material {
    crate::hle::Material {
        sampling: Default::default(),
        texture: rgba.to_vec(),
        tex_w: 1,
        tex_h: 1,
        selectors: crate::hle::combiner::decode_combine(0, 0),
        cycle_type: 2, // G_CYC_COPY
        filter_mode: 0,
        prim: [0, 0, 0, 0],
        env: [0, 0, 0, 0],
        convert: Default::default(),
        key: Default::default(),
        tex_enable: true,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 2,
        blend_color: [0, 0, 0, 255],
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    }
}

/// Hand-computed EXACT cross-check (no golden file): proves the rect clip-space mapping, the
/// exclusive lower-right `+1`, the scissor clamp, the FillRect flat-PRIM combine, and the COPY
/// TEXRECT TEXEL0-passthrough all land byte-exactly — BEFORE any `UPDATE_GOLDENS` blesses the scenes.
#[test]
fn golden_2d_rect_geometry_exact() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    // Readback requires bytes_per_row (= w*4) to be 256-aligned, so the FB is 64 wide.
    const W: u32 = 64;
    const H: u32 = 16;
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, W, H, dual);

    // (A) Full-FB solid fill. RGBA16 0xF801 = R5=31,G5=0,B5=0,A1=1 → (255,0,0,255). The fill word
    // replicates the 16-bit pixel across both halves, so color_raw = 0xF801_F801.
    let scene = fill_rect_scene(
        W,
        H,
        crate::hle::Rect {
            ulx: 0,
            uly: 0,
            lrx: W as i32 - 1,
            lry: H as i32 - 1,
        },
        0xF801_F801,
    );
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    for y in 0..H {
        for x in 0..W {
            assert_eq!(
                common::pixel(&buf, W, x, y),
                [255, 0, 0, 255],
                "full-FB fill: pixel ({x},{y}) must be the resolved RGBA16 fill color"
            );
        }
    }

    // (B) Sub-region fill [2,2]..=[5,5] (inclusive) over a CLEAR_COLOR background. The quad spans
    // continuous pixel space [2,6)×[2,6) (exclusive +1), so pixel centers 2.5..5.5 are covered and
    // 1.5 / 6.5 are not — a precise test of the exclusive +1 and binary (no-MSAA) coverage.
    let scene = fill_rect_scene(
        W,
        H,
        crate::hle::Rect {
            ulx: 2,
            uly: 2,
            lrx: 5,
            lry: 5,
        },
        0xF801_F801,
    );
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    let clear = clear_color_rgb_local();
    for y in 0..H {
        for x in 0..W {
            let p = common::pixel(&buf, W, x, y);
            let inside = (2..=5).contains(&x) && (2..=5).contains(&y);
            if inside {
                assert_eq!(p, [255, 0, 0, 255], "inside fill: pixel ({x},{y})");
            } else {
                // CLEAR_COLOR ≈ (13,13,20); allow ±2 for unorm rounding.
                assert!(
                    p[0].abs_diff(clear[0]) <= 2
                        && p[1].abs_diff(clear[1]) <= 2
                        && p[2].abs_diff(clear[2]) <= 2,
                    "outside fill: pixel ({x},{y}) must be CLEAR_COLOR, got {p:?}"
                );
            }
        }
    }

    // (C) COPY TEXRECT over a 1×1 texel → every covered pixel is the texel exactly (no combine, no
    // filtering ambiguity on a 1×1 source). Verifies the TexRect quad + TEXEL0-passthrough combine.
    let texel = [10u8, 200, 30, 255];
    let scene = crate::hle::Scene {
        materials: vec![tex1x1_material(texel)],
        framebuffer_pairs: vec![crate::hle::FramebufferPair {
            color_image: crate::hle::ColorImage {
                fmt: 0,
                siz: 2,
                width: W as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![crate::hle::SceneOp::TexRect {
                rect: crate::hle::TexRectBounds {
                    ulx: 0,
                    uly: 0,
                    lrx: (W as i32 - 1) * 4,
                    lry: (H as i32 - 1) * 4,
                },
                tile: 0,
                uls: 0,
                ult: 0,
                dsdx: 1024,
                dtdy: 1024,
                flip: false,
                copy_mode: true,
                material_index: 0,
                render_mode_index: 0,
                fog_color: [0; 4],
                prim_depth: Default::default(),
                fb_source: None,
            }],
            active_scissor: crate::hle::Scissor {
                ulx: 0,
                uly: 0,
                lrx: W as i32,
                lry: H as i32,
                mode: 0,
            },
            size_extent: (W, H),
            is_depth_clear: false,
        }],
        ..Default::default()
    };
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, W, H);
    for y in 0..H {
        for x in 0..W {
            assert_eq!(
                common::pixel(&buf, W, x, y),
                texel,
                "copy texrect over 1×1: pixel ({x},{y}) must equal the source texel"
            );
        }
    }
}

/// CLEAR_COLOR (0.05,0.05,0.08) rendered into an Rgba8Unorm target → ≈(13,13,20). Local copy
/// (goldens.rs cannot see `common::clear_color_rgb` is `pub` without it being used elsewhere here).
fn clear_color_rgb_local() -> [u8; 3] {
    [
        (CLEAR_COLOR.r * 255.0).round() as u8,
        (CLEAR_COLOR.g * 255.0).round() as u8,
        (CLEAR_COLOR.b * 255.0).round() as u8,
    ]
}

/// Scene 1 — `fill-texrect`: FILL clears the 64×64 CIMG to blue, then a COPY TEXRECT blits the
/// 4×4 `quad_tex` checker over the whole surface. The checker maps row-0-at-top (verified against
/// the texel alpha pattern: rows 0–1 α=255, rows 2–3 α=0 → a 4-px vertical period).
#[test]
fn golden_2d_fill_texrect() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("fill-texrect--rgba16-4x4");
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    // Hand cross-check the vertical orientation via the texel alpha period (dtdy=1024 → 1 texel/px):
    // screen rows 0–1 sample texrows 0–1 (α=255); row 2 samples texrow 2 (α=0). A vertical flip or a
    // wrong exclusive-+1 would invert this. (Linear filtering keeps the row centers near-pure.)
    assert!(
        common::pixel(&buf, 64, 2, 0)[3] > 200 && common::pixel(&buf, 64, 2, 1)[3] > 200,
        "fill-texrect: top rows (texrows 0–1) must be opaque (α≈255) — orientation/flip check"
    );
    assert!(
        common::pixel(&buf, 64, 2, 2)[3] < 60,
        "fill-texrect: row 2 (texrow 2) must be transparent (α≈0) — orientation/flip check"
    );
    // The blue FILL must be fully overwritten by the checker (no pure-blue, full-α pixel remains).
    assert!(
        common::pixel(&buf, 64, 2, 0)[3] != 0 || common::pixel(&buf, 64, 2, 0)[2] < 255,
        "fill-texrect: the TEXRECT must have drawn over the FILL"
    );
    compare_or_write("2d-fill-texrect", &buf, 64, 64);
}

/// Scene 2 — `hud-over-3d`: a Gouraud 3D quad in the center, then a COPY TEXRECT HUD in the
/// top-left 16×16 corner. Verifies tris + rects coexist in one pair and the HUD lands top-left.
#[test]
fn golden_2d_hud_over_3d() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("hud-over-3d--rgba16-4x4");
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    // Top-left corner = HUD checker (came from a copy texrect; texels there are α=0, B/Y/R/G).
    assert!(
        common::pixel(&buf, 64, 2, 2)[3] < 60,
        "hud: top-left corner must be the HUD checker overlay (α≈0)"
    );
    // Center = the 3D quad (opaque, not the background clear).
    let c = common::pixel(&buf, 64, 32, 32);
    assert!(
        c[3] == 255 && c != [13, 13, 20, 255],
        "hud: center must be the opaque 3D quad, got {c:?}"
    );
    // Bottom-right corner = background (outside both the HUD corner and the centered quad).
    let br = common::pixel(&buf, 64, 60, 60);
    assert!(
        br[0].abs_diff(13) <= 2 && br[2].abs_diff(20) <= 2,
        "hud: bottom-right must be the background clear, got {br:?}"
    );
    compare_or_write("2d-hud-over-3d", &buf, 64, 64);
}

/// Scene 4 — `texrectflip`: COPY TEXRECT with S/T axes swapped (`gsSPTextureRectangleFlip`). The
/// flipped UVs transpose the checker vs `fill-texrect`'s un-flipped layout.
#[test]
fn golden_2d_texrectflip() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let flip = common::scene_from_fixture("texrectflip--rgba16-4x4");
    let plain = common::scene_from_fixture("fill-texrect--rgba16-4x4");
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &flip, 64, 64);
    let buf_plain = common::render_to_pixels(&device, &queue, &mut sr, &plain, 64, 64);
    // Flip transposes the pattern: under FLIP the texel ALPHA period runs along X (left columns
    // sample texrows 0–1 → α≈255) instead of along Y. So col 0 is opaque where the un-flipped col 0
    // (row 2) was transparent — a direct flip-vs-plain orientation discriminator at pixel (0,2).
    assert!(
        common::pixel(&buf, 64, 0, 2)[3] > 200,
        "texrectflip: left column must be opaque (texrows 0–1 run along X under flip)"
    );
    assert_ne!(
        common::pixel(&buf, 64, 0, 2),
        common::pixel(&buf_plain, 64, 0, 2),
        "texrectflip must differ from the un-flipped fill-texrect"
    );
    compare_or_write("2d-texrectflip", &buf, 64, 64);
}

/// Bgra8 headless cover: render `fill-texrect` once at `Rgba8Unorm` and once at `Bgra8Unorm` and
/// assert the readback bytes are R↔B swapped (G/A identical). Exercises the present pipeline's
/// surface-format path (`SceneRenderer::new(.., Bgra8Unorm, ..)`) without a real surface.
#[test]
fn golden_2d_bgra8_present_cover() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("fill-texrect--rgba16-4x4");

    let mut sr_rgba = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let rgba = common::render_to_pixels(&device, &queue, &mut sr_rgba, &scene, 64, 64);

    let mut sr_bgra = SceneRenderer::new(&device, wgpu::TextureFormat::Bgra8Unorm, 64, 64, dual);
    let bgra = common::render_to_pixels_fmt(
        &device,
        &queue,
        &mut sr_bgra,
        &scene,
        64,
        64,
        wgpu::TextureFormat::Bgra8Unorm,
    );
    assert_eq!(rgba.len(), bgra.len());
    for i in (0..rgba.len()).step_by(4) {
        assert!(
            bgra[i].abs_diff(rgba[i + 2]) <= 2,
            "B channel mismatch at {i}"
        );
        assert!(bgra[i + 1].abs_diff(rgba[i + 1]) <= 2, "G channel at {i}");
        assert!(bgra[i + 2].abs_diff(rgba[i]) <= 2, "R channel at {i}");
        assert!(bgra[i + 3].abs_diff(rgba[i + 3]) <= 2, "A channel at {i}");
    }
}

/// Scene 3 — `offscreen-then-sample`: two `FramebufferPair`s. Pair 0 (scratch 0x00200000) is filled
/// orange via FILLRECT (RGBA16 0xFB81 → R=255, G=115, B=0, A=255). Pair 1 (scanout 0x00100000)
/// uses a COPY TEXRECT with `fb_source = Some(0x00200000)` to sample the scratch buffer into the
/// scanout via the FB-as-texture alias (spec §2.4, Task 10 Step 1).
///
/// **Hand cross-check (BEFORE UPDATE_GOLDENS):** The scratch FILLRECT resolves RGBA16 0xFB81 to
/// R8=(31<<3|31>>2)=255, G8=(14<<3|14>>2)=115, B8=0, A=255 — a saturated orange. With the
/// FB-as-texture alias active, the scanout TEXRECT samples that orange directly; without it the
/// scanout would show CLEAR_COLOR (≈13,13,20). The per-pixel assertion below verifies ORANGE
/// (R>200, G>80, B<60) before the golden is committed.
#[test]
fn golden_2d_offscreen_then_sample() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("offscreen-then-sample--white1");
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Hand cross-check: RGBA16 0xFB81 fills the scratch buffer orange.
    // R5=31 → R8=255, G5=14 → G8=115, B5=0 → B8=0, A1=1 → A=255.
    // With FB-as-texture the scanout should be orange; without it CLEAR_COLOR (≈13,13,20).
    // Verify a representative pixel at the center and corners — all should be orange.
    for &(x, y) in &[(0u32, 0u32), (32, 32), (63, 63), (0, 63), (63, 0)] {
        let p = common::pixel(&buf, 64, x, y);
        assert!(
            p[0] > 200 && p[1] > 80 && p[2] < 60,
            "offscreen-then-sample pixel ({x},{y}): expected orange (R>200,G>80,B<60) \
             from FB-as-texture alias, got {p:?}. \
             If this shows CLEAR_COLOR the FB alias is not firing."
        );
    }
    compare_or_write("2d-offscreen-then-sample", &buf, 64, 64);
}

// ── Alpha-blended TexRect regression golden (alpha HUD blend fix) ─────────────────────────────────

fn scene_from_fixture(name: &str) -> crate::hle::Scene {
    let (rdram, entry_addr) = crate::tests::fixtures::fixture(name);
    let r = crate::hle::interpret_rdram(rdram, entry_addr as u32);
    assert!(r.diags.is_empty(), "unexpected HLE diags: {:?}", r.diags);
    r.scene
}

/// Alpha-blend regression golden: a non-COPY XLU TEXRECT must blend over the background.
///
/// Scene: solid green FILLRECT (RGBA16 G5=31 → G8=255) followed by an alpha-blended TEXRECT
/// using a 2×2 texture (top row opaque red / bottom row transparent) and G_RM_AA_ZB_XLU_SURF.
///
/// **Hand-verify BEFORE UPDATE_GOLDENS:**
///   Even rows (texrow 0, α=255): AlphaOver writes red (255,0,0) — R>200, G<30.
///   Odd  rows (texrow 1, α=0):   AlphaOver passes background through — G>200, R<30.
///
/// **With the OLD Replace pipeline (pre-fix):**
///   Transparent pixels get REPLACED with the combiner output (red) ignoring α → odd rows
///   are red, NOT green. The `odd_pix[1] > 200` assertion fails → regression detected.
///
/// **Byte-identity:** existing COPY/FILL scenes are unaffected (Replace paths unchanged).
#[test]
fn golden_2d_alpha_texrect_over_bg() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = scene_from_fixture("texrect--alpha-over-green");
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Even row (row 0): texrow 0 is fully opaque red (α=255). AlphaOver: out = red.
    let even_pix = common::pixel(&buf, 64, 32, 0);
    assert!(
        even_pix[0] > 200 && even_pix[1] < 30,
        "alpha-texrect even row (texrow 0, opaque): expected red (R>200, G<30), \
         got R={} G={} B={} — blend or combiner broken?",
        even_pix[0],
        even_pix[1],
        even_pix[2]
    );

    // Odd row (row 1): texrow 1 is transparent (α=0). AlphaOver: out = green background.
    // With the old Replace pipeline this would be red (combiner output ignoring α).
    let odd_pix = common::pixel(&buf, 64, 32, 1);
    assert!(
        odd_pix[1] > 200 && odd_pix[0] < 30,
        "alpha-texrect odd row (texrow 1, transparent): expected green background (G>200, R<30), \
         got R={} G={} B={} — Replace pipeline still active (ignoring α of transparent texel)?",
        odd_pix[0],
        odd_pix[1],
        odd_pix[2]
    );

    compare_or_write("2d-alpha-texrect-over-bg", &buf, 64, 64);
}

// ── COPY-mode alpha-keyed TexRect regression (sm64 HUD/text glyph fix) ────────────────────────────

/// COPY-mode alpha-key regression: mirrors sm64's HUD/text glyph setup exactly.
///
/// sm64 `bin/segment2.c` (`dl_hud_*`) issues HUD/number/logo glyphs under:
///   gsDPSetCycleType(G_CYC_COPY); gsDPSetAlphaCompare(G_AC_THRESHOLD);
///   gsDPSetBlendColor(255,255,255,255); gsDPSetTextureFilter(G_TF_POINT);
/// The glyphs are RGBA5551 (1-bit alpha): background texels have a1=0 (α=0), foreground a1=1
/// (α=255). The N64 RDP alpha-keys those α=0 texels away (background shows through). The bug:
/// `tex_copy()` hardcoded `alpha_mode=0` (no discard), so α=0 texels wrote as OPAQUE BLACK boxes.
///
/// This scene: green FILLRECT background, then a COPY TEXRECT whose decoded render mode has
/// `alpha_compare == Threshold` over a 2×2 texture (top row α=255 red, bottom row α=0). With
/// dtdy=1024 the 2-row texture tiles every 2 screen rows.
///
/// **Hand-verify (BEFORE UPDATE_GOLDENS):**
///   Even screen rows (texrow 0, α=255): the opaque red texel is copied → R>200, G<30.
///   Odd  screen rows (texrow 1, α=0):   alpha-keyed away → green background shows → G>200, R<30.
///
/// **With the OLD buggy tex_copy() (alpha_mode=0):** the α=0 odd rows write OPAQUE BLACK
/// (TEXEL0 RGB=0, no discard) → `odd_pix[1] > 200` fails. That is the bug this asserts against.
fn copy_alpha_keyed_scene() -> crate::hle::Scene {
    use crate::hle::{
        AlphaCompare, ColorImage, FramebufferPair, Material, Rect, RenderMode, Scene, SceneOp,
        Scissor,
    };
    const W: u32 = 64;
    const H: u32 = 64;
    // 2×2 RGBA8: top row opaque red, bottom row transparent (α=0). RGB stays red on the
    // transparent row to prove the discard (not a black RGB) is what reveals the background.
    let texture = vec![
        255, 0, 0, 255, 255, 0, 0, 255, // row 0 — α=255 (opaque)
        255, 0, 0, 0, 255, 0, 0, 0, // row 1 — α=0   (alpha-keyed hole)
    ];
    let material = Material {
        sampling: crate::hle::tile_sampling::TileSampling::from_tile(
            &crate::hle::rdp::TileDescriptor {
                width: 2,
                height: 2,
                lrs: 4,
                lrt: 4,
                masks: 1,
                maskt: 1,
                ..Default::default()
            },
            0,
        ),
        texture,
        tex_w: 2,
        tex_h: 2,
        selectors: crate::hle::combiner::decode_combine(0, 0),
        cycle_type: 2, // G_CYC_COPY
        filter_mode: 0,
        prim: [0, 0, 0, 0],
        env: [0, 0, 0, 0],
        convert: Default::default(),
        key: Default::default(),
        tex_enable: true,
        wrap_s: 0,
        wrap_t: 0,
        fmt: 0,                            // RGBA
        siz: 2,                            // 16b (RGBA5551, 1-bit alpha)
        blend_color: [255, 255, 255, 255], // sm64 sets blend_color.a = 255 → threshold must NOT be blend_a
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    // Decoded render mode for sm64's HUD copy setup: G_AC_THRESHOLD → alpha_compare = Threshold.
    let rm = RenderMode {
        alpha_compare: AlphaCompare::Threshold,
        ..Default::default()
    };
    // Green FILLRECT background (RGBA16 0x07C1 → R=0,G=255,B=0,A=1) then the alpha-keyed COPY rect.
    Scene {
        materials: vec![material],
        render_modes: vec![rm],
        framebuffer_pairs: vec![FramebufferPair {
            color_image: ColorImage {
                fmt: 0,
                siz: 2,
                width: W as u16,
                addr: 0x0010_0000,
            },
            depth_image: None,
            ops: vec![
                SceneOp::FillRect {
                    convert: Default::default(),
                    key: Default::default(),
                    rect: Rect {
                        ulx: 0,
                        uly: 0,
                        lrx: W as i32 - 1,
                        lry: H as i32 - 1,
                    },
                    color_raw: 0x07C1_07C1,
                },
                SceneOp::TexRect {
                    rect: crate::hle::TexRectBounds {
                        ulx: 0,
                        uly: 0,
                        lrx: (W as i32 - 1) * 4,
                        lry: (H as i32 - 1) * 4,
                    },
                    tile: 0,
                    uls: 0,
                    ult: 0,
                    dsdx: 1024,
                    dtdy: 1024,
                    flip: false,
                    copy_mode: true,
                    material_index: 0,
                    render_mode_index: 0,
                    fog_color: [0; 4],
                    prim_depth: Default::default(),
                    fb_source: None,
                },
            ],
            active_scissor: Scissor {
                ulx: 0,
                uly: 0,
                lrx: W as i32,
                lry: H as i32,
                mode: 0,
            },
            size_extent: (W, H),
            is_depth_clear: false,
        }],
        ..Default::default()
    }
}

/// COPY-mode alpha-keyed TexRect must discard its α=0 texels (background shows through), not
/// write them as opaque black. Regression guard for the sm64 HUD/text black-box bug.
#[test]
fn golden_2d_copy_alpha_keyed_over_bg() {
    use crate::render::SceneRenderer;
    let (device, queue, dual) = headless_device();
    let scene = copy_alpha_keyed_scene();
    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);

    // Even row (texrow 0, α=255): opaque red copied verbatim.
    let even_pix = common::pixel(&buf, 64, 32, 0);
    assert!(
        even_pix[0] > 200 && even_pix[1] < 30,
        "copy-alpha-keyed even row (texrow 0, opaque): expected red (R>200, G<30), got {even_pix:?}"
    );

    // Odd row (texrow 1, α=0): alpha-keyed away → green background shows through.
    // With the buggy tex_copy() (alpha_mode=0) this is OPAQUE BLACK (R=0,G=0,B=0) — bug fails here.
    let odd_pix = common::pixel(&buf, 64, 32, 1);
    assert!(
        odd_pix[1] > 200 && odd_pix[0] < 30,
        "copy-alpha-keyed odd row (texrow 1, α=0): expected green background (G>200, R<30), \
         got {odd_pix:?} — copy-mode rect wrote the α=0 texel as opaque black (the bug)?"
    );

    compare_or_write("2d-copy-alpha-keyed-over-bg", &buf, 64, 64);
}

// ── Paired coplanar-decal regression (decal two-pass in per-pair rendering) ───────────────────────

/// Build a coplanar-decal scene: a BLACK opaque base quad (run 0, `G_RM_AA_ZB_OPA_SURF`) and a
/// coplanar BRIGHT MAGENTA decal quad (run 1, `G_RM_AA_ZB_OPA_DECAL`), both full-screen at Z=0.
/// This is the pair-LESS form (flat `draw_runs`). Mirrors `render.rs::build_decal_smoke_scene`.
///
/// The 320×240 viewport fills the pairless logical canvas and covers the smaller 64×64 pairs.
fn build_decal_scene() -> crate::hle::Scene {
    // PRIM-passthrough combiner (combine_l=0, combine_h=0xC3 → cd1=PRIM, ad1=PRIM).
    let selectors = crate::hle::combiner::decode_combine(0x0000_0000, 0x0000_00C3);
    let mat = |prim: [u8; 4]| crate::hle::Material {
        sampling: Default::default(),
        texture: vec![255u8, 255, 255, 255],
        tex_w: 1,
        tex_h: 1,
        selectors: selectors.clone(),
        cycle_type: 0,
        filter_mode: 0,
        prim,
        env: [0, 0, 0, 255],
        convert: Default::default(),
        key: Default::default(),
        blend_color: [0, 0, 0, 255],
        tex_enable: false,
        wrap_s: 2,
        wrap_t: 2,
        fmt: 0,
        siz: 0,
        tile_count: 1,
        tex1: None,
        prim_lod_frac: 0.0,
        prim_min_level: 0.0,
        lod: false,
        num_levels: 1,
        text_detail: 0,
        mip_levels: Vec::new(),
        detail_tex: None,
    };
    let rm_base =
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_OPA_SURF, 0, 0);
    let rm_decal =
        crate::hle::decode_render_mode(crate::hle::consts::rdp::G_RM_AA_ZB_OPA_DECAL, 0, 0);
    assert_eq!(
        rm_decal.z_mode,
        crate::hle::ZMode::Decal,
        "run 1 must be a decal run"
    );
    assert_eq!(
        rm_base.z_mode,
        crate::hle::ZMode::Opa,
        "run 0 must be opaque"
    );

    let half_w = crate::hle::rsp::FB_WIDTH / 2.0;
    let half_h = crate::hle::rsp::FB_HEIGHT / 2.0;
    let ds = 511.0 / crate::hle::rsp::DEPTH_RANGE;
    let vp = ([half_w, half_h, ds], [half_w, half_h, ds]);
    let quad: [[f32; 3]; 4] = [
        [-1.0, -1.0, 0.0],
        [1.0, -1.0, 0.0],
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
    ];
    let mut scene = crate::hle::Scene {
        materials: vec![mat([0, 0, 0, 255]), mat([220, 40, 255, 255])],
        mvp_table: vec![crate::hle::math::identity()],
        viewport_table: vec![vp],
        texcoord_table: vec![[0.0, 0.0]],
        render_modes: vec![rm_base, rm_decal],
        ..Default::default()
    };
    for _ in 0..2 {
        for v in &quad {
            scene.raw_pos.push(*v);
            scene.mtx_index.push(0);
            scene.viewport_index.push(0);
            scene.raw_st.push([0.0, 0.0]);
            scene.texcoord_index.push(0);
            scene.cn.push(0xFF_FF_FF_FF);
            scene.light_index.push(0);
            scene.light_count.push(0);
        }
    }
    scene.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    scene.indices.extend_from_slice(&[4, 5, 6, 4, 6, 7]);
    scene.draw_runs = vec![
        crate::hle::DrawRun {
            fog_color: [0; 4],
            prim_depth: Default::default(),
            material_index: 0,
            render_mode_index: 0,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 0,
        },
        crate::hle::DrawRun {
            fog_color: [0; 4],
            prim_depth: Default::default(),
            material_index: 1,
            render_mode_index: 1,
            cull: crate::hle::CullKind::None,
            index_count: 6,
            index_start: 6,
        },
    ];
    scene
}

#[test]
fn golden_paired_decal_matches_pair_less() {
    use crate::render::SceneRenderer;
    const DIM: u32 = 64;
    let (device, queue, dual) = headless_device();

    let pair_less = build_decal_scene();
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
            width: DIM as u16,
            addr: 0x0010_0000,
        },
        depth_image: Some(0x0020_0000), // distinct from CIMG → a real depth FB (not a depth-clear)
        ops,
        active_scissor: crate::hle::Scissor {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32,
            lry: DIM as i32,
            mode: 0,
        },
        size_extent: (DIM, DIM),
        is_depth_clear: false,
    }];

    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, DIM, DIM, dual);
    let buf_pl = common::render_to_pixels(&device, &queue, &mut sr, &pair_less, DIM, DIM);
    let buf_pr = common::render_to_pixels(&device, &queue, &mut sr, &paired, DIM, DIM);

    // (1) The paired center must show the BRIGHT MAGENTA decal — not the black base. If the decal
    // z-fought (the bug), the center would be the black opaque base.
    let c = common::pixel(&buf_pr, DIM, DIM / 2, DIM / 2);
    assert!(
        c[0] > 180 && c[2] > 180 && c[1] < 90,
        "paired decal center must be the bright magenta decal (R>180,B>180,G<90), got {c:?} — \
         decal z-fighting/vanishing in the per-pair path?"
    );

    // (2) The paired render must match the pair-less render within TOL: a coplanar decal looks the
    // same whether the scene is paired or not.
    assert_eq!(buf_pl.len(), buf_pr.len(), "buffers must match in length");
    let max = buf_pl
        .iter()
        .zip(buf_pr.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max <= TOL,
        "paired vs pair-less decal max per-channel diff {max} > {TOL} — \
         paired and pairless decal output diverged"
    );
}

#[test]
fn golden_paired_decal_respects_op_order() {
    use crate::render::SceneRenderer;
    const DIM: u32 = 64;
    let (device, queue, dual) = headless_device();

    let mut paired = build_decal_scene();
    // Op stream: a leading green full-FB FILLRECT, then the opaque base run, then the decal run.
    let mut ops: Vec<crate::hle::SceneOp> = vec![crate::hle::SceneOp::FillRect {
        convert: Default::default(),
        key: Default::default(),
        rect: crate::hle::Rect {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32 - 1,
            lry: DIM as i32 - 1,
        },
        color_raw: 0x07C1_07C1, // RGBA16 green (0,255,0,255), replicated across both halves
    }];
    ops.extend(paired.draw_runs.drain(..).map(crate::hle::SceneOp::Tris));
    paired.framebuffer_pairs = vec![crate::hle::FramebufferPair {
        color_image: crate::hle::ColorImage {
            fmt: 0,
            siz: 2, // G_IM_SIZ_16b
            width: DIM as u16,
            addr: 0x0010_0000,
        },
        depth_image: Some(0x0020_0000),
        ops,
        active_scissor: crate::hle::Scissor {
            ulx: 0,
            uly: 0,
            lrx: DIM as i32,
            lry: DIM as i32,
            mode: 0,
        },
        size_extent: (DIM, DIM),
        is_depth_clear: false,
    }];

    let mut sr = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, DIM, DIM, dual);
    let buf = common::render_to_pixels(&device, &queue, &mut sr, &paired, DIM, DIM);
    let c = common::pixel(&buf, DIM, DIM / 2, DIM / 2);
    // Correct in-order render → magenta decal on top. The reorder bug → green fill covers everything.
    assert!(
        c[0] > 180 && c[2] > 180 && c[1] < 90,
        "paired op-order center must be the magenta decal (R>180,B>180,G<90), got {c:?} — a GREEN \
         center means the leading FILLRECT was reordered AFTER the geometry (the interior black-out)"
    );
}

// ── Pair-less facade characterization goldens (Phase 1) ─────────────────────────────────────────
// These route through `SceneRenderer::render`'s PAIR-LESS branch (empty `framebuffer_pairs`) via
// `common::render_to_pixels`, unlike the 21 tier-1/2D goldens (manual `render_scene_with_device`).
// Captured against the current straight-to-target output; the Phase-1 internal-FB rework must keep
// them byte-identical (the present blit at 1:1 is an identity resample).

#[test]
fn golden_pairless_flat_color() {
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("flat-color--white1");
    assert!(
        scene.framebuffer_pairs.is_empty(),
        "flat-color must be a pair-less scene"
    );
    let mut sr =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let px = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    compare_or_write("pairless-flat-color", &px, 64, 64);
}

#[test]
fn golden_pairless_chrome_icosphere() {
    let (device, queue, dual) = headless_device();
    let scene = common::scene_from_fixture("chrome-icosphere--orange");
    assert!(
        scene.framebuffer_pairs.is_empty()
            && scene.render_modes.iter().any(|r| r.z_test || r.z_write),
        "chrome-icosphere must be a pair-less DEPTH scene (exercises the owned depth buffer)"
    );
    let mut sr =
        crate::render::SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, 64, 64, dual);
    let px = common::render_to_pixels(&device, &queue, &mut sr, &scene, 64, 64);
    compare_or_write("pairless-chrome-icosphere", &px, 64, 64);
}

// Independent raw-word slot decoders (do not trust the CycleSel enums — decode from bits here).
fn lod_guard_bits(v: u32, pos: u32, n: u32) -> u32 {
    (v >> pos) & ((1 << n) - 1)
}

// The combiner computes (a - b) * c + d per channel. A LOD selector in the C (multiply) slot
// changes the OUTPUT only when (a - b) is not provably zero. COLOR A/B annulment is delegated to
// `crate::hle::combiner::color_ab_provably_equal` (single source of truth, unit-tested there
// against a synthetic a_idx==b_idx==6 case) — the two color mux tables are asymmetric, so a naive
// "same index" test is unsound (see that function's doc comment for the full case analysis).
//
// color-C field: cyc0 = L[15,5], cyc1 = L[0,5]; LOD indices = 13 (LOD_FRACTION) / 14
// (PRIM_LOD_FRAC). color A/B: cyc0 a=L[20,4] b=H[28,4]; cyc1 a=L[5,4] b=H[24,4].
fn color_c_lod_affects_output(l: u32, h: u32, second: bool) -> bool {
    let (c_idx, a_idx, b_idx) = if second {
        (
            lod_guard_bits(l, 0, 5),
            lod_guard_bits(l, 5, 4),
            lod_guard_bits(h, 24, 4),
        )
    } else {
        (
            lod_guard_bits(l, 15, 5),
            lod_guard_bits(l, 20, 4),
            lod_guard_bits(h, 28, 4),
        )
    };
    (c_idx == 13 || c_idx == 14) && !crate::hle::combiner::color_ab_provably_equal(a_idx, b_idx)
}
// alpha-C field: cyc0 = L[9,3], cyc1 = H[18,3]; LOD indices = 0 (LOD_FRACTION) / 6
// (PRIM_LOD_FRAC). alpha A/B: cyc0 a=L[12,3] b=H[12,3]; cyc1 a=H[21,3] b=H[3,3]. Unlike color,
// alpha A and B BOTH decode through the single shared `alpha_abd` mux table (ground truth:
// `alpha_abd` in render/combiner_prelude.wgsl and hle/combiner.rs's `alpha_abd`) — so index
// equality alone guarantees value equality on both sides, and `a_idx != b_idx` remains a sound (no
// fix needed) annulment test here.
fn alpha_c_lod_affects_output(l: u32, h: u32, second: bool) -> bool {
    let (c_idx, a_idx, b_idx) = if second {
        (
            lod_guard_bits(h, 18, 3),
            lod_guard_bits(h, 21, 3),
            lod_guard_bits(h, 3, 3),
        )
    } else {
        (
            lod_guard_bits(l, 9, 3),
            lod_guard_bits(l, 12, 3),
            lod_guard_bits(h, 12, 3),
        )
    };
    (c_idx == 0 || c_idx == 6) && a_idx != b_idx
}

/// Per-material half of the LOD byte-identity guard, extracted so it can be exercised directly by
/// a focused unit test (below) as well as by the full 32-scene sweep. `raw_l`/`raw_h` are the raw
/// combine words (`mat.selectors.raw_l/raw_h`), `cycle_type` is `mat.cycle_type`, `is_lod` is
/// `mat.lod`. Returns human-readable violation strings (empty = no output-affecting LOD reference).
///
/// cyc1 is evaluated in BOTH 1-cycle and 2-cycle mode (F3DEX2 1-cycle convention uses the
/// cyc1/index-1 slots — see `build_material`'s own `selectors.cyc1.unwired()` gate, which is
/// likewise unconditional on cycle_type). cyc0 is ONLY live when cycle_type == 1 (2-cycle): in
/// 1-cycle mode the shader never evaluates cyc0, so whatever those bits happen to decode to
/// (including a LOD selector with differing A/B) is dead and must NOT be flagged. Mirrors the
/// codebase's own `cycle_uses_texel0`/`cycle_uses_texel1` pattern (`build_material`), which gates
/// cyc0 checks on `cycle_type == 1` the same way.
fn lod_violations_for_material(
    name: &str,
    raw_l: u32,
    raw_h: u32,
    cycle_type: u32,
    is_lod: bool,
) -> Vec<String> {
    let mut violations = Vec::new();
    if is_lod {
        violations.push(format!("{name}: material has G_TL_LOD set (unexpected)"));
    }
    let seconds: &[bool] = if cycle_type == 1 {
        &[false, true]
    } else {
        &[true]
    };
    for &second in seconds {
        if color_c_lod_affects_output(raw_l, raw_h, second) {
            violations.push(format!(
                "{name}: color-C cycle{} selects a LOD index (13/14) with a non-annulled A/B \
                 pair (output-affecting) in a non-LOD draw (combine_l={raw_l:#010x} \
                 combine_h={raw_h:#010x}, cycle_type={cycle_type})",
                second as u8
            ));
        }
        if alpha_c_lod_affects_output(raw_l, raw_h, second) {
            violations.push(format!(
                "{name}: alpha-C cycle{} selects a LOD index (0/6) with A≠B (output-affecting) \
                 in a non-LOD draw (combine_l={raw_l:#010x} combine_h={raw_h:#010x}, \
                 cycle_type={cycle_type})",
                second as u8
            ));
        }
    }
    violations
}

#[test]
fn lod_selectors_unreferenced_in_every_non_lod_scene() {
    let mut scene_count = 0;
    let mut material_count = 0;
    let mut violations = Vec::new();
    for scene in crate::tests::fixtures::scenes() {
        let name = format!("{scene}--white64");
        let (rdram, entry_addr) = crate::tests::fixtures::fixture(&name);
        let r = crate::hle::interpret_rdram(rdram, entry_addr as u32);
        scene_count += 1;

        for mat in &r.scene.materials {
            material_count += 1;
            violations.extend(lod_violations_for_material(
                &name,
                mat.selectors.raw_l,
                mat.selectors.raw_h,
                mat.cycle_type,
                mat.lod,
            ));
        }
    }

    assert!(scene_count > 0, "no scenes were decoded — dir glob failed");
    assert!(
        material_count > 0,
        "no materials decoded across {scene_count} scenes — interpret path broken"
    );
    assert!(
        violations.is_empty(),
        "LOD selectors ARE referenced by a non-LOD scene — wiring LOD_FRACTION=1.0/PRIM_LOD_FRAC \
         would NOT be byte-identical. STOP: do not regenerate goldens; a human must approve.\n{}",
        violations.join("\n")
    );
}

#[test]
fn one_cycle_material_ignores_a_lod_selector_living_in_the_dead_cyc0_slots() {
    // Regression for the byte-identity-guard hardening fix: in 1-cycle mode (cycle_type == 0) the
    // shader only ever evaluates cyc1 slots — cyc0 bits are dead regardless of what they decode to.
    // Construct a combine word whose cyc0 fields decode to a color-C LOD selector (idx 13) with a
    // GENUINELY differing, non-annulled A/B pair (a_idx=1 TEXEL0, b_idx=0 COMBINED) — a pattern
    // that, if checked, the guard would correctly flag as output-affecting. cyc1 is left fully
    // clean (all-zero: A=B=C=COMBINED, no LOD reference) so the ONLY possible violation source is
    // cyc0.
    //
    // color: cyc0 a=L[20,4] b=H[28,4] c=L[15,5]; cyc1 a=L[5,4] b=H[24,4] c=L[0,5].
    let l = (1u32 << 20) | (13u32 << 15); // cyc0: a_idx=1 (TEXEL0), c_idx=13 (LOD_FRACTION)
    let h = 0u32; // cyc0: b_idx=0 (COMBINED) -> a_idx != b_idx, NOT annulled

    // Precondition: if this were checked (2-cycle), it WOULD be flagged.
    assert!(
        color_c_lod_affects_output(l, h, /* second (cyc1) = */ false),
        "precondition: cyc0 must be a genuine, non-annulled color-C LOD reference"
    );
    // Precondition: cyc1 is clean regardless of cycle_type.
    assert!(!color_c_lod_affects_output(
        l, h, /* second (cyc1) = */ true
    ));
    assert!(!alpha_c_lod_affects_output(l, h, false));
    assert!(!alpha_c_lod_affects_output(l, h, true));

    // cycle_type = 0 (1-cycle): the guard must NOT flag the dead cyc0 LOD reference.
    let violations = lod_violations_for_material("synthetic-1-cycle", l, h, 0, false);
    assert!(
        violations.is_empty(),
        "1-cycle material must ignore its dead cyc0 slots, but got: {violations:?}"
    );

    // Sanity check the fix actually gates on cycle_type: the SAME raw words with cycle_type = 1
    // (2-cycle, cyc0 live) MUST be flagged — otherwise this test would pass vacuously.
    let violations_2cycle = lod_violations_for_material("synthetic-2-cycle", l, h, 1, false);
    assert!(
        !violations_2cycle.is_empty(),
        "2-cycle material must flag its live cyc0 LOD reference (sanity check for the gate itself)"
    );
}
