//! Import resolution: `ImportResolver` type, `parse_file` entry point, and related plumbing.
//!
//! File and dependency graph traversal is in `resolve`, AST merging in `merge`,
//! export filtering in `export`, and module name mangling in `mangle`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::{Level, debug, info, instrument, span};

use crate::ast::*;
use crate::error::Result;

mod export;
mod mangle;
mod merge;
mod resolve;

use resolve::find_std_lib_dir;

const PRELUDE_NAMES: [&str; 3] = ["prelude", "prelude_2023", "prelude_2026"];

/// Resolves and merges a source file's import graph.
pub struct ImportResolver {
    edition: u16,
    root_dir: PathBuf,
    std_lib_dir: PathBuf,
    package_roots: HashMap<String, PathBuf>,
    overlays: HashMap<PathBuf, String>,
    visited: HashSet<PathBuf>,
    global_offset: usize,
}

impl ImportResolver {
    #[must_use]
    pub fn new(edition: u16, root_dir: PathBuf) -> Self {
        let _span =
            span!(Level::DEBUG, "import_resolver_init", edition, root_dir = %root_dir.display())
                .entered();
        let std_lib_dir = find_std_lib_dir(&root_dir);
        debug!(
            edition,
            root_dir = %root_dir.display(),
            std_lib_dir = %std_lib_dir.display(),
            "ImportResolver initialized"
        );
        Self {
            edition,
            std_lib_dir,
            root_dir,
            package_roots: HashMap::new(),
            overlays: HashMap::new(),
            visited: HashSet::new(),
            global_offset: 0,
        }
    }

    /// Sets external package root mappings for dependency resolution.
    #[must_use]
    pub fn with_package_roots(mut self, package_roots: HashMap<String, PathBuf>) -> Self {
        self.package_roots = package_roots;
        self
    }

    /// Sets in-memory file overlays for resolving unsaved editor buffers.
    #[must_use]
    pub fn with_overlays(mut self, overlays: HashMap<PathBuf, String>) -> Self {
        self.overlays = overlays;
        self
    }

    /// # Errors
    ///
    /// Returns an error if the source path cannot be resolved, read, or parsed.
    #[instrument(skip(self), fields(path = %path.display()), level = "info")]
    pub fn resolve(&mut self, path: &Path) -> Result<Ast> {
        info!("Starting top-level resolve");
        self.parse_file_recursive(path)
    }
}

impl Default for ImportResolver {
    fn default() -> Self {
        Self::new(2026, PathBuf::from("."))
    }
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(skip(path), fields(path = %path.display(), edition), level = "info")]
pub fn parse_file(path: &Path, edition: u16) -> Result<Ast> {
    parse_file_with_packages(path, edition, HashMap::new())
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(skip(path, package_roots), fields(path = %path.display(), edition), level = "info")]
pub fn parse_file_with_packages(
    path: &Path,
    edition: u16,
    package_roots: HashMap<String, PathBuf>,
) -> Result<Ast> {
    parse_file_with_options(path, edition, package_roots, HashMap::new())
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(skip(path, overlays), fields(path = %path.display(), edition), level = "info")]
pub fn parse_file_with_overlays(
    path: &Path,
    edition: u16,
    overlays: HashMap<PathBuf, String>,
) -> Result<Ast> {
    parse_file_with_options(path, edition, HashMap::new(), overlays)
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(skip(path, package_roots, overlays), fields(path = %path.display(), edition), level = "info")]
pub fn parse_file_with_options(
    path: &Path,
    edition: u16,
    package_roots: HashMap<String, PathBuf>,
    overlays: HashMap<PathBuf, String>,
) -> Result<Ast> {
    let root_dir = path
        .parent()
        .and_then(|p| {
            p.ancestors().find(|a| {
                a.join("jmcc.toml").is_file()
                    || a.join("jmc.toml").is_file()
                    || a.join("std").is_dir()
                    || a.join("jmcc").join("std").is_dir()
            })
        })
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    debug!(root_dir = %root_dir.display(), "Current directory resolved");
    let mut resolver = ImportResolver::new(edition, root_dir)
        .with_package_roots(package_roots)
        .with_overlays(overlays);
    let mut ast = resolver.resolve(path)?;
    crate::ast::lambda_lift::lift_lambdas(&mut ast);
    Ok(ast)
}

/// `std` library directory (`<...>/jmcc/std`) for the current working directory.
///
/// Lets a `std` source be told apart from a user file, e.g. `std` may use the raw actions
/// that implement operators. Resolves via [`find_std_lib_dir`] to avoid a second copy of it.
#[must_use]
pub(crate) fn std_lib_root() -> PathBuf {
    std::env::current_dir().map_or_else(
        |_| PathBuf::from("std"),
        |dir| find_std_lib_dir(&dir).join("std"),
    )
}
