use fast3d_fixture_tool::{
    compiler_provenance, materialize_texture_recipe, parse_manifest, sha256_hex, verify_repository,
    write_capture_envelope, Capture, DataFormat, Error, Microcode, Result, SourceInput, SourceKind,
    TextureInput, REPOSITORY,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_PLAN: &str = "fast3d/tests/fixtures/capture-plan.toml";
const DEFAULT_FIXTURE_ROOT: &str = "fast3d/tests/fixtures";
const PILOT_CAPTURE_COUNT: usize = 3;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturePlan {
    schema: u32,
    expected_count: usize,
    #[serde(rename = "capture")]
    captures: Vec<PlanEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanEntry {
    id: String,
    source_path: String,
    source_logical_id: String,
    time_f32_bits: String,
    texture_name: String,
    texture_recipe: String,
    texture_width: u32,
    texture_height: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fast3d-fixture-capture: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage());
    };
    let repo_root = find_repo_root(&env::current_dir()?)?;
    ensure_capture_inputs_clean(&repo_root)?;
    let revision = git_output(&repo_root, &["rev-parse", "HEAD"])?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(format!(
            "git returned abbreviated or invalid revision '{revision}'"
        )));
    }
    let plan_path = resolve(
        &repo_root,
        option(&arguments[1..], "--registry")?.unwrap_or(DEFAULT_PLAN),
    );
    let plan = read_plan(&plan_path)?;
    validate_plan(&plan)?;

    match command {
        "capture" => {
            require_known_options(&arguments[1..], &["--registry", "--out"])?;
            let out = option(&arguments[1..], "--out")?
                .ok_or_else(|| Error::new("capture requires --out <directory>"))?;
            let out = resolve(&repo_root, out);
            ensure_empty_output(&out)?;
            for entry in &plan.captures {
                let capture = capture_entry(&repo_root, &revision, entry)?;
                write_capture_envelope(&out, &capture)?;
                println!(
                    "captured '{}' ({} bytes, entry 0x{:08X})",
                    entry.id,
                    capture.rdram.len(),
                    capture.entry_addr
                );
            }
        }
        "verify" => {
            require_known_options(&arguments[1..], &["--registry", "--root"])?;
            let fixture_root = resolve(
                &repo_root,
                option(&arguments[1..], "--root")?.unwrap_or(DEFAULT_FIXTURE_ROOT),
            );
            verify_repository(&repo_root, &fixture_root)?;
            for entry in &plan.captures {
                let capture = capture_entry(&repo_root, &revision, entry)?;
                verify_checked_capture(&fixture_root, entry, &capture)?;
                println!("byte parity verified for '{}'", entry.id);
            }
        }
        _ => return Err(usage()),
    }
    Ok(())
}

fn read_plan(path: &Path) -> Result<CapturePlan> {
    let text = fs::read_to_string(path)
        .map_err(|error| Error::new(format!("cannot read {}: {error}", path.display())))?;
    toml::from_str(&text)
        .map_err(|error| Error::new(format!("invalid {}: {error}", path.display())))
}

fn validate_plan(plan: &CapturePlan) -> Result<()> {
    if plan.schema != 1 {
        return Err(Error::new(format!(
            "unsupported capture plan schema {}",
            plan.schema
        )));
    }
    if plan.expected_count != PILOT_CAPTURE_COUNT || plan.captures.len() != PILOT_CAPTURE_COUNT {
        return Err(Error::new(format!(
            "slice 1 requires exactly {PILOT_CAPTURE_COUNT} pilot tuples, plan declares {} and contains {}",
            plan.expected_count,
            plan.captures.len()
        )));
    }
    let expected = BTreeSet::from([
        ("scene/morphcube/t-00000000/tex-white1", "00000000"),
        ("scene/morphcube/t-3fc90fdb/tex-white1", "3fc90fdb"),
        ("scene/morphcube/t-40490fdb/tex-white1", "40490fdb"),
    ]);
    let actual = plan
        .captures
        .iter()
        .map(|entry| (entry.id.as_str(), entry.time_f32_bits.as_str()))
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(Error::new(
            "capture plan does not contain the exact three reviewed morphcube tuples",
        ));
    }
    if plan.captures.iter().any(|entry| {
        entry.source_path != "fast3d/tests/scenes/morphcube.n64"
            || entry.source_logical_id != "scene/morphcube"
            || entry.texture_name != "$legacy"
            || entry.texture_recipe != "solid-rgba8:ffffffff"
            || entry.texture_width != 1
            || entry.texture_height != 1
    }) {
        return Err(Error::new(
            "capture plan changed the reviewed morphcube source or 1x1-white texture input",
        ));
    }
    Ok(())
}

