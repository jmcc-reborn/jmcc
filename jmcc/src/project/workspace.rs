//! Workspace configuration and members handling.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::dependency::Dependency;
use super::error::Result;

/// Workspace configuration defined in `[workspace]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceConfig {
    /// List of member directories or glob-like relative paths (e.g. `["crates/*", "libs/foo"]`).
    #[serde(default)]
    pub members: Vec<String>,
    /// List of directories or patterns to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Inheritable package metadata across workspace members (`[workspace.package]`).
    pub package: Option<WorkspacePackageConfig>,
    /// Inheritable dependency definitions across workspace members (`[workspace.dependencies]`).
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// Common package metadata in `[workspace.package]` inherited via `*.workspace = true`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspacePackageConfig {
    pub version: Option<String>,
    pub edition: Option<u16>,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub repository: Option<String>,
    pub locale: Option<String>,
    #[serde(alias = "disable_action_limit")]
    pub disable_action_limit: Option<bool>,
    #[serde(
        alias = "upload_target",
        alias = "upload_method",
        alias = "upload-method"
    )]
    pub upload_target: Option<crate::upload::UploadTarget>,
    #[serde(alias = "webhook_url", alias = "webhook", alias = "upload_webhook")]
    pub webhook_url: Option<String>,
}

/// Resolved workspace containing root location and discovered member manifests.
#[derive(Clone, Debug)]
pub struct Workspace {
    pub root_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub config: WorkspaceConfig,
}

impl Workspace {
    /// Attempts to locate the root workspace manifest starting from `from` directory and walking upwards.
    #[must_use]
    pub fn find_root(from: &Path) -> Option<PathBuf> {
        let mut curr = if from.is_file() {
            from.parent().unwrap_or(from).to_path_buf()
        } else {
            from.to_path_buf()
        };

        loop {
            let candidate = curr.join("jmcc.toml");
            if candidate.is_file()
                && let Ok(content) = std::fs::read_to_string(&candidate)
                && content.contains("[workspace]")
            {
                return Some(candidate);
            }
            if !curr.pop() {
                break;
            }
        }
        None
    }

    /// Discovers all member directories for this workspace.
    ///
    /// # Errors
    /// Returns an error if member paths cannot be read.
    pub fn find_member_manifests(&self) -> Result<Vec<PathBuf>> {
        let mut results = Vec::new();

        for member_pat in &self.config.members {
            if member_pat.ends_with("/*") {
                let base_rel = &member_pat[..member_pat.len() - 2];
                let base_dir = self.root_dir.join(base_rel);
                if base_dir.is_dir() {
                    let read_dir = std::fs::read_dir(&base_dir)?;
                    for entry in read_dir {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_dir() {
                            let manifest = path.join("jmcc.toml");
                            if manifest.is_file() && !self.is_excluded(&path) {
                                results.push(manifest);
                            }
                        }
                    }
                }
            } else {
                let candidate = self.root_dir.join(member_pat);
                let manifest = if candidate.is_dir() {
                    candidate.join("jmcc.toml")
                } else {
                    candidate
                };
                if manifest.is_file() && !self.is_excluded(manifest.parent().unwrap_or(&manifest)) {
                    results.push(manifest);
                }
            }
        }

        results.sort();
        results.dedup();
        Ok(results)
    }

    fn is_excluded(&self, path: &Path) -> bool {
        let rel = path.strip_prefix(&self.root_dir).unwrap_or(path);
        let rel_str = rel.to_string_lossy();
        self.config
            .exclude
            .iter()
            .any(|ex| rel_str == *ex || rel_str.starts_with(format!("{ex}/").as_str()))
    }
}
