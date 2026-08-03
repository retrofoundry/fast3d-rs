//! Package-local validation for embedded test fixtures.
//!
//! The repository tool remains responsible for capture, import, canonical dump rendering, and
//! registry generation. The generated registry locks each canonical manifest by SHA-256, so this
//! module can validate packaged fixtures without depending on an unpublishable workspace crate.

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
