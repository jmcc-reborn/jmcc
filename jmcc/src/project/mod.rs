//! Project system for the `JustCode` (`.jc`) language.
//!
//! Any directory containing a `jmcc.toml` manifest is treated as a project.

pub mod dependency;
pub mod error;
pub mod git;
pub mod init;
pub mod lockfile;
pub mod manifest;
pub mod profile;
pub mod resolve;
pub mod workspace;

use std::path::{Path, PathBuf};
use tracing::info;

pub use dependency::{Dependency, DetailedDependency};
pub use error::{ProjectError, Result};
pub use git::GitCache;
pub use init::{NewPackageOptions, PackageType, create_package};
pub use lockfile::{LockedPackage, Lockfile};
pub use manifest::{Inheritable, Manifest, PackageConfig};
pub use profile::ProfileConfig;
pub use resolve::{DependencyResolver, PackageGraph, ResolvedPackage};
pub use workspace::{Workspace, WorkspaceConfig};

/// A `JustCode` project defined by a `jmcc.toml` manifest.
#[derive(Clone, Debug)]
pub struct Project {
    /// Absolute path to the project's `jmcc.toml`.
    pub manifest_path: PathBuf,
    /// Directory containing the `jmcc.toml`.
    pub root_dir: PathBuf,
    /// Parsed and validated manifest.
    pub manifest: Manifest,
}

impl Project {
    /// Opens a project from a path pointing to either `jmcc.toml` or its containing directory.
    ///
    /// # Errors
    /// Returns an error if the manifest is missing, invalid, or cannot be read.
    pub fn open(path_or_dir: &Path) -> Result<Self> {
        let manifest_path = if path_or_dir.is_dir() {
            let candidate = path_or_dir.join("jmcc.toml");
            if !candidate.is_file() {
                return Err(ProjectError::MissingProjectSection { path: candidate });
            }
            candidate
        } else {
            path_or_dir.to_path_buf()
        };

        let canon_manifest = manifest_path.canonicalize()?;
        let root_dir = canon_manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        let mut manifest = Manifest::from_file(&canon_manifest)?;

        // If the project is part of a workspace, resolve any inherited fields
        if let Some(ws_root) = Workspace::find_root(&root_dir)
            && ws_root != canon_manifest
            && let Ok(ws_manifest) = Manifest::from_file(&ws_root)
            && let Some(ws_cfg) = &ws_manifest.workspace
        {
            manifest.resolve_inherited(
                &canon_manifest,
                ws_cfg.package.as_ref(),
                Some(&ws_cfg.dependencies),
            )?;
        }

        Ok(Self {
            manifest_path: canon_manifest,
            root_dir,
            manifest,
        })
    }

    /// Searches upwards from `start_dir` to find an enclosing project with `jmcc.toml`.
    ///
    /// # Errors
    /// Returns an error if a manifest was found but failed to load.
    pub fn find(start_dir: &Path) -> Result<Option<Self>> {
        Manifest::find_manifest(start_dir).map_or(Ok(None), |manifest_path| {
            Self::open(&manifest_path).map(Some)
        })
    }

