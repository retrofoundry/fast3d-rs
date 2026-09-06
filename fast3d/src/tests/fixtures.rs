use std::sync::OnceLock;

use super::dl_builder::Built;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureSpec {
    White(u32),
    Orange,
    Blue,
    OrangeBlue,
    Rgba16Quad,
    OpaqueQuad,
    IntensityRamp,
    WrapQuad,
    Ci8Palette,
    Ci4Palette,
    ThreeColorTlut,
    AlphaTexrect,
    Empty,
}

pub(crate) struct Fixture {
    pub name: &'static str,
    pub entry_addr: u32,
    pub time_bits: u32,
    pub texture: TextureSpec,
    pub source: &'static str,
    pub curated: bool,
    pub build: Option<fn(&Fixture) -> Built>,
    pub frozen: Option<&'static [u8]>,
}

use TextureSpec::*;

#[rustfmt::skip]
pub(crate) const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "alpha-threshold", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/alpha-threshold.n64", curated: true, build: Some(super::scene_builders::alpha_threshold),
        frozen: None,
    },
    Fixture {
        name: "alpha-threshold--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/alpha-threshold.n64", curated: false, build: Some(super::scene_builders::alpha_threshold),
        frozen: None,
    },
    Fixture {
        name: "backface-culling", entry_addr: 2240, time_bits: 0, texture: White(32),
        source: "tests/scenes/backface-culling.n64", curated: true, build: Some(super::scene_builders::backface_culling),
        frozen: None,
    },
    Fixture {
        name: "backface-culling--white64", entry_addr: 8384, time_bits: 0, texture: White(64),
        source: "tests/scenes/backface-culling.n64", curated: false, build: Some(super::scene_builders::backface_culling),
        frozen: None,
    },
    Fixture {
        name: "chrome-icosphere", entry_addr: 3800, time_bits: 0, texture: White(32),
        source: "tests/scenes/chrome-icosphere.n64", curated: true, build: Some(super::scene_builders::chrome_icosphere),
        frozen: None,
    },
    Fixture {
        name: "chrome-icosphere--white64", entry_addr: 9944, time_bits: 0, texture: White(64),
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: Some(super::scene_builders::chrome_icosphere),
        frozen: None,
    },
    Fixture {
        name: "ci4-canary", entry_addr: 728, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci4-canary.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci4-canary--white64", entry_addr: 2264, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci4-canary.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci4-grid", entry_addr: 728, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci4-grid.n64", curated: true, build: Some(super::scene_builders::ci4_grid),
        frozen: None,
    },
    Fixture {
        name: "ci4-grid--white64", entry_addr: 2264, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci4-grid.n64", curated: false, build: Some(super::scene_builders::ci4_grid),
        frozen: None,
    },
    Fixture {
        name: "ci8-canary", entry_addr: 1240, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci8-canary.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci8-canary--white64", entry_addr: 4312, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci8-canary.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci8-ramp", entry_addr: 1240, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci8-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci8-ramp--white64", entry_addr: 4312, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci8-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "decal", entry_addr: 2384, time_bits: 0, texture: White(32),
        source: "tests/scenes/decal.n64", curated: true, build: Some(super::scene_builders::decal),
        frozen: None,
    },
    Fixture {
        name: "decal--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/decal.n64", curated: false, build: Some(super::scene_builders::decal),
        frozen: None,
    },
    Fixture {
        name: "fill-texrect", entry_addr: 2064, time_bits: 0, texture: White(32),
        source: "tests/scenes/fill-texrect.n64", curated: true, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "fill-texrect--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/fill-texrect.n64", curated: false, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "flat-color", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/flat-color.n64", curated: true, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "flat-color--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/flat-color.n64", curated: false, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "fogworld", entry_addr: 2320, time_bits: 0, texture: White(32),
        source: "tests/scenes/fogworld.n64", curated: true, build: Some(super::scene_builders::fogworld),
        frozen: None,
    },
    Fixture {
        name: "fogworld--white64", entry_addr: 8464, time_bits: 0, texture: White(64),
        source: "tests/scenes/fogworld.n64", curated: false, build: Some(super::scene_builders::fogworld),
        frozen: None,
    },
    Fixture {
        name: "framebuffer-extent", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/framebuffer-extent.n64", curated: true, build: Some(super::scene_builders::framebuffer_extent),
        frozen: None,
    },
    Fixture {
        name: "framebuffer-extent--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/framebuffer-extent.n64", curated: false, build: Some(super::scene_builders::framebuffer_extent),
        frozen: None,
    },
    Fixture {
        name: "high-poly", entry_addr: 2688, time_bits: 0, texture: White(32),
        source: "tests/scenes/high-poly.n64", curated: true, build: Some(super::scene_builders::high_poly),
        frozen: None,
    },
    Fixture {
        name: "high-poly--white64", entry_addr: 8832, time_bits: 0, texture: White(64),
        source: "tests/scenes/high-poly.n64", curated: false, build: Some(super::scene_builders::high_poly),
        frozen: None,
    },
    Fixture {
        name: "hud-over-3d", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/hud-over-3d.n64", curated: true, build: Some(super::scene_builders::hud_over_3d),
        frozen: None,
    },
    Fixture {
        name: "hud-over-3d--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/hud-over-3d.n64", curated: false, build: Some(super::scene_builders::hud_over_3d),
        frozen: None,
    },
    Fixture {
        name: "i4-ramp", entry_addr: 720, time_bits: 0, texture: White(32),
        source: "tests/scenes/i4-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "i4-ramp--white64", entry_addr: 2256, time_bits: 0, texture: White(64),
        source: "tests/scenes/i4-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "i8-ramp", entry_addr: 1232, time_bits: 0, texture: White(32),
        source: "tests/scenes/i8-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "i8-ramp--white64", entry_addr: 4304, time_bits: 0, texture: White(64),
        source: "tests/scenes/i8-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia16-ramp", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia16-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia16-ramp--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia16-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia4-ramp", entry_addr: 720, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia4-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia4-ramp--white64", entry_addr: 2256, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia4-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia8-ramp", entry_addr: 1232, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia8-ramp.n64", curated: true, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia8-ramp--white64", entry_addr: 4304, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia8-ramp.n64", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "lights", entry_addr: 4296, time_bits: 0, texture: White(32),
        source: "tests/scenes/lights.n64", curated: true, build: Some(super::scene_builders::lights),
        frozen: None,
    },
    Fixture {
        name: "lights--white64", entry_addr: 10440, time_bits: 0, texture: White(64),
        source: "tests/scenes/lights.n64", curated: false, build: Some(super::scene_builders::lights),
        frozen: None,
    },
    Fixture {
        name: "matrix-stack", entry_addr: 2432, time_bits: 0, texture: White(32),
        source: "tests/scenes/matrix-stack.n64", curated: true, build: Some(super::scene_builders::matrix_stack),
        frozen: None,
    },
    Fixture {
        name: "matrix-stack--white64", entry_addr: 8576, time_bits: 0, texture: White(64),
        source: "tests/scenes/matrix-stack.n64", curated: false, build: Some(super::scene_builders::matrix_stack),
        frozen: None,
    },
    Fixture {
        name: "mirror-repeat", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/mirror-repeat.n64", curated: true, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "mirror-repeat--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/mirror-repeat.n64", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "morphcube", entry_addr: 2672, time_bits: 0, texture: White(32),
        source: "tests/scenes/morphcube.n64", curated: true, build: Some(super::scene_builders::morphcube),
        frozen: None,
    },
    Fixture {
        name: "morphcube--white64", entry_addr: 8816, time_bits: 0, texture: White(64),
        source: "tests/scenes/morphcube.n64", curated: false, build: Some(super::scene_builders::morphcube),
        frozen: None,
    },
    Fixture {
        name: "multi-material", entry_addr: 2384, time_bits: 0, texture: White(32),
        source: "tests/scenes/multi-material.n64", curated: true, build: Some(super::scene_builders::multi_material),
        frozen: None,
    },
    Fixture {
        name: "multi-material--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/multi-material.n64", curated: false, build: Some(super::scene_builders::multi_material),
        frozen: None,
    },
    Fixture {
        name: "offscreen-then-sample", entry_addr: 2064, time_bits: 0, texture: White(32),
        source: "tests/scenes/offscreen-then-sample.n64", curated: true, build: Some(super::scene_builders::offscreen_then_sample),
        frozen: None,
    },
    Fixture {
        name: "offscreen-then-sample--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/offscreen-then-sample.n64", curated: false, build: Some(super::scene_builders::offscreen_then_sample),
        frozen: None,
    },
    Fixture {
        name: "onetri", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/onetri.n64", curated: true, build: Some(super::scene_builders::onetri),
        frozen: None,
    },
    Fixture {
        name: "onetri--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/onetri.n64", curated: false, build: Some(super::scene_builders::onetri),
        frozen: None,
    },
    Fixture {
        name: "perspective-cube", entry_addr: 2384, time_bits: 0, texture: White(32),
        source: "tests/scenes/perspective-cube.n64", curated: true, build: Some(super::scene_builders::perspective_cube),
        frozen: None,
    },
    Fixture {
        name: "perspective-cube--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/perspective-cube.n64", curated: false, build: Some(super::scene_builders::perspective_cube),
        frozen: None,
    },
    Fixture {
        name: "segmented-sub-dl", entry_addr: 2384, time_bits: 0, texture: White(32),
        source: "tests/scenes/segmented-sub-dl.n64", curated: true, build: Some(super::scene_builders::segmented_sub_dl),
        frozen: None,
    },
    Fixture {
        name: "segmented-sub-dl--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/segmented-sub-dl.n64", curated: false, build: Some(super::scene_builders::segmented_sub_dl),
        frozen: None,
    },
    Fixture {
        name: "texrectflip", entry_addr: 2064, time_bits: 0, texture: White(32),
        source: "tests/scenes/texrectflip.n64", curated: true, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "texrectflip--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/texrectflip.n64", curated: false, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "textured-quad", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/textured-quad.n64", curated: true, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "textured-quad--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/textured-quad.n64", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "tron", entry_addr: 2320, time_bits: 0, texture: White(32),
        source: "tests/scenes/tron.n64", curated: true, build: Some(super::scene_builders::tron),
        frozen: None,
    },
    Fixture {
        name: "tron--white64", entry_addr: 8464, time_bits: 0, texture: White(64),
        source: "tests/scenes/tron.n64", curated: false, build: Some(super::scene_builders::tron),
        frozen: None,
    },
    Fixture {
        name: "two-cycle-combiner", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/two-cycle-combiner.n64", curated: true, build: Some(super::scene_builders::two_cycle_combiner),
        frozen: None,
    },
    Fixture {
        name: "two-cycle-combiner--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/two-cycle-combiner.n64", curated: false, build: Some(super::scene_builders::two_cycle_combiner),
        frozen: None,
    },
    Fixture {
        name: "wrap-repeat", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/wrap-repeat.n64", curated: true, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "wrap-repeat--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/wrap-repeat.n64", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "flat-color--white1", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "tests/scenes/flat-color.n64", curated: false, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "perspective-cube--white1", entry_addr: 344, time_bits: 0, texture: White(1),
        source: "tests/scenes/perspective-cube.n64", curated: false, build: Some(super::scene_builders::perspective_cube),
        frozen: None,
    },
    Fixture {
        name: "two-cycle-combiner--white1", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "tests/scenes/two-cycle-combiner.n64", curated: false, build: Some(super::scene_builders::two_cycle_combiner),
        frozen: None,
    },
    Fixture {
        name: "framebuffer-extent--white1", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "tests/scenes/framebuffer-extent.n64", curated: false, build: Some(super::scene_builders::framebuffer_extent),
        frozen: None,
    },
    Fixture {
        name: "offscreen-then-sample--white1", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "tests/scenes/offscreen-then-sample.n64", curated: false, build: Some(super::scene_builders::offscreen_then_sample),
        frozen: None,
    },
    Fixture {
        name: "chrome-icosphere--orange", entry_addr: 3800, time_bits: 0, texture: Orange,
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: Some(super::scene_builders::chrome_icosphere),
        frozen: None,
    },
    Fixture {
        name: "chrome-icosphere--blue", entry_addr: 3800, time_bits: 0, texture: Blue,
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: Some(super::scene_builders::chrome_icosphere),
        frozen: None,
    },
    Fixture {
        name: "morphcube--time-zero", entry_addr: 632, time_bits: 0x00000000, texture: White(1),
        source: "tests/scenes/morphcube.n64", curated: false, build: Some(super::scene_builders::morphcube),
        frozen: None,
    },
    Fixture {
        name: "morphcube--time-half-pi", entry_addr: 632, time_bits: 0x3fc90fdb, texture: White(1),
        source: "tests/scenes/morphcube.n64", curated: false, build: Some(super::scene_builders::morphcube),
        frozen: None,
    },
    Fixture {
        name: "morphcube--time-pi", entry_addr: 632, time_bits: 0x40490fdb, texture: White(1),
        source: "tests/scenes/morphcube.n64", curated: false, build: Some(super::scene_builders::morphcube),
        frozen: None,
    },
    Fixture {
        name: "perspective-cube--time-zero", entry_addr: 344, time_bits: 0x00000000, texture: White(1),
        source: "tests/scenes/perspective-cube.n64", curated: false, build: Some(super::scene_builders::perspective_cube),
        frozen: None,
    },
    Fixture {
        name: "perspective-cube--time-two", entry_addr: 344, time_bits: 0x40000000, texture: White(1),
        source: "tests/scenes/perspective-cube.n64", curated: false, build: Some(super::scene_builders::perspective_cube),
        frozen: None,
    },
    Fixture {
        name: "textured-quad--orange-blue", entry_addr: 2256, time_bits: 0, texture: OrangeBlue,
        source: "tests/scenes/textured-quad.n64", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "textured-quad--blend-color", entry_addr: 2256, time_bits: 0, texture: OrangeBlue,
        source: "e2e::BLEND_COLOR_SOURCE", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "colored-triangle--white1", entry_addr: 200, time_bits: 0, texture: White(1),
        source: "decode::SAMPLE", curated: false, build: Some(super::scene_builders::colored_triangle),
        frozen: None,
    },
    Fixture {
        name: "lookat--positive-z", entry_addr: 48, time_bits: 0, texture: Empty,
        source: "lookat_roundtrip::sp_lookat_emit_decode_sets_lookat_axes", curated: false, build: Some(super::scene_builders::lookat),
        frozen: None,
    },
    Fixture {
        name: "texrect--invalid-combiner", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "combiner_tests::combiner_texrect_uses_shared_validation", curated: false, build: Some(super::scene_builders::texrect),
        frozen: None,
    },
    Fixture {
        name: "flat-color--vertex-colors", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "combiner_tests::pixels::scene", curated: false, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "texrect--combiner-roles", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "combiner_tests::pixels::check_rect_roles", curated: false, build: Some(super::scene_builders::texrect),
        frozen: None,
    },
    Fixture {
        name: "colored-triangle--missing-render-mode", entry_addr: 192, time_bits: 0, texture: Empty,
        source: "asm_tests::source_map_resolves_missing_render_mode_diagnostic", curated: false, build: Some(super::scene_builders::colored_triangle),
        frozen: None,
    },
    Fixture {
        name: "ci8--three-color-tlut", entry_addr: 32, time_bits: 0, texture: ThreeColorTlut,
        source: "asm_tests::ci8_assembler_hle_tlut_roundtrip_correct_count_and_content", curated: false, build: Some(super::scene_builders::three_color_tlut),
        frozen: None,
    },
    Fixture {
        name: "textured-quad--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "goldens::RGBA16_QUAD_SRC", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "i8-ramp--4x4", entry_addr: 224, time_bits: 0, texture: IntensityRamp,
        source: "goldens::I8_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "i4-ramp--4x4", entry_addr: 216, time_bits: 0, texture: IntensityRamp,
        source: "goldens::I4_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia16-ramp--4x4", entry_addr: 240, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA16_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia8-ramp--4x4", entry_addr: 224, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA8_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ia4-ramp--4x4", entry_addr: 216, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA4_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "wrap-repeat--4x4", entry_addr: 240, time_bits: 0, texture: WrapQuad,
        source: "goldens::WRAP_REPEAT_SRC", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "mirror-repeat--4x4", entry_addr: 240, time_bits: 0, texture: WrapQuad,
        source: "goldens::MIRROR_REPEAT_SRC", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "ci8-ramp--palette", entry_addr: 1296, time_bits: 0, texture: Ci8Palette,
        source: "goldens::CI8_RAMP_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci8-ramp--canary", entry_addr: 1296, time_bits: 0, texture: Ci8Palette,
        source: "goldens::CI8_CANARY_SRC", curated: false, build: Some(super::scene_builders::texture_ramp),
        frozen: None,
    },
    Fixture {
        name: "ci4-grid--palette", entry_addr: 752, time_bits: 0, texture: Ci4Palette,
        source: "goldens::CI4_GRID_SRC", curated: false, build: Some(super::scene_builders::ci4_grid),
        frozen: None,
    },
    Fixture {
        name: "ci4-grid--canary", entry_addr: 752, time_bits: 0, texture: Ci4Palette,
        source: "goldens::CI4_CANARY_SRC", curated: false, build: Some(super::scene_builders::ci4_grid),
        frozen: None,
    },
    Fixture {
        name: "flat-color--translucent", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "goldens::XLU_QUAD_SRC", curated: false, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "multi-material--rgba16-4x4", entry_addr: 368, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/multi-material.n64", curated: false, build: Some(super::scene_builders::multi_material),
        frozen: None,
    },
    Fixture {
        name: "tron--empty-texture", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "tests/scenes/tron.n64", curated: false, build: Some(super::scene_builders::tron),
        frozen: None,
    },
    Fixture {
        name: "fogworld--empty-texture", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "tests/scenes/fogworld.n64", curated: false, build: Some(super::scene_builders::fogworld),
        frozen: None,
    },
    Fixture {
        name: "alpha-threshold--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/alpha-threshold.n64", curated: false, build: Some(super::scene_builders::alpha_threshold),
        frozen: None,
    },
    Fixture {
        name: "decal--empty-texture", entry_addr: 336, time_bits: 0, texture: Empty,
        source: "tests/scenes/decal.n64", curated: false, build: Some(super::scene_builders::decal),
        frozen: None,
    },
    Fixture {
        name: "decal--in-front", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "goldens::DECAL_IN_FRONT_SRC", curated: false, build: Some(super::scene_builders::decal),
        frozen: None,
    },
    Fixture {
        name: "decal--behind", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "goldens::DECAL_BEHIND_SRC", curated: false, build: Some(super::scene_builders::decal),
        frozen: None,
    },
    Fixture {
        name: "high-poly--empty-texture", entry_addr: 640, time_bits: 0, texture: Empty,
        source: "tests/scenes/high-poly.n64", curated: false, build: Some(super::scene_builders::high_poly),
        frozen: None,
    },
    Fixture {
        name: "fill-texrect--rgba16-4x4", entry_addr: 48, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/fill-texrect.n64", curated: false, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "hud-over-3d--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/hud-over-3d.n64", curated: false, build: Some(super::scene_builders::hud_over_3d),
        frozen: None,
    },
    Fixture {
        name: "texrectflip--rgba16-4x4", entry_addr: 48, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/texrectflip.n64", curated: false, build: Some(super::scene_builders::fill_texrect),
        frozen: None,
    },
    Fixture {
        name: "wrap-repeat--white4", entry_addr: 240, time_bits: 0, texture: White(4),
        source: "goldens::WRAP_REPEAT_SRC", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "mirror-repeat--white4", entry_addr: 240, time_bits: 0, texture: White(4),
        source: "goldens::MIRROR_REPEAT_SRC", curated: false, build: Some(super::scene_builders::repeat),
        frozen: None,
    },
    Fixture {
        name: "texrect--alpha-over-green", entry_addr: 24, time_bits: 0, texture: AlphaTexrect,
        source: "goldens::ALPHA_TEXRECT_OVER_BG_SRC", curated: false, build: Some(super::scene_builders::texrect),
        frozen: None,
    },
    Fixture {
        name: "textured-quad--opaque-4x4", entry_addr: 240, time_bits: 0, texture: OpaqueQuad,
        source: "render::RGBA16_QUAD_SRC", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
];

pub(crate) fn scenes() -> impl Iterator<Item = &'static str> {
    FIXTURES.iter().filter(|f| f.curated).map(|f| f.name)
}

pub(crate) fn fixture(name: &str) -> (&'static [u8], u64) {
    static BUILT: OnceLock<Vec<Option<Built>>> = OnceLock::new();
    let built = BUILT.get_or_init(|| {
        FIXTURES
            .iter()
            .map(|f| f.build.map(|build| build(f)))
            .collect()
    });
    let index = FIXTURES
        .iter()
        .position(|f| f.name == name)
        .unwrap_or_else(|| panic!("unknown fixture: {name}"));
    match &built[index] {
        Some(built) => (&built.rdram, u64::from(built.entry)),
        None => (
            FIXTURES[index].frozen.expect("fixture has no input"),
            u64::from(FIXTURES[index].entry_addr),
        ),
    }
}