fn capture_entry(repo_root: &Path, revision: &str, entry: &PlanEntry) -> Result<Capture> {
    let source_bytes = fs::read(repo_root.join(&entry.source_path))?;
    let source = std::str::from_utf8(&source_bytes)
        .map_err(|error| Error::new(format!("{} is not UTF-8: {error}", entry.source_path)))?;
    let time_bits = parse_time_bits(&entry.time_f32_bits)?;
    let texture_bytes = materialize_texture_recipe(
        &entry.texture_recipe,
        entry.texture_width,
        entry.texture_height,
    )?;
    let expected_texture_len = u64::from(entry.texture_width)
        .checked_mul(u64::from(entry.texture_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::new("texture dimensions overflow"))?;
    if texture_bytes.len() as u64 != expected_texture_len {
        return Err(Error::new(format!(
            "texture recipe produced {} bytes, expected {expected_texture_len}",
            texture_bytes.len()
        )));
    }

    let compile = || -> Result<fast3d::asm::Image> {
        fast3d::asm::assemble_at(
            source,
            f32::from_bits(time_bits),
            Some((
                texture_bytes.as_slice(),
                entry.texture_width,
                entry.texture_height,
            )),
        )
        .map_err(|diagnostics| {
            Error::new(format!(
                "compiler rejected '{}' at time bits {}: {diagnostics:?}",
                entry.id, entry.time_f32_bits
            ))
        })
    };
    let first = compile()?;
    let second = compile()?;
    if first.entry_addr != second.entry_addr || first.rdram != second.rdram {
        return Err(Error::new(format!(
            "compiler output for '{}' is nondeterministic across two invocations",
            entry.id
        )));
    }
    let source_hash = sha256_hex(&source_bytes);
    let texture_hash = sha256_hex(&texture_bytes);
    Ok(Capture {
        id: entry.id.clone(),
        rdram: first.rdram,
        entry_addr: first.entry_addr,
        microcode: Microcode::F3dex2,
        data_format: DataFormat::Fixed,
        time_f32_bits: time_bits,
        source: SourceInput {
            kind: SourceKind::File,
            logical_id: entry.source_logical_id.clone(),
            repository: REPOSITORY.into(),
            revision: revision.into(),
            path: entry.source_path.clone(),
            sha256: source_hash,
        },
        source_bytes,
        textures: vec![(
            TextureInput {
                name: entry.texture_name.clone(),
                format: "rgba8".into(),
                width: entry.texture_width,
                height: entry.texture_height,
                sha256: texture_hash,
                repository: REPOSITORY.into(),
                revision: revision.into(),
                path: None,
                recipe: Some(entry.texture_recipe.clone()),
            },
            texture_bytes,
        )],
        provenance: compiler_provenance(revision),
    })
}

fn verify_checked_capture(fixture_root: &Path, entry: &PlanEntry, capture: &Capture) -> Result<()> {
    let directory = fixture_root.join("v1").join(&entry.id);
    let manifest = parse_manifest(&fs::read_to_string(directory.join("fixture.toml"))?)?;
    let rdram = fs::read(directory.join("image.rdram"))?;
    let checked_entry = manifest.entry_addr()?;
    if checked_entry != capture.entry_addr {
        return Err(Error::new(format!(
            "fixture '{}' entry drift: checked 0x{checked_entry:08X}, compiler 0x{:08X}",
            entry.id, capture.entry_addr
        )));
    }
    if rdram != capture.rdram {
        let offset = rdram
            .iter()
            .zip(&capture.rdram)
            .position(|(checked, current)| checked != current)
            .unwrap_or_else(|| rdram.len().min(capture.rdram.len()));
        let checked = rdram.get(offset).copied();
        let current = capture.rdram.get(offset).copied();
        return Err(Error::new(format!(
            "fixture '{}' differs from current compiler output at byte 0x{offset:08X}: checked={checked:?}, compiler={current:?} (lengths {} vs {})",
            entry.id,
            rdram.len(),
            capture.rdram.len()
        )));
    }
    if manifest.input.source.sha256 != capture.source.sha256
        || manifest.input.time_f32_bits != entry.time_f32_bits
        || manifest.input.textures.len() != capture.textures.len()
        || manifest
            .input
            .textures
            .iter()
            .zip(&capture.textures)
            .any(|(checked, (current, _))| {
                checked.name != current.name
                    || checked.format != current.format
                    || checked.width != current.width
                    || checked.height != current.height
                    || checked.sha256 != current.sha256
                    || checked.path != current.path
                    || checked.recipe != current.recipe
            })
    {
        return Err(Error::new(format!(
            "fixture '{}' input tuple metadata differs from the capture plan",
            entry.id
        )));
    }
    Ok(())
}

fn parse_time_bits(value: &str) -> Result<u32> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::new(format!(
            "time_f32_bits '{value}' is not exactly eight hexadecimal digits"
        )));
    }
    u32::from_str_radix(value, 16)
        .map_err(|error| Error::new(format!("invalid time_f32_bits '{value}': {error}")))
}