    /// Package/project name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.manifest
            .package()
            .map(|p| p.name.as_str())
            .unwrap_or("unnamed")
    }

    /// Language edition.
    #[must_use]
    pub fn edition(&self) -> u16 {
        self.manifest.edition()
    }

    /// Determines the entry point source file (`.jc`) for this project.
    ///
    /// # Errors
    /// Returns `ProjectError::EntryPointNotFound` if no entry file could be located.
    pub fn entry_point(&self) -> Result<PathBuf> {
        let pkg = self.manifest.package();

        if let Some(explicit) = pkg.and_then(|p| p.entry.as_ref()) {
            let candidate = self.root_dir.join(explicit);
            if candidate.is_file() {
                return Ok(candidate);
            }
            return Err(ProjectError::EntryPointNotFound {
                project: self.name().to_owned(),
                searched: vec![candidate],
            });
        }

        let candidates = [
            self.root_dir.join("src").join("main.jc"),
            self.root_dir.join("main.jc"),
            self.root_dir.join("src").join("lib.jc"),
            self.root_dir.join("lib.jc"),
        ];

        for path in &candidates {
            if path.is_file() {
                return Ok(path.clone());
            }
        }

        Err(ProjectError::EntryPointNotFound {
            project: self.name().to_owned(),
            searched: candidates.to_vec(),
        })
    }

    /// Returns the library entry point file (`src/lib.jc` or `lib.jc`) if this is a library package.
    #[must_use]
    pub fn lib_entry_point(&self) -> Option<PathBuf> {
        let candidates = [
            self.root_dir.join("src").join("lib.jc"),
            self.root_dir.join("lib.jc"),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Path to `jmcc.lock` in project root directory.
    #[must_use]
    pub fn lockfile_path(&self) -> PathBuf {
        self.root_dir.join("jmcc.lock")
    }

    /// Loads the `jmcc.lock` lockfile if it exists on disk.
    ///
    /// # Errors
    /// Returns an error if the file exists but fails to parse.
    pub fn load_lockfile(&self) -> Result<Option<Lockfile>> {
        let path = self.lockfile_path();
        if path.is_file() {
            Lockfile::from_file(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Recursively resolves all project dependencies into a `PackageGraph`.
    ///
    /// # Errors
    /// Returns an error if any dependency cannot be checked out or contains circular references.
    pub fn resolve_dependencies(&self, include_dev: bool) -> Result<PackageGraph> {
        self.resolve_dependencies_ext(include_dev, false, false)
    }

    /// Recursively resolves all project dependencies with `--locked` and `--offline` options.
    ///
    /// If `locked` is true, verifies that `jmcc.lock` exists and matches the manifest without writing.
    /// If `locked` is false, automatically creates or updates `jmcc.lock` on disk when resolution changes.
    ///
    /// # Errors
    /// Returns an error if dependencies cannot be resolved, network access is required in offline mode,
    /// or if `--locked` was specified and lockfile is missing or out of date.
    pub fn resolve_dependencies_ext(
        &self,
        include_dev: bool,
        locked: bool,
        offline: bool,
    ) -> Result<PackageGraph> {
        let lock_path = self.lockfile_path();
        let existing_lockfile = self.load_lockfile()?;

        if locked {
            let Some(lock) = &existing_lockfile else {
                return Err(ProjectError::LockfileNotFound { path: lock_path });
            };
            lock.validate_against_manifest(&self.manifest, &self.root_dir, include_dev)?;
        }

        let resolver = DependencyResolver::new(Some(&self.root_dir))
            .with_lockfile(existing_lockfile.clone())
            .with_offline(offline);

        let graph = resolver.resolve_all(&self.manifest, &self.root_dir, include_dev)?;
        let new_lock = Lockfile::generate(&self.manifest, &self.root_dir, &graph);

        if locked {
            if let Some(existing) = &existing_lockfile
                && existing != &new_lock
            {
                return Err(ProjectError::LockfileOutOfDate {
                    path: lock_path,
                    reason: "Dependencies in manifest differ from jmcc.lock".to_owned(),
                });
            }
        } else if existing_lockfile.as_ref() != Some(&new_lock) {
            new_lock.write_to_file(&lock_path)?;
        }

        Ok(graph)
    }

    /// Updates dependencies by pulling fresh commits from git and re-generating `jmcc.lock`.
    ///
    /// # Errors
    /// Returns error if git fetch/pull fails or dependencies fail to resolve.
    pub fn update_dependencies(&self, target_package: Option<&str>) -> Result<()> {
        let lock_path = self.lockfile_path();
        let existing_lockfile = self.load_lockfile()?;

        // If target_package is specified, only clear lock for that package
        let lockfile_for_resolve = target_package.and_then(|target| {
            existing_lockfile.map(|mut l| {
                l.packages.retain(|p| p.name != target);
                l
            })
        });

        let resolver =
            DependencyResolver::new(Some(&self.root_dir)).with_lockfile(lockfile_for_resolve);

        let graph = resolver.resolve_all(&self.manifest, &self.root_dir, true)?;
        let new_lock = Lockfile::generate(&self.manifest, &self.root_dir, &graph);
        new_lock.write_to_file(&lock_path)?;
        info!("Updated dependencies and wrote '{}'", lock_path.display());

        Ok(())
    }

    /// Returns the effective `ProfileConfig` for a named profile (e.g. "dev" or "release").
    #[must_use]
    pub fn profile_options(&self, profile_name: &str) -> ProfileConfig {
        let mut base = if profile_name.eq_ignore_ascii_case("release") {
            ProfileConfig::default_release()
        } else {
            ProfileConfig::default_dev()
        };

        if let Some(custom) = self.manifest.profile.get(profile_name) {
            base.merge(custom);
        }

        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parse_basic() {
        let toml_str = r#"
            [project]
            name = "hello_world"
            version = "0.2.0"
            edition = 2026

            [dependencies]
            math = { path = "../math" }

            [profile.release]
            opt_level = 3
        "#;
        let manifest = Manifest::from_str(toml_str, Path::new("jmcc.toml")).unwrap();
        assert_eq!(
            manifest.name(Path::new("jmcc.toml")).unwrap(),
            "hello_world"
        );
        assert_eq!(manifest.version(), "0.2.0");
        assert_eq!(manifest.edition(), 2026);
        assert!(manifest.dependencies.contains_key("math"));
        let rel_profile = &manifest.profile["release"];
        assert_eq!(rel_profile.opt_level, Some(3));
    }

    #[test]
    fn test_manifest_git_dependency() {
        let toml_str = r#"
            [package]
            name = "my_app"
            version = "1.0.0"

            [dependencies]
            net = { git = "https://github.com/example/net.git", branch = "main" }
        "#;
        let manifest = Manifest::from_str(toml_str, Path::new("jmcc.toml")).unwrap();
        let net_dep = &manifest.dependencies["net"];
        assert_eq!(net_dep.git(), Some("https://github.com/example/net.git"));
    }

    #[test]
    fn test_workspace_inheritance() {
        let root_ws_str = r#"
            [workspace]
            members = ["crates/*"]

            [workspace.package]
            version = "2.5.0"
            edition = 2026
            authors = ["Alice"]

            [workspace.dependencies]
            common = { path = "../common" }
        "#;
        let ws_manifest = Manifest::from_str(root_ws_str, Path::new("jmcc.toml")).unwrap();
        let ws_cfg = ws_manifest.workspace.unwrap();

        let member_str = r#"
            [package]
            name = "sub_crate"
            version.workspace = true
            edition.workspace = true

            [dependencies]
            common.workspace = true
        "#;
        let mut member_manifest =
            Manifest::from_str(member_str, Path::new("crates/sub_crate/jmcc.toml")).unwrap();
        member_manifest
            .resolve_inherited(
                Path::new("crates/sub_crate/jmcc.toml"),
                ws_cfg.package.as_ref(),
                Some(&ws_cfg.dependencies),
            )
            .unwrap();

        assert_eq!(member_manifest.version(), "2.5.0");
        assert_eq!(member_manifest.edition(), 2026);
        let common = &member_manifest.dependencies["common"];
        assert_eq!(common.path(), Some(Path::new("../common")));
    }
}
