//! Recursive dependency graph resolver.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use tracing::debug;

use super::dependency::Dependency;
use super::error::{ProjectError, Result};
use super::git::GitCache;
use super::manifest::Manifest;

/// An individually resolved dependency package.
#[derive(Clone, Debug)]
pub struct ResolvedPackage {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Root directory of the package on disk.
    pub root_dir: PathBuf,
    /// Library entry file (e.g. `src/lib.jc`, `lib.jc`), if found.
    pub lib_file: Option<PathBuf>,
    /// Parsed manifest, if `jmcc.toml` exists in the dependency directory.
    pub manifest: Option<Manifest>,
}

/// The complete resolved dependency graph.
#[derive(Clone, Debug, Default)]
pub struct PackageGraph {
    pub packages: BTreeMap<String, ResolvedPackage>,
}

impl PackageGraph {
    /// Returns the resolved package for a given name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ResolvedPackage> {
        self.packages.get(name)
    }

    /// Converts the package graph into a mapping of `package_name -> source_root_path`.
    ///
    /// If the package has a `src/` directory, maps to that directory; otherwise maps to root directory.
    #[must_use]
    pub fn to_package_roots(&self) -> HashMap<String, PathBuf> {
        let mut map = HashMap::new();
        for (name, pkg) in &self.packages {
            let src_dir = pkg.root_dir.join("src");
            let target_dir = if src_dir.is_dir() {
                src_dir
            } else {
                pkg.root_dir.clone()
            };
            map.insert(name.clone(), target_dir);
        }
        map
    }
}

use super::lockfile::Lockfile;

/// Dependency graph resolver engine.
pub struct DependencyResolver {
    git_cache: GitCache,
    lockfile: Option<Lockfile>,
    visiting: Vec<String>,
    resolved: BTreeMap<String, ResolvedPackage>,
}

impl DependencyResolver {
    /// Creates a new resolver.
    #[must_use]
    pub fn new(fallback_dir: Option<&Path>) -> Self {
        Self {
            git_cache: GitCache::new(fallback_dir),
            lockfile: None,
            visiting: Vec::new(),
            resolved: BTreeMap::new(),
        }
    }

    /// Sets the lockfile to use for fixing git commit SHAs.
    #[must_use]
    pub fn with_lockfile(mut self, lockfile: Option<Lockfile>) -> Self {
        self.lockfile = lockfile;
        self
    }

    /// Sets offline mode (disables network operations).
    #[must_use]
    pub fn with_offline(mut self, offline: bool) -> Self {
        self.git_cache = self.git_cache.with_offline(offline);
        self
    }

    /// Recursively resolves all dependencies starting from the root manifest.
    ///
    /// # Errors
    /// Returns an error if dependencies cannot be located, parsed, or if a cycle is detected.
    pub fn resolve_all(
        mut self,
        root_manifest: &Manifest,
        root_dir: &Path,
        include_dev: bool,
    ) -> Result<PackageGraph> {
        let root_name = root_manifest.name(root_dir).unwrap_or("root").to_owned();

        self.visiting.push(root_name);
        self.resolve_deps_recursive(&root_manifest.all_dependencies(include_dev), root_dir)?;
        self.visiting.pop();

        Ok(PackageGraph {
            packages: self.resolved,
        })
    }

    fn resolve_deps_recursive(
        &mut self,
        deps: &BTreeMap<String, Dependency>,
        current_dir: &Path,
    ) -> Result<()> {
        for (dep_name, dep) in deps {
            // Check for circular dependency
            if self.visiting.contains(dep_name) {
                let mut cycle = self.visiting.clone();
                cycle.push(dep_name.clone());
                return Err(ProjectError::CircularDependency {
                    cycle: cycle.join(" -> "),
                });
            }

            // Already resolved
            if self.resolved.contains_key(dep_name) {
                continue;
            }

            let detailed = dep.detailed();
            let dep_root = self.resolve_dep_location(dep_name, &detailed, current_dir)?;

            // Check if dependency has its own jmcc.toml
            let dep_manifest_path = dep_root.join("jmcc.toml");
            let manifest = if dep_manifest_path.is_file() {
                Some(Manifest::from_file(&dep_manifest_path)?)
            } else {
                None
            };

            let version = manifest
                .as_ref()
                .map(Manifest::version)
                .or_else(|| detailed.version.clone())
                .unwrap_or_else(|| "0.1.0".to_owned());

            let lib_file = Self::find_lib_entry(&dep_root);

            let resolved_pkg = ResolvedPackage {
                name: dep_name.clone(),
                version,
                root_dir: dep_root.clone(),
                lib_file,
                manifest: manifest.clone(),
            };

            self.resolved.insert(dep_name.clone(), resolved_pkg);

            // Recursively resolve sub-dependencies
            if let Some(sub_manifest) = manifest {
                self.visiting.push(dep_name.clone());
                self.resolve_deps_recursive(&sub_manifest.dependencies, &dep_root)?;
                self.visiting.pop();
            }
        }

        Ok(())
    }

    fn resolve_dep_location(
        &self,
        dep_name: &str,
        detailed: &super::dependency::DetailedDependency,
        current_dir: &Path,
    ) -> Result<PathBuf> {
        // Path dependency
        if let Some(p) = &detailed.path {
            let full_path = if p.is_absolute() {
                p.clone()
            } else {
                current_dir.join(p)
            };

            let canon =
                full_path
                    .canonicalize()
                    .map_err(|_err| ProjectError::PathDependencyNotFound {
                        name: dep_name.to_owned(),
                        path: full_path.clone(),
                    })?;

            if !canon.is_dir() {
                return Err(ProjectError::PathDependencyNotFound {
                    name: dep_name.to_owned(),
                    path: canon,
                });
            }

            debug!(dep = dep_name, path = %canon.display(), "Resolved local path dependency");
            return Ok(canon);
        }

        // Git dependency
        if let Some(git_url) = &detailed.git {
            let locked_rev = self
                .lockfile
                .as_ref()
                .and_then(|l| l.get_package(dep_name))
                .and_then(|p| p.source.as_ref())
                .and_then(|s| s.split_once('#'))
                .map(|(_, sha)| sha);

            let effective_rev = detailed.rev.as_deref().or(locked_rev);

            let checkout_dir = self.git_cache.checkout(
                dep_name,
                git_url,
                detailed.branch.as_deref(),
                detailed.tag.as_deref(),
                effective_rev,
            )?;
            return Ok(checkout_dir);
        }

        Err(ProjectError::Generic(format!(
            "Dependency '{dep_name}' has no `path` or `git` source specified."
        )))
    }

    fn find_lib_entry(root: &Path) -> Option<PathBuf> {
        let candidates = [
            root.join("src").join("lib.jc"),
            root.join("lib.jc"),
            root.join("src").join("main.jc"),
            root.join("main.jc"),
        ];

        candidates.into_iter().find(|p| p.is_file())
    }
}
