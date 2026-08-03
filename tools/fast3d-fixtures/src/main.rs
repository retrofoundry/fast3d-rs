use fast3d_fixture_tool::{
    build_literal_fixture, import_captures, read_index, verify_repository, write_fixture_files,
    write_registry, Error, Result,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_FIXTURE_ROOT: &str = "fast3d/tests/fixtures";

fn main() {
    if let Err(error) = run() {
        eprintln!("fast3d-fixtures: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage());
    };
    let repo_root = find_repo_root(&env::current_dir()?)?;
    match command {
        "verify" => {
            require_known_options(&arguments[1..], &["--root"])?;
            let fixture_root = resolve(
                &repo_root,
                option(&arguments[1..], "--root")?.unwrap_or(DEFAULT_FIXTURE_ROOT),
            );
            let report = verify_repository(&repo_root, &fixture_root)?;
            println!(
                "verified {} fixtures ({} bytes of manifest/RDRAM/dump data)",
                report.fixture_count, report.fixture_bytes
            );
        }
        "registry" => {
            require_known_options(&arguments[1..], &["--root"])?;
            let fixture_root = resolve(
                &repo_root,
                option(&arguments[1..], "--root")?.unwrap_or(DEFAULT_FIXTURE_ROOT),
            );
            write_registry(&repo_root, &fixture_root)?;
            println!("wrote checked fixture registry");
        }
        "import" => {
            require_known_options(&arguments[1..], &["--from", "--root"])?;
            let from = option(&arguments[1..], "--from")?
                .ok_or_else(|| Error::new("import requires --from <capture-directory>"))?;
            let fixture_root = resolve(
                &repo_root,
                option(&arguments[1..], "--root")?.unwrap_or(DEFAULT_FIXTURE_ROOT),
            );
            let imported = import_captures(&resolve(&repo_root, from), &fixture_root)?;
            write_registry(&repo_root, &fixture_root)?;
            println!("imported {imported} captures and regenerated the registry");
        }
        "build" => {
            let Some(id) = arguments.get(1) else {
                return Err(Error::new("build requires a literal fixture ID"));
            };
            require_known_options(&arguments[2..], &["--root"])?;
            let fixture_root = resolve(
                &repo_root,
                option(&arguments[2..], "--root")?.unwrap_or(DEFAULT_FIXTURE_ROOT),
            );
            let index = read_index(&fixture_root)?;
            if !index.fixtures.iter().any(|entry| entry.id == *id) {
                return Err(Error::new(format!(
                    "literal fixture '{id}' is not registered in {}; add its [[fixture]] entry before building",
                    fixture_root.join("index.toml").display()
                )));
            }
            let revision = git_output(&repo_root, &["rev-parse", "HEAD"])?;
            let files = build_literal_fixture(id, &revision)?;
            write_fixture_files(&fixture_root, &files)?;
            write_registry(&repo_root, &fixture_root)?;
            println!("built literal fixture '{id}' and regenerated the registry");
        }
        _ => return Err(usage()),
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
        if candidate.join("Cargo.toml").is_file()
            && candidate.join("fast3d/Cargo.toml").is_file()
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
        "usage: fast3d-fixtures <verify|registry|import|build> [options]\n\
         verify [--root <fixtures>]\n\
         registry [--root <fixtures>]\n\
         import --from <captures> [--root <fixtures>]\n\
         build <literal-id> [--root <fixtures>]",
    )
}
