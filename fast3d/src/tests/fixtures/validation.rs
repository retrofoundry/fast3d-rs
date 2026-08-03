//! Validation for embedded test fixtures.
//!
//! The generated registry pins each canonical manifest by SHA-256, allowing tests to validate
//! artifacts without depending on the unpublished repository tool. That tool owns capture, import,
//! canonical dump rendering, and registry generation.

use crate::{DataFormat, Microcode};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};

const FIXTURE_SCHEMA: u32 = 1;
const RDRAM_FILE: &str = "image.rdram";
const DUMP_FILE: &str = "image.dump";

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ManifestMicrocode {
    F3dex2,
    F3d,
}

impl ManifestMicrocode {
    fn identity_byte(self) -> u8 {
        match self {
            Self::F3dex2 => 0,
            Self::F3d => 1,
        }
    }

    fn runtime(self) -> Microcode {
        match self {
            Self::F3dex2 => Microcode::F3dex2,
            Self::F3d => Microcode::F3d,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ManifestDataFormat {
    Fixed,
    Float,
}

impl ManifestDataFormat {
    fn identity_byte(self) -> u8 {
        match self {
            Self::Fixed => 0,
            Self::Float => 1,
        }
    }

    fn runtime(self) -> DataFormat {
        match self {
            Self::Fixed => DataFormat::Fixed,
            Self::Float => DataFormat::Float,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SourceKind {
    File,
    External,
    LiteralRecipe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct SourceInput {
    kind: SourceKind,
    logical_id: String,
    repository: String,
    revision: String,
    path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct TextureInput {
    name: String,
    format: String,
    width: u32,
    height: u32,
    sha256: String,
    repository: String,
    revision: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    recipe: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct FixtureInput {
    time_f32_bits: String,
    source: SourceInput,
    textures: Vec<TextureInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompilerProvenance {
    name: String,
    repository: Option<String>,
    revision: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct N64GbiProvenance {
    repository: String,
    revision: String,
    crate_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratorProvenance {
    name: String,
    fixture_format_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    capture_kind: String,
    evidence: String,
    compiler: CompilerProvenance,
    n64_gbi: N64GbiProvenance,
    generator: GeneratorProvenance,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    id: String,
    rdram_file: String,
    dump_file: String,
    rdram_len: u64,
    entry_addr: String,
    microcode: ManifestMicrocode,
    data_format: ManifestDataFormat,
    rdram_sha256: String,
    dump_sha256: String,
    artifact_sha256: String,
    input: FixtureInput,
    provenance: Provenance,
}

pub(super) struct ValidatedFixture {
    pub(super) entry_addr: u32,
    pub(super) microcode: Microcode,
    pub(super) data_format: DataFormat,
}

pub(super) fn validate_embedded(
    expected_id: &str,
    manifest_text: &str,
    expected_manifest_sha256: &str,
    rdram: &[u8],
    dump: &str,
) -> Result<ValidatedFixture> {
    validate_id(expected_id).map_err(|error| fixture_error(expected_id, error))?;
    validate_hash(expected_manifest_sha256, "generated manifest_sha256")
        .map_err(|error| fixture_error(expected_id, error))?;
    let actual_manifest_sha256 = sha256_hex(manifest_text.as_bytes());
    if actual_manifest_sha256 != expected_manifest_sha256 {
        return Err(fixture_error(
            expected_id,
            format!(
                "fixture.toml differs from the canonical generated registry: expected {expected_manifest_sha256}, actual {actual_manifest_sha256}"
            ),
        ));
    }
    if manifest_text.contains('\r')
        || !manifest_text.ends_with('\n')
        || manifest_text.ends_with("\n\n")
    {
        return Err(fixture_error(
            expected_id,
            "fixture.toml must be UTF-8/LF with exactly one terminal newline",
        ));
    }

    let manifest: Manifest = toml::from_str(manifest_text)
        .map_err(|error| fixture_error(expected_id, format!("invalid fixture.toml: {error}")))?;
    if manifest.schema != FIXTURE_SCHEMA {
        return Err(fixture_error(
            expected_id,
            format!(
                "unsupported schema {}, expected {FIXTURE_SCHEMA}",
                manifest.schema
            ),
        ));
    }
    if manifest.id != expected_id {
        return Err(fixture_error(
            expected_id,
            format!("manifest id is '{}'", manifest.id),
        ));
    }
    if manifest.rdram_file != RDRAM_FILE || manifest.dump_file != DUMP_FILE {
        return Err(fixture_error(
            expected_id,
            "rdram_file and dump_file must be exactly image.rdram and image.dump",
        ));
    }
    if manifest.rdram_len != rdram.len() as u64 {
        return Err(fixture_error(
            expected_id,
            format!(
                "rdram_len declares {}, actual length is {}",
                manifest.rdram_len,
                rdram.len()
            ),
        ));
    }

    let entry_addr = parse_prefixed_u32(&manifest.entry_addr, "entry_addr")
        .map_err(|error| fixture_error(expected_id, error))?;
    if !entry_addr.is_multiple_of(8) || entry_addr as usize >= rdram.len() {
        return Err(fixture_error(
            expected_id,
            format!("entry_addr 0x{entry_addr:08X} must be aligned and within RDRAM"),
        ));
    }
    if !(rdram.len() - entry_addr as usize).is_multiple_of(8) {
        return Err(fixture_error(
            expected_id,
            "the command arena length is not a multiple of eight bytes",
        ));
    }

    validate_hash(&manifest.rdram_sha256, "rdram_sha256")
        .map_err(|error| fixture_error(expected_id, error))?;
    validate_hash(&manifest.dump_sha256, "dump_sha256")
        .map_err(|error| fixture_error(expected_id, error))?;
    validate_hash(&manifest.artifact_sha256, "artifact_sha256")
        .map_err(|error| fixture_error(expected_id, error))?;
    validate_input(expected_id, &manifest.input)?;
    validate_provenance(expected_id, &manifest.provenance)?;

    let actual_rdram_sha256 = sha256_hex(rdram);
    if actual_rdram_sha256 != manifest.rdram_sha256 {
        return Err(fixture_error(
            expected_id,
            format!(
                "rdram_sha256 declares {}, actual is {actual_rdram_sha256}",
                manifest.rdram_sha256
            ),
        ));
    }
    let actual_artifact_sha256 = artifact_sha256(
        entry_addr,
        manifest.microcode,
        manifest.data_format,
        &manifest.input,
        rdram,
    )?;
    if actual_artifact_sha256 != manifest.artifact_sha256 {
        return Err(fixture_error(
            expected_id,
            format!(
                "artifact_sha256 declares {}, actual is {actual_artifact_sha256}",
                manifest.artifact_sha256
            ),
        ));
    }
    if dump.contains('\r') || !dump.ends_with('\n') || dump.ends_with("\n\n") {
        return Err(fixture_error(
            expected_id,
            "image.dump must be UTF-8/LF with exactly one terminal newline",
        ));
    }
    let actual_dump_sha256 = sha256_hex(dump.as_bytes());
    if actual_dump_sha256 != manifest.dump_sha256 {
        return Err(fixture_error(
            expected_id,
            format!(
                "dump_sha256 declares {}, actual is {actual_dump_sha256}",
                manifest.dump_sha256
            ),
        ));
    }

    Ok(ValidatedFixture {
        entry_addr,
        microcode: manifest.microcode.runtime(),
        data_format: manifest.data_format.runtime(),
    })
}

fn validate_input(id: &str, input: &FixtureInput) -> Result<()> {
    parse_fixed_u32(&input.time_f32_bits, "input.time_f32_bits")
        .map_err(|error| fixture_error(id, error))?;
    validate_hash(&input.source.sha256, "input.source.sha256")
        .map_err(|error| fixture_error(id, error))?;
    if input.source.logical_id.is_empty()
        || input.source.repository.is_empty()
        || input.source.path.is_empty()
    {
        return Err(fixture_error(id, "input.source fields must not be empty"));
    }
    validate_relative_path(&input.source.path, "input.source.path")
        .map_err(|error| fixture_error(id, error))?;
    validate_full_revision(&input.source.revision, "input.source.revision")
        .map_err(|error| fixture_error(id, error))?;

    let mut sorted = input.textures.clone();
    sorted.sort_by(|left, right| left.name.cmp(&right.name));
    if sorted != input.textures {
        return Err(fixture_error(
            id,
            "input.textures must be sorted by logical name",
        ));
    }
    if input.textures.is_empty() {
        return Err(fixture_error(
            id,
            "input.textures must contain an explicit $none entry when no texture is supplied",
        ));
    }
    let mut names = BTreeSet::new();
    for texture in &input.textures {
        if !names.insert(texture.name.as_str()) {
            return Err(fixture_error(
                id,
                format!("duplicate texture input '{}'", texture.name),
            ));
        }
        if texture.format != "rgba8" {
            return Err(fixture_error(
                id,
                format!("texture '{}' format must be rgba8", texture.name),
            ));
        }
        if texture.repository.is_empty() {
            return Err(fixture_error(
                id,
                format!("texture '{}' repository must not be empty", texture.name),
            ));
        }
        if let Some(path) = &texture.path {
            validate_relative_path(path, "input.textures[].path")
                .map_err(|error| fixture_error(id, error))?;
        }
        validate_hash(&texture.sha256, "input.textures[].sha256")
            .map_err(|error| fixture_error(id, error))?;
        validate_full_revision(&texture.revision, "input.textures[].revision")
            .map_err(|error| fixture_error(id, error))?;
        if texture.name == "$none" {
            if texture.width != 0
                || texture.height != 0
                || texture.sha256 != sha256_hex(&[])
                || texture.recipe.as_deref() != Some("empty")
            {
                return Err(fixture_error(
                    id,
                    "$none texture must be 0x0 with the empty-byte hash and recipe 'empty'",
                ));
            }
        } else {
            let expected_len = u64::from(texture.width)
                .checked_mul(u64::from(texture.height))
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| fixture_error(id, "texture dimensions overflow"))?;
            if expected_len == 0 {
                return Err(fixture_error(
                    id,
                    format!("texture '{}' has zero dimensions", texture.name),
                ));
            }
            if texture.path.is_none() && texture.recipe.is_none() {
                return Err(fixture_error(
                    id,
                    format!(
                        "texture '{}' needs a path or deterministic recipe",
                        texture.name
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn validate_provenance(id: &str, provenance: &Provenance) -> Result<()> {
    if provenance.generator.name != "fast3d-fixtures"
        || provenance.generator.fixture_format_version != FIXTURE_SCHEMA
    {
        return Err(fixture_error(
            id,
            "unknown fixture generator or format version",
        ));
    }
    if provenance.n64_gbi.repository.is_empty() || provenance.n64_gbi.crate_version.is_empty() {
        return Err(fixture_error(
            id,
            "n64-gbi provenance fields must not be empty",
        ));
    }
    validate_full_revision(&provenance.n64_gbi.revision, "provenance.n64_gbi.revision")
        .map_err(|error| fixture_error(id, error))?;
    match provenance.capture_kind.as_str() {
        "compiler-compatibility" => {
            if provenance.compiler.name == "none"
                || provenance
                    .compiler
                    .repository
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty()
                || provenance.compiler.revision.is_none()
                || provenance.evidence != "compiler-origin; not independent protocol evidence"
            {
                return Err(fixture_error(
                    id,
                    "invalid compiler-compatibility provenance",
                ));
            }
            validate_full_revision(
                provenance.compiler.revision.as_deref().unwrap_or_default(),
                "provenance.compiler.revision",
            )
            .map_err(|error| fixture_error(id, error))?;
        }
        "fast3d-literal" => {
            if provenance.compiler.name != "none"
                || provenance.compiler.repository.is_some()
                || provenance.compiler.revision.is_some()
            {
                return Err(fixture_error(
                    id,
                    "literal provenance must name no compiler",
                ));
            }
        }
        other => {
            return Err(fixture_error(
                id,
                format!("unknown provenance.capture_kind '{other}'"),
            ));
        }
    }
    Ok(())
}

fn artifact_sha256(
    entry_addr: u32,
    microcode: ManifestMicrocode,
    data_format: ManifestDataFormat,
    input: &FixtureInput,
    rdram: &[u8],
) -> Result<String> {
    let mut identity = Vec::new();
    identity.extend_from_slice(b"fast3d-fixture\0v1\0");
    identity.extend_from_slice(&entry_addr.to_be_bytes());
    identity.push(microcode.identity_byte());
    identity.push(data_format.identity_byte());
    identity.extend_from_slice(&decode_hash(&input.source.sha256)?);
    identity.extend_from_slice(
        &parse_fixed_u32(&input.time_f32_bits, "input.time_f32_bits")?.to_be_bytes(),
    );
    let mut textures = input.textures.iter().collect::<Vec<_>>();
    textures.sort_by(|left, right| left.name.cmp(&right.name));
    identity.extend_from_slice(&(textures.len() as u32).to_be_bytes());
    for texture in textures {
        identity.extend_from_slice(&(texture.name.len() as u32).to_be_bytes());
        identity.extend_from_slice(texture.name.as_bytes());
        identity.extend_from_slice(&texture.width.to_be_bytes());
        identity.extend_from_slice(&texture.height.to_be_bytes());
        identity.extend_from_slice(&decode_hash(&texture.sha256)?);
    }
    identity.extend_from_slice(&(rdram.len() as u64).to_be_bytes());
    identity.extend_from_slice(rdram);
    Ok(sha256_hex(&identity))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn parse_prefixed_u32(value: &str, field: &str) -> Result<u32> {
    let Some(digits) = value.strip_prefix("0x") else {
        return Err(format!(
            "{field} must be a string formatted as 0x followed by eight uppercase hexadecimal digits"
        ));
    };
    if digits.len() != 8
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
    {
        return Err(format!(
            "{field} must be a string formatted as 0x followed by eight uppercase hexadecimal digits"
        ));
    }
    u32::from_str_radix(digits, 16).map_err(|error| format!("invalid {field}: {error}"))
}

fn parse_fixed_u32(value: &str, field: &str) -> Result<u32> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{field} must be a string containing exactly eight hexadecimal digits"
        ));
    }
    u32::from_str_radix(value, 16).map_err(|error| format!("invalid {field}: {error}"))
}

fn validate_full_revision(value: &str, field: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field} must be a full 40-digit Git revision"));
    }
    Ok(())
}

fn validate_hash(value: &str, field: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{field} must be 64 lowercase hexadecimal digits"));
    }
    Ok(())
}

fn decode_hash(value: &str) -> Result<[u8; 32]> {
    validate_hash(value, "sha256")?;
    let mut bytes = [0u8; 32];
    for (index, output) in bytes.iter_mut().enumerate() {
        *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|error| format!("invalid SHA-256: {error}"))?;
    }
    Ok(bytes)
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.starts_with('/')
        || id.ends_with('/')
        || id.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(format!("invalid fixture id '{id}'"));
    }
    Ok(())
}

fn validate_relative_path(value: &str, field: &str) -> Result<()> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{field} must be a normalized repository-relative path"
        ));
    }
    Ok(())
}

fn fixture_error(id: &str, message: impl std::fmt::Display) -> String {
    format!("fixture '{id}' is missing or stale: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_ID: &str = "validator/fixture/v1";
    const SOURCE_REPOSITORY: &str = "https://example.invalid/source";
    const TEXTURE_REPOSITORY: &str = "https://example.invalid/texture";
    const N64_GBI_REPOSITORY: &str = "https://example.invalid/n64-gbi";

    #[derive(Clone)]
    struct FixtureCase {
        expected_id: String,
        manifest: String,
        rdram: Vec<u8>,
        dump: String,
    }

    fn repeated(character: char, length: usize) -> String {
        character.to_string().repeat(length)
    }

    fn hash(character: char) -> String {
        repeated(character, 64)
    }

    fn revision(character: char) -> String {
        repeated(character, 40)
    }

    fn valid_input() -> FixtureInput {
        FixtureInput {
            time_f32_bits: "00000000".into(),
            source: SourceInput {
                kind: SourceKind::LiteralRecipe,
                logical_id: VALID_ID.into(),
                repository: SOURCE_REPOSITORY.into(),
                revision: revision('1'),
                path: "fixtures/source.n64".into(),
                sha256: hash('a'),
            },
            textures: vec![TextureInput {
                name: "texture".into(),
                format: "rgba8".into(),
                width: 1,
                height: 1,
                sha256: hash('b'),
                repository: TEXTURE_REPOSITORY.into(),
                revision: revision('2'),
                path: None,
                recipe: Some("solid-rgba8:ffffffff".into()),
            }],
        }
    }

    fn texture_block(name: &str, hash: &str) -> String {
        format!(
            r#"[[input.textures]]
name = "{name}"
format = "rgba8"
width = 1
height = 1
sha256 = "{hash}"
repository = "{TEXTURE_REPOSITORY}"
revision = "{}"
recipe = "solid-rgba8:ffffffff"
"#,
            revision('2')
        )
    }

    fn valid_case() -> FixtureCase {
        let rdram = (0_u8..16).collect::<Vec<_>>();
        let dump = "validator dump\n".to_owned();
        let input = valid_input();
        let rdram_sha256 = sha256_hex(&rdram);
        let dump_sha256 = sha256_hex(dump.as_bytes());
        let artifact_sha256 = artifact_sha256(
            8,
            ManifestMicrocode::F3dex2,
            ManifestDataFormat::Fixed,
            &input,
            &rdram,
        )
        .expect("the valid test input must have an artifact identity");
        let texture = texture_block("texture", &hash('b'));
        let manifest = format!(
            r#"schema = {FIXTURE_SCHEMA}
id = "{VALID_ID}"
rdram_file = "{RDRAM_FILE}"
dump_file = "{DUMP_FILE}"
rdram_len = 16
entry_addr = "0x00000008"
microcode = "f3dex2"
data_format = "fixed"
rdram_sha256 = "{rdram_sha256}"
dump_sha256 = "{dump_sha256}"
artifact_sha256 = "{artifact_sha256}"

[input]
time_f32_bits = "00000000"

[input.source]
kind = "literal-recipe"
logical_id = "{VALID_ID}"
repository = "{SOURCE_REPOSITORY}"
revision = "{}"
path = "fixtures/source.n64"
sha256 = "{}"

{texture}
[provenance]
capture_kind = "fast3d-literal"
evidence = "literal test fixture"

[provenance.compiler]
name = "none"

[provenance.n64_gbi]
repository = "{N64_GBI_REPOSITORY}"
revision = "{}"
crate_version = "0.1.0"

[provenance.generator]
name = "fast3d-fixtures"
fixture_format_version = {FIXTURE_SCHEMA}
"#,
            revision('1'),
            hash('a'),
            revision('3')
        );
        FixtureCase {
            expected_id: VALID_ID.into(),
            manifest,
            rdram,
            dump,
        }
    }

    fn replace_once(case: &mut FixtureCase, original: &str, replacement: &str) {
        assert_eq!(
            case.manifest.matches(original).count(),
            1,
            "test mutation target must occur exactly once: {original:?}"
        );
        case.manifest = case.manifest.replacen(original, replacement, 1);
    }

    fn append_texture(case: &mut FixtureCase, name: &str, texture_hash: &str) {
        let provenance_offset = case
            .manifest
            .find("[provenance]")
            .expect("the test manifest must contain provenance");
        case.manifest
            .insert_str(provenance_offset, &texture_block(name, texture_hash));
    }

    fn remove_textures(case: &mut FixtureCase) {
        let texture_offset = case
            .manifest
            .find("[[input.textures]]")
            .expect("the test manifest must contain a texture");
        let provenance_offset = case
            .manifest
            .find("[provenance]")
            .expect("the test manifest must contain provenance");
        case.manifest
            .replace_range(texture_offset..provenance_offset, "");
    }

    fn compiler_case() -> FixtureCase {
        let mut case = valid_case();
        replace_once(
            &mut case,
            "capture_kind = \"fast3d-literal\"",
            "capture_kind = \"compiler-compatibility\"",
        );
        replace_once(
            &mut case,
            "evidence = \"literal test fixture\"",
            "evidence = \"compiler-origin; not independent protocol evidence\"",
        );
        replace_once(
            &mut case,
            "[provenance.compiler]\nname = \"none\"",
            &format!(
                "[provenance.compiler]\nname = \"test-compiler\"\nrepository = \"https://example.invalid/compiler\"\nrevision = \"{}\"",
                revision('4')
            ),
        );
        case
    }

    fn validation_error_with_manifest_hash(case: &FixtureCase, manifest_hash: &str) -> String {
        match validate_embedded(
            &case.expected_id,
            &case.manifest,
            manifest_hash,
            &case.rdram,
            &case.dump,
        ) {
            Ok(_) => panic!("fixture unexpectedly validated"),
            Err(error) => error,
        }
    }

    fn validation_error(case: &FixtureCase) -> String {
        let manifest_hash = sha256_hex(case.manifest.as_bytes());
        validation_error_with_manifest_hash(case, &manifest_hash)
    }

    fn assert_rejected(case: &FixtureCase, message: impl std::fmt::Display) {
        assert_eq!(
            validation_error(case),
            fixture_error(&case.expected_id, message)
        );
    }

    fn parsed_manifest(case: &FixtureCase) -> Manifest {
        toml::from_str(&case.manifest).expect("the test mutation must remain valid TOML")
    }

    fn assert_artifact_mismatch(case: &FixtureCase) {
        let manifest = parsed_manifest(case);
        let entry_addr = parse_prefixed_u32(&manifest.entry_addr, "entry_addr")
            .expect("the test entry address must remain valid");
        let actual = artifact_sha256(
            entry_addr,
            manifest.microcode,
            manifest.data_format,
            &manifest.input,
            &case.rdram,
        )
        .expect("the mutated declared input must remain well formed");
        assert_ne!(
            actual, manifest.artifact_sha256,
            "the mutation must change artifact identity"
        );
        assert_rejected(
            case,
            format!(
                "artifact_sha256 declares {}, actual is {actual}",
                manifest.artifact_sha256
            ),
        );
    }

    #[test]
    fn accepts_well_formed_fixture() {
        let case = valid_case();
        let manifest_hash = sha256_hex(case.manifest.as_bytes());
        let validated = validate_embedded(
            &case.expected_id,
            &case.manifest,
            &manifest_hash,
            &case.rdram,
            &case.dump,
        )
        .expect("a well-formed fixture must validate");

        assert_eq!(validated.entry_addr, 8);
        assert_eq!(validated.microcode, Microcode::F3dex2);
        assert_eq!(validated.data_format, DataFormat::Fixed);
    }

    #[test]
    fn rejects_every_malformed_fixture_id_shape() {
        for id in [
            "",
            "/absolute",
            "trailing/",
            "empty//segment",
            "dot/./segment",
            "parent/../segment",
            "invalid/space here",
        ] {
            let mut case = valid_case();
            case.expected_id = id.into();
            assert_rejected(&case, format!("invalid fixture id '{id}'"));
        }
    }

    #[test]
    fn rejects_malformed_generated_manifest_hash() {
        let case = valid_case();
        for invalid_hash in [
            repeated('a', 63),
            repeated('a', 65),
            repeated('A', 64),
            format!("{}g", repeated('a', 63)),
        ] {
            assert_eq!(
                validation_error_with_manifest_hash(&case, &invalid_hash),
                fixture_error(
                    &case.expected_id,
                    "generated manifest_sha256 must be 64 lowercase hexadecimal digits"
                )
            );
        }
    }

    #[test]
    fn rejects_manifest_hash_mismatch() {
        let case = valid_case();
        let expected = hash('0');
        let actual = sha256_hex(case.manifest.as_bytes());
        assert_eq!(
            validation_error_with_manifest_hash(&case, &expected),
            fixture_error(
                &case.expected_id,
                format!(
                    "fixture.toml differs from the canonical generated registry: expected {expected}, actual {actual}"
                )
            )
        );
    }

    #[test]
    fn rejects_invalid_manifest_newline_forms() {
        let mut carriage_return = valid_case();
        carriage_return.manifest = carriage_return.manifest.replacen('\n', "\r\n", 1);
        let mut missing_newline = valid_case();
        assert_eq!(missing_newline.manifest.pop(), Some('\n'));
        let mut double_newline = valid_case();
        double_newline.manifest.push('\n');

        for case in [carriage_return, missing_newline, double_newline] {
            assert_rejected(
                &case,
                "fixture.toml must be UTF-8/LF with exactly one terminal newline",
            );
        }
    }

    #[test]
    fn rejects_invalid_manifest_toml() {
        let mut malformed = valid_case();
        replace_once(&mut malformed, "schema = 1", "schema = [");
        let mut unknown_field = valid_case();
        unknown_field.manifest.insert_str(0, "unknown = true\n");
        let mut invalid_enum = valid_case();
        replace_once(
            &mut invalid_enum,
            "microcode = \"f3dex2\"",
            "microcode = \"unknown\"",
        );
        let mut missing_field = valid_case();
        replace_once(&mut missing_field, "dump_file = \"image.dump\"\n", "");

        for case in [malformed, unknown_field, invalid_enum, missing_field] {
            let parse_error = toml::from_str::<Manifest>(&case.manifest)
                .expect_err("the mutation must be rejected by manifest deserialization");
            assert_rejected(&case, format!("invalid fixture.toml: {parse_error}"));
        }
    }

    #[test]
    fn rejects_unsupported_schema() {
        let mut case = valid_case();
        replace_once(&mut case, "schema = 1", "schema = 2");
        assert_rejected(&case, "unsupported schema 2, expected 1");
    }

    #[test]
    fn rejects_manifest_id_mismatch() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("schema = 1\nid = \"{VALID_ID}\""),
            "schema = 1\nid = \"validator/other/v1\"",
        );
        assert_rejected(&case, "manifest id is 'validator/other/v1'");
    }

    #[test]
    fn rejects_wrong_artifact_file_names() {
        let mut wrong_rdram = valid_case();
        replace_once(
            &mut wrong_rdram,
            "rdram_file = \"image.rdram\"",
            "rdram_file = \"other.rdram\"",
        );
        let mut wrong_dump = valid_case();
        replace_once(
            &mut wrong_dump,
            "dump_file = \"image.dump\"",
            "dump_file = \"other.dump\"",
        );

        for case in [wrong_rdram, wrong_dump] {
            assert_rejected(
                &case,
                "rdram_file and dump_file must be exactly image.rdram and image.dump",
            );
        }
    }

    #[test]
    fn rejects_truncated_rdram() {
        let mut case = valid_case();
        case.rdram.pop();
        assert_rejected(&case, "rdram_len declares 16, actual length is 15");
    }

    #[test]
    fn rejects_entry_address_without_prefix() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            "entry_addr = \"0x00000008\"",
            "entry_addr = \"00000008\"",
        );
        assert_rejected(
            &case,
            "entry_addr must be a string formatted as 0x followed by eight uppercase hexadecimal digits",
        );
    }

    #[test]
    fn rejects_malformed_entry_address_digits() {
        for entry_addr in ["0x0000008", "0x0000000a", "0x0000000G"] {
            let mut case = valid_case();
            replace_once(
                &mut case,
                "entry_addr = \"0x00000008\"",
                &format!("entry_addr = \"{entry_addr}\""),
            );
            assert_rejected(
                &case,
                "entry_addr must be a string formatted as 0x followed by eight uppercase hexadecimal digits",
            );
        }
    }

    #[test]
    fn rejects_unaligned_or_out_of_range_entry_address() {
        for (entry_addr, rendered) in [("0x00000004", "0x00000004"), ("0x00000010", "0x00000010")] {
            let mut case = valid_case();
            replace_once(
                &mut case,
                "entry_addr = \"0x00000008\"",
                &format!("entry_addr = \"{entry_addr}\""),
            );
            assert_rejected(
                &case,
                format!("entry_addr {rendered} must be aligned and within RDRAM"),
            );
        }
    }

    #[test]
    fn rejects_non_command_aligned_rdram_tail() {
        let mut case = valid_case();
        case.rdram.push(16);
        replace_once(&mut case, "rdram_len = 16", "rdram_len = 17");
        assert_rejected(
            &case,
            "the command arena length is not a multiple of eight bytes",
        );
    }

    #[test]
    fn rejects_malformed_rdram_hash() {
        let mut case = valid_case();
        let actual = sha256_hex(&case.rdram);
        replace_once(
            &mut case,
            &format!("rdram_sha256 = \"{actual}\""),
            &format!("rdram_sha256 = \"{}\"", repeated('a', 63)),
        );
        assert_rejected(
            &case,
            "rdram_sha256 must be 64 lowercase hexadecimal digits",
        );
    }

    #[test]
    fn rejects_malformed_dump_hash() {
        let mut case = valid_case();
        let actual = sha256_hex(case.dump.as_bytes());
        replace_once(
            &mut case,
            &format!("dump_sha256 = \"{actual}\""),
            &format!("dump_sha256 = \"{}\"", repeated('A', 64)),
        );
        assert_rejected(&case, "dump_sha256 must be 64 lowercase hexadecimal digits");
    }

    #[test]
    fn rejects_malformed_artifact_hash() {
        let mut case = valid_case();
        let declared = parsed_manifest(&case).artifact_sha256;
        replace_once(
            &mut case,
            &format!("artifact_sha256 = \"{declared}\""),
            &format!("artifact_sha256 = \"{}g\"", repeated('a', 63)),
        );
        assert_rejected(
            &case,
            "artifact_sha256 must be 64 lowercase hexadecimal digits",
        );
    }

    #[test]
    fn rejects_malformed_time_bits() {
        for time_bits in ["0000000", "000000000", "0000000g"] {
            let mut case = valid_case();
            replace_once(
                &mut case,
                "time_f32_bits = \"00000000\"",
                &format!("time_f32_bits = \"{time_bits}\""),
            );
            assert_rejected(
                &case,
                "input.time_f32_bits must be a string containing exactly eight hexadecimal digits",
            );
        }
    }

    #[test]
    fn rejects_malformed_source_hash() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("sha256 = \"{}\"", hash('a')),
            &format!("sha256 = \"{}\"", repeated('a', 63)),
        );
        assert_rejected(
            &case,
            "input.source.sha256 must be 64 lowercase hexadecimal digits",
        );
    }

    #[test]
    fn rejects_empty_source_fields() {
        let mut empty_logical_id = valid_case();
        replace_once(
            &mut empty_logical_id,
            &format!("logical_id = \"{VALID_ID}\""),
            "logical_id = \"\"",
        );
        let mut empty_repository = valid_case();
        replace_once(
            &mut empty_repository,
            &format!("repository = \"{SOURCE_REPOSITORY}\""),
            "repository = \"\"",
        );
        let mut empty_path = valid_case();
        replace_once(
            &mut empty_path,
            "path = \"fixtures/source.n64\"",
            "path = \"\"",
        );

        for case in [empty_logical_id, empty_repository, empty_path] {
            assert_rejected(&case, "input.source fields must not be empty");
        }
    }

    #[test]
    fn rejects_absolute_or_traversing_source_path() {
        for path in ["/absolute/source.n64", "../escape/source.n64"] {
            let mut case = valid_case();
            replace_once(
                &mut case,
                "path = \"fixtures/source.n64\"",
                &format!("path = \"{path}\""),
            );
            assert_rejected(
                &case,
                "input.source.path must be a normalized repository-relative path",
            );
        }
    }

    #[test]
    fn rejects_malformed_source_revision() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("revision = \"{}\"", revision('1')),
            &format!("revision = \"{}\"", repeated('1', 39)),
        );
        assert_rejected(
            &case,
            "input.source.revision must be a full 40-digit Git revision",
        );
    }

    #[test]
    fn rejects_unsorted_textures() {
        let mut case = valid_case();
        append_texture(&mut case, "alpha", &hash('c'));
        assert_rejected(&case, "input.textures must be sorted by logical name");
    }

    #[test]
    fn rejects_empty_textures() {
        let mut case = valid_case();
        remove_textures(&mut case);
        replace_once(
            &mut case,
            "[input]\ntime_f32_bits = \"00000000\"",
            "[input]\ntime_f32_bits = \"00000000\"\ntextures = []",
        );
        assert_rejected(
            &case,
            "input.textures must contain an explicit $none entry when no texture is supplied",
        );
    }

    #[test]
    fn rejects_duplicate_texture_names() {
        let mut case = valid_case();
        append_texture(&mut case, "texture", &hash('c'));
        assert_rejected(&case, "duplicate texture input 'texture'");
    }

    #[test]
    fn rejects_non_rgba8_texture() {
        let mut case = valid_case();
        replace_once(&mut case, "format = \"rgba8\"", "format = \"rgba16\"");
        assert_rejected(&case, "texture 'texture' format must be rgba8");
    }

    #[test]
    fn rejects_empty_texture_repository() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("repository = \"{TEXTURE_REPOSITORY}\""),
            "repository = \"\"",
        );
        assert_rejected(&case, "texture 'texture' repository must not be empty");
    }

    #[test]
    fn rejects_absolute_or_traversing_texture_path() {
        for path in ["/absolute/texture.rgba8", "../escape/texture.rgba8"] {
            let mut case = valid_case();
            replace_once(
                &mut case,
                "recipe = \"solid-rgba8:ffffffff\"",
                &format!("path = \"{path}\"\nrecipe = \"solid-rgba8:ffffffff\""),
            );
            assert_rejected(
                &case,
                "input.textures[].path must be a normalized repository-relative path",
            );
        }
    }

    #[test]
    fn rejects_malformed_texture_hash() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("sha256 = \"{}\"", hash('b')),
            &format!("sha256 = \"{}\"", repeated('b', 65)),
        );
        assert_rejected(
            &case,
            "input.textures[].sha256 must be 64 lowercase hexadecimal digits",
        );
    }

    #[test]
    fn rejects_malformed_texture_revision() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("revision = \"{}\"", revision('2')),
            &format!("revision = \"{}g\"", repeated('2', 39)),
        );
        assert_rejected(
            &case,
            "input.textures[].revision must be a full 40-digit Git revision",
        );
    }

    #[test]
    fn rejects_invalid_none_texture() {
        let mut valid_none = valid_case();
        replace_once(&mut valid_none, "name = \"texture\"", "name = \"$none\"");
        replace_once(&mut valid_none, "width = 1", "width = 0");
        replace_once(&mut valid_none, "height = 1", "height = 0");
        replace_once(
            &mut valid_none,
            &format!("sha256 = \"{}\"", hash('b')),
            &format!("sha256 = \"{}\"", sha256_hex(&[])),
        );
        replace_once(
            &mut valid_none,
            "recipe = \"solid-rgba8:ffffffff\"",
            "recipe = \"empty\"",
        );

        let mut wrong_width = valid_none.clone();
        replace_once(&mut wrong_width, "width = 0", "width = 1");
        let mut wrong_height = valid_none.clone();
        replace_once(&mut wrong_height, "height = 0", "height = 1");
        let mut wrong_hash = valid_none.clone();
        replace_once(
            &mut wrong_hash,
            &format!("sha256 = \"{}\"", sha256_hex(&[])),
            &format!("sha256 = \"{}\"", hash('b')),
        );
        let mut wrong_recipe = valid_none;
        replace_once(
            &mut wrong_recipe,
            "recipe = \"empty\"",
            "recipe = \"not-empty\"",
        );

        for case in [wrong_width, wrong_height, wrong_hash, wrong_recipe] {
            assert_rejected(
                &case,
                "$none texture must be 0x0 with the empty-byte hash and recipe 'empty'",
            );
        }
    }

    #[test]
    fn rejects_texture_dimension_overflow() {
        let mut case = valid_case();
        replace_once(&mut case, "width = 1", "width = 4294967295");
        replace_once(&mut case, "height = 1", "height = 4294967295");
        assert_rejected(&case, "texture dimensions overflow");
    }

    #[test]
    fn rejects_zero_texture_dimensions() {
        let mut case = valid_case();
        replace_once(&mut case, "width = 1", "width = 0");
        assert_rejected(&case, "texture 'texture' has zero dimensions");
    }

    #[test]
    fn rejects_texture_without_path_or_recipe() {
        let mut case = valid_case();
        replace_once(&mut case, "recipe = \"solid-rgba8:ffffffff\"\n", "");
        assert_rejected(
            &case,
            "texture 'texture' needs a path or deterministic recipe",
        );
    }

    #[test]
    fn rejects_unknown_generator_or_format_version() {
        let mut wrong_name = valid_case();
        replace_once(
            &mut wrong_name,
            "name = \"fast3d-fixtures\"",
            "name = \"other-generator\"",
        );
        let mut wrong_version = valid_case();
        replace_once(
            &mut wrong_version,
            "fixture_format_version = 1",
            "fixture_format_version = 2",
        );

        for case in [wrong_name, wrong_version] {
            assert_rejected(&case, "unknown fixture generator or format version");
        }
    }

    #[test]
    fn rejects_empty_n64_gbi_provenance_fields() {
        let mut empty_repository = valid_case();
        replace_once(
            &mut empty_repository,
            &format!("repository = \"{N64_GBI_REPOSITORY}\""),
            "repository = \"\"",
        );
        let mut empty_version = valid_case();
        replace_once(
            &mut empty_version,
            "crate_version = \"0.1.0\"",
            "crate_version = \"\"",
        );

        for case in [empty_repository, empty_version] {
            assert_rejected(&case, "n64-gbi provenance fields must not be empty");
        }
    }

    #[test]
    fn rejects_malformed_n64_gbi_revision() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("revision = \"{}\"", revision('3')),
            "revision = \"not-a-revision\"",
        );
        assert_rejected(
            &case,
            "provenance.n64_gbi.revision must be a full 40-digit Git revision",
        );
    }

    #[test]
    fn rejects_invalid_compiler_compatibility_provenance() {
        let valid = compiler_case();
        let mut compiler_none = valid.clone();
        replace_once(
            &mut compiler_none,
            "name = \"test-compiler\"",
            "name = \"none\"",
        );
        let mut empty_repository = valid.clone();
        replace_once(
            &mut empty_repository,
            "repository = \"https://example.invalid/compiler\"",
            "repository = \"\"",
        );
        let mut missing_repository = valid.clone();
        replace_once(
            &mut missing_repository,
            "repository = \"https://example.invalid/compiler\"\n",
            "",
        );
        let mut missing_revision = valid.clone();
        replace_once(
            &mut missing_revision,
            &format!("revision = \"{}\"\n", revision('4')),
            "",
        );
        let mut wrong_evidence = valid;
        replace_once(
            &mut wrong_evidence,
            "evidence = \"compiler-origin; not independent protocol evidence\"",
            "evidence = \"unsubstantiated\"",
        );

        for case in [
            compiler_none,
            empty_repository,
            missing_repository,
            missing_revision,
            wrong_evidence,
        ] {
            assert_rejected(&case, "invalid compiler-compatibility provenance");
        }
    }

    #[test]
    fn rejects_malformed_compiler_revision() {
        let mut case = compiler_case();
        replace_once(
            &mut case,
            &format!("revision = \"{}\"", revision('4')),
            &format!("revision = \"{}\"", repeated('4', 39)),
        );
        assert_rejected(
            &case,
            "provenance.compiler.revision must be a full 40-digit Git revision",
        );
    }

    #[test]
    fn rejects_compiler_on_literal_provenance() {
        let mut named_compiler = valid_case();
        replace_once(
            &mut named_compiler,
            "[provenance.compiler]\nname = \"none\"",
            "[provenance.compiler]\nname = \"test-compiler\"",
        );
        let mut compiler_repository = valid_case();
        replace_once(
            &mut compiler_repository,
            "[provenance.compiler]\nname = \"none\"",
            "[provenance.compiler]\nname = \"none\"\nrepository = \"https://example.invalid/compiler\"",
        );
        let mut compiler_revision = valid_case();
        replace_once(
            &mut compiler_revision,
            "[provenance.compiler]\nname = \"none\"",
            &format!(
                "[provenance.compiler]\nname = \"none\"\nrevision = \"{}\"",
                revision('4')
            ),
        );

        for case in [named_compiler, compiler_repository, compiler_revision] {
            assert_rejected(&case, "literal provenance must name no compiler");
        }
    }

    #[test]
    fn rejects_unknown_capture_kind() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            "capture_kind = \"fast3d-literal\"",
            "capture_kind = \"unknown\"",
        );
        assert_rejected(&case, "unknown provenance.capture_kind 'unknown'");
    }

    #[test]
    fn rejects_rdram_hash_mismatch() {
        let mut case = valid_case();
        case.rdram[0] ^= 0xff;
        let manifest = parsed_manifest(&case);
        let actual = sha256_hex(&case.rdram);
        assert_rejected(
            &case,
            format!(
                "rdram_sha256 declares {}, actual is {actual}",
                manifest.rdram_sha256
            ),
        );
    }

    #[test]
    fn rejects_declared_artifact_hash_mismatch() {
        let mut case = valid_case();
        let declared = parsed_manifest(&case).artifact_sha256;
        replace_once(
            &mut case,
            &format!("artifact_sha256 = \"{declared}\""),
            &format!("artifact_sha256 = \"{}\"", hash('0')),
        );
        assert_artifact_mismatch(&case);
    }

    #[test]
    fn rejects_source_hash_identity_mismatch() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("sha256 = \"{}\"", hash('a')),
            &format!("sha256 = \"{}\"", hash('c')),
        );
        assert_artifact_mismatch(&case);
    }

    #[test]
    fn rejects_time_bits_identity_mismatch() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            "time_f32_bits = \"00000000\"",
            "time_f32_bits = \"3F800000\"",
        );
        assert_artifact_mismatch(&case);
    }

    #[test]
    fn rejects_texture_hash_identity_mismatch() {
        let mut case = valid_case();
        replace_once(
            &mut case,
            &format!("sha256 = \"{}\"", hash('b')),
            &format!("sha256 = \"{}\"", hash('c')),
        );
        assert_artifact_mismatch(&case);
    }

    #[test]
    fn rejects_texture_dimension_identity_mismatch() {
        let mut changed_width = valid_case();
        replace_once(&mut changed_width, "width = 1", "width = 2");
        let mut changed_height = valid_case();
        replace_once(&mut changed_height, "height = 1", "height = 2");

        for case in [changed_width, changed_height] {
            assert_artifact_mismatch(&case);
        }
    }

    #[test]
    fn rejects_invalid_dump_newline_forms() {
        let mut carriage_return = valid_case();
        carriage_return.dump = "validator\r\ndump\n".into();
        let mut missing_newline = valid_case();
        assert_eq!(missing_newline.dump.pop(), Some('\n'));
        let mut double_newline = valid_case();
        double_newline.dump.push('\n');

        for case in [carriage_return, missing_newline, double_newline] {
            assert_rejected(
                &case,
                "image.dump must be UTF-8/LF with exactly one terminal newline",
            );
        }
    }

    #[test]
    fn rejects_dump_hash_mismatch() {
        let mut case = valid_case();
        case.dump = "changed dump\n".into();
        let manifest = parsed_manifest(&case);
        let actual = sha256_hex(case.dump.as_bytes());
        assert_rejected(
            &case,
            format!(
                "dump_sha256 declares {}, actual is {actual}",
                manifest.dump_sha256
            ),
        );
    }

    #[test]
    fn artifact_identity_rejects_malformed_source_hash() {
        let mut input = valid_input();
        input.source.sha256 = repeated('a', 63);
        assert_eq!(
            artifact_sha256(
                8,
                ManifestMicrocode::F3dex2,
                ManifestDataFormat::Fixed,
                &input,
                &[0; 16],
            ),
            Err("sha256 must be 64 lowercase hexadecimal digits".into())
        );
    }

    #[test]
    fn artifact_identity_rejects_malformed_time_bits() {
        let mut input = valid_input();
        input.time_f32_bits = "0000000g".into();
        assert_eq!(
            artifact_sha256(
                8,
                ManifestMicrocode::F3dex2,
                ManifestDataFormat::Fixed,
                &input,
                &[0; 16],
            ),
            Err(
                "input.time_f32_bits must be a string containing exactly eight hexadecimal digits"
                    .into()
            )
        );
    }

    #[test]
    fn artifact_identity_rejects_malformed_texture_hash() {
        let mut input = valid_input();
        input.textures[0].sha256 = repeated('b', 65);
        assert_eq!(
            artifact_sha256(
                8,
                ManifestMicrocode::F3dex2,
                ManifestDataFormat::Fixed,
                &input,
                &[0; 16],
            ),
            Err("sha256 must be 64 lowercase hexadecimal digits".into())
        );
    }

    #[test]
    fn decode_hash_rejects_bad_length_or_alphabet() {
        assert_eq!(
            decode_hash(&hash('f')).expect("the maximum byte hash must decode"),
            [u8::MAX; 32]
        );
        for invalid_hash in [
            repeated('a', 63),
            repeated('a', 65),
            repeated('A', 64),
            format!("{}g", repeated('a', 63)),
        ] {
            assert_eq!(
                decode_hash(&invalid_hash),
                Err("sha256 must be 64 lowercase hexadecimal digits".into())
            );
        }
    }
}
