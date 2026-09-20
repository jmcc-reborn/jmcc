//! `jmcc.toml` manifest parser and data structures.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::dependency::Dependency;
use super::error::{ProjectError, Result};
use super::profile::ProfileConfig;
use super::workspace::{WorkspaceConfig, WorkspacePackageConfig};

/// Field that can either provide a concrete value or inherit from workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Inheritable<T> {
    Value(T),
    Inherit { workspace: bool },
}

impl<T: Clone> Inheritable<T> {
    #[must_use]
    pub fn value(&self) -> Option<T> {
        match self {
            Self::Value(v) => Some(v.clone()),
            Self::Inherit { .. } => None,
        }
    }

    #[must_use]
    pub const fn is_inherited(&self) -> bool {
        matches!(self, Self::Inherit { workspace: true })
    }
}

/// Package configuration defined in `[project]` or `[package]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageConfig {
    /// Package/project name.
    pub name: String,
    /// `SemVer` version string (e.g. `"0.1.0"`).
    pub version: Option<Inheritable<String>>,
    /// Language edition (default: `2026`, or `2023`).
    pub edition: Option<Inheritable<u16>>,
    /// Explicit main source entry point (e.g. `"src/main.jc"`).
    pub entry: Option<PathBuf>,
    /// Brief description of the package.
    pub description: Option<Inheritable<String>>,
    /// List of package authors.
    pub authors: Option<Inheritable<Vec<String>>>,
    /// SPDX license identifier.
    pub license: Option<Inheritable<String>>,
    /// Path to README file.
    pub readme: Option<Inheritable<String>>,
    /// URL to repository.
    pub repository: Option<Inheritable<String>>,
    /// Locale/language for compiler diagnostics ("ru" or "en").
    pub locale: Option<Inheritable<String>>,
    /// Disable action limit per line.
    #[serde(alias = "disable_action_limit")]
    pub disable_action_limit: Option<Inheritable<bool>>,
    /// Target service for module uploading ("official" or "webhook").
    #[serde(
        alias = "upload_target",
        alias = "upload_method",
        alias = "upload-method"
    )]
    pub upload_target: Option<Inheritable<crate::upload::UploadTarget>>,
    /// Custom webhook URL for unofficial upload target.
    #[serde(alias = "webhook_url", alias = "webhook", alias = "upload_webhook")]
    pub webhook_url: Option<Inheritable<String>>,
}

/// Configuration for module uploading (`[upload]`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct UploadConfig {
    /// Whether upload is enabled by default for this project.
    #[serde(alias = "enabled", alias = "auto_upload", alias = "upload")]
    pub enabled: Option<bool>,
    /// Target service: official or webhook.
    #[serde(
        alias = "upload_target",
        alias = "upload_method",
        alias = "upload-method",
        alias = "target",
        alias = "method"
    )]
    pub target: Option<crate::upload::UploadTarget>,
    /// Custom webhook URL for Discord uploading.
    #[serde(alias = "webhook_url", alias = "webhook")]
    pub webhook_url: Option<String>,
}

/// The top-level `jmcc.toml` manifest structure.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    /// Primary package definition (`[package]`).
    pub package: Option<PackageConfig>,
    /// Alias for package definition (`[project]`).
    pub project: Option<PackageConfig>,
    /// Workspace definition (`[workspace]`).
    pub workspace: Option<WorkspaceConfig>,
    /// Upload settings (`[upload]`).
    #[serde(default)]
    pub upload: Option<UploadConfig>,
    /// Dependencies table (`[dependencies]`).
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    /// Development dependencies (`[dev-dependencies]`).
    #[serde(default)]
    pub dev_dependencies: BTreeMap<String, Dependency>,
    /// Alternate spelling for development dependencies (`[dev_dependencies]`).
    #[serde(default, rename = "dev_dependencies")]
    pub dev_dependencies_snake: BTreeMap<String, Dependency>,
    /// Build profiles (`[profile.<name>]`).
    #[serde(default)]
    pub profile: BTreeMap<String, ProfileConfig>,
}

