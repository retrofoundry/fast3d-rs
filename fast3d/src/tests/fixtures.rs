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
        source: "tests/scenes/alpha-threshold.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/alpha-threshold.rdram")),
    },
    Fixture {
        name: "alpha-threshold--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/alpha-threshold.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/alpha-threshold--white64.rdram")),
    },
    Fixture {
        name: "backface-culling", entry_addr: 2240, time_bits: 0, texture: White(32),
        source: "tests/scenes/backface-culling.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/backface-culling.rdram")),
    },
    Fixture {
        name: "backface-culling--white64", entry_addr: 8384, time_bits: 0, texture: White(64),
        source: "tests/scenes/backface-culling.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/backface-culling--white64.rdram")),
    },
    Fixture {
        name: "chrome-icosphere", entry_addr: 3800, time_bits: 0, texture: White(32),
        source: "tests/scenes/chrome-icosphere.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/chrome-icosphere.rdram")),
    },
    Fixture {
        name: "chrome-icosphere--white64", entry_addr: 9944, time_bits: 0, texture: White(64),
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--white64.rdram")),
    },
    Fixture {
        name: "ci4-canary", entry_addr: 728, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci4-canary.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci4-canary.rdram")),
    },
    Fixture {
        name: "ci4-canary--white64", entry_addr: 2264, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci4-canary.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci4-canary--white64.rdram")),
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
        source: "tests/scenes/ci8-canary.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-canary.rdram")),
    },
    Fixture {
        name: "ci8-canary--white64", entry_addr: 4312, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci8-canary.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-canary--white64.rdram")),
    },
    Fixture {
        name: "ci8-ramp", entry_addr: 1240, time_bits: 0, texture: White(32),
        source: "tests/scenes/ci8-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-ramp.rdram")),
    },
    Fixture {
        name: "ci8-ramp--white64", entry_addr: 4312, time_bits: 0, texture: White(64),
        source: "tests/scenes/ci8-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-ramp--white64.rdram")),
    },
    Fixture {
        name: "decal", entry_addr: 2384, time_bits: 0, texture: White(32),
        source: "tests/scenes/decal.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/decal.rdram")),
    },
    Fixture {
        name: "decal--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/decal.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/decal--white64.rdram")),
    },
    Fixture {
        name: "fill-texrect", entry_addr: 2064, time_bits: 0, texture: White(32),
        source: "tests/scenes/fill-texrect.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fill-texrect.rdram")),
    },
    Fixture {
        name: "fill-texrect--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/fill-texrect.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fill-texrect--white64.rdram")),
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
        source: "tests/scenes/fogworld.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fogworld.rdram")),
    },
    Fixture {
        name: "fogworld--white64", entry_addr: 8464, time_bits: 0, texture: White(64),
        source: "tests/scenes/fogworld.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fogworld--white64.rdram")),
    },
    Fixture {
        name: "framebuffer-extent", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/framebuffer-extent.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/framebuffer-extent.rdram")),
    },
    Fixture {
        name: "framebuffer-extent--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/framebuffer-extent.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/framebuffer-extent--white64.rdram")),
    },
    Fixture {
        name: "high-poly", entry_addr: 2688, time_bits: 0, texture: White(32),
        source: "tests/scenes/high-poly.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/high-poly.rdram")),
    },
    Fixture {
        name: "high-poly--white64", entry_addr: 8832, time_bits: 0, texture: White(64),
        source: "tests/scenes/high-poly.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/high-poly--white64.rdram")),
    },
    Fixture {
        name: "hud-over-3d", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/hud-over-3d.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/hud-over-3d.rdram")),
    },
    Fixture {
        name: "hud-over-3d--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/hud-over-3d.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/hud-over-3d--white64.rdram")),
    },
    Fixture {
        name: "i4-ramp", entry_addr: 720, time_bits: 0, texture: White(32),
        source: "tests/scenes/i4-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i4-ramp.rdram")),
    },
    Fixture {
        name: "i4-ramp--white64", entry_addr: 2256, time_bits: 0, texture: White(64),
        source: "tests/scenes/i4-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i4-ramp--white64.rdram")),
    },
    Fixture {
        name: "i8-ramp", entry_addr: 1232, time_bits: 0, texture: White(32),
        source: "tests/scenes/i8-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i8-ramp.rdram")),
    },
    Fixture {
        name: "i8-ramp--white64", entry_addr: 4304, time_bits: 0, texture: White(64),
        source: "tests/scenes/i8-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i8-ramp--white64.rdram")),
    },
    Fixture {
        name: "ia16-ramp", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia16-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia16-ramp.rdram")),
    },
    Fixture {
        name: "ia16-ramp--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia16-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia16-ramp--white64.rdram")),
    },
    Fixture {
        name: "ia4-ramp", entry_addr: 720, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia4-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia4-ramp.rdram")),
    },
    Fixture {
        name: "ia4-ramp--white64", entry_addr: 2256, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia4-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia4-ramp--white64.rdram")),
    },
    Fixture {
        name: "ia8-ramp", entry_addr: 1232, time_bits: 0, texture: White(32),
        source: "tests/scenes/ia8-ramp.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia8-ramp.rdram")),
    },
    Fixture {
        name: "ia8-ramp--white64", entry_addr: 4304, time_bits: 0, texture: White(64),
        source: "tests/scenes/ia8-ramp.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia8-ramp--white64.rdram")),
    },
    Fixture {
        name: "lights", entry_addr: 4296, time_bits: 0, texture: White(32),
        source: "tests/scenes/lights.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/lights.rdram")),
    },
    Fixture {
        name: "lights--white64", entry_addr: 10440, time_bits: 0, texture: White(64),
        source: "tests/scenes/lights.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/lights--white64.rdram")),
    },
    Fixture {
        name: "matrix-stack", entry_addr: 2432, time_bits: 0, texture: White(32),
        source: "tests/scenes/matrix-stack.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/matrix-stack.rdram")),
    },
    Fixture {
        name: "matrix-stack--white64", entry_addr: 8576, time_bits: 0, texture: White(64),
        source: "tests/scenes/matrix-stack.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/matrix-stack--white64.rdram")),
    },
    Fixture {
        name: "mirror-repeat", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/mirror-repeat.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/mirror-repeat.rdram")),
    },
    Fixture {
        name: "mirror-repeat--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/mirror-repeat.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/mirror-repeat--white64.rdram")),
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
        source: "tests/scenes/multi-material.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/multi-material.rdram")),
    },
    Fixture {
        name: "multi-material--white64", entry_addr: 8528, time_bits: 0, texture: White(64),
        source: "tests/scenes/multi-material.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/multi-material--white64.rdram")),
    },
    Fixture {
        name: "offscreen-then-sample", entry_addr: 2064, time_bits: 0, texture: White(32),
        source: "tests/scenes/offscreen-then-sample.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample.rdram")),
    },
    Fixture {
        name: "offscreen-then-sample--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/offscreen-then-sample.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample--white64.rdram")),
    },
    Fixture {
        name: "onetri", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/onetri.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/onetri.rdram")),
    },
    Fixture {
        name: "onetri--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/onetri.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/onetri--white64.rdram")),
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
        source: "tests/scenes/texrectflip.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrectflip.rdram")),
    },
    Fixture {
        name: "texrectflip--white64", entry_addr: 8208, time_bits: 0, texture: White(64),
        source: "tests/scenes/texrectflip.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrectflip--white64.rdram")),
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
        source: "tests/scenes/tron.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/tron.rdram")),
    },
    Fixture {
        name: "tron--white64", entry_addr: 8464, time_bits: 0, texture: White(64),
        source: "tests/scenes/tron.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/tron--white64.rdram")),
    },
    Fixture {
        name: "two-cycle-combiner", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/two-cycle-combiner.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner.rdram")),
    },
    Fixture {
        name: "two-cycle-combiner--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/two-cycle-combiner.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner--white64.rdram")),
    },
    Fixture {
        name: "wrap-repeat", entry_addr: 2256, time_bits: 0, texture: White(32),
        source: "tests/scenes/wrap-repeat.n64", curated: true, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/wrap-repeat.rdram")),
    },
    Fixture {
        name: "wrap-repeat--white64", entry_addr: 8400, time_bits: 0, texture: White(64),
        source: "tests/scenes/wrap-repeat.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/wrap-repeat--white64.rdram")),
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
        source: "tests/scenes/two-cycle-combiner.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner--white1.rdram")),
    },
    Fixture {
        name: "framebuffer-extent--white1", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "tests/scenes/framebuffer-extent.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/framebuffer-extent--white1.rdram")),
    },
    Fixture {
        name: "offscreen-then-sample--white1", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "tests/scenes/offscreen-then-sample.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample--white1.rdram")),
    },
    Fixture {
        name: "chrome-icosphere--orange", entry_addr: 3800, time_bits: 0, texture: Orange,
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--orange.rdram")),
    },
    Fixture {
        name: "chrome-icosphere--blue", entry_addr: 3800, time_bits: 0, texture: Blue,
        source: "tests/scenes/chrome-icosphere.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--blue.rdram")),
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
        source: "decode::SAMPLE", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/colored-triangle--white1.rdram")),
    },
    Fixture {
        name: "lookat--positive-z", entry_addr: 48, time_bits: 0, texture: Empty,
        source: "lookat_roundtrip::sp_lookat_emit_decode_sets_lookat_axes", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/lookat--positive-z.rdram")),
    },
    Fixture {
        name: "texrect--invalid-combiner", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "combiner_tests::combiner_texrect_uses_shared_validation", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrect--invalid-combiner.rdram")),
    },
    Fixture {
        name: "flat-color--vertex-colors", entry_addr: 216, time_bits: 0, texture: White(1),
        source: "combiner_tests::pixels::scene", curated: false, build: Some(super::scene_builders::flat_color),
        frozen: None,
    },
    Fixture {
        name: "texrect--combiner-roles", entry_addr: 24, time_bits: 0, texture: White(1),
        source: "combiner_tests::pixels::check_rect_roles", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrect--combiner-roles.rdram")),
    },
    Fixture {
        name: "colored-triangle--missing-render-mode", entry_addr: 192, time_bits: 0, texture: Empty,
        source: "asm_tests::source_map_resolves_missing_render_mode_diagnostic", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/colored-triangle--missing-render-mode.rdram")),
    },
    Fixture {
        name: "ci8--three-color-tlut", entry_addr: 32, time_bits: 0, texture: ThreeColorTlut,
        source: "asm_tests::ci8_assembler_hle_tlut_roundtrip_correct_count_and_content", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8--three-color-tlut.rdram")),
    },
    Fixture {
        name: "textured-quad--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "goldens::RGBA16_QUAD_SRC", curated: false, build: Some(super::scene_builders::textured_quad),
        frozen: None,
    },
    Fixture {
        name: "i8-ramp--4x4", entry_addr: 224, time_bits: 0, texture: IntensityRamp,
        source: "goldens::I8_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i8-ramp--4x4.rdram")),
    },
    Fixture {
        name: "i4-ramp--4x4", entry_addr: 216, time_bits: 0, texture: IntensityRamp,
        source: "goldens::I4_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/i4-ramp--4x4.rdram")),
    },
    Fixture {
        name: "ia16-ramp--4x4", entry_addr: 240, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA16_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia16-ramp--4x4.rdram")),
    },
    Fixture {
        name: "ia8-ramp--4x4", entry_addr: 224, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA8_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia8-ramp--4x4.rdram")),
    },
    Fixture {
        name: "ia4-ramp--4x4", entry_addr: 216, time_bits: 0, texture: IntensityRamp,
        source: "goldens::IA4_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ia4-ramp--4x4.rdram")),
    },
    Fixture {
        name: "wrap-repeat--4x4", entry_addr: 240, time_bits: 0, texture: WrapQuad,
        source: "goldens::WRAP_REPEAT_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/wrap-repeat--4x4.rdram")),
    },
    Fixture {
        name: "mirror-repeat--4x4", entry_addr: 240, time_bits: 0, texture: WrapQuad,
        source: "goldens::MIRROR_REPEAT_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/mirror-repeat--4x4.rdram")),
    },
    Fixture {
        name: "ci8-ramp--palette", entry_addr: 1296, time_bits: 0, texture: Ci8Palette,
        source: "goldens::CI8_RAMP_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-ramp--palette.rdram")),
    },
    Fixture {
        name: "ci8-ramp--canary", entry_addr: 1296, time_bits: 0, texture: Ci8Palette,
        source: "goldens::CI8_CANARY_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/ci8-ramp--canary.rdram")),
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
        source: "tests/scenes/multi-material.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/multi-material--rgba16-4x4.rdram")),
    },
    Fixture {
        name: "tron--empty-texture", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "tests/scenes/tron.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/tron--empty-texture.rdram")),
    },
    Fixture {
        name: "fogworld--empty-texture", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "tests/scenes/fogworld.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fogworld--empty-texture.rdram")),
    },
    Fixture {
        name: "alpha-threshold--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/alpha-threshold.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/alpha-threshold--rgba16-4x4.rdram")),
    },
    Fixture {
        name: "decal--empty-texture", entry_addr: 336, time_bits: 0, texture: Empty,
        source: "tests/scenes/decal.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/decal--empty-texture.rdram")),
    },
    Fixture {
        name: "decal--in-front", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "goldens::DECAL_IN_FRONT_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/decal--in-front.rdram")),
    },
    Fixture {
        name: "decal--behind", entry_addr: 272, time_bits: 0, texture: Empty,
        source: "goldens::DECAL_BEHIND_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/decal--behind.rdram")),
    },
    Fixture {
        name: "high-poly--empty-texture", entry_addr: 640, time_bits: 0, texture: Empty,
        source: "tests/scenes/high-poly.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/high-poly--empty-texture.rdram")),
    },
    Fixture {
        name: "fill-texrect--rgba16-4x4", entry_addr: 48, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/fill-texrect.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/fill-texrect--rgba16-4x4.rdram")),
    },
    Fixture {
        name: "hud-over-3d--rgba16-4x4", entry_addr: 240, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/hud-over-3d.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/hud-over-3d--rgba16-4x4.rdram")),
    },
    Fixture {
        name: "texrectflip--rgba16-4x4", entry_addr: 48, time_bits: 0, texture: Rgba16Quad,
        source: "tests/scenes/texrectflip.n64", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrectflip--rgba16-4x4.rdram")),
    },
    Fixture {
        name: "wrap-repeat--white4", entry_addr: 240, time_bits: 0, texture: White(4),
        source: "goldens::WRAP_REPEAT_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/wrap-repeat--white4.rdram")),
    },
    Fixture {
        name: "mirror-repeat--white4", entry_addr: 240, time_bits: 0, texture: White(4),
        source: "goldens::MIRROR_REPEAT_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/mirror-repeat--white4.rdram")),
    },
    Fixture {
        name: "texrect--alpha-over-green", entry_addr: 24, time_bits: 0, texture: AlphaTexrect,
        source: "goldens::ALPHA_TEXRECT_OVER_BG_SRC", curated: false, build: None,
        frozen: Some(include_bytes!("../../tests/fixtures/assembled/texrect--alpha-over-green.rdram")),
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