fn ensure_capture_inputs_clean(repo_root: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "fast3d/src/asm",
            "fast3d/tests/scenes/morphcube.n64",
            "n64-gbi",
        ])
        .output()?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "could not inspect compiler/n64-gbi cleanliness: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let status = String::from_utf8_lossy(&output.stdout);
    if !status.trim().is_empty() {
        return Err(Error::new(format!(
            "capture refuses dirty compiler, source, or n64-gbi inputs:\n{}",
            status.trim_end()
        )));
    }
    Ok(())
}

fn ensure_empty_output(path: &Path) -> Result<()> {
    if path.exists() {
        let mut entries = fs::read_dir(path)?;
        if entries.next().transpose()?.is_some() {
            return Err(Error::new(format!(
                "capture output {} is not empty",
                path.display()
            )));
        }
    } else {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn option<'a>(arguments: &'a [String], name: &str) -> Result<Option<&'a str>> {
    let mut result = None;
    let mut index = 0usize;
    while index < arguments.len() {
        if arguments[index] == name {
            let value = arguments
                .get(index + 1)
                .ok_or_else(|| Error::new(format!("{name} requires a value")))?;
            if result.replace(value.as_str()).is_some() {
                return Err(Error::new(format!("{name} was specified more than once")));
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(result)
}

fn require_known_options(arguments: &[String], allowed: &[&str]) -> Result<()> {
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if !allowed.contains(&argument.as_str()) {
            return Err(Error::new(format!("unexpected argument '{argument}'")));
        }
        if arguments.get(index + 1).is_none() {
            return Err(Error::new(format!("{argument} requires a value")));
        }
        index += 2;
    }
    Ok(())
}

fn resolve(repo_root: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_owned()
    } else {
        repo_root.join(path)
    }
}

fn find_repo_root(start: &Path) -> Result<PathBuf> {
    for candidate in start.ancestors() {
        if candidate.join("fast3d/Cargo.toml").is_file()
            && candidate.join("n64-gbi/Cargo.toml").is_file()
        {
            return Ok(candidate.to_owned());
        }
    }
    Err(Error::new(format!(
        "could not find the fast3d-rs workspace above {}",
        start.display()
    )))
}

fn git_output(repo_root: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(Error::new(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn usage() -> Error {
    Error::new(
        "usage: fast3d-fixture-capture <capture|verify> [options]\n\
         capture [--registry <plan>] --out <directory>\n\
         verify [--registry <plan>] [--root <fixtures>]",
    )
}
