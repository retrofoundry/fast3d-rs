use crate::{DataFormat, Hardware, Microcode, Rdram, RdramImage};

struct EmbeddedFixture {
    id: &'static str,
    manifest: &'static str,
    manifest_sha256: &'static str,
    rdram: &'static [u8],
    dump: &'static str,
}

mod generated;
mod validation;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Fixture {
    pub(crate) id: &'static str,
    pub(crate) rdram: &'static [u8],
    pub(crate) entry_addr: u32,
    pub(crate) microcode: Microcode,
    pub(crate) data_format: DataFormat,
}

pub(crate) fn get(id: &str) -> Fixture {
    let embedded = generated::EMBEDDED
        .iter()
        .find(|fixture| fixture.id == id)
        .unwrap_or_else(|| {
            panic!(
                "fixture '{id}' is not registered; regenerate fast3d/src/tests/fixtures/generated.rs"
            )
        });
    let validated = validation::validate_embedded(
        embedded.id,
        embedded.manifest,
        embedded.manifest_sha256,
        embedded.rdram,
        embedded.dump,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    Fixture {
        id: embedded.id,
        rdram: embedded.rdram,
        entry_addr: validated.entry_addr,
        microcode: validated.microcode,
        data_format: validated.data_format,
    }
}

impl Fixture {
    pub(crate) fn interpret(&self) -> crate::hle::InterpResult {
        crate::hle::interpret(
            RdramImage::new(self.rdram),
            self.entry_addr as u64,
            self.microcode.into(),
            self.data_format,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn process_dl(
        &self,
        renderer: &mut crate::Renderer,
        diags: &mut dyn crate::DiagSink,
    ) -> crate::DlSummary {
        assert_eq!(
            renderer.data_format, self.data_format,
            "fixture '{}' requires {:?} data; configure the renderer once before process_dl",
            self.id, self.data_format
        );
        renderer.process_dl(self, self.entry_addr as u64, self.microcode, diags)
    }
}

impl Hardware for Fixture {
    fn rdram(&self) -> impl Rdram + '_ {
        RdramImage::new(self.rdram)
    }
}

#[cfg(feature = "asm")]
pub(crate) fn assert_morphcube_compiler_parity(source: &str) {
    let texture = [255u8; 4];
    for (id, time_bits) in [
        ("scene/morphcube/t-00000000/tex-white1", 0x0000_0000),
        ("scene/morphcube/t-3fc90fdb/tex-white1", 0x3fc9_0fdb),
        ("scene/morphcube/t-40490fdb/tex-white1", 0x4049_0fdb),
    ] {
        let compile = || {
            crate::asm::assemble_at(
                source,
                f32::from_bits(time_bits),
                Some((texture.as_slice(), 1, 1)),
            )
            .unwrap_or_else(|diagnostics| {
                panic!("compiler rejected fixture '{id}': {diagnostics:?}")
            })
        };
        let first = compile();
        let second = compile();
        assert_eq!(
            first.entry_addr, second.entry_addr,
            "compiler entry for fixture '{id}' is nondeterministic"
        );
        assert_eq!(
            first.rdram, second.rdram,
            "compiler RDRAM for fixture '{id}' is nondeterministic"
        );

        let fixture = get(id);
        assert_eq!(
            fixture.entry_addr, first.entry_addr,
            "fixture '{id}' entry differs from current compiler output"
        );
        assert_eq!(
            fixture.rdram,
            first.rdram.as_slice(),
            "fixture '{id}' RDRAM differs from current compiler output"
        );
    }
}

pub(crate) fn assert_literal_colored_triangle_interprets() {
    let fixture = get("literal/colored-triangle/v1");
    let interpreted = fixture.interpret();
    assert!(
        interpreted.diags.is_empty(),
        "literal triangle diagnostics: {:?}",
        interpreted.diags
    );
    assert_eq!(interpreted.scene.raw_pos.len(), 3);
    assert_eq!(interpreted.scene.indices, [0, 1, 2]);
}
