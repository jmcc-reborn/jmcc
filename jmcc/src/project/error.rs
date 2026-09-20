//! Error types for project and manifest handling.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when loading, resolving, or building a `.jc` project.
#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse manifest '{path}': {source}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error(
        "Manifest '{path}' must contain either a [project] or [package] section, or a [workspace] section"
    )]
    MissingProjectSection { path: PathBuf },

    #[error("Invalid project name '{name}' in '{path}': {reason}")]
    InvalidProjectName {
        name: String,
        path: PathBuf,
        reason: String,
    },

    #[error("Entry point not found for project '{project}'. Searched paths: {searched:?}")]
    EntryPointNotFound {
        project: String,
        searched: Vec<PathBuf>,
    },

    #[error("Dependency '{name}' not found at path: '{path}'")]
    PathDependencyNotFound { name: String, path: PathBuf },

    #[error("Git error for dependency '{name}': {message}")]
    GitError { name: String, message: String },

    #[error("Circular dependency detected: {cycle}")]
    CircularDependency { cycle: String },

    #[error("Failed to parse lockfile '{path}': {source}")]
    LockfileParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("Lockfile not found at '{path}', but --locked was specified")]
    LockfileNotFound { path: PathBuf },

    #[error(
        "Lockfile at '{path}' is out of date: {reason}. Run 'jmcc update' to update dependencies"
    )]
    LockfileOutOfDate { path: PathBuf, reason: String },

    #[error(
        "Network access disabled by --offline, but dependency '{name}' ({url}) is not in local cache"
    )]
    OfflineNetworkForbidden { name: String, url: String },

    #[error(
        "Workspace inheritance error for field '{field}' in '{path}': root workspace does not provide this field"
    )]
    WorkspaceInheritanceMissing { field: String, path: PathBuf },

    #[error("{0}")]
    Generic(String),
}

pub type Result<T> = std::result::Result<T, ProjectError>;