impl Manifest {
    /// Parses a manifest from a TOML string.
    ///
    /// # Errors
    /// Returns an error if the string is not valid TOML.
    pub fn from_str(toml_str: &str, path: &Path) -> Result<Self> {
        let manifest: Self =
            toml::from_str(toml_str).map_err(|source| ProjectError::ManifestParse {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(manifest)
    }

    /// Loads and parses a manifest from a file.
    ///
    /// # Errors
    /// Returns an error if reading or parsing the file fails.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let manifest = Self::from_str(&content, path)?;
        manifest.validate(path)?;
        Ok(manifest)
    }

    /// Returns the active package definition (`[package]` or `[project]`).
    #[must_use]
    pub fn package(&self) -> Option<&PackageConfig> {
        self.package.as_ref().or(self.project.as_ref())
    }

    /// Returns the mutable active package definition.
    pub const fn package_mut(&mut self) -> Option<&mut PackageConfig> {
        if self.package.is_some() {
            self.package.as_mut()
        } else {
            self.project.as_mut()
        }
    }

    /// Returns the package name, or returns an error if neither `[package]` nor `[project]` is present.
    ///
    /// # Errors
    /// Returns `ProjectError::MissingProjectSection` if no package table is present.
    pub fn name(&self, path: &Path) -> Result<&str> {
        self.package()
            .map(|p| p.name.as_str())
            .ok_or_else(|| ProjectError::MissingProjectSection {
                path: path.to_path_buf(),
            })
    }

    /// Returns the effective language edition (defaults to 2026).
    #[must_use]
    pub fn edition(&self) -> u16 {
        self.package()
            .and_then(|p| p.edition.as_ref())
            .and_then(Inheritable::value)
            .unwrap_or(2026)
    }

    /// Returns the effective package version string (defaults to "0.1.0").
    #[must_use]
    pub fn version(&self) -> String {
        self.package()
            .and_then(|p| p.version.as_ref())
            .and_then(Inheritable::value)
            .unwrap_or_else(|| "0.1.0".to_owned())
    }

    /// Returns the configured package locale if any.
    #[must_use]
    pub fn locale(&self) -> Option<String> {
        self.package()
            .and_then(|p| p.locale.as_ref())
            .and_then(Inheritable::value)
    }

    /// Returns whether action line limit splitting is disabled (default false).
    #[must_use]
    pub fn disable_action_limit(&self) -> bool {
        self.package()
            .and_then(|p| p.disable_action_limit.as_ref())
            .and_then(Inheritable::value)
            .unwrap_or(false)
    }

    /// Returns the target service for uploading if configured.
    #[must_use]
    pub fn upload_target(&self) -> Option<crate::upload::UploadTarget> {
        self.upload.as_ref().and_then(|u| u.target).or_else(|| {
            self.package()
                .and_then(|p| p.upload_target.as_ref())
                .and_then(Inheritable::value)
        })
    }

    /// Returns whether upload is enabled by default in manifest.
    #[must_use]
    pub fn auto_upload(&self) -> bool {
        self.upload
            .as_ref()
            .and_then(|u| u.enabled)
            .unwrap_or(false)
    }

    /// Returns custom webhook URL for uploading if configured.
    #[must_use]
    pub fn webhook_url(&self) -> Option<String> {
        self.upload
            .as_ref()
            .and_then(|u| u.webhook_url.clone())
            .or_else(|| {
                self.package()
                    .and_then(|p| p.webhook_url.as_ref())
                    .and_then(Inheritable::value)
            })
    }

    /// Returns combined dependencies (including `dev-dependencies` if requested).
    #[must_use]
    pub fn all_dependencies(&self, include_dev: bool) -> BTreeMap<String, Dependency> {
        let mut deps = self.dependencies.clone();
        if include_dev {
            for (k, v) in &self.dev_dependencies {
                deps.entry(k.clone()).or_insert_with(|| v.clone());
            }
            for (k, v) in &self.dev_dependencies_snake {
                deps.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        deps
    }

    /// Validates basic semantic rules of the manifest.
    ///
    /// # Errors
    /// Returns error if validation fails.
    pub fn validate(&self, path: &Path) -> Result<()> {
        if self.package().is_none() && self.workspace.is_none() {
            return Err(ProjectError::MissingProjectSection {
                path: path.to_path_buf(),
            });
        }

        if let Some(pkg) = self.package() {
            if pkg.name.trim().is_empty() {
                return Err(ProjectError::InvalidProjectName {
                    name: pkg.name.clone(),
                    path: path.to_path_buf(),
                    reason: "Project name cannot be empty".to_owned(),
                });
            }
            let valid = pkg
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !valid {
                return Err(ProjectError::InvalidProjectName {
                    name: pkg.name.clone(),
                    path: path.to_path_buf(),
                    reason:
                        "Project name must contain only ASCII alphanumeric characters, '_' or '-'"
                            .to_owned(),
                });
            }
        }

        for (dep_name, dep) in &self.dependencies {
            dep.validate(dep_name)?;
        }

        Ok(())
    }

    /// Resolves inherited fields from a parent workspace.
    ///
    /// # Errors
    /// Returns error if an inherited field is missing in workspace config.
    pub fn resolve_inherited(
        &mut self,
        path: &Path,
        ws_pkg: Option<&WorkspacePackageConfig>,
        ws_deps: Option<&BTreeMap<String, Dependency>>,
    ) -> Result<()> {
        if let Some(pkg) = self.package_mut() {
            if matches!(&pkg.version, Some(Inheritable::Inherit { workspace: true })) {
                let val = ws_pkg.and_then(|w| w.version.clone()).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "version".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.version = Some(Inheritable::Value(val));
            }
            if matches!(&pkg.edition, Some(Inheritable::Inherit { workspace: true })) {
                let val = ws_pkg.and_then(|w| w.edition).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "edition".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.edition = Some(Inheritable::Value(val));
            }
            if matches!(&pkg.authors, Some(Inheritable::Inherit { workspace: true })) {
                let val = ws_pkg.and_then(|w| w.authors.clone()).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "authors".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.authors = Some(Inheritable::Value(val));
            }
            if matches!(&pkg.license, Some(Inheritable::Inherit { workspace: true })) {
                let val = ws_pkg.and_then(|w| w.license.clone()).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "license".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.license = Some(Inheritable::Value(val));
            }
            if matches!(&pkg.locale, Some(Inheritable::Inherit { workspace: true })) {
                let val = ws_pkg.and_then(|w| w.locale.clone()).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "locale".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.locale = Some(Inheritable::Value(val));
            }
            if matches!(
                &pkg.disable_action_limit,
                Some(Inheritable::Inherit { workspace: true })
            ) {
                let val = ws_pkg.and_then(|w| w.disable_action_limit).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "disable_action_limit".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.disable_action_limit = Some(Inheritable::Value(val));
            }
            if matches!(
                &pkg.upload_target,
                Some(Inheritable::Inherit { workspace: true })
            ) {
                let val = ws_pkg.and_then(|w| w.upload_target).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "upload_target".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.upload_target = Some(Inheritable::Value(val));
            }
            if matches!(
                &pkg.webhook_url,
                Some(Inheritable::Inherit { workspace: true })
            ) {
                let val = ws_pkg.and_then(|w| w.webhook_url.clone()).ok_or_else(|| {
                    ProjectError::WorkspaceInheritanceMissing {
                        field: "webhook_url".to_owned(),
                        path: path.to_path_buf(),
                    }
                })?;
                pkg.webhook_url = Some(Inheritable::Value(val));
            }
        }

        // Inherit dependencies
        for (dep_name, dep) in &mut self.dependencies {
            if dep.is_workspace() {
                if let Some(ws_dep) = ws_deps.and_then(|m| m.get(dep_name)) {
                    *dep = ws_dep.clone();
                } else {
                    return Err(ProjectError::WorkspaceInheritanceMissing {
                        field: format!("dependencies.{dep_name}"),
                        path: path.to_path_buf(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Looks for a `jmcc.toml` starting in `from` and walking up to filesystem root.
    #[must_use]
    pub fn find_manifest(from: &Path) -> Option<PathBuf> {
        let mut curr = if from.is_file() {
            from.parent().unwrap_or(from).to_path_buf()
        } else {
            from.to_path_buf()
        };

        loop {
            let candidate = curr.join("jmcc.toml");
            if candidate.is_file() {
                return Some(candidate);
            }
            if !curr.pop() {
                break;
            }
        }
        None
    }
}
