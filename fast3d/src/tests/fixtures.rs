pub(crate) const SCENES: &[&str] = &[
    "alpha-threshold",
    "backface-culling",
    "chrome-icosphere",
    "ci4-canary",
    "ci4-grid",
    "ci8-canary",
    "ci8-ramp",
    "decal",
    "fill-texrect",
    "flat-color",
    "fogworld",
    "framebuffer-extent",
    "high-poly",
    "hud-over-3d",
    "i4-ramp",
    "i8-ramp",
    "ia16-ramp",
    "ia4-ramp",
    "ia8-ramp",
    "lights",
    "matrix-stack",
    "mirror-repeat",
    "morphcube",
    "multi-material",
    "offscreen-then-sample",
    "onetri",
    "perspective-cube",
    "segmented-sub-dl",
    "texrectflip",
    "textured-quad",
    "tron",
    "two-cycle-combiner",
    "wrap-repeat",
];

const FIXTURES: &[(&str, u64, &[u8])] = &[
    (
        "alpha-threshold",
        2256,
        include_bytes!("../../tests/fixtures/assembled/alpha-threshold.rdram"),
    ),
    (
        "alpha-threshold--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/alpha-threshold--white64.rdram"),
    ),
    (
        "backface-culling",
        2240,
        include_bytes!("../../tests/fixtures/assembled/backface-culling.rdram"),
    ),
    (
        "backface-culling--white64",
        8384,
        include_bytes!("../../tests/fixtures/assembled/backface-culling--white64.rdram"),
    ),
    (
        "chrome-icosphere",
        3800,
        include_bytes!("../../tests/fixtures/assembled/chrome-icosphere.rdram"),
    ),
    (
        "chrome-icosphere--white64",
        9944,
        include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--white64.rdram"),
    ),
    (
        "ci4-canary",
        728,
        include_bytes!("../../tests/fixtures/assembled/ci4-canary.rdram"),
    ),
    (
        "ci4-canary--white64",
        2264,
        include_bytes!("../../tests/fixtures/assembled/ci4-canary--white64.rdram"),
    ),
    (
        "ci4-grid",
        728,
        include_bytes!("../../tests/fixtures/assembled/ci4-grid.rdram"),
    ),
    (
        "ci4-grid--white64",
        2264,
        include_bytes!("../../tests/fixtures/assembled/ci4-grid--white64.rdram"),
    ),
    (
        "ci8-canary",
        1240,
        include_bytes!("../../tests/fixtures/assembled/ci8-canary.rdram"),
    ),
    (
        "ci8-canary--white64",
        4312,
        include_bytes!("../../tests/fixtures/assembled/ci8-canary--white64.rdram"),
    ),
    (
        "ci8-ramp",
        1240,
        include_bytes!("../../tests/fixtures/assembled/ci8-ramp.rdram"),
    ),
    (
        "ci8-ramp--white64",
        4312,
        include_bytes!("../../tests/fixtures/assembled/ci8-ramp--white64.rdram"),
    ),
    (
        "decal",
        2384,
        include_bytes!("../../tests/fixtures/assembled/decal.rdram"),
    ),
    (
        "decal--white64",
        8528,
        include_bytes!("../../tests/fixtures/assembled/decal--white64.rdram"),
    ),
    (
        "fill-texrect",
        2064,
        include_bytes!("../../tests/fixtures/assembled/fill-texrect.rdram"),
    ),
    (
        "fill-texrect--white64",
        8208,
        include_bytes!("../../tests/fixtures/assembled/fill-texrect--white64.rdram"),
    ),
    (
        "flat-color",
        2256,
        include_bytes!("../../tests/fixtures/assembled/flat-color.rdram"),
    ),
    (
        "flat-color--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/flat-color--white64.rdram"),
    ),
    (
        "fogworld",
        2320,
        include_bytes!("../../tests/fixtures/assembled/fogworld.rdram"),
    ),
    (
        "fogworld--white64",
        8464,
        include_bytes!("../../tests/fixtures/assembled/fogworld--white64.rdram"),
    ),
    (
        "framebuffer-extent",
        2256,
        include_bytes!("../../tests/fixtures/assembled/framebuffer-extent.rdram"),
    ),
    (
        "framebuffer-extent--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/framebuffer-extent--white64.rdram"),
    ),
    (
        "high-poly",
        2688,
        include_bytes!("../../tests/fixtures/assembled/high-poly.rdram"),
    ),
    (
        "high-poly--white64",
        8832,
        include_bytes!("../../tests/fixtures/assembled/high-poly--white64.rdram"),
    ),
    (
        "hud-over-3d",
        2256,
        include_bytes!("../../tests/fixtures/assembled/hud-over-3d.rdram"),
    ),
    (
        "hud-over-3d--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/hud-over-3d--white64.rdram"),
    ),
    (
        "i4-ramp",
        720,
        include_bytes!("../../tests/fixtures/assembled/i4-ramp.rdram"),
    ),
    (
        "i4-ramp--white64",
        2256,
        include_bytes!("../../tests/fixtures/assembled/i4-ramp--white64.rdram"),
    ),
    (
        "i8-ramp",
        1232,
        include_bytes!("../../tests/fixtures/assembled/i8-ramp.rdram"),
    ),
    (
        "i8-ramp--white64",
        4304,
        include_bytes!("../../tests/fixtures/assembled/i8-ramp--white64.rdram"),
    ),
    (
        "ia16-ramp",
        2256,
        include_bytes!("../../tests/fixtures/assembled/ia16-ramp.rdram"),
    ),
    (
        "ia16-ramp--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/ia16-ramp--white64.rdram"),
    ),
    (
        "ia4-ramp",
        720,
        include_bytes!("../../tests/fixtures/assembled/ia4-ramp.rdram"),
    ),
    (
        "ia4-ramp--white64",
        2256,
        include_bytes!("../../tests/fixtures/assembled/ia4-ramp--white64.rdram"),
    ),
    (
        "ia8-ramp",
        1232,
        include_bytes!("../../tests/fixtures/assembled/ia8-ramp.rdram"),
    ),
    (
        "ia8-ramp--white64",
        4304,
        include_bytes!("../../tests/fixtures/assembled/ia8-ramp--white64.rdram"),
    ),
    (
        "lights",
        4296,
        include_bytes!("../../tests/fixtures/assembled/lights.rdram"),
    ),
    (
        "lights--white64",
        10440,
        include_bytes!("../../tests/fixtures/assembled/lights--white64.rdram"),
    ),
    (
        "matrix-stack",
        2432,
        include_bytes!("../../tests/fixtures/assembled/matrix-stack.rdram"),
    ),
    (
        "matrix-stack--white64",
        8576,
        include_bytes!("../../tests/fixtures/assembled/matrix-stack--white64.rdram"),
    ),
    (
        "mirror-repeat",
        2256,
        include_bytes!("../../tests/fixtures/assembled/mirror-repeat.rdram"),
    ),
    (
        "mirror-repeat--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/mirror-repeat--white64.rdram"),
    ),
    (
        "morphcube",
        2672,
        include_bytes!("../../tests/fixtures/assembled/morphcube.rdram"),
    ),
    (
        "morphcube--white64",
        8816,
        include_bytes!("../../tests/fixtures/assembled/morphcube--white64.rdram"),
    ),
    (
        "multi-material",
        2384,
        include_bytes!("../../tests/fixtures/assembled/multi-material.rdram"),
    ),
    (
        "multi-material--white64",
        8528,
        include_bytes!("../../tests/fixtures/assembled/multi-material--white64.rdram"),
    ),
    (
        "offscreen-then-sample",
        2064,
        include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample.rdram"),
    ),
    (
        "offscreen-then-sample--white64",
        8208,
        include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample--white64.rdram"),
    ),
    (
        "onetri",
        2256,
        include_bytes!("../../tests/fixtures/assembled/onetri.rdram"),
    ),
    (
        "onetri--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/onetri--white64.rdram"),
    ),
    (
        "perspective-cube",
        2384,
        include_bytes!("../../tests/fixtures/assembled/perspective-cube.rdram"),
    ),
    (
        "perspective-cube--white64",
        8528,
        include_bytes!("../../tests/fixtures/assembled/perspective-cube--white64.rdram"),
    ),
    (
        "segmented-sub-dl",
        2384,
        include_bytes!("../../tests/fixtures/assembled/segmented-sub-dl.rdram"),
    ),
    (
        "segmented-sub-dl--white64",
        8528,
        include_bytes!("../../tests/fixtures/assembled/segmented-sub-dl--white64.rdram"),
    ),
    (
        "texrectflip",
        2064,
        include_bytes!("../../tests/fixtures/assembled/texrectflip.rdram"),
    ),
    (
        "texrectflip--white64",
        8208,
        include_bytes!("../../tests/fixtures/assembled/texrectflip--white64.rdram"),
    ),
    (
        "textured-quad",
        2256,
        include_bytes!("../../tests/fixtures/assembled/textured-quad.rdram"),
    ),
    (
        "textured-quad--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/textured-quad--white64.rdram"),
    ),
    (
        "tron",
        2320,
        include_bytes!("../../tests/fixtures/assembled/tron.rdram"),
    ),
    (
        "tron--white64",
        8464,
        include_bytes!("../../tests/fixtures/assembled/tron--white64.rdram"),
    ),
    (
        "two-cycle-combiner",
        2256,
        include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner.rdram"),
    ),
    (
        "two-cycle-combiner--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner--white64.rdram"),
    ),
    (
        "wrap-repeat",
        2256,
        include_bytes!("../../tests/fixtures/assembled/wrap-repeat.rdram"),
    ),
    (
        "wrap-repeat--white64",
        8400,
        include_bytes!("../../tests/fixtures/assembled/wrap-repeat--white64.rdram"),
    ),
    (
        "flat-color--white1",
        216,
        include_bytes!("../../tests/fixtures/assembled/flat-color--white1.rdram"),
    ),
    (
        "perspective-cube--white1",
        344,
        include_bytes!("../../tests/fixtures/assembled/perspective-cube--white1.rdram"),
    ),
    (
        "two-cycle-combiner--white1",
        216,
        include_bytes!("../../tests/fixtures/assembled/two-cycle-combiner--white1.rdram"),
    ),
    (
        "framebuffer-extent--white1",
        216,
        include_bytes!("../../tests/fixtures/assembled/framebuffer-extent--white1.rdram"),
    ),
    (
        "offscreen-then-sample--white1",
        24,
        include_bytes!("../../tests/fixtures/assembled/offscreen-then-sample--white1.rdram"),
    ),
    (
        "chrome-icosphere--orange",
        3800,
        include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--orange.rdram"),
    ),
    (
        "chrome-icosphere--blue",
        3800,
        include_bytes!("../../tests/fixtures/assembled/chrome-icosphere--blue.rdram"),
    ),
    (
        "morphcube--t00000000",
        632,
        include_bytes!("../../tests/fixtures/assembled/morphcube--t00000000.rdram"),
    ),
    (
        "morphcube--t3fc90fdb",
        632,
        include_bytes!("../../tests/fixtures/assembled/morphcube--t3fc90fdb.rdram"),
    ),
    (
        "morphcube--t40490fdb",
        632,
        include_bytes!("../../tests/fixtures/assembled/morphcube--t40490fdb.rdram"),
    ),
    (
        "perspective-cube--t00000000",
        344,
        include_bytes!("../../tests/fixtures/assembled/perspective-cube--t00000000.rdram"),
    ),
    (
        "perspective-cube--t40000000",
        344,
        include_bytes!("../../tests/fixtures/assembled/perspective-cube--t40000000.rdram"),
    ),
    (
        "textured-quad--orange-blue",
        2256,
        include_bytes!("../../tests/fixtures/assembled/textured-quad--orange-blue.rdram"),
    ),
    (
        "textured-quad--blend-color",
        2256,
        include_bytes!("../../tests/fixtures/assembled/textured-quad--blend-color.rdram"),
    ),
    (
        "colored-triangle--white1",
        200,
        include_bytes!("../../tests/fixtures/assembled/colored-triangle--white1.rdram"),
    ),
    (
        "lookat--positive-z",
        48,
        include_bytes!("../../tests/fixtures/assembled/lookat--positive-z.rdram"),
    ),
    (
        "texrect--invalid-combiner",
        24,
        include_bytes!("../../tests/fixtures/assembled/texrect--invalid-combiner.rdram"),
    ),
    (
        "flat-color--vertex-colors",
        216,
        include_bytes!("../../tests/fixtures/assembled/flat-color--vertex-colors.rdram"),
    ),
    (
        "texrect--combiner-roles",
        24,
        include_bytes!("../../tests/fixtures/assembled/texrect--combiner-roles.rdram"),
    ),
    (
        "colored-triangle--missing-render-mode",
        192,
        include_bytes!(
            "../../tests/fixtures/assembled/colored-triangle--missing-render-mode.rdram"
        ),
    ),
    (
        "ci8--three-color-tlut",
        32,
        include_bytes!("../../tests/fixtures/assembled/ci8--three-color-tlut.rdram"),
    ),
    (
        "textured-quad--rgba16-4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/textured-quad--rgba16-4x4.rdram"),
    ),
    (
        "i8-ramp--4x4",
        224,
        include_bytes!("../../tests/fixtures/assembled/i8-ramp--4x4.rdram"),
    ),
    (
        "i4-ramp--4x4",
        216,
        include_bytes!("../../tests/fixtures/assembled/i4-ramp--4x4.rdram"),
    ),
    (
        "ia16-ramp--4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/ia16-ramp--4x4.rdram"),
    ),
    (
        "ia8-ramp--4x4",
        224,
        include_bytes!("../../tests/fixtures/assembled/ia8-ramp--4x4.rdram"),
    ),
    (
        "ia4-ramp--4x4",
        216,
        include_bytes!("../../tests/fixtures/assembled/ia4-ramp--4x4.rdram"),
    ),
    (
        "wrap-repeat--4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/wrap-repeat--4x4.rdram"),
    ),
    (
        "mirror-repeat--4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/mirror-repeat--4x4.rdram"),
    ),
    (
        "ci8-ramp--palette",
        1296,
        include_bytes!("../../tests/fixtures/assembled/ci8-ramp--palette.rdram"),
    ),
    (
        "ci8-ramp--canary",
        1296,
        include_bytes!("../../tests/fixtures/assembled/ci8-ramp--canary.rdram"),
    ),
    (
        "ci4-grid--palette",
        752,
        include_bytes!("../../tests/fixtures/assembled/ci4-grid--palette.rdram"),
    ),
    (
        "ci4-grid--canary",
        752,
        include_bytes!("../../tests/fixtures/assembled/ci4-grid--canary.rdram"),
    ),
    (
        "flat-color--translucent",
        216,
        include_bytes!("../../tests/fixtures/assembled/flat-color--translucent.rdram"),
    ),
    (
        "multi-material--rgba16-4x4",
        368,
        include_bytes!("../../tests/fixtures/assembled/multi-material--rgba16-4x4.rdram"),
    ),
    (
        "tron--empty-texture",
        272,
        include_bytes!("../../tests/fixtures/assembled/tron--empty-texture.rdram"),
    ),
    (
        "fogworld--empty-texture",
        272,
        include_bytes!("../../tests/fixtures/assembled/fogworld--empty-texture.rdram"),
    ),
    (
        "alpha-threshold--rgba16-4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/alpha-threshold--rgba16-4x4.rdram"),
    ),
    (
        "decal--empty-texture",
        336,
        include_bytes!("../../tests/fixtures/assembled/decal--empty-texture.rdram"),
    ),
    (
        "decal--in-front",
        272,
        include_bytes!("../../tests/fixtures/assembled/decal--in-front.rdram"),
    ),
    (
        "decal--behind",
        272,
        include_bytes!("../../tests/fixtures/assembled/decal--behind.rdram"),
    ),
    (
        "high-poly--empty-texture",
        640,
        include_bytes!("../../tests/fixtures/assembled/high-poly--empty-texture.rdram"),
    ),
    (
        "fill-texrect--rgba16-4x4",
        48,
        include_bytes!("../../tests/fixtures/assembled/fill-texrect--rgba16-4x4.rdram"),
    ),
    (
        "hud-over-3d--rgba16-4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/hud-over-3d--rgba16-4x4.rdram"),
    ),
    (
        "texrectflip--rgba16-4x4",
        48,
        include_bytes!("../../tests/fixtures/assembled/texrectflip--rgba16-4x4.rdram"),
    ),
    (
        "wrap-repeat--white4",
        240,
        include_bytes!("../../tests/fixtures/assembled/wrap-repeat--white4.rdram"),
    ),
    (
        "mirror-repeat--white4",
        240,
        include_bytes!("../../tests/fixtures/assembled/mirror-repeat--white4.rdram"),
    ),
    (
        "texrect--alpha-over-green",
        24,
        include_bytes!("../../tests/fixtures/assembled/texrect--alpha-over-green.rdram"),
    ),
    (
        "textured-quad--opaque-4x4",
        240,
        include_bytes!("../../tests/fixtures/assembled/textured-quad--opaque-4x4.rdram"),
    ),
];

