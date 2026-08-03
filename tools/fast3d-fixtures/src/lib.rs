//! Compiler-independent support for fast3d's checked display-list fixtures.
//!
//! The binary image is the execution authority. The adjacent manifest binds the exact input
//! tuple to that image, while the dump is a deterministic, review-oriented projection generated
//! only from those two files.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::fs;
use std::path::Component;
use std::path::{Path, PathBuf};

pub const FIXTURE_SCHEMA: u32 = 1;
pub const INDEX_SCHEMA: u32 = 1;
pub const CAPTURE_SCHEMA: u32 = 1;
pub const RDRAM_FILE: &str = "image.rdram";
pub const DUMP_FILE: &str = "image.dump";
pub const MANIFEST_FILE: &str = "fixture.toml";
pub const CAPTURE_FILE: &str = "capture.toml";
pub const DEFAULT_REGISTRY_FILE: &str = "fast3d/src/tests/fixtures/generated.rs";
pub const REPOSITORY: &str = "https://github.com/retrofoundry/fast3d-rs";

const VERIFY_COMMAND: &str = "cargo run -p fast3d-fixtures -- verify";
const FIXTURE_BUDGET_BYTES: u64 = 4 * 1024 * 1024;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn fixture(id: &str, message: impl fmt::Display) -> Self {
        Self(format!(
            "fixture '{id}' is missing or stale: {message}; run `{VERIFY_COMMAND}`"
        ))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(value: toml::ser::Error) -> Self {
        Self(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Microcode {
    F3dex2,
    F3d,
}

impl Microcode {
    fn identity_byte(self) -> u8 {
        match self {
            Self::F3dex2 => 0,
            Self::F3d => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataFormat {
    Fixed,
    Float,
}

impl DataFormat {
    fn identity_byte(self) -> u8 {
        match self {
            Self::Fixed => 0,
            Self::Float => 1,
        }
    }

    fn vertex_stride(self) -> usize {
        match self {
            Self::Fixed => 16,
            Self::Float => 24,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// A repository-relative source that must exist and match its recorded hash.
    File,
    /// Provenance for a source owned elsewhere; repository, revision, path, and hash stay recorded.
    External,
    /// A deterministic literal recipe implemented by the fast3d-local fixture tool.
    LiteralRecipe,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub kind: SourceKind,
    pub logical_id: String,
    pub repository: String,
    pub revision: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextureInput {
    pub name: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
    pub repository: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureInput {
    pub time_f32_bits: String,
    pub source: SourceInput,
    pub textures: Vec<TextureInput>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerProvenance {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct N64GbiProvenance {
    pub repository: String,
    pub revision: String,
    pub crate_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorProvenance {
    pub name: String,
    pub fixture_format_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub capture_kind: String,
    pub evidence: String,
    pub compiler: CompilerProvenance,
    pub n64_gbi: N64GbiProvenance,
    pub generator: GeneratorProvenance,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub rdram_file: String,
    pub dump_file: String,
    pub rdram_len: u64,
    pub entry_addr: String,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub rdram_sha256: String,
    pub dump_sha256: String,
    pub artifact_sha256: String,
    pub input: FixtureInput,
    pub provenance: Provenance,
}

impl Manifest {
    pub fn entry_addr(&self) -> Result<u32> {
        parse_prefixed_u32(&self.entry_addr, "entry_addr")
    }

    pub fn time_f32_bits(&self) -> Result<u32> {
        parse_fixed_u32(&self.input.time_f32_bits, "input.time_f32_bits")
    }
}

#[derive(Clone, Debug)]
pub struct FixtureBuild {
    pub id: String,
    pub rdram: Vec<u8>,
    pub entry_addr: u32,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub input: FixtureInput,
    pub provenance: Provenance,
}

#[derive(Clone, Debug)]
pub struct FixtureFiles {
    pub manifest: Manifest,
    pub manifest_text: String,
    pub rdram: Vec<u8>,
    pub dump: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedFixture {
    pub entry_addr: u32,
    pub microcode: Microcode,
    pub data_format: DataFormat,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureIndex {
    pub schema: u32,
    pub registry_file: String,
    #[serde(rename = "fixture")]
    pub fixtures: Vec<IndexEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub id: String,
    pub coverage: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureEnvelope {
    pub schema: u32,
    pub id: String,
    pub rdram_file: String,
    pub source_file: String,
    pub rdram_len: u64,
    pub rdram_sha256: String,
    pub entry_addr: String,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub time_f32_bits: String,
    pub source: SourceInput,
    pub textures: Vec<CaptureTexture>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTexture {
    #[serde(flatten)]
    pub input: TextureInput,
    pub input_file: String,
}

#[derive(Clone, Debug)]
pub struct Capture {
    pub id: String,
    pub rdram: Vec<u8>,
    pub entry_addr: u32,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub time_f32_bits: u32,
    pub source: SourceInput,
    pub source_bytes: Vec<u8>,
    pub textures: Vec<(TextureInput, Vec<u8>)>,
    pub provenance: Provenance,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub fn compiler_provenance(revision: &str) -> Provenance {
    Provenance {
        capture_kind: "compiler-compatibility".into(),
        evidence: "compiler-origin; not independent protocol evidence".into(),
        compiler: CompilerProvenance {
            name: "fast3d::asm".into(),
            repository: Some(REPOSITORY.into()),
            revision: Some(revision.into()),
        },
        n64_gbi: N64GbiProvenance {
            repository: REPOSITORY.into(),
            revision: revision.into(),
            crate_version: "0.1.0".into(),
        },
        generator: GeneratorProvenance {
            name: "fast3d-fixtures".into(),
            fixture_format_version: FIXTURE_SCHEMA,
        },
    }
}

pub fn literal_provenance(revision: &str) -> Provenance {
    Provenance {
        capture_kind: "fast3d-literal".into(),
        evidence: "literal words and packed records are the review authority".into(),
        compiler: CompilerProvenance {
            name: "none".into(),
            repository: None,
            revision: None,
        },
        n64_gbi: N64GbiProvenance {
            repository: REPOSITORY.into(),
            revision: revision.into(),
            crate_version: "0.1.0".into(),
        },
        generator: GeneratorProvenance {
            name: "fast3d-fixtures".into(),
            fixture_format_version: FIXTURE_SCHEMA,
        },
    }
}

pub fn build_fixture(mut build: FixtureBuild) -> Result<FixtureFiles> {
    validate_id(&build.id)?;
    if !build.entry_addr.is_multiple_of(8) {
        return Err(Error::fixture(
            &build.id,
            format_args!(
                "entry_addr 0x{:08X} is not eight-byte aligned",
                build.entry_addr
            ),
        ));
    }
    if build.entry_addr as usize >= build.rdram.len() {
        return Err(Error::fixture(
            &build.id,
            format_args!(
                "entry_addr 0x{:08X} is outside {} bytes of RDRAM",
                build.entry_addr,
                build.rdram.len()
            ),
        ));
    }
    if !(build.rdram.len() - build.entry_addr as usize).is_multiple_of(8) {
        return Err(Error::fixture(
            &build.id,
            "the command arena length is not a multiple of eight bytes",
        ));
    }
    validate_input(&build.id, &mut build.input)?;
    validate_provenance(&build.id, &build.provenance)?;

    let artifact_sha256 = artifact_sha256(
        build.entry_addr,
        build.microcode,
        build.data_format,
        &build.input,
        &build.rdram,
    )?;
    let rdram_sha256 = sha256_hex(&build.rdram);
    let mut manifest = Manifest {
        schema: FIXTURE_SCHEMA,
        id: build.id,
        rdram_file: RDRAM_FILE.into(),
        dump_file: DUMP_FILE.into(),
        rdram_len: build.rdram.len() as u64,
        entry_addr: format!("0x{:08X}", build.entry_addr),
        microcode: build.microcode,
        data_format: build.data_format,
        rdram_sha256,
        dump_sha256: "0".repeat(64),
        artifact_sha256,
        input: build.input,
        provenance: build.provenance,
    };
    let dump = render_dump(&manifest, &build.rdram)?;
    manifest.dump_sha256 = sha256_hex(dump.as_bytes());
    let manifest_text = render_manifest(&manifest);
    Ok(FixtureFiles {
        manifest,
        manifest_text,
        rdram: build.rdram,
        dump,
    })
}

pub fn parse_manifest(text: &str) -> Result<Manifest> {
    Ok(toml::from_str(text)?)
}

pub fn validate_embedded(
    expected_id: &str,
    manifest_text: &str,
    rdram: &[u8],
    dump: &str,
) -> Result<ValidatedFixture> {
    let manifest = parse_manifest(manifest_text).map_err(|error| {
        Error::fixture(expected_id, format_args!("invalid fixture.toml: {error}"))
    })?;
    validate_fixture(expected_id, &manifest, manifest_text, rdram, dump)
}

pub fn validate_fixture(
    expected_id: &str,
    manifest: &Manifest,
    manifest_text: &str,
    rdram: &[u8],
    dump: &str,
) -> Result<ValidatedFixture> {
    validate_id(expected_id).map_err(|error| Error::fixture(expected_id, error))?;
    if manifest.schema != FIXTURE_SCHEMA {
        return Err(Error::fixture(
            expected_id,
            format_args!(
                "unsupported schema {}, expected {FIXTURE_SCHEMA}",
                manifest.schema
            ),
        ));
    }
    if manifest.id != expected_id {
        return Err(Error::fixture(
            expected_id,
            format_args!("manifest id is '{}'", manifest.id),
        ));
    }
    if manifest.rdram_file != RDRAM_FILE || manifest.dump_file != DUMP_FILE {
        return Err(Error::fixture(
            expected_id,
            "rdram_file and dump_file must be exactly image.rdram and image.dump",
        ));
    }
    if manifest.rdram_len != rdram.len() as u64 {
        return Err(Error::fixture(
            expected_id,
            format_args!(
                "rdram_len declares {}, actual length is {}",
                manifest.rdram_len,
                rdram.len()
            ),
        ));
    }
    let entry_addr = manifest
        .entry_addr()
        .map_err(|error| Error::fixture(expected_id, error))?;
    if !entry_addr.is_multiple_of(8) || entry_addr as usize >= rdram.len() {
        return Err(Error::fixture(
            expected_id,
            format_args!("entry_addr 0x{entry_addr:08X} must be aligned and within RDRAM"),
        ));
    }
    if !(rdram.len() - entry_addr as usize).is_multiple_of(8) {
        return Err(Error::fixture(
            expected_id,
            "the command arena length is not a multiple of eight bytes",
        ));
    }
    manifest
        .time_f32_bits()
        .map_err(|error| Error::fixture(expected_id, error))?;
    validate_hash(&manifest.rdram_sha256, "rdram_sha256")
        .map_err(|error| Error::fixture(expected_id, error))?;
    validate_hash(&manifest.dump_sha256, "dump_sha256")
        .map_err(|error| Error::fixture(expected_id, error))?;
    validate_hash(&manifest.artifact_sha256, "artifact_sha256")
        .map_err(|error| Error::fixture(expected_id, error))?;
    let mut input = manifest.input.clone();
    validate_input(expected_id, &mut input)?;
    if input.textures != manifest.input.textures {
        return Err(Error::fixture(
            expected_id,
            "input.textures must be sorted by logical name",
        ));
    }
    validate_provenance(expected_id, &manifest.provenance)?;

    let actual_rdram_sha = sha256_hex(rdram);
    if manifest.rdram_sha256 != actual_rdram_sha {
        return Err(Error::fixture(
            expected_id,
            format_args!(
                "rdram_sha256 declares {}, actual is {actual_rdram_sha}",
                manifest.rdram_sha256
            ),
        ));
    }
    let actual_artifact_sha = artifact_sha256(
        entry_addr,
        manifest.microcode,
        manifest.data_format,
        &manifest.input,
        rdram,
    )?;
    if manifest.artifact_sha256 != actual_artifact_sha {
        return Err(Error::fixture(
            expected_id,
            format_args!(
                "artifact_sha256 declares {}, actual is {actual_artifact_sha}",
                manifest.artifact_sha256
            ),
        ));
    }
    if dump.contains('\r') || !dump.ends_with('\n') || dump.ends_with("\n\n") {
        return Err(Error::fixture(
            expected_id,
            "image.dump must be UTF-8/LF with exactly one terminal newline",
        ));
    }
    let actual_dump_sha = sha256_hex(dump.as_bytes());
    if manifest.dump_sha256 != actual_dump_sha {
        return Err(Error::fixture(
            expected_id,
            format_args!(
                "dump_sha256 declares {}, actual is {actual_dump_sha}",
                manifest.dump_sha256
            ),
        ));
    }
    let expected_dump = render_dump(manifest, rdram)?;
    if dump != expected_dump {
        let difference = first_differing_line(dump, &expected_dump);
        return Err(Error::fixture(
            expected_id,
            format_args!("image.dump is noncanonical at {difference}"),
        ));
    }
    let canonical_manifest = render_manifest(manifest);
    if manifest_text != canonical_manifest {
        let difference = first_differing_line(manifest_text, &canonical_manifest);
        return Err(Error::fixture(
            expected_id,
            format_args!("fixture.toml is noncanonical at {difference}"),
        ));
    }

    Ok(ValidatedFixture {
        entry_addr,
        microcode: manifest.microcode,
        data_format: manifest.data_format,
    })
}

fn validate_input(id: &str, input: &mut FixtureInput) -> Result<()> {
    parse_fixed_u32(&input.time_f32_bits, "input.time_f32_bits")
        .map_err(|error| Error::fixture(id, error))?;
    validate_hash(&input.source.sha256, "input.source.sha256")
        .map_err(|error| Error::fixture(id, error))?;
    if input.source.logical_id.is_empty()
        || input.source.repository.is_empty()
        || input.source.path.is_empty()
    {
        return Err(Error::fixture(id, "input.source fields must not be empty"));
    }
    validate_relative_path(&input.source.path, "input.source.path")
        .map_err(|error| Error::fixture(id, error))?;
    validate_full_revision(&input.source.revision, "input.source.revision")
        .map_err(|error| Error::fixture(id, error))?;
    input
        .textures
        .sort_by(|left, right| left.name.cmp(&right.name));
    if input.textures.is_empty() {
        return Err(Error::fixture(
            id,
            "input.textures must contain an explicit $none entry when no texture is supplied",
        ));
    }
    let mut names = BTreeSet::new();
    for texture in &input.textures {
        if !names.insert(texture.name.as_str()) {
            return Err(Error::fixture(
                id,
                format_args!("duplicate texture input '{}'", texture.name),
            ));
        }
        if texture.format != "rgba8" {
            return Err(Error::fixture(
                id,
                format_args!("texture '{}' format must be rgba8", texture.name),
            ));
        }
        if let Some(path) = &texture.path {
            validate_relative_path(path, "input.textures[].path")
                .map_err(|error| Error::fixture(id, error))?;
        }
        validate_hash(&texture.sha256, "input.textures[].sha256")
            .map_err(|error| Error::fixture(id, error))?;
        validate_full_revision(&texture.revision, "input.textures[].revision")
            .map_err(|error| Error::fixture(id, error))?;
        if texture.name == "$none" {
            if texture.width != 0
                || texture.height != 0
                || texture.sha256 != sha256_hex(&[])
                || texture.recipe.as_deref() != Some("empty")
            {
                return Err(Error::fixture(
                    id,
                    "$none texture must be 0x0 with the empty-byte hash and recipe 'empty'",
                ));
            }
        } else {
            let expected_len = u64::from(texture.width)
                .checked_mul(u64::from(texture.height))
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| Error::fixture(id, "texture dimensions overflow"))?;
            if expected_len == 0 {
                return Err(Error::fixture(
                    id,
                    format_args!("texture '{}' has zero dimensions", texture.name),
                ));
            }
            if texture.path.is_none() && texture.recipe.is_none() {
                return Err(Error::fixture(
                    id,
                    format_args!(
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
        return Err(Error::fixture(
            id,
            "unknown fixture generator or format version",
        ));
    }
    validate_full_revision(&provenance.n64_gbi.revision, "provenance.n64_gbi.revision")
        .map_err(|error| Error::fixture(id, error))?;
    match provenance.capture_kind.as_str() {
        "compiler-compatibility" => {
            if provenance.compiler.name == "none"
                || provenance.compiler.repository.is_none()
                || provenance.compiler.revision.is_none()
                || provenance.evidence != "compiler-origin; not independent protocol evidence"
            {
                return Err(Error::fixture(
                    id,
                    "invalid compiler-compatibility provenance",
                ));
            }
            validate_full_revision(
                provenance.compiler.revision.as_deref().unwrap_or_default(),
                "provenance.compiler.revision",
            )
            .map_err(|error| Error::fixture(id, error))?;
        }
        "fast3d-literal" => {
            if provenance.compiler.name != "none"
                || provenance.compiler.repository.is_some()
                || provenance.compiler.revision.is_some()
            {
                return Err(Error::fixture(
                    id,
                    "literal provenance must name no compiler",
                ));
            }
        }
        other => {
            return Err(Error::fixture(
                id,
                format_args!("unknown provenance.capture_kind '{other}'"),
            ));
        }
    }
    Ok(())
}

fn artifact_sha256(
    entry_addr: u32,
    microcode: Microcode,
    data_format: DataFormat,
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

fn parse_prefixed_u32(value: &str, field: &str) -> Result<u32> {
    let Some(digits) = value.strip_prefix("0x") else {
        return Err(Error::new(format!(
            "{field} must be a string formatted as 0x followed by eight uppercase hexadecimal digits"
        )));
    };
    if digits.len() != 8
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
    {
        return Err(Error::new(format!(
            "{field} must be a string formatted as 0x followed by eight uppercase hexadecimal digits"
        )));
    }
    u32::from_str_radix(digits, 16).map_err(|error| Error::new(format!("invalid {field}: {error}")))
}

fn parse_fixed_u32(value: &str, field: &str) -> Result<u32> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(format!(
            "{field} must be a string containing exactly eight hexadecimal digits"
        )));
    }
    u32::from_str_radix(value, 16).map_err(|error| Error::new(format!("invalid {field}: {error}")))
}

fn validate_full_revision(value: &str, field: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(format!(
            "{field} must be a full 40-digit Git revision"
        )));
    }
    Ok(())
}

fn validate_hash(value: &str, field: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::new(format!(
            "{field} must be 64 lowercase hexadecimal digits"
        )));
    }
    Ok(())
}

fn decode_hash(value: &str) -> Result<[u8; 32]> {
    validate_hash(value, "sha256")?;
    let mut bytes = [0u8; 32];
    for (index, output) in bytes.iter_mut().enumerate() {
        *output = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|error| Error::new(format!("invalid SHA-256: {error}")))?;
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
        return Err(Error::new(format!("invalid fixture id '{id}'")));
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
        return Err(Error::new(format!(
            "{field} must be a normalized repository-relative path"
        )));
    }
    Ok(())
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

pub fn render_manifest(manifest: &Manifest) -> String {
    let mut output = String::new();
    writeln!(output, "schema = {}", manifest.schema).unwrap();
    writeln!(output, "id = {}", toml_string(&manifest.id)).unwrap();
    writeln!(output, "rdram_file = {}", toml_string(&manifest.rdram_file)).unwrap();
    writeln!(output, "dump_file = {}", toml_string(&manifest.dump_file)).unwrap();
    writeln!(output, "rdram_len = {}", manifest.rdram_len).unwrap();
    writeln!(output, "entry_addr = {}", toml_string(&manifest.entry_addr)).unwrap();
    writeln!(
        output,
        "microcode = {}",
        toml_string(microcode_name(manifest.microcode))
    )
    .unwrap();
    writeln!(
        output,
        "data_format = {}",
        toml_string(data_format_name(manifest.data_format))
    )
    .unwrap();
    writeln!(
        output,
        "rdram_sha256 = {}",
        toml_string(&manifest.rdram_sha256)
    )
    .unwrap();
    writeln!(
        output,
        "dump_sha256 = {}",
        toml_string(&manifest.dump_sha256)
    )
    .unwrap();
    writeln!(
        output,
        "artifact_sha256 = {}",
        toml_string(&manifest.artifact_sha256)
    )
    .unwrap();

    writeln!(output, "\n[input]").unwrap();
    writeln!(
        output,
        "time_f32_bits = {}",
        toml_string(&manifest.input.time_f32_bits)
    )
    .unwrap();
    writeln!(output, "\n[input.source]").unwrap();
    render_source(&mut output, &manifest.input.source);
    for texture in &manifest.input.textures {
        writeln!(output, "\n[[input.textures]]").unwrap();
        writeln!(output, "name = {}", toml_string(&texture.name)).unwrap();
        writeln!(output, "format = {}", toml_string(&texture.format)).unwrap();
        writeln!(output, "width = {}", texture.width).unwrap();
        writeln!(output, "height = {}", texture.height).unwrap();
        writeln!(output, "sha256 = {}", toml_string(&texture.sha256)).unwrap();
        writeln!(output, "repository = {}", toml_string(&texture.repository)).unwrap();
        writeln!(output, "revision = {}", toml_string(&texture.revision)).unwrap();
        if let Some(path) = &texture.path {
            writeln!(output, "path = {}", toml_string(path)).unwrap();
        }
        if let Some(recipe) = &texture.recipe {
            writeln!(output, "recipe = {}", toml_string(recipe)).unwrap();
        }
    }

    writeln!(output, "\n[provenance]").unwrap();
    writeln!(
        output,
        "capture_kind = {}",
        toml_string(&manifest.provenance.capture_kind)
    )
    .unwrap();
    writeln!(
        output,
        "evidence = {}",
        toml_string(&manifest.provenance.evidence)
    )
    .unwrap();
    writeln!(output, "\n[provenance.compiler]").unwrap();
    writeln!(
        output,
        "name = {}",
        toml_string(&manifest.provenance.compiler.name)
    )
    .unwrap();
    if let Some(repository) = &manifest.provenance.compiler.repository {
        writeln!(output, "repository = {}", toml_string(repository)).unwrap();
    }
    if let Some(revision) = &manifest.provenance.compiler.revision {
        writeln!(output, "revision = {}", toml_string(revision)).unwrap();
    }
    writeln!(output, "\n[provenance.n64_gbi]").unwrap();
    writeln!(
        output,
        "repository = {}",
        toml_string(&manifest.provenance.n64_gbi.repository)
    )
    .unwrap();
    writeln!(
        output,
        "revision = {}",
        toml_string(&manifest.provenance.n64_gbi.revision)
    )
    .unwrap();
    writeln!(
        output,
        "crate_version = {}",
        toml_string(&manifest.provenance.n64_gbi.crate_version)
    )
    .unwrap();
    writeln!(output, "\n[provenance.generator]").unwrap();
    writeln!(
        output,
        "name = {}",
        toml_string(&manifest.provenance.generator.name)
    )
    .unwrap();
    writeln!(
        output,
        "fixture_format_version = {}",
        manifest.provenance.generator.fixture_format_version
    )
    .unwrap();
    output
}

fn render_source(output: &mut String, source: &SourceInput) {
    let kind = match source.kind {
        SourceKind::File => "file",
        SourceKind::External => "external",
        SourceKind::LiteralRecipe => "literal-recipe",
    };
    writeln!(output, "kind = {}", toml_string(kind)).unwrap();
    writeln!(output, "logical_id = {}", toml_string(&source.logical_id)).unwrap();
    writeln!(output, "repository = {}", toml_string(&source.repository)).unwrap();
    writeln!(output, "revision = {}", toml_string(&source.revision)).unwrap();
    writeln!(output, "path = {}", toml_string(&source.path)).unwrap();
    writeln!(output, "sha256 = {}", toml_string(&source.sha256)).unwrap();
}

fn microcode_name(microcode: Microcode) -> &'static str {
    match microcode {
        Microcode::F3dex2 => "f3dex2",
        Microcode::F3d => "f3d",
    }
}

fn data_format_name(data_format: DataFormat) -> &'static str {
    match data_format {
        DataFormat::Fixed => "fixed",
        DataFormat::Float => "float",
    }
}

fn first_differing_line(actual: &str, expected: &str) -> String {
    let actual_lines = actual.lines().collect::<Vec<_>>();
    let expected_lines = expected.lines().collect::<Vec<_>>();
    let count = actual_lines.len().max(expected_lines.len());
    for index in 0..count {
        if actual_lines.get(index) != expected_lines.get(index) {
            return format!(
                "line {} (actual {:?}, expected {:?})",
                index + 1,
                actual_lines.get(index).copied().unwrap_or("<missing>"),
                expected_lines.get(index).copied().unwrap_or("<missing>")
            );
        }
    }
    "terminal newline".into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VertexEncoding {
    Color,
    Normal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RegionKind {
    Viewport,
    Vertices {
        count: usize,
        encoding: VertexEncoding,
    },
    Matrix,
    Light,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DataRegion {
    start: usize,
    end: usize,
    kind: RegionKind,
}

/// Produce the canonical review dump from only the manifest and canonical RDRAM image.
pub fn render_dump(manifest: &Manifest, rdram: &[u8]) -> Result<String> {
    let entry = manifest.entry_addr()? as usize;
    if entry > rdram.len() || !rdram.len().saturating_sub(entry).is_multiple_of(8) {
        return Err(Error::fixture(
            &manifest.id,
            "cannot dump an invalid command arena",
        ));
    }

    let regions = infer_regions(rdram, entry, manifest.microcode, manifest.data_format);
    let mut output = String::new();
    writeln!(output, "# fast3d-rdram-dump v1").unwrap();
    writeln!(output, "# id: {}", manifest.id).unwrap();
    writeln!(output, "# artifact-sha256: {}", manifest.artifact_sha256).unwrap();
    writeln!(output, "# entry: 0x{entry:08X}").unwrap();
    writeln!(output, "DATA 0x00000000..0x{entry:08X}").unwrap();
    render_data(
        &mut output,
        &rdram[..entry],
        &regions,
        manifest.data_format,
        &manifest.input.textures,
    );
    writeln!(output, "COMMANDS 0x{entry:08X}..0x{:08X}", rdram.len()).unwrap();
    for (word_index, bytes) in rdram[entry..].chunks_exact(8).enumerate() {
        let address = entry + word_index * 8;
        let w0 = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        let w1 = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
        writeln!(
            output,
            "{address:08X}: {w0:08X} {w1:08X}  {}",
            annotate_command(w0, w1, manifest.microcode)
        )
        .unwrap();
    }
    Ok(output)
}

fn infer_regions(
    rdram: &[u8],
    entry: usize,
    microcode: Microcode,
    data_format: DataFormat,
) -> Vec<DataRegion> {
    let mut candidates = Vec::new();
    let mut geometry_mode = 0u32;
    for bytes in rdram[entry..].chunks_exact(8) {
        let w0 = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
        let w1 = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
        let opcode = (w0 >> 24) as u8;
        match microcode {
            Microcode::F3dex2 => match opcode {
                0x01 => {
                    let count = ((w0 >> 12) & 0xff) as usize;
                    let start = masked_address(w1);
                    let encoding = if geometry_mode & 0x0002_0000 == 0 {
                        VertexEncoding::Color
                    } else {
                        VertexEncoding::Normal
                    };
                    push_region(
                        &mut candidates,
                        start,
                        count.saturating_mul(data_format.vertex_stride()),
                        entry,
                        RegionKind::Vertices { count, encoding },
                    );
                }
                0xD9 => {
                    geometry_mode = (geometry_mode & (w0 & 0x00ff_ffff)) | w1;
                }
                0xDA => push_region(
                    &mut candidates,
                    masked_address(w1),
                    64,
                    entry,
                    RegionKind::Matrix,
                ),
                0xDC => {
                    let length = ((((w0 >> 19) & 0x1f) + 1) * 8) as usize;
                    let kind = match (w0 & 0xff) as u8 {
                        0x08 => Some(RegionKind::Viewport),
                        0x0A => Some(RegionKind::Light),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        push_region(&mut candidates, masked_address(w1), length, entry, kind);
                    }
                }
                _ => {}
            },
            Microcode::F3d => match opcode {
                0x01 => push_region(
                    &mut candidates,
                    masked_address(w1),
                    64,
                    entry,
                    RegionKind::Matrix,
                ),
                0x03 if ((w0 >> 16) & 0xff) == 0x80 => push_region(
                    &mut candidates,
                    masked_address(w1),
                    16,
                    entry,
                    RegionKind::Viewport,
                ),
                0x04 => {
                    let count = (((w0 >> 20) & 0x0f) + 1) as usize;
                    push_region(
                        &mut candidates,
                        masked_address(w1),
                        count.saturating_mul(data_format.vertex_stride()),
                        entry,
                        RegionKind::Vertices {
                            count,
                            encoding: VertexEncoding::Color,
                        },
                    );
                }
                _ => {}
            },
        }
    }
    candidates.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.end.cmp(&right.end))
    });
    candidates.dedup();

    let mut regions: Vec<DataRegion> = Vec::new();
    for candidate in candidates {
        if let Some(previous) = regions.last() {
            if candidate.start < previous.end {
                continue;
            }
        }
        regions.push(candidate);
    }
    regions
}

fn masked_address(address: u32) -> usize {
    (address & 0x00ff_fff8) as usize
}

fn push_region(
    regions: &mut Vec<DataRegion>,
    start: usize,
    length: usize,
    entry: usize,
    kind: RegionKind,
) {
    if length == 0 {
        return;
    }
    if let Some(end) = start.checked_add(length) {
        if end <= entry {
            regions.push(DataRegion { start, end, kind });
        }
    }
}

fn render_data(
    output: &mut String,
    data: &[u8],
    regions: &[DataRegion],
    data_format: DataFormat,
    textures: &[TextureInput],
) {
    let mut cursor = 0usize;
    let texture_gap = select_texture_gap(data.len(), regions, textures);
    for region in regions {
        if cursor < region.start {
            render_unclassified(
                output,
                data,
                cursor,
                region.start,
                texture_gap == Some((cursor, region.start)),
                textures,
            );
        }
        match &region.kind {
            RegionKind::Viewport => render_viewport(output, data, region),
            RegionKind::Vertices { count, encoding } => {
                render_vertices(output, data, region, *count, *encoding, data_format)
            }
            RegionKind::Matrix => render_matrix(output, data, region, data_format),
            RegionKind::Light => render_light(output, data, region),
        }
        cursor = region.end;
    }
    if cursor < data.len() {
        render_unclassified(
            output,
            data,
            cursor,
            data.len(),
            texture_gap == Some((cursor, data.len())),
            textures,
        );
    }
}

fn select_texture_gap(
    data_len: usize,
    regions: &[DataRegion],
    textures: &[TextureInput],
) -> Option<(usize, usize)> {
    let encoded_len = textures
        .iter()
        .filter(|texture| texture.name != "$none")
        .try_fold(0usize, |total, texture| {
            let pixels = usize::try_from(texture.width)
                .ok()?
                .checked_mul(usize::try_from(texture.height).ok()?)?;
            total.checked_add(pixels.checked_mul(2)?)
        })?;
    if encoded_len == 0 {
        return None;
    }

    let mut candidates = Vec::new();
    let mut cursor = 0usize;
    for region in regions {
        if cursor < region.start {
            candidates.push((cursor, region.start));
        }
        cursor = region.end;
    }
    if cursor < data_len {
        candidates.push((cursor, data_len));
    }
    candidates.retain(|(start, end)| encoded_len <= end - start && end - start - encoded_len < 8);

    let exact = candidates
        .iter()
        .copied()
        .filter(|(start, end)| end - start == encoded_len)
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return exact.first().copied();
    }
    if candidates.len() == 1 {
        return candidates.first().copied();
    }
    candidates.iter().copied().find(|(_, end)| *end == data_len)
}

fn render_viewport(output: &mut String, data: &[u8], region: &DataRegion) {
    let bytes = &data[region.start..region.end];
    if bytes.len() == 16 {
        let mut values = [0i16; 8];
        for (index, value) in values.iter_mut().enumerate() {
            *value = read_i16(bytes, index * 2);
        }
        write_hex_prefix(output, region.start, bytes);
        writeln!(
            output,
            "  ; viewport vscale={:?} vtrans={:?}",
            &values[..4],
            &values[4..]
        )
        .unwrap();
    } else {
        render_hex_range(output, data, region.start, region.end, "viewport");
    }
}

fn render_vertices(
    output: &mut String,
    data: &[u8],
    region: &DataRegion,
    count: usize,
    encoding: VertexEncoding,
    data_format: DataFormat,
) {
    let stride = data_format.vertex_stride();
    for index in 0..count {
        let start = region.start + index * stride;
        let end = start + stride;
        if end > region.end {
            break;
        }
        let bytes = &data[start..end];
        write_hex_prefix(output, start, bytes);
        match data_format {
            DataFormat::Fixed => {
                let x = read_i16(bytes, 0);
                let y = read_i16(bytes, 2);
                let z = read_i16(bytes, 4);
                let flag = read_u16(bytes, 6);
                let s = read_i16(bytes, 8);
                let t = read_i16(bytes, 10);
                match encoding {
                    VertexEncoding::Color => writeln!(
                        output,
                        "  ; vtx[{index}] x={x} y={y} z={z} flag=0x{flag:04X} s={s} t={t} rgba={},{},{},{}",
                        bytes[12], bytes[13], bytes[14], bytes[15]
                    )
                    .unwrap(),
                    VertexEncoding::Normal => writeln!(
                        output,
                        "  ; vtx[{index}] x={x} y={y} z={z} flag=0x{flag:04X} s={s} t={t} normal={},{},{} a={}",
                        bytes[12] as i8, bytes[13] as i8, bytes[14] as i8, bytes[15]
                    )
                    .unwrap(),
                }
            }
            DataFormat::Float => {
                let x = read_f32(bytes, 0);
                let y = read_f32(bytes, 4);
                let z = read_f32(bytes, 8);
                let flag = read_u16(bytes, 12);
                let s = read_i16(bytes, 14);
                let t = read_i16(bytes, 16);
                writeln!(
                    output,
                    "  ; vtx[{index}] x={x:.6} y={y:.6} z={z:.6} flag=0x{flag:04X} s={s} t={t} cn={},{},{},{}",
                    bytes[18], bytes[19], bytes[20], bytes[21]
                )
                .unwrap();
            }
        }
    }
}

fn render_matrix(output: &mut String, data: &[u8], region: &DataRegion, data_format: DataFormat) {
    let bytes = &data[region.start..region.end];
    render_hex_range(output, data, region.start, region.end, "matrix raw");
    if bytes.len() != 64 {
        return;
    }
    for row in 0..4 {
        write!(output, "         ; matrix row[{row}] = [").unwrap();
        for column in 0..4 {
            if column != 0 {
                output.push_str(", ");
            }
            let index = row * 4 + column;
            let value = match data_format {
                DataFormat::Fixed => {
                    let integer = read_i16(bytes, index * 2) as i32;
                    let fraction = read_u16(bytes, 32 + index * 2) as i32;
                    ((integer << 16) | fraction) as f32 / 65536.0
                }
                DataFormat::Float => read_f32(bytes, index * 4),
            };
            write!(output, "{value:.6}").unwrap();
        }
        writeln!(output, "]").unwrap();
    }
}

fn render_light(output: &mut String, data: &[u8], region: &DataRegion) {
    let bytes = &data[region.start..region.end];
    write_hex_prefix(output, region.start, bytes);
    if bytes.len() >= 12 {
        writeln!(
            output,
            "  ; light color={},{},{} color_copy={},{},{} direction={},{},{}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[8] as i8,
            bytes[9] as i8,
            bytes[10] as i8
        )
        .unwrap();
    } else {
        writeln!(output, "  ; light").unwrap();
    }
}

fn render_unclassified(
    output: &mut String,
    data: &[u8],
    start: usize,
    end: usize,
    is_texture_gap: bool,
    textures: &[TextureInput],
) {
    let real_textures = textures
        .iter()
        .filter(|texture| texture.name != "$none")
        .collect::<Vec<_>>();
    let encoded_len = real_textures.iter().try_fold(0usize, |total, texture| {
        let pixels = usize::try_from(texture.width)
            .ok()?
            .checked_mul(usize::try_from(texture.height).ok()?)?;
        total.checked_add(pixels.checked_mul(2)?)
    });
    if is_texture_gap {
        if let Some(encoded_len) = encoded_len {
            debug_assert!(!real_textures.is_empty());
            debug_assert!(encoded_len <= end - start && end - start - encoded_len < 8);
            let mut cursor = start;
            for texture in real_textures {
                let length = texture.width as usize * texture.height as usize * 2;
                let bytes = &data[cursor..cursor + length];
                render_texture(output, cursor, bytes, texture);
                cursor += length;
            }
            if cursor < end {
                render_hex_range(output, data, cursor, end, "alignment padding");
            }
            return;
        }
    }
    render_hex_range(output, data, start, end, "data");
}

fn render_texture(output: &mut String, start: usize, bytes: &[u8], texture: &TextureInput) {
    let texels = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    let uniform = texels
        .first()
        .copied()
        .filter(|first| texels.iter().all(|texel| texel == first));
    write_hex_prefix(output, start, bytes);
    if let Some(value) = uniform {
        writeln!(
            output,
            "  ; texels rgba16 {}x{} uniform=0x{value:04X} sha256={}",
            texture.width,
            texture.height,
            sha256_hex(bytes)
        )
        .unwrap();
    } else {
        let preview = texels
            .iter()
            .take(8)
            .map(|texel| format!("{texel:04X}"))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            output,
            "  ; texels rgba16 {}x{} first=[{preview}] sha256={}",
            texture.width,
            texture.height,
            sha256_hex(bytes)
        )
        .unwrap();
    }
}

fn render_hex_range(output: &mut String, data: &[u8], start: usize, end: usize, comment: &str) {
    let mut cursor = start;
    while cursor < end {
        let line_end = (cursor + 16).min(end);
        let chunk = &data[cursor..line_end];
        if chunk.len() == 16 {
            let mut repeated = 1usize;
            while cursor + (repeated + 1) * 16 <= end
                && data[cursor + repeated * 16..cursor + (repeated + 1) * 16] == *chunk
            {
                repeated += 1;
            }
            write_hex_prefix(output, cursor, chunk);
            writeln!(output, "  ; {comment}").unwrap();
            if repeated > 1 {
                writeln!(
                    output,
                    "*        ; repeated {repeated} identical 16-byte lines through 0x{:08X}",
                    cursor + repeated * 16
                )
                .unwrap();
                cursor += repeated * 16;
                continue;
            }
        } else {
            write_hex_prefix(output, cursor, chunk);
            writeln!(output, "  ; {comment}").unwrap();
        }
        cursor = line_end;
    }
}

fn write_hex_prefix(output: &mut String, address: usize, bytes: &[u8]) {
    write!(output, "{address:08X}:").unwrap();
    for byte in bytes {
        write!(output, " {byte:02x}").unwrap();
    }
}

fn read_i16(bytes: &[u8], offset: usize) -> i16 {
    i16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn annotate_command(w0: u32, w1: u32, microcode: Microcode) -> String {
    let opcode = (w0 >> 24) as u8;
    if microcode == Microcode::F3d {
        return match opcode {
            0x01 => format!(
                "gsSPMatrix(addr=0x{w1:08X}, params=0x{:02X})",
                (w0 >> 16) & 0xff
            ),
            0x03 => format!(
                "gsSPMoveMem(index=0x{:02X}, addr=0x{w1:08X})",
                (w0 >> 16) & 0xff
            ),
            0x04 => format!(
                "gsSPVertexF3D(addr=0x{w1:08X}, raw=0x{:06X})",
                w0 & 0x00ff_ffff
            ),
            0x06 => format!("gsSPDisplayListF3D(addr=0x{w1:08X})"),
            0xB8 => "gsSPEndDisplayList()".into(),
            _ => annotate_rdp_or_unknown(opcode, w0, w1),
        };
    }
    match opcode {
        0x01 => {
            let count = (w0 >> 12) & 0xff;
            let destination = ((w0 >> 1) & 0x7f).saturating_sub(count);
            format!("gsSPVertex(addr=0x{w1:08X}, count={count}, v0={destination})")
        }
        0x05 => format!(
            "gsSP1Triangle({}, {}, {})",
            (w0 >> 17) & 0x7f,
            (w0 >> 9) & 0x7f,
            (w0 >> 1) & 0x7f
        ),
        0x06 => format!(
            "gsSP2Triangles({}, {}, {}; {}, {}, {})",
            (w0 >> 17) & 0x7f,
            (w0 >> 9) & 0x7f,
            (w0 >> 1) & 0x7f,
            (w1 >> 17) & 0x7f,
            (w1 >> 9) & 0x7f,
            (w1 >> 1) & 0x7f
        ),
        0xD7 => format!("gsSPTexture(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xD8 => format!("gsSPPopMatrix(count={})", w1 >> 6),
        0xD9 => format!(
            "gsSPGeometryMode(clear-mask=0x{:06X}, set=0x{w1:08X})",
            w0 & 0x00ff_ffff
        ),
        0xDA => format!("gsSPMatrix(addr=0x{w1:08X}, params=0x{:02X})", w0 & 0xff),
        0xDB if ((w0 >> 16) & 0xff) == 0x0e => {
            format!("gsSPPerspNormalize(0x{:04X})", w1 & 0xffff)
        }
        0xDB => format!("gsSPMoveWord(raw0=0x{w0:08X}, value=0x{w1:08X})"),
        0xDC if (w0 & 0xff) == 0x08 => format!("gsSPViewport(0x{w1:08X})"),
        0xDC if (w0 & 0xff) == 0x0a => format!(
            "gsSPMoveMemLight(offset=0x{:02X}, addr=0x{w1:08X})",
            (w0 >> 8) & 0xff
        ),
        0xDC => format!("gsSPMoveMem(raw0=0x{w0:08X}, addr=0x{w1:08X})"),
        0xDE if w0 & 0x0001_0000 == 0 => format!("gsSPDisplayList(0x{w1:08X})"),
        0xDE => format!("gsSPBranchList(0x{w1:08X})"),
        0xDF => "gsSPEndDisplayList()".into(),
        0xE2 => format!("gsDPSetOtherModeL(raw0=0x{w0:08X}, value=0x{w1:08X})"),
        0xE3 => format!("gsDPSetOtherModeH(raw0=0x{w0:08X}, value=0x{w1:08X})"),
        _ => annotate_rdp_or_unknown(opcode, w0, w1),
    }
}

fn annotate_rdp_or_unknown(opcode: u8, w0: u32, w1: u32) -> String {
    match opcode {
        0xE6 => "gsDPLoadSync()".into(),
        0xE7 => "gsDPPipeSync()".into(),
        0xE8 => "gsDPTileSync()".into(),
        0xE9 => "gsDPFullSync()".into(),
        0xED => format!("gsDPSetScissor(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xF2 => format!("gsDPSetTileSize(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xF3 => format!("gsDPLoadBlock(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xF4 => format!("gsDPLoadTile(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xF5 => format!("gsDPSetTile(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xF7 => format!("gsDPSetFillColor(0x{w1:08X})"),
        0xFA => format!("gsDPSetPrimColor(raw0=0x{w0:08X}, rgba=0x{w1:08X})"),
        0xFB => format!("gsDPSetEnvColor(0x{w1:08X})"),
        0xFC => format!("gsDPSetCombine(raw0=0x{w0:08X}, raw1=0x{w1:08X})"),
        0xFD => format!(
            "gsDPSetTextureImage(fmt={}, siz={}, width={}, addr=0x{w1:08X})",
            (w0 >> 21) & 0x7,
            (w0 >> 19) & 0x3,
            (w0 & 0x0fff) + 1
        ),
        0xFE => format!("gsDPSetDepthImage(0x{w1:08X})"),
        0xFF => format!("gsDPSetColorImage(raw0=0x{w0:08X}, addr=0x{w1:08X})"),
        _ => format!("unknown(opcode=0x{opcode:02X})"),
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum DataKind {
    Viewport = 0,
    Vertex = 1,
    Matrix = 2,
    Texture = 3,
    Light = 4,
    Data = 5,
}

#[derive(Clone, Debug)]
struct LiteralDataRegion {
    address: u32,
    kind: DataKind,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct LiteralWordRegion {
    address: u32,
    words: Vec<(u32, u32)>,
}

/// A compiler-free fixture artifact built from explicit addresses, packed records, and literal
/// command words. It performs no symbol resolution or fixups.
#[derive(Clone, Debug)]
pub struct LiteralArtifact {
    id: String,
    entry_addr: u32,
    microcode: Microcode,
    data_format: DataFormat,
    data: Vec<LiteralDataRegion>,
    words: Vec<LiteralWordRegion>,
}

#[derive(Clone, Debug)]
pub struct LiteralImage {
    pub id: String,
    pub rdram: Vec<u8>,
    pub entry_addr: u32,
    pub microcode: Microcode,
    pub data_format: DataFormat,
    pub recipe_sha256: String,
}

impl LiteralArtifact {
    pub fn new(
        id: impl Into<String>,
        entry_addr: u32,
        microcode: Microcode,
        data_format: DataFormat,
    ) -> Self {
        Self {
            id: id.into(),
            entry_addr,
            microcode,
            data_format,
            data: Vec::new(),
            words: Vec::new(),
        }
    }

    pub fn data_at(&mut self, address: u32, kind: DataKind, bytes: impl AsRef<[u8]>) -> &mut Self {
        self.data.push(LiteralDataRegion {
            address,
            kind,
            bytes: bytes.as_ref().to_vec(),
        });
        self
    }

    pub fn words_at(&mut self, address: u32, words: &[(u32, u32)]) -> &mut Self {
        self.words.push(LiteralWordRegion {
            address,
            words: words.to_vec(),
        });
        self
    }

    pub fn finish(mut self) -> Result<LiteralImage> {
        validate_id(&self.id)?;
        if !self.entry_addr.is_multiple_of(8) {
            return Err(Error::fixture(
                &self.id,
                "literal entry address is not eight-byte aligned",
            ));
        }
        self.data.sort_by_key(|region| region.address);
        self.words.sort_by_key(|region| region.address);

        let mut previous_end = 0u32;
        for region in &self.data {
            let length = u32::try_from(region.bytes.len())
                .map_err(|_| Error::fixture(&self.id, "literal data region is too large"))?;
            let end = region
                .address
                .checked_add(length)
                .ok_or_else(|| Error::fixture(&self.id, "literal data region address overflow"))?;
            if region.bytes.is_empty() || region.address < previous_end || end > self.entry_addr {
                return Err(Error::fixture(
                    &self.id,
                    format_args!(
                        "literal data region 0x{:08X}..0x{end:08X} overlaps or crosses entry 0x{:08X}",
                        region.address, self.entry_addr
                    ),
                ));
            }
            previous_end = end;
        }

        let mut command_end = self.entry_addr;
        for (index, region) in self.words.iter().enumerate() {
            if !region.address.is_multiple_of(8) || region.words.is_empty() {
                return Err(Error::fixture(
                    &self.id,
                    "literal word regions must be aligned and nonempty",
                ));
            }
            if (index == 0 && region.address != self.entry_addr)
                || (index != 0 && region.address != command_end)
            {
                return Err(Error::fixture(
                    &self.id,
                    "literal command regions must be contiguous from entry_addr",
                ));
            }
            let length = u32::try_from(region.words.len())
                .ok()
                .and_then(|count| count.checked_mul(8))
                .ok_or_else(|| Error::fixture(&self.id, "literal command region is too large"))?;
            command_end = region
                .address
                .checked_add(length)
                .ok_or_else(|| Error::fixture(&self.id, "literal command address overflow"))?;
        }
        if self.words.is_empty() {
            return Err(Error::fixture(
                &self.id,
                "literal artifact has no command words",
            ));
        }

        let mut rdram = vec![0u8; command_end as usize];
        for region in &self.data {
            let start = region.address as usize;
            rdram[start..start + region.bytes.len()].copy_from_slice(&region.bytes);
        }
        for region in &self.words {
            let mut cursor = region.address as usize;
            for &(w0, w1) in &region.words {
                rdram[cursor..cursor + 4].copy_from_slice(&w0.to_be_bytes());
                rdram[cursor + 4..cursor + 8].copy_from_slice(&w1.to_be_bytes());
                cursor += 8;
            }
        }

        let mut recipe = Vec::new();
        recipe.extend_from_slice(b"fast3d-literal\0v1\0");
        recipe.extend_from_slice(&(self.id.len() as u32).to_be_bytes());
        recipe.extend_from_slice(self.id.as_bytes());
        recipe.extend_from_slice(&self.entry_addr.to_be_bytes());
        recipe.push(self.microcode.identity_byte());
        recipe.push(self.data_format.identity_byte());
        recipe.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
        for region in &self.data {
            recipe.extend_from_slice(&region.address.to_be_bytes());
            recipe.push(region.kind as u8);
            recipe.extend_from_slice(&(region.bytes.len() as u64).to_be_bytes());
            recipe.extend_from_slice(&region.bytes);
        }
        recipe.extend_from_slice(&(self.words.len() as u32).to_be_bytes());
        for region in &self.words {
            recipe.extend_from_slice(&region.address.to_be_bytes());
            recipe.extend_from_slice(&(region.words.len() as u32).to_be_bytes());
            for &(w0, w1) in &region.words {
                recipe.extend_from_slice(&w0.to_be_bytes());
                recipe.extend_from_slice(&w1.to_be_bytes());
            }
        }

        Ok(LiteralImage {
            id: self.id,
            rdram,
            entry_addr: self.entry_addr,
            microcode: self.microcode,
            data_format: self.data_format,
            recipe_sha256: sha256_hex(&recipe),
        })
    }
}

pub const COLORED_TRIANGLE_ID: &str = "literal/colored-triangle/v1";

/// The literal colored-triangle recipe shared by fixture tests and the authoring tool.
pub fn colored_triangle_literal() -> Result<LiteralImage> {
    use n64_gbi::encode::{Vp, VtxColored};

    const VP_ADDR: u32 = 0x0000;
    const VTX_ADDR: u32 = 0x0010;
    const ENTRY: u32 = 0x0040;

    let mut artifact = LiteralArtifact::new(
        COLORED_TRIANGLE_ID,
        ENTRY,
        Microcode::F3dex2,
        DataFormat::Fixed,
    );
    artifact.data_at(
        VP_ADDR,
        DataKind::Viewport,
        Vp {
            vscale: [640, 480, 511, 0],
            vtrans: [640, 480, 511, 0],
        }
        .to_bytes(),
    );
    for (index, (x, y, rgba)) in [
        (-1, -1, [255, 0, 0, 255]),
        (1, -1, [0, 255, 0, 255]),
        (0, 1, [0, 0, 255, 255]),
    ]
    .into_iter()
    .enumerate()
    {
        artifact.data_at(
            VTX_ADDR + index as u32 * 16,
            DataKind::Vertex,
            VtxColored {
                x,
                y,
                z: 0,
                flag: 0,
                s: 0,
                t: 0,
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            }
            .to_bytes(),
        );
    }
    artifact.words_at(
        ENTRY,
        &[
            (0xDC08_0008, VP_ADDR),
            (0xD9FF_FFFF, 0x0000_0004),
            (0xE300_0A01, 0x0000_0000),
            (0xE200_001C, 0x0F0A_4040),
            (0xFCFF_FFFF, 0xFFFE_793C),
            (0x0100_3006, VTX_ADDR),
            (0x0500_0204, 0x0000_0000),
            (0xDF00_0000, 0x0000_0000),
        ],
    );
    artifact.finish()
}

pub fn build_literal_fixture(id: &str, revision: &str) -> Result<FixtureFiles> {
    validate_full_revision(revision, "revision")?;
    let literal = literal_recipe(id)?;
    let input = FixtureInput {
        time_f32_bits: "00000000".into(),
        source: SourceInput {
            kind: SourceKind::LiteralRecipe,
            logical_id: id.into(),
            repository: REPOSITORY.into(),
            revision: revision.into(),
            path: "tools/fast3d-fixtures/src/lib.rs".into(),
            sha256: literal.recipe_sha256.clone(),
        },
        textures: vec![TextureInput {
            name: "$none".into(),
            format: "rgba8".into(),
            width: 0,
            height: 0,
            sha256: sha256_hex(&[]),
            repository: REPOSITORY.into(),
            revision: revision.into(),
            path: None,
            recipe: Some("empty".into()),
        }],
    };
    build_fixture(FixtureBuild {
        id: literal.id,
        rdram: literal.rdram,
        entry_addr: literal.entry_addr,
        microcode: literal.microcode,
        data_format: literal.data_format,
        input,
        provenance: literal_provenance(revision),
    })
}

fn rebuild_literal_fixture(manifest: &Manifest) -> Result<FixtureFiles> {
    let literal = literal_recipe(&manifest.id)?;
    if manifest.input.source.sha256 != literal.recipe_sha256 {
        return Err(Error::fixture(
            &manifest.id,
            format_args!(
                "literal recipe hash declares {}, actual is {}",
                manifest.input.source.sha256, literal.recipe_sha256
            ),
        ));
    }
    build_fixture(FixtureBuild {
        id: literal.id,
        rdram: literal.rdram,
        entry_addr: literal.entry_addr,
        microcode: literal.microcode,
        data_format: literal.data_format,
        input: manifest.input.clone(),
        provenance: manifest.provenance.clone(),
    })
}

fn literal_recipe(id: &str) -> Result<LiteralImage> {
    match id {
        COLORED_TRIANGLE_ID => colored_triangle_literal(),
        _ => Err(Error::new(format!("unknown literal fixture recipe '{id}'"))),
    }
}

pub fn write_fixture_files(root: &Path, files: &FixtureFiles) -> Result<()> {
    let directory = root.join("v1").join(&files.manifest.id);
    fs::create_dir_all(&directory)?;
    write_atomic(
        &directory.join(MANIFEST_FILE),
        files.manifest_text.as_bytes(),
    )?;
    write_atomic(&directory.join(RDRAM_FILE), &files.rdram)?;
    write_atomic(&directory.join(DUMP_FILE), files.dump.as_bytes())?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("fast3d-fixtures.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temporary, path)?;
    Ok(())
}

pub fn write_capture_envelope(root: &Path, capture: &Capture) -> Result<()> {
    validate_id(&capture.id)?;
    if capture.source.sha256 != sha256_hex(&capture.source_bytes) {
        return Err(Error::new(format!(
            "capture '{}' source bytes do not match source_sha256",
            capture.id
        )));
    }
    let directory = root.join(&capture.id);
    fs::create_dir_all(&directory)?;
    let source_file = "source.bin".to_owned();
    write_atomic(&directory.join(&source_file), &capture.source_bytes)?;

    let mut envelope_textures = Vec::with_capacity(capture.textures.len());
    for (index, (input, bytes)) in capture.textures.iter().enumerate() {
        if input.sha256 != sha256_hex(bytes) {
            return Err(Error::new(format!(
                "capture '{}' texture '{}' bytes do not match sha256",
                capture.id, input.name
            )));
        }
        let input_file = format!("texture-{index:03}.rgba8");
        write_atomic(&directory.join(&input_file), bytes)?;
        envelope_textures.push(CaptureTexture {
            input: input.clone(),
            input_file,
        });
    }
    write_atomic(&directory.join(RDRAM_FILE), &capture.rdram)?;
    let envelope = CaptureEnvelope {
        schema: CAPTURE_SCHEMA,
        id: capture.id.clone(),
        rdram_file: RDRAM_FILE.into(),
        source_file,
        rdram_len: capture.rdram.len() as u64,
        rdram_sha256: sha256_hex(&capture.rdram),
        entry_addr: format!("0x{:08X}", capture.entry_addr),
        microcode: capture.microcode,
        data_format: capture.data_format,
        time_f32_bits: format!("{:08x}", capture.time_f32_bits),
        source: capture.source.clone(),
        textures: envelope_textures,
        provenance: capture.provenance.clone(),
    };
    let mut manifest_text = toml::to_string_pretty(&envelope)?;
    if !manifest_text.ends_with('\n') {
        manifest_text.push('\n');
    }
    write_atomic(&directory.join(CAPTURE_FILE), manifest_text.as_bytes())?;
    Ok(())
}

pub fn import_captures(from: &Path, fixture_root: &Path) -> Result<usize> {
    let index = read_index(fixture_root)?;
    let registered = index
        .fixtures
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut paths = Vec::new();
    collect_named_files(from, CAPTURE_FILE, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        return Err(Error::new(format!(
            "no {CAPTURE_FILE} files found below {}",
            from.display()
        )));
    }
    for path in &paths {
        let text = fs::read_to_string(path)?;
        let envelope: CaptureEnvelope = toml::from_str(&text)?;
        if envelope.schema != CAPTURE_SCHEMA {
            return Err(Error::new(format!(
                "{} uses unsupported capture schema {}",
                path.display(),
                envelope.schema
            )));
        }
        validate_id(&envelope.id)?;
        let expected_envelope_path = from.join(&envelope.id).join(CAPTURE_FILE);
        if path != &expected_envelope_path {
            return Err(Error::new(format!(
                "capture '{}' must live at {}",
                envelope.id,
                expected_envelope_path.display()
            )));
        }
        if !registered.contains(envelope.id.as_str()) {
            return Err(Error::new(format!(
                "capture '{}' is not registered in index.toml",
                envelope.id
            )));
        }
        if envelope.rdram_file != RDRAM_FILE || envelope.source_file != "source.bin" {
            return Err(Error::new(format!(
                "capture '{}' uses a noncanonical envelope path",
                envelope.id
            )));
        }
        let directory = path.parent().unwrap_or(from);
        let source_bytes = fs::read(directory.join(&envelope.source_file))?;
        if sha256_hex(&source_bytes) != envelope.source.sha256 {
            return Err(Error::new(format!(
                "capture '{}' source bytes are stale",
                envelope.id
            )));
        }
        std::str::from_utf8(&source_bytes).map_err(|error| {
            Error::new(format!(
                "capture '{}' source is not UTF-8: {error}",
                envelope.id
            ))
        })?;
        let rdram = fs::read(directory.join(&envelope.rdram_file))?;
        if rdram.len() as u64 != envelope.rdram_len || sha256_hex(&rdram) != envelope.rdram_sha256 {
            return Err(Error::new(format!(
                "capture '{}' RDRAM length/hash mismatch",
                envelope.id
            )));
        }
        let mut textures = Vec::with_capacity(envelope.textures.len());
        for texture in &envelope.textures {
            if !matches!(
                Path::new(&texture.input_file)
                    .components()
                    .collect::<Vec<_>>()[..],
                [Component::Normal(_)]
            ) {
                return Err(Error::new(format!(
                    "capture '{}' texture path escapes its directory",
                    envelope.id
                )));
            }
            let bytes = fs::read(directory.join(&texture.input_file))?;
            if sha256_hex(&bytes) != texture.input.sha256 {
                return Err(Error::new(format!(
                    "capture '{}' texture '{}' is stale",
                    envelope.id, texture.input.name
                )));
            }
            let expected = u64::from(texture.input.width)
                .checked_mul(u64::from(texture.input.height))
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| Error::new("capture texture dimensions overflow"))?;
            if bytes.len() as u64 != expected {
                return Err(Error::new(format!(
                    "capture '{}' texture '{}' expected {expected} bytes, got {}",
                    envelope.id,
                    texture.input.name,
                    bytes.len()
                )));
            }
            textures.push(texture.input.clone());
        }
        let files = build_fixture(FixtureBuild {
            id: envelope.id.clone(),
            rdram,
            entry_addr: parse_prefixed_u32(&envelope.entry_addr, "entry_addr")?,
            microcode: envelope.microcode,
            data_format: envelope.data_format,
            input: FixtureInput {
                time_f32_bits: envelope.time_f32_bits.clone(),
                source: envelope.source.clone(),
                textures,
            },
            provenance: envelope.provenance.clone(),
        })?;
        write_fixture_files(fixture_root, &files)?;
    }
    Ok(paths.len())
}

pub fn read_index(root: &Path) -> Result<FixtureIndex> {
    let path = root.join("index.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    let index: FixtureIndex = toml::from_str(&text)
        .map_err(|error| Error::new(format!("invalid {}: {error}", path.display())))?;
    if index.schema != INDEX_SCHEMA {
        return Err(Error::new(format!(
            "unsupported fixture index schema {}, expected {INDEX_SCHEMA}",
            index.schema
        )));
    }
    if index.registry_file != DEFAULT_REGISTRY_FILE {
        return Err(Error::new(format!(
            "index registry_file must be {DEFAULT_REGISTRY_FILE}"
        )));
    }
    Ok(index)
}

pub fn render_registry(fixture_root: &Path, index: &FixtureIndex) -> Result<String> {
    let mut entries = index.fixtures.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let mut output = String::new();
    writeln!(
        output,
        "// @generated by `cargo run -p fast3d-fixtures -- registry`; do not edit."
    )
    .unwrap();
    writeln!(output, "use super::EmbeddedFixture;\n").unwrap();
    writeln!(
        output,
        "pub(super) static EMBEDDED: &[EmbeddedFixture] = &["
    )
    .unwrap();
    for entry in entries {
        validate_id(&entry.id)?;
        let directory = fixture_root.join("v1").join(&entry.id);
        let manifest_text = fs::read_to_string(directory.join(MANIFEST_FILE)).map_err(|error| {
            Error::fixture(
                &entry.id,
                format_args!("cannot read {MANIFEST_FILE}: {error}"),
            )
        })?;
        let manifest = parse_manifest(&manifest_text).map_err(|error| {
            Error::fixture(&entry.id, format_args!("invalid {MANIFEST_FILE}: {error}"))
        })?;
        let rdram = fs::read(directory.join(RDRAM_FILE)).map_err(|error| {
            Error::fixture(&entry.id, format_args!("cannot read {RDRAM_FILE}: {error}"))
        })?;
        let dump = fs::read_to_string(directory.join(DUMP_FILE)).map_err(|error| {
            Error::fixture(&entry.id, format_args!("cannot read {DUMP_FILE}: {error}"))
        })?;
        validate_fixture(&entry.id, &manifest, &manifest_text, &rdram, &dump)?;
        let manifest_sha256 = sha256_hex(manifest_text.as_bytes());
        let base = format!("/tests/fixtures/v1/{}", entry.id);
        writeln!(output, "    EmbeddedFixture {{").unwrap();
        writeln!(output, "        id: {:?},", entry.id).unwrap();
        render_include(
            &mut output,
            "manifest",
            "include_str",
            &format!("{base}/{MANIFEST_FILE}"),
        );
        writeln!(output, "        manifest_sha256: {manifest_sha256:?},").unwrap();
        render_include(
            &mut output,
            "rdram",
            "include_bytes",
            &format!("{base}/{RDRAM_FILE}"),
        );
        render_include(
            &mut output,
            "dump",
            "include_str",
            &format!("{base}/{DUMP_FILE}"),
        );
        writeln!(output, "    }},").unwrap();
    }
    writeln!(output, "];").unwrap();
    Ok(output)
}

fn render_include(output: &mut String, field: &str, macro_name: &str, path: &str) {
    writeln!(output, "        {field}: {macro_name}!(concat!(").unwrap();
    writeln!(output, "            env!(\"CARGO_MANIFEST_DIR\"),").unwrap();
    writeln!(output, "            {path:?}").unwrap();
    writeln!(output, "        )),").unwrap();
}

pub fn write_registry(repo_root: &Path, fixture_root: &Path) -> Result<()> {
    let index = read_index(fixture_root)?;
    let generated = render_registry(fixture_root, &index)?;
    let path = repo_root.join(&index.registry_file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(&path, generated.as_bytes())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    pub fixture_count: usize,
    pub fixture_bytes: u64,
}

pub fn verify_repository(repo_root: &Path, fixture_root: &Path) -> Result<VerificationReport> {
    let index = read_index(fixture_root)?;
    let mut ids = BTreeSet::new();
    let mut prior_id: Option<&str> = None;
    let mut indexed_directories = BTreeSet::new();
    let mut tuple_owners = BTreeMap::new();
    let mut fixture_bytes = 0u64;

    for entry in &index.fixtures {
        validate_id(&entry.id)?;
        if !ids.insert(entry.id.as_str()) {
            return Err(Error::new(format!(
                "fixture '{}' appears more than once in index.toml",
                entry.id
            )));
        }
        if prior_id.is_some_and(|previous| previous >= entry.id.as_str()) {
            return Err(Error::new("index.toml fixture IDs must be strictly sorted"));
        }
        prior_id = Some(&entry.id);
        if !matches!(entry.coverage.as_str(), "fast3d-covered" | "fast3d-local") {
            return Err(Error::new(format!(
                "fixture '{}' has unknown coverage '{}'",
                entry.id, entry.coverage
            )));
        }
        let mut sorted_groups = entry.groups.clone();
        sorted_groups.sort();
        sorted_groups.dedup();
        if sorted_groups != entry.groups {
            return Err(Error::new(format!(
                "fixture '{}' groups must be sorted and unique",
                entry.id
            )));
        }

        let directory = fixture_root.join("v1").join(&entry.id);
        indexed_directories.insert(directory.clone());
        let manifest_path = directory.join(MANIFEST_FILE);
        let rdram_path = directory.join(RDRAM_FILE);
        let dump_path = directory.join(DUMP_FILE);
        let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| {
            Error::fixture(
                &entry.id,
                format_args!("cannot read {}: {error}", manifest_path.display()),
            )
        })?;
        let rdram = fs::read(&rdram_path).map_err(|error| {
            Error::fixture(
                &entry.id,
                format_args!("cannot read {}: {error}", rdram_path.display()),
            )
        })?;
        let dump = fs::read_to_string(&dump_path).map_err(|error| {
            Error::fixture(
                &entry.id,
                format_args!("cannot read {}: {error}", dump_path.display()),
            )
        })?;
        let manifest = parse_manifest(&manifest_text).map_err(|error| {
            Error::fixture(&entry.id, format_args!("invalid manifest: {error}"))
        })?;
        validate_fixture(&entry.id, &manifest, &manifest_text, &rdram, &dump)?;
        verify_live_inputs(repo_root, &manifest)?;

        let tuple = tuple_key(&manifest);
        if let Some(previous) = tuple_owners.insert(tuple, entry.id.as_str()) {
            return Err(Error::new(format!(
                "fixtures '{previous}' and '{}' duplicate the same input tuple",
                entry.id
            )));
        }
        if entry.coverage == "fast3d-local" {
            let rebuilt = rebuild_literal_fixture(&manifest)?;
            if rebuilt.manifest_text != manifest_text
                || rebuilt.rdram != rdram
                || rebuilt.dump != dump
            {
                return Err(Error::fixture(
                    &entry.id,
                    "literal recipe reproduction differs from the checked artifact",
                ));
            }
        }
        fixture_bytes = fixture_bytes
            .checked_add(manifest_text.len() as u64)
            .and_then(|total| total.checked_add(rdram.len() as u64))
            .and_then(|total| total.checked_add(dump.len() as u64))
            .ok_or_else(|| Error::new("fixture byte count overflow"))?;
    }

    let mut actual_manifests = Vec::new();
    collect_named_files(
        &fixture_root.join("v1"),
        MANIFEST_FILE,
        &mut actual_manifests,
    )?;
    let actual_directories = actual_manifests
        .into_iter()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>();
    if actual_directories != indexed_directories {
        let orphan = actual_directories.difference(&indexed_directories).next();
        let missing = indexed_directories.difference(&actual_directories).next();
        return Err(Error::new(format!(
            "index/directory drift (orphan={}, missing={})",
            orphan
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".into()),
            missing
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".into())
        )));
    }

    let expected_registry = render_registry(fixture_root, &index)?;
    let registry_path = repo_root.join(&index.registry_file);
    let actual_registry = fs::read_to_string(&registry_path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", registry_path.display())))?;
    if actual_registry != expected_registry {
        return Err(Error::new(format!(
            "generated registry is stale at {}; run `cargo run -p fast3d-fixtures -- registry`",
            first_differing_line(&actual_registry, &expected_registry)
        )));
    }
    if fixture_bytes > FIXTURE_BUDGET_BYTES {
        return Err(Error::new(format!(
            "fixture subtree is {fixture_bytes} bytes, over the {FIXTURE_BUDGET_BYTES}-byte budget"
        )));
    }
    Ok(VerificationReport {
        fixture_count: index.fixtures.len(),
        fixture_bytes,
    })
}

fn verify_live_inputs(repo_root: &Path, manifest: &Manifest) -> Result<()> {
    match manifest.input.source.kind {
        SourceKind::LiteralRecipe => {
            let literal = literal_recipe(&manifest.id)?;
            if literal.recipe_sha256 != manifest.input.source.sha256 {
                return Err(Error::fixture(
                    &manifest.id,
                    "literal source recipe hash drifted",
                ));
            }
        }
        SourceKind::File => {
            let path = repo_root.join(&manifest.input.source.path);
            let bytes = fs::read(&path).map_err(|error| {
                Error::fixture(
                    &manifest.id,
                    format_args!("cannot read source {}: {error}", path.display()),
                )
            })?;
            std::str::from_utf8(&bytes).map_err(|error| {
                Error::fixture(
                    &manifest.id,
                    format_args!("source {} is not UTF-8: {error}", path.display()),
                )
            })?;
            let actual = sha256_hex(&bytes);
            if actual != manifest.input.source.sha256 {
                return Err(Error::fixture(
                    &manifest.id,
                    format_args!(
                        "source hash declares {}, actual is {actual}",
                        manifest.input.source.sha256
                    ),
                ));
            }
        }
        SourceKind::External => {}
    }
    for texture in &manifest.input.textures {
        let bytes = if let Some(path) = &texture.path {
            fs::read(repo_root.join(path))?
        } else if let Some(recipe) = &texture.recipe {
            materialize_texture_recipe(recipe, texture.width, texture.height)?
        } else {
            return Err(Error::fixture(
                &manifest.id,
                format_args!("texture '{}' has no path or recipe", texture.name),
            ));
        };
        let expected_len = u64::from(texture.width) * u64::from(texture.height) * 4;
        if bytes.len() as u64 != expected_len || sha256_hex(&bytes) != texture.sha256 {
            return Err(Error::fixture(
                &manifest.id,
                format_args!("texture '{}' input bytes drifted", texture.name),
            ));
        }
    }
    Ok(())
}

pub fn materialize_texture_recipe(recipe: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    if recipe == "empty" {
        if width == 0 && height == 0 {
            return Ok(Vec::new());
        }
        return Err(Error::new("the empty recipe requires 0x0 dimensions"));
    }
    let Some(pixel_hex) = recipe.strip_prefix("solid-rgba8:") else {
        return Err(Error::new(format!("unknown texture recipe '{recipe}'")));
    };
    if pixel_hex.len() != 8 || !pixel_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(format!(
            "texture recipe '{recipe}' needs exactly one RGBA8 pixel"
        )));
    }
    let mut pixel = [0u8; 4];
    for (index, byte) in pixel.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&pixel_hex[index * 2..index * 2 + 2], 16)
            .map_err(|error| Error::new(format!("invalid texture recipe: {error}")))?;
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| Error::new("texture recipe dimensions overflow"))?;
    let mut bytes = Vec::with_capacity(pixels.saturating_mul(4));
    for _ in 0..pixels {
        bytes.extend_from_slice(&pixel);
    }
    Ok(bytes)
}

fn tuple_key(manifest: &Manifest) -> String {
    let mut key = format!(
        "{}:{}:{}:{}",
        manifest.input.source.sha256,
        manifest.input.time_f32_bits,
        microcode_name(manifest.microcode),
        data_format_name(manifest.data_format)
    );
    let mut textures = manifest.input.textures.iter().collect::<Vec<_>>();
    textures.sort_by(|left, right| left.name.cmp(&right.name));
    for texture in textures {
        write!(
            key,
            ":{}:{}:{}:{}",
            texture.name, texture.width, texture.height, texture.sha256
        )
        .unwrap();
    }
    key
}

fn collect_named_files(root: &Path, name: &str, output: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_named_files(&path, name, output)?;
        } else if entry.file_name() == name {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVISION: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn repository_verifier_covers_the_checked_fixture_tree() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture_root = repo_root.join("fast3d/tests/fixtures");
        let expected_count = read_index(&fixture_root).unwrap().fixtures.len();
        let report = verify_repository(&repo_root, &fixture_root)
            .expect("the checked fixture tree must pass repository verification");
        assert_eq!(report.fixture_count, expected_count);
    }

    #[test]
    fn external_source_records_do_not_require_a_local_file_or_change_artifact_identity() {
        let mut files = build_literal_fixture(COLORED_TRIANGLE_ID, REVISION)
            .expect("the literal fixture must build");
        let before = artifact_sha256(
            files.manifest.entry_addr().unwrap(),
            files.manifest.microcode,
            files.manifest.data_format,
            &files.manifest.input,
            &files.rdram,
        )
        .unwrap();
        files.manifest.input.source.kind = SourceKind::External;
        files.manifest.input.source.path = "upstream/scenes/colored-triangle.n64".into();
        let after = artifact_sha256(
            files.manifest.entry_addr().unwrap(),
            files.manifest.microcode,
            files.manifest.data_format,
            &files.manifest.input,
            &files.rdram,
        )
        .unwrap();

        assert_eq!(before, after, "source location is provenance, not identity");
        verify_live_inputs(Path::new("unused-repository-root"), &files.manifest)
            .expect("external source provenance must not require a local source file");
    }

    #[test]
    fn texture_gap_before_an_inferred_region_is_rendered_semantically() {
        let data = [
            0xff, 0xff, 0x00, 0x01, // two RGBA16 texels
            0, 1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 7, 0, 8, // viewport
        ];
        let regions = [DataRegion {
            start: 4,
            end: data.len(),
            kind: RegionKind::Viewport,
        }];
        let textures = [TextureInput {
            name: "$texture".into(),
            format: "rgba8".into(),
            width: 2,
            height: 1,
            sha256: "0".repeat(64),
            repository: REPOSITORY.into(),
            revision: REVISION.into(),
            path: None,
            recipe: Some("solid-rgba8:ffffffff".into()),
        }];
        let mut output = String::new();

        render_data(&mut output, &data, &regions, DataFormat::Fixed, &textures);

        assert!(
            output.contains("; texels rgba16 2x1 first=[FFFF,0001]"),
            "non-final texture gap degraded to raw hex:\n{output}"
        );
        assert!(output.contains("; viewport"));
    }
}
