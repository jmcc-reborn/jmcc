//! Dependency specifications in `jmcc.toml`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

use super::error::{ProjectError, Result};

/// A dependency specification in `[dependencies]` or `[dev-dependencies]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// Simple version string specification, e.g. `foo = "0.1.0"`.
    Simple(String),
    /// Detailed table specification, e.g. `foo = { path = "../foo", version = "0.1" }`.
    Detailed(Box<DetailedDependency>),
}

/// Detailed configuration for a dependency.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct DetailedDependency {
    /// Semantic version requirement (e.g. `"^0.1.0"`).
    pub version: Option<String>,
    /// Filesystem path to the dependency directory.
    pub path: Option<PathBuf>,
    /// Git repository URL.
    pub git: Option<String>,
    /// Git branch to track.
    pub branch: Option<String>,
    /// Git tag to check out.
    pub tag: Option<String>,
    /// Explicit Git commit revision (SHA).
    pub rev: Option<String>,
    /// Package name alias (e.g. `foo = { git = "...", package = "bar" }`).
    pub package: Option<String>,
    /// Inherit definition from `[workspace.dependencies]`.
    pub workspace: Option<bool>,
}

impl Dependency {
    /// Creates a path-based dependency.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::Detailed(Box::new(DetailedDependency {
            path: Some(path.into()),
            ..Default::default()
        }))
    }

    /// Creates a git-based dependency.
    #[must_use]
    pub fn from_git(git: impl Into<String>, branch: Option<String>) -> Self {
        Self::Detailed(Box::new(DetailedDependency {
            git: Some(git.into()),
            branch,
            ..Default::default()
        }))
    }

    /// Returns the detailed dependency struct, or default if simple string.
    #[must_use]
    pub fn detailed(&self) -> DetailedDependency {
        match self {
            Self::Simple(version) => DetailedDependency {
                version: Some(version.clone()),
                ..Default::default()
            },
            Self::Detailed(detailed) => *detailed.clone(),
        }
    }

    /// Returns the path if specified.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Simple(_) => None,
            Self::Detailed(d) => d.path.as_deref(),
        }
    }

    /// Returns the git URL if specified.
    #[must_use]
    pub fn git(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::Detailed(d) => d.git.as_deref(),
        }
    }

    /// Returns whether this dependency inherits from workspace.
    #[must_use]
    pub fn is_workspace(&self) -> bool {
        match self {
            Self::Simple(_) => false,
            Self::Detailed(d) => d.workspace.unwrap_or(false),
        }
    }

    /// Validates that the dependency has a valid source given that there is no central registry.
    ///
    /// # Errors
    /// Returns an error if neither `path` nor `git` is provided (unless `workspace = true`).
    pub fn validate(&self, dep_name: &str) -> Result<()> {
        match self {
            Self::Simple(ver) => Err(ProjectError::Generic(format!(
                "Dependency '{dep_name}' specifies only version \"{ver}\", but no package registry exists yet. Please provide a `path` or `git` source."
            ))),
            Self::Detailed(d) => {
                if d.workspace.unwrap_or(false) {
                    return Ok(());
                }
                if d.path.is_none() && d.git.is_none() {
                    return Err(ProjectError::Generic(format!(
                        "Dependency '{dep_name}' must specify either `path` or `git` (no central registry available)."
                    )));
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simple(v) => write!(f, "\"{v}\""),
            Self::Detailed(d) => {
                let mut parts = Vec::new();
                if let Some(p) = &d.path {
                    parts.push(format!("path = \"{}\"", p.display()));
                }
                if let Some(g) = &d.git {
                    parts.push(format!("git = \"{g}\""));
                }
                if let Some(b) = &d.branch {
                    parts.push(format!("branch = \"{b}\""));
                }
                if let Some(t) = &d.tag {
                    parts.push(format!("tag = \"{t}\""));
                }
                if let Some(r) = &d.rev {
                    parts.push(format!("rev = \"{r}\""));
                }
                if let Some(v) = &d.version {
                    parts.push(format!("version = \"{v}\""));
                }
                write!(f, "{{ {} }}", parts.join(", "))
            }
        }
    }
}
