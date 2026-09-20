//! Git repository caching and dependency checkout.

use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{debug, info};

use super::error::{ProjectError, Result};

/// Cache manager for external Git dependencies.
#[derive(Clone, Debug)]
pub struct GitCache {
    base_dir: PathBuf,
    offline: bool,
}

impl GitCache {
    /// Initializes Git cache in `$JMCC_CACHE_DIR`, `~/.jmcc/cache/git`, or a project fallback directory.
    #[must_use]
    pub fn new(fallback_project_dir: Option<&Path>) -> Self {
        let base_dir = std::env::var("JMCC_CACHE_DIR")
            .ok()
            .map(|val| PathBuf::from(val).join("git"))
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".jmcc").join("cache").join("git"))
            })
            .or_else(|| fallback_project_dir.map(|p| p.join(".jmcc").join("cache").join("git")))
            .unwrap_or_else(|| PathBuf::from(".jmcc").join("cache").join("git"));

        Self {
            base_dir,
            offline: false,
        }
    }

    /// Sets offline mode.
    #[must_use]
    pub const fn with_offline(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Returns the commit SHA of HEAD in a repository checkout directory.
    ///
    /// # Errors
    /// Returns error if git rev-parse HEAD fails.
    pub fn head_commit(checkout_dir: &Path) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(checkout_dir)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .map_err(|e| ProjectError::GitError {
                name: checkout_dir.display().to_string(),
                message: format!("Failed to run git rev-parse HEAD: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProjectError::GitError {
                name: checkout_dir.display().to_string(),
                message: format!("git rev-parse HEAD failed: {stderr}"),
            });
        }

        let sha = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(sha)
    }

    /// Pulls latest changes from remote in an existing checkout.
    ///
    /// # Errors
    /// Returns error if git pull or git checkout fails.
    pub fn update(&self, dep_name: &str, checkout_dir: &Path, branch: Option<&str>) -> Result<()> {
        if self.offline {
            return Err(ProjectError::OfflineNetworkForbidden {
                name: dep_name.to_owned(),
                url: checkout_dir.display().to_string(),
            });
        }

        let target_branch = branch.unwrap_or("HEAD");
        let pull_status = Command::new("git")
            .arg("-C")
            .arg(checkout_dir)
            .arg("pull")
            .arg("--quiet")
            .status()
            .map_err(|e| ProjectError::GitError {
                name: dep_name.to_owned(),
                message: format!("Failed to spawn git pull: {e}"),
            })?;

        if !pull_status.success() {
            return Err(ProjectError::GitError {
                name: dep_name.to_owned(),
                message: format!("git pull failed for '{dep_name}' on branch '{target_branch}'"),
            });
        }

        Ok(())
    }

    /// Resolves and clones/checkouts a git repository to a local cached directory.
    ///
    /// # Errors
    /// Returns an error if Git execution fails or network/disk errors occur.
    pub fn checkout(
        &self,
        dep_name: &str,
        url: &str,
        branch: Option<&str>,
        tag: Option<&str>,
        rev: Option<&str>,
    ) -> Result<PathBuf> {
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        let url_hash = format!("{:016x}", hasher.finish());

        let ref_key = rev
            .map(|r| format!("rev_{r}"))
            .or_else(|| tag.map(|t| format!("tag_{t}")))
            .or_else(|| branch.map(|b| format!("branch_{b}")))
            .unwrap_or_else(|| "head".to_owned());

        let checkout_dir = self
            .base_dir
            .join("checkouts")
            .join(&url_hash)
            .join(&ref_key);

        if checkout_dir.is_dir() && checkout_dir.join(".git").exists() {
            debug!(
                dep = dep_name,
                path = %checkout_dir.display(),
                "Reusing existing git checkout from cache"
            );
            return Ok(checkout_dir);
        }

        if self.offline {
            return Err(ProjectError::OfflineNetworkForbidden {
                name: dep_name.to_owned(),
                url: url.to_owned(),
            });
        }

        info!(
            dep = dep_name,
            url,
            ref_key = %ref_key,
            path = %checkout_dir.display(),
            "Cloning git dependency..."
        );

        fs::create_dir_all(&checkout_dir)?;

        // Execute git clone
        let clone_status = Command::new("git")
            .arg("clone")
            .arg("--quiet")
            .arg(url)
            .arg(&checkout_dir)
            .status()
            .map_err(|e| ProjectError::GitError {
                name: dep_name.to_owned(),
                message: format!("Failed to spawn git process: {e}"),
            })?;

        if !clone_status.success() {
            drop(fs::remove_dir_all(&checkout_dir));
            return Err(ProjectError::GitError {
                name: dep_name.to_owned(),
                message: format!("'git clone {url}' failed with exit code: {clone_status:?}"),
            });
        }

        // Checkout target ref if specified
        let target_ref = rev.or(tag).or(branch);
        if let Some(target) = target_ref {
            let checkout_status = Command::new("git")
                .arg("-C")
                .arg(&checkout_dir)
                .arg("checkout")
                .arg("--quiet")
                .arg(target)
                .status()
                .map_err(|e| ProjectError::GitError {
                    name: dep_name.to_owned(),
                    message: format!("Failed to spawn git checkout: {e}"),
                })?;

            if !checkout_status.success() {
                drop(fs::remove_dir_all(&checkout_dir));
                return Err(ProjectError::GitError {
                    name: dep_name.to_owned(),
                    message: format!("'git checkout {target}' failed for '{dep_name}'"),
                });
            }
        }

        Ok(checkout_dir)
    }
}
