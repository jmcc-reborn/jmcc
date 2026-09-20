//! Package creation (`jmcc new`).

use std::fs;
use std::path::Path;
use std::process::Command;

use super::error::{ProjectError, Result};
use crate::i18n::{Lang, current_lang};

/// Type of package to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackageType {
    /// Application with `src/main.jc`.
    #[default]
    Binary,
    /// Library with `src/lib.jc`.
    Library,
}

/// Options for `jmcc new`.
#[derive(Debug, Clone)]
pub struct NewPackageOptions {
    /// Explicit package name (defaults to folder name).
    pub name: Option<String>,
    /// Package type: binary or library.
    pub package_type: PackageType,
    /// Language edition: 2026 (default) or 2023.
    pub edition: u16,
    /// Version control system to initialize ("git" or "none").
    pub vcs: Option<String>,
}

impl Default for NewPackageOptions {
    fn default() -> Self {
        Self {
            name: None,
            package_type: PackageType::Binary,
            edition: 2026,
            vcs: Some("git".to_owned()),
        }
    }
}

/// Validates package name characters.
fn validate_package_name(name: &str, path: &Path) -> Result<()> {
    if name.trim().is_empty() {
        return Err(ProjectError::InvalidProjectName {
            name: name.to_owned(),
            path: path.to_path_buf(),
            reason: "Package name cannot be empty".to_owned(),
        });
    }
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err(ProjectError::InvalidProjectName {
            name: name.to_owned(),
            path: path.to_path_buf(),
            reason: "Package name must contain only ASCII alphanumeric characters, '_' or '-'"
                .to_owned(),
        });
    }
    Ok(())
}

/// Creates a new `JustCode` package at `path`.
///
/// # Errors
/// Returns error if path already exists and is not empty, or writing fails.
fn check_destination(path: &Path) -> Result<()> {
    if path.exists() {
        if path.is_dir() {
            let is_empty = fs::read_dir(path)?.next().is_none();
            if !is_empty {
                return Err(ProjectError::Generic(format!(
                    "destination `{}` already exists and is not an empty directory",
                    path.display()
                )));
            }
        } else {
            return Err(ProjectError::Generic(format!(
                "destination `{}` already exists and is a file",
                path.display()
            )));
        }
    } else {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

fn init_vcs(path: &Path, vcs: Option<&str>) -> Result<()> {
    let vcs = vcs.unwrap_or("git");
    if vcs.eq_ignore_ascii_case("git") {
        let gitignore_content = r#"/target/
*.ast
*.hir
*.mir
*.json
"#;
        let gitignore_path = path.join(".gitignore");
        if !gitignore_path.exists() {
            fs::write(gitignore_path, gitignore_content)?;
        }
        let git_result = Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if let Err(err) = git_result {
            tracing::debug!("failed to run git init: {err}");
        }
    }
    Ok(())
}

#[expect(clippy::print_stdout, reason = "user-facing package creation feedback")]
fn print_created_message(pkg_name: &str, package_type: PackageType) {
    match current_lang() {
        Lang::Ru => match package_type {
            PackageType::Binary => {
                println!("     \x1b[32mСоздан\x1b[0m бинарный (application) `{pkg_name}` пакет");
            }
            PackageType::Library => {
                println!("     \x1b[32mСоздан\x1b[0m библиотечный (library) `{pkg_name}` пакет");
            }
        },
        Lang::En => match package_type {
            PackageType::Binary => {
                println!("     \x1b[32mCreated\x1b[0m binary (application) `{pkg_name}` package");
            }
            PackageType::Library => {
                println!("     \x1b[32mCreated\x1b[0m library `{pkg_name}` package");
            }
        },
    }
}

/// Creates a new `JustCode` package at `path`.
///
/// # Errors
/// Returns error if path already exists and is not empty, or writing fails.
pub fn create_package(path: &Path, options: &NewPackageOptions) -> Result<()> {
    let pkg_name = options.name.as_ref().map_or_else(
        || {
            path.file_name()
                .and_then(|f| f.to_str())
                .map(|s| s.replace(' ', "_"))
                .unwrap_or_else(|| "my_project".to_owned())
        },
        Clone::clone,
    );
    validate_package_name(&pkg_name, path)?;

    check_destination(path)?;

    let src_dir = path.join("src");
    fs::create_dir_all(&src_dir)?;

    let manifest_content = format!(
        r#"[package]
name = "{pkg_name}"
version = "0.1.0"
edition = {edition}

[dependencies]
"#,
        edition = options.edition
    );
    fs::write(path.join("jmcc.toml"), manifest_content)?;

    match options.package_type {
        PackageType::Binary => {
            let main_jc = r#"event<world_start> {
    player::message("Hello, world!");
}
"#;
            fs::write(src_dir.join("main.jc"), main_jc)?;
        }
        PackageType::Library => {
            let lib_jc = r#"export function add(a: number, b: number) -> number {
    return a + b;
}

@test
function test_add() {
    assert(add(2, 2) == 4);
}
"#;
            fs::write(src_dir.join("lib.jc"), lib_jc)?;
        }
    }

    init_vcs(path, options.vcs.as_deref())?;
    print_created_message(&pkg_name, options.package_type);

    Ok(())
}
