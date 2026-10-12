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
    package_features: HashMap<String, HashSet<String>>,
    root_features: HashSet<String>,
    known_features: Option<HashSet<String>>,
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
            package_features: HashMap::new(),
            root_features: HashSet::new(),
            known_features: None,
            overlays: HashMap::new(),
            visited: HashSet::new(),
            global_offset: 0,
        }
    }

    /// Sets the standard library directory.
    #[must_use]
    pub fn with_std_lib_dir(mut self, std_lib_dir: PathBuf) -> Self {
        self.std_lib_dir = std_lib_dir;
        self
    }

    /// Sets external package root mappings for dependency resolution.
    #[must_use]
    pub fn with_package_roots(mut self, package_roots: HashMap<String, PathBuf>) -> Self {
        self.package_roots = package_roots;
        self
    }

    /// Sets enabled features for the root package.
    #[must_use]
    pub fn with_root_features(mut self, root_features: HashSet<String>) -> Self {
        self.root_features = root_features;
        self
    }

    /// Sets per-package enabled features.
    #[must_use]
    pub fn with_package_features(
        mut self,
        package_features: HashMap<String, HashSet<String>>,
    ) -> Self {
        self.package_features = package_features;
        self
    }

    /// Sets known declared features for typo checking.
    #[must_use]
    pub fn with_known_features(mut self, known_features: Option<HashSet<String>>) -> Self {
        self.known_features = known_features;
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
    parse_file_with_full_options(path, edition, package_roots, overlays, None)
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(
    skip(path, package_roots, root_features, package_features, known_features),
    fields(path = %path.display(), edition),
    level = "info"
)]
pub fn parse_file_with_features(
    path: &Path,
    edition: u16,
    package_roots: HashMap<String, PathBuf>,
    root_features: HashSet<String>,
    package_features: HashMap<String, HashSet<String>>,
    known_features: Option<HashSet<String>>,
) -> Result<Ast> {
    parse_file_with_features_ext(
        path,
        edition,
        package_roots,
        root_features,
        package_features,
        known_features,
        HashMap::new(),
        None,
    )
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(
    skip(
        path,
        package_roots,
        root_features,
        package_features,
        known_features,
        overlays,
        custom_std_dir
    ),
    fields(path = %path.display(), edition),
    level = "info"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "Full parameter import resolver configuration"
)]
pub fn parse_file_with_features_ext(
    path: &Path,
    edition: u16,
    package_roots: HashMap<String, PathBuf>,
    root_features: HashSet<String>,
    package_features: HashMap<String, HashSet<String>>,
    known_features: Option<HashSet<String>>,
    overlays: HashMap<PathBuf, String>,
    custom_std_dir: Option<PathBuf>,
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
        .with_root_features(root_features)
        .with_package_features(package_features)
        .with_known_features(known_features)
        .with_overlays(overlays);
    if let Some(std_dir) = custom_std_dir {
        let resolved_std = if std_dir.join("std").is_dir() {
            std_dir
        } else if (std_dir.join("prelude_2026.jc").is_file()
            || std_dir.join("prelude_2023.jc").is_file())
            && let Some(parent) = std_dir.parent()
        {
            parent.to_path_buf()
        } else {
            std_dir
        };
        resolver = resolver.with_std_lib_dir(resolved_std);
    }
    let mut ast = resolver.resolve(path)?;
    crate::ast::lambda_lift::lift_lambdas(&mut ast);
    Ok(ast)
}

/// # Errors
///
/// Returns an error if the source path cannot be resolved, read, or parsed.
#[instrument(skip(path, package_roots, overlays, custom_std_dir), fields(path = %path.display(), edition), level = "info")]
pub fn parse_file_with_full_options(
    path: &Path,
    edition: u16,
    package_roots: HashMap<String, PathBuf>,
    overlays: HashMap<PathBuf, String>,
    custom_std_dir: Option<PathBuf>,
) -> Result<Ast> {
    parse_file_with_features_ext(
        path,
        edition,
        package_roots,
        HashSet::new(),
        HashMap::new(),
        None,
        overlays,
        custom_std_dir,
    )
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