pub(crate) fn fixture(name: &str) -> (&'static [u8], u64) {
    let &(_, entry, bytes) = FIXTURES
        .iter()
        .find(|&&(key, _, _)| key == name)
        .unwrap_or_else(|| panic!("unknown fixture: {name}"));
    (bytes, entry)
}

#[test]
#[ignore]
fn write_fixtures() {
    fn freeze(name: &str, assemble: impl Fn() -> crate::asm::Image) {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assembled");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.rdram"));
        let image = assemble();
        assert_eq!(
            fixture(name),
            (image.rdram.as_slice(), u64::from(image.entry_addr)),
            "{name}"
        );
        std::fs::write(&path, &image.rdram).unwrap();
        let repeated = assemble();
        assert_eq!(repeated.rdram, std::fs::read(&path).unwrap(), "{name}");
        assert_eq!(repeated.entry_addr, image.entry_addr, "{name}");
        println!(
            "({name:?}, {}, include_bytes!(\"../../tests/fixtures/assembled/{name}.rdram\")),",
            image.entry_addr
        );
    }
    let white32 = vec![255u8; 32 * 32 * 4];
    let white64 = vec![255u8; 64 * 64 * 4];
    {
        freeze("alpha-threshold", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/alpha-threshold.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("alpha-threshold--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/alpha-threshold.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("backface-culling", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/backface-culling.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("backface-culling--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/backface-culling.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("chrome-icosphere", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/chrome-icosphere.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("chrome-icosphere--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/chrome-icosphere.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ci4-canary", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci4-canary.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ci4-canary--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci4-canary.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ci4-grid", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci4-grid.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ci4-grid--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci4-grid.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ci8-canary", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci8-canary.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ci8-canary--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci8-canary.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ci8-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci8-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ci8-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ci8-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("decal", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/decal.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("decal--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/decal.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("fill-texrect", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/fill-texrect.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("fill-texrect--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/fill-texrect.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("flat-color", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/flat-color.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("flat-color--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/flat-color.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("fogworld", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/fogworld.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("fogworld--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/fogworld.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("framebuffer-extent", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/framebuffer-extent.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("framebuffer-extent--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/framebuffer-extent.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("high-poly", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/high-poly.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("high-poly--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/high-poly.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("hud-over-3d", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/hud-over-3d.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("hud-over-3d--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/hud-over-3d.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("i4-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/i4-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("i4-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/i4-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("i8-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/i8-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("i8-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/i8-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ia16-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia16-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ia16-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia16-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ia4-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia4-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ia4-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia4-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("ia8-ramp", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia8-ramp.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("ia8-ramp--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/ia8-ramp.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("lights", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/lights.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("lights--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/lights.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("matrix-stack", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/matrix-stack.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("matrix-stack--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/matrix-stack.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("mirror-repeat", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/mirror-repeat.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("mirror-repeat--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/mirror-repeat.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("morphcube", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/morphcube.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("morphcube--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/morphcube.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("multi-material", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/multi-material.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("multi-material--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/multi-material.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("offscreen-then-sample", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/offscreen-then-sample.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("offscreen-then-sample--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/offscreen-then-sample.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("onetri", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/onetri.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("onetri--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/onetri.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("perspective-cube", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/perspective-cube.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("perspective-cube--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/perspective-cube.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("segmented-sub-dl", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/segmented-sub-dl.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("segmented-sub-dl--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/segmented-sub-dl.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("texrectflip", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/texrectflip.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("texrectflip--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/texrectflip.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("textured-quad", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/textured-quad.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("textured-quad--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/textured-quad.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("tron", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/tron.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("tron--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/tron.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("two-cycle-combiner", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/two-cycle-combiner.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("two-cycle-combiner--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/two-cycle-combiner.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("wrap-repeat", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/wrap-repeat.n64"),
                &white32,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("wrap-repeat--white64", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/wrap-repeat.n64"),
                &white64,
                64,
                64,
            )
            .unwrap()
        });
    }
    {
        freeze("flat-color--white1", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/flat-color.n64"),
                &[255; 4],
                1,
                1,
            )
            .unwrap()
        });
    }
    {
        freeze("perspective-cube--white1", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/perspective-cube.n64"),
                &[255; 4],
                1,
                1,
            )
            .unwrap()
        });
    }
    {
        freeze("two-cycle-combiner--white1", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/two-cycle-combiner.n64"),
                &[255; 4],
                1,
                1,
            )
            .unwrap()
        });
    }
    {
        freeze("framebuffer-extent--white1", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/framebuffer-extent.n64"),
                &[255; 4],
                1,
                1,
            )
            .unwrap()
        });
    }
    {
        freeze("offscreen-then-sample--white1", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/offscreen-then-sample.n64"),
                &[255; 4],
                1,
                1,
            )
            .unwrap()
        });
    }
    {
        let texture = crate::tests::common::solid_env_texture([200, 100, 50]);
        freeze("chrome-icosphere--orange", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/chrome-icosphere.n64"),
                &texture,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        let texture = crate::tests::common::solid_env_texture([40, 60, 220]);
        freeze("chrome-icosphere--blue", || {
            crate::asm::assemble_with_texture(
                include_str!("../../tests/scenes/chrome-icosphere.n64"),
                &texture,
                32,
                32,
            )
            .unwrap()
        });
    }
    {
        freeze("morphcube--t00000000", || {
            crate::asm::assemble_at(
                include_str!("../../tests/scenes/morphcube.n64"),
                0.0,
                Some((&[255; 4], 1, 1)),
            )
            .unwrap()
        });
    }
    {
        freeze("morphcube--t3fc90fdb", || {
            crate::asm::assemble_at(
                include_str!("../../tests/scenes/morphcube.n64"),
                std::f32::consts::FRAC_PI_2,
                Some((&[255; 4], 1, 1)),
            )
            .unwrap()
        });
    }
    {
        freeze("morphcube--t40490fdb", || {
            crate::asm::assemble_at(
                include_str!("../../tests/scenes/morphcube.n64"),
                std::f32::consts::PI,
                Some((&[255; 4], 1, 1)),
            )
            .unwrap()
        });
    }
    {
        freeze("perspective-cube--t00000000", || {
            crate::asm::assemble_at(
                include_str!("../../tests/scenes/perspective-cube.n64"),
                0.0,
                Some((&[255; 4], 1, 1)),
            )
            .unwrap()
        });
    }
    {
        freeze("perspective-cube--t40000000", || {
            crate::asm::assemble_at(
                include_str!("../../tests/scenes/perspective-cube.n64"),
                2.0,
                Some((&[255; 4], 1, 1)),
            )
            .unwrap()
        });
    }
    {
        const SAMPLE_SOURCE_RUST: &str = include_str!("../../tests/scenes/textured-quad.n64");
        const BLEND_COLOR_SOURCE: &str = r#"
Texture tex = { 32, 32, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetBlendColor(18, 52, 86, 120)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;

        fn default_rgba() -> Vec<u8> {
            let mut data = Vec::with_capacity(32 * 32 * 4);
            for row in 0..32usize {
                for _col in 0..32usize {
                    if row < 16 {
                        data.extend_from_slice(&[200u8, 100, 50, 255]);
                    } else {
                        data.extend_from_slice(&[50u8, 100, 200, 255]);
                    }
                }
            }
            data
        }
        let rgba = default_rgba();
        {
            freeze("textured-quad--orange-blue", || {
                crate::asm::assemble_with_texture(SAMPLE_SOURCE_RUST, &rgba, 32, 32).unwrap()
            });
        }
        {
            freeze("textured-quad--blend-color", || {
                crate::asm::assemble_with_texture(BLEND_COLOR_SOURCE, &rgba, 32, 32).unwrap()
            });
        }
    }
    {
        const SAMPLE: &str = "\
// Walking-skeleton sample: one vertex-colored triangle (F3DEX2).
Mtx proj = scale(0.015625)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0, 0, 0, 255,   0,   0, 255 }
Vtx {  48, -48, 0, 0, 0, 0,   0, 255,   0, 255 }
Vtx {   0,  48, 0, 0, 0, 0,   0,   0, 255, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPClearGeometryMode(G_LIGHTING, G_CULL_BACK)
gsSPSetGeometryMode(G_SHADE, G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 3, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSPEndDisplayList()
";
        freeze("colored-triangle--white1", || {
            crate::asm::assemble_with_texture(SAMPLE, &[255u8; 4], 1, 1).unwrap()
        });
    }
    {
        let src = "\
LookAt la = lookat_reflect(0, 0, 100, 0, 0, 0, 0, 1, 0)
Gfx main[] = {
  gsSPLookAt(la)
  gsSPEndDisplayList()
}
";
        freeze("lookat--positive-z", || crate::asm::assemble(src).unwrap());
    }
    {
        let source = r#"
gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 64, 0x00100000)
gsDPSetScissor(0, 0, 0, 256, 256)
gsDPSetOtherMode_H(G_CYC_2CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(7, 0, SHADE, 0, 0, 0, 0, 1, 0, 0, 0, COMBINED, 0, 0, 0, 1)
gsSPTextureRectangle(0, 0, 64, 64, 0, 0, 0, 1024, 1024)
gsDPSetCombineLERP(0, 0, 0, PRIMITIVE, 0, 0, 0, 1, 0, 0, 0, PRIMITIVE, 0, 0, 0, 1)
gsSPTextureRectangle(64, 64, 128, 128, 0, 0, 0, 1024, 1024)
gsSPEndDisplayList()
"#;
        freeze("texrect--invalid-combiner", || {
            crate::asm::assemble_with_texture(source, &[255; 4], 1, 1).unwrap()
        });
    }
    {
        let src = include_str!("../../tests/scenes/flat-color.n64")
            .replace("255,255,255,255", "96,128,160,144");
        freeze("flat-color--vertex-colors", || {
            crate::asm::assemble_with_texture(&src, &[255; 4], 1, 1).unwrap()
        });
    }
    {
        let src = r#"
gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 64, 0x00100000)
gsDPSetScissor(0, 0, 0, 256, 256)
gsDPSetOtherMode_H(G_CYC_2CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(0, 0, 0, PRIMITIVE, 0, 0, 0, 1, 0, 0, 0, PRIMITIVE, 0, 0, 0, 1)
gsSPTextureRectangle(0, 0, 252, 252, 0, 48, 0, 0, 0)
gsSPEndDisplayList()
"#;
        freeze("texrect--combiner-roles", || {
            crate::asm::assemble_with_texture(src, &[255; 4], 1, 1).unwrap()
        });
    }
    {
        const SOURCE_MAP_SRC: &str = "\
Mtx p = scale(0.015625)
Mtx m = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0, 0, 0, 255, 0, 0, 255 }
Vtx {  48, -48, 0, 0, 0, 0, 0, 255, 0, 255 }
Vtx {   0,  48, 0, 0, 0, 0, 0, 0, 255, 255 }

gsSPMatrix(p, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(m, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 3, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSPEndDisplayList()
";
        let source = SOURCE_MAP_SRC.replace("gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)", "");
        freeze("colored-triangle--missing-render-mode", || {
            crate::asm::assemble_at_with_textures(&source, 0.0, &[]).unwrap()
        });
    }
    {
        let src_n64 = "\
Texture tex = { 3, 1, CI8 }
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 3, 1)
gsSPEndDisplayList()
";
        let rgba: [u8; 12] = [0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255];
        freeze("ci8--three-color-tlut", || {
            crate::asm::assemble_with_texture(src_n64, &rgba, 3, 1).unwrap()
        });
    }
    {
        const RGBA16_QUAD_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const RGBA16_QUAD_TEX: &[u8] = &[
            255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 255,
            0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 0, 255, 255, 0, 0, 0, 0, 255, 0,
            255, 255, 0, 0, 0, 0, 255, 0, 255, 255, 0, 0, 0, 0, 255, 0, 255, 255, 0, 0,
        ];
        const I8_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, I8 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_I, G_IM_SIZ_8b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const I4_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, I4 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_I, G_IM_SIZ_4b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const RAMP_TEX: &[u8] = &[
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 85, 85, 85, 255, 85, 85, 85,
            255, 85, 85, 85, 255, 85, 85, 85, 255, 170, 170, 170, 255, 170, 170, 170, 255, 170,
            170, 170, 255, 170, 170, 170, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255,
        ];
        const fn gen_ci8_tex() -> [u8; 32 * 32 * 4] {
            let mut data = [0u8; 32 * 32 * 4];
            let mut row = 0usize;
            while row < 32 {
                let lum = (row * 8) as u8;
                let alpha: u8 = if row.is_multiple_of(2) { 255 } else { 0 };
                let mut col = 0usize;
                while col < 32 {
                    let base = (row * 32 + col) * 4;
                    data[base] = lum;
                    data[base + 1] = lum;
                    data[base + 2] = lum;
                    data[base + 3] = alpha;
                    col += 1;
                }
                row += 1;
            }
            data
        }
        const CI8_TEX_ARRAY: [u8; 32 * 32 * 4] = gen_ci8_tex();
        const CI8_TEX: &[u8] = &CI8_TEX_ARRAY;
        const CI8_RAMP_SRC: &str = r#"
Texture tex = { 32, 32, CI8 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const CI8_CANARY_SRC: &str = r#"
Texture tex = { 32, 32, CI8 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(0, 0, 0, 0, 0, 0, 0, 0, ONE, 0, 8, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_8b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const IA16_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_16b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const IA8_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA8 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_8b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const IA4_RAMP_SRC: &str = r#"
Texture tex = { 4, 4, IA4 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_IA, G_IM_SIZ_4b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const WRAP_TEX: &[u8] = &[
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0,
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 255, 0,
            255, 255, 255, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 255, 255, 0, 255, 255, 255, 0,
            255,
        ];
        const WRAP_REPEAT_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,   0,   0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 256,   0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 256, 256, 255,255,255,255 }
Vtx { -48,  48, 0, 0,   0, 256, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4, 0, 0, 0, 0)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const MIRROR_REPEAT_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,   0,   0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 256,   0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 256, 256, 255,255,255,255 }
Vtx { -48,  48, 0, 0,   0, 256, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4, 1, 0, 1, 0)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const fn gen_ci4_tex() -> [u8; 32 * 32 * 4] {
            const COLORS: [(u8, u8, u8, u8); 16] = [
                (255, 0, 0, 255),
                (255, 128, 0, 0),
                (255, 255, 0, 255),
                (128, 255, 0, 0),
                (0, 255, 0, 255),
                (0, 255, 128, 0),
                (0, 255, 255, 255),
                (0, 128, 255, 0),
                (0, 0, 255, 255),
                (128, 0, 255, 0),
                (255, 0, 255, 255),
                (255, 0, 128, 0),
                (255, 255, 128, 255),
                (128, 255, 255, 0),
                (255, 128, 255, 255),
                (128, 128, 255, 0),
            ];
            let mut data = [0u8; 32 * 32 * 4];
            let mut cell_row = 0usize;
            while cell_row < 4 {
                let mut cell_col = 0usize;
                while cell_col < 4 {
                    let (r, g, b, a) = COLORS[cell_row * 4 + cell_col];
                    let mut py = 0usize;
                    while py < 8 {
                        let mut px = 0usize;
                        while px < 8 {
                            let base = ((cell_row * 8 + py) * 32 + (cell_col * 8 + px)) * 4;
                            data[base] = r;
                            data[base + 1] = g;
                            data[base + 2] = b;
                            data[base + 3] = a;
                            px += 1;
                        }
                        py += 1;
                    }
                    cell_col += 1;
                }
                cell_row += 1;
            }
            data
        }
        const CI4_TEX_ARRAY: [u8; 32 * 32 * 4] = gen_ci4_tex();
        const CI4_TEX: &[u8] = &CI4_TEX_ARRAY;
        const CI4_GRID_SRC: &str = r#"
Texture tex = { 32, 32, CI4 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_4b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const CI4_CANARY_SRC: &str = r#"
Texture tex = { 32, 32, CI4 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetOtherMode_H(14, 2, 0x8000)
gsDPSetCombineLERP(0, 0, 0, 0, 0, 0, 0, 0, ONE, 0, 8, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_CI, G_IM_SIZ_4b, 32, 32)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const XLU_QUAD_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx {  128, -128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx {  128,  128, 0, 0, 0, 0, 255, 255, 255, 255 }
Vtx { -128,  128, 0, 0, 0, 0, 255, 255, 255, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2)
gsDPSetCombineLERP(0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE, 0, 0, 0, PRIMITIVE)
gsDPSetPrimColor(0, 0, 255, 0, 0, 128)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const MULTI_MATERIAL_SRC: &str = include_str!("../../tests/scenes/multi-material.n64");
        const TRON_SRC: &str = include_str!("../../tests/scenes/tron.n64");
        const FOGWORLD_SRC: &str = include_str!("../../tests/scenes/fogworld.n64");
        const ALPHA_THRESHOLD_SRC: &str = include_str!("../../tests/scenes/alpha-threshold.n64");
        const DECAL_SRC: &str = include_str!("../../tests/scenes/decal.n64");

        const DECAL_IN_FRONT_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128, -128, -98, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128, -128, -98, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128,  128,  94, 0, 0, 0, 240, 220,  40, 255 }
Vtx { -128,  128,  94, 0, 0, 0, 240, 220,  40, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 8, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_DECAL, G_RM_AA_ZB_OPA_DECAL2)
gsSP1Triangle(4, 5, 6, 0)
gsSP1Triangle(4, 6, 7, 0)
gsSPEndDisplayList()
"#;
        const DECAL_BEHIND_SRC: &str = r#"
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128, -128, -96, 0, 0, 0,  40,  40, 200, 255 }
Vtx {  128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128,  128,  96, 0, 0, 0,  40,  40, 200, 255 }
Vtx { -128, -128, -94, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128, -128, -94, 0, 0, 0, 240, 220,  40, 255 }
Vtx {  128,  128,  98, 0, 0, 0, 240, 220,  40, 255 }
Vtx { -128,  128,  98, 0, 0, 0, 240, 220,  40, 255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetCombineLERP(0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE, 0, 0, 0, SHADE)
gsSPVertex(verts, 8, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_SURF, G_RM_AA_ZB_OPA_SURF2)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsDPSetRenderMode(G_RM_AA_ZB_OPA_DECAL, G_RM_AA_ZB_OPA_DECAL2)
gsSP1Triangle(4, 5, 6, 0)
gsSP1Triangle(4, 6, 7, 0)
gsSPEndDisplayList()
"#;
        const HIGH_POLY_SRC: &str = include_str!("../../tests/scenes/high-poly.n64");
        const ALPHA_TEXRECT_TEX: &[u8] =
            &[255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 255, 0, 0, 0];
        const ALPHA_TEXRECT_OVER_BG_SRC: &str = r#"
Texture tex = { 2, 2, RGBA16 }
gsDPSetColorImage(G_IM_FMT_RGBA, G_IM_SIZ_16b, 64, 0x00100000)
gsDPSetScissor(0, 0, 0, 256, 256)
// Pass 1 — FILL mode: flood the CIMG with solid green (RGBA16 0x07C1 = G5=31 opaque).
gsDPSetOtherMode_H(G_CYC_FILL)
gsDPSetFillColor(0x07C107C1)
gsDPFillRectangle(0, 0, 256, 256)
// Pass 2 — 1-cycle XLU TEXRECT: TEXEL0 passthrough + AlphaOver blend over green bg.
// G_RM_AA_ZB_XLU_SURF has FORCE_BL → classified DualSrc primary / AlphaOver fallback.
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_AA_ZB_XLU_SURF, G_RM_AA_ZB_XLU_SURF2)
gsDPSetCombineLERP(0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0, 0, 0, 0, TEXEL0)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 2, 2)
gsSPTextureRectangle(0, 0, 256, 256, 0, 0, 0, 1024, 1024)
gsSPEndDisplayList()
"#;
        {
            let texture: &[u8] = RGBA16_QUAD_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("textured-quad--rgba16-4x4", || {
                crate::asm::assemble_with_texture(RGBA16_QUAD_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RAMP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("i8-ramp--4x4", || {
                crate::asm::assemble_with_texture(I8_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RAMP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("i4-ramp--4x4", || {
                crate::asm::assemble_with_texture(I4_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RAMP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ia16-ramp--4x4", || {
                crate::asm::assemble_with_texture(IA16_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RAMP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ia8-ramp--4x4", || {
                crate::asm::assemble_with_texture(IA8_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RAMP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ia4-ramp--4x4", || {
                crate::asm::assemble_with_texture(IA4_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = WRAP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("wrap-repeat--4x4", || {
                crate::asm::assemble_with_texture(WRAP_REPEAT_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = WRAP_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("mirror-repeat--4x4", || {
                crate::asm::assemble_with_texture(MIRROR_REPEAT_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = CI8_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ci8-ramp--palette", || {
                crate::asm::assemble_with_texture(CI8_RAMP_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = CI8_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ci8-ramp--canary", || {
                crate::asm::assemble_with_texture(CI8_CANARY_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = CI4_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ci4-grid--palette", || {
                crate::asm::assemble_with_texture(CI4_GRID_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = CI4_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("ci4-grid--canary", || {
                crate::asm::assemble_with_texture(CI4_CANARY_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[255, 255, 255, 255];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("flat-color--translucent", || {
                crate::asm::assemble_with_texture(XLU_QUAD_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RGBA16_QUAD_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("multi-material--rgba16-4x4", || {
                crate::asm::assemble_with_texture(MULTI_MATERIAL_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("tron--empty-texture", || {
                crate::asm::assemble_with_texture(TRON_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("fogworld--empty-texture", || {
                crate::asm::assemble_with_texture(FOGWORLD_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = RGBA16_QUAD_TEX;
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("alpha-threshold--rgba16-4x4", || {
                crate::asm::assemble_with_texture(ALPHA_THRESHOLD_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("decal--empty-texture", || {
                crate::asm::assemble_with_texture(DECAL_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("decal--in-front", || {
                crate::asm::assemble_with_texture(DECAL_IN_FRONT_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("decal--behind", || {
                crate::asm::assemble_with_texture(DECAL_BEHIND_SRC, texture, side, side).unwrap()
            });
        }
        {
            let texture: &[u8] = &[];
            let side = ((texture.len() / 4) as f64).sqrt() as u32;
            freeze("high-poly--empty-texture", || {
                crate::asm::assemble_with_texture(HIGH_POLY_SRC, texture, side, side).unwrap()
            });
        }
        {
            freeze("fill-texrect--rgba16-4x4", || {
                crate::asm::assemble_with_texture(
                    include_str!("../../tests/scenes/fill-texrect.n64"),
                    RGBA16_QUAD_TEX,
                    4,
                    4,
                )
                .unwrap()
            });
        }
        {
            freeze("hud-over-3d--rgba16-4x4", || {
                crate::asm::assemble_with_texture(
                    include_str!("../../tests/scenes/hud-over-3d.n64"),
                    RGBA16_QUAD_TEX,
                    4,
                    4,
                )
                .unwrap()
            });
        }
        {
            freeze("texrectflip--rgba16-4x4", || {
                crate::asm::assemble_with_texture(
                    include_str!("../../tests/scenes/texrectflip.n64"),
                    RGBA16_QUAD_TEX,
                    4,
                    4,
                )
                .unwrap()
            });
        }
        {
            freeze("wrap-repeat--white4", || {
                crate::asm::assemble_with_texture(WRAP_REPEAT_SRC, &[255; 4 * 4 * 4], 4, 4).unwrap()
            });
        }
        {
            freeze("mirror-repeat--white4", || {
                crate::asm::assemble_with_texture(MIRROR_REPEAT_SRC, &[255; 4 * 4 * 4], 4, 4)
                    .unwrap()
            });
        }
        {
            freeze("texrect--alpha-over-green", || {
                crate::asm::assemble_with_texture(
                    ALPHA_TEXRECT_OVER_BG_SRC,
                    ALPHA_TEXRECT_TEX,
                    2,
                    2,
                )
                .unwrap()
            });
        }
    }
    {
        const RGBA16_QUAD_SRC: &str = r#"
Texture tex = { 4, 4, RGBA16 }
Mtx proj = scale(0.0078125)
Mtx model = identity()
Vp { 640, 480, 511, 0, 640, 480, 511, 0 }
Vtx { -48, -48, 0, 0,    0,    0, 255,255,255,255 }
Vtx {  48, -48, 0, 0, 1024,    0, 255,255,255,255 }
Vtx {  48,  48, 0, 0, 1024, 1024, 255,255,255,255 }
Vtx { -48,  48, 0, 0,    0, 1024, 255,255,255,255 }
gsSPMatrix(proj, G_MTX_PROJECTION | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPMatrix(model, G_MTX_MODELVIEW | G_MTX_LOAD | G_MTX_NOPUSH)
gsSPViewport(vp)
gsSPSetGeometryMode(G_SHADE | G_SHADING_SMOOTH)
gsDPSetOtherMode_H(G_CYC_1CYCLE)
gsDPSetRenderMode(G_RM_OPA_SURF, G_RM_OPA_SURF2)
gsDPSetCombineLERP(TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE, TEXEL0, 0, SHADE, 0, 0, 0, 0, SHADE)
gsDPSetPrimColor(0, 0, 255, 255, 255, 255)
gsDPSetEnvColor(0, 0, 0, 255)
gsDPLoadTextureBlock(tex, G_IM_FMT_RGBA, G_IM_SIZ_16b, 4, 4)
gsSPTexture(0xFFFF, 0xFFFF, 0, G_TX_RENDERTILE, G_ON)
gsSPVertex(verts, 4, 0)
gsSP1Triangle(0, 1, 2, 0)
gsSP1Triangle(0, 2, 3, 0)
gsSPEndDisplayList()
"#;
        const RGBA16_QUAD_TEX: &[u8] = &[
            255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 255,
            0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 0, 255,
            255, 255, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0,
            255,
        ];
        freeze("textured-quad--opaque-4x4", || {
            crate::asm::assemble_with_texture(RGBA16_QUAD_SRC, RGBA16_QUAD_TEX, 4, 4).unwrap()
        });
    }
}
