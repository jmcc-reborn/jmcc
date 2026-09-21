//! File resolution and import graph traversal: prelude, std, directories, visited set.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use id_arena::Arena;
use lasso::Rodeo;
use tracing::{Level, debug, error, info, instrument, span, trace, warn};
use walkdir::WalkDir;

use crate::ast::parser::parse_string;
use crate::error::{JmccError, Result};

use super::*;

impl ImportResolver {
    #[instrument(skip(self), fields(path = %path.display()), level = "debug")]
    fn get_module_name(&self, path: &Path) -> Result<String> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        for (pkg_name, pkg_root) in &self.package_roots {
            let canon_pkg = pkg_root.canonicalize().unwrap_or_else(|_| pkg_root.clone());
            if let Ok(rel) = canon.strip_prefix(&canon_pkg) {
                let mut parts = vec![pkg_name.clone()];
                for c in rel.components() {
                    let os_str = c.as_os_str();
                    let part = Path::new(os_str).file_stem().map_or_else(
                        || os_str.to_string_lossy().into_owned(),
                        |st| st.to_string_lossy().into_owned(),
                    );
                    if !part.is_empty() && part != "lib" && part != "main" {
                        parts.push(part);
                    }
                }
                let module_name = parts.join("::");
                trace!(module_name = %module_name, "Resolved package module name");
                return Ok(module_name);
            }
        }

        let rel = canon
            .strip_prefix(&self.std_lib_dir)
            .or_else(|_| canon.strip_prefix(&self.root_dir))
            .unwrap_or(&canon);

        let module_name = rel
            .components()
            .map(|c| {
                let os_str = c.as_os_str();
                Path::new(os_str).file_stem().map_or_else(
                    || os_str.to_string_lossy().into_owned(),
                    |st| st.to_string_lossy().into_owned(),
                )
            })
            .collect::<Vec<_>>()
            .join("::");

        trace!(module_name = %module_name, "Resolved module name");
        Ok(module_name)
    }

    fn find_prelude_path(&self) -> Option<PathBuf> {
        let _span = span!(Level::DEBUG, "find_prelude_path", edition = self.edition).entered();
        let name = if self.edition >= 2026 {
            "prelude_2026.jc"
        } else {
            "prelude_2023.jc"
        };
        debug!(prelude_file = name, "Looking for prelude");

        let try_in = |base: &Path| -> Option<PathBuf> {
            [
                base.join("std").join(name),
                base.join("std").join("prelude.jc"),
                base.join(name),
                base.join("prelude.jc"),
            ]
            .into_iter()
            .find(|p| {
                let exists = p.exists();
                trace!(path = %p.display(), exists, "Probing prelude candidate");
                exists
            })
        };
        let result = try_in(&self.std_lib_dir).or_else(|| {
            let mut current = self.root_dir.clone();
            loop {
                if let Some(p) = try_in(&current) {
                    return Some(p);
                }
                if !current.pop() {
                    return None;
                }
            }
        });

        if let Some(p) = &result {
            info!(prelude_path = %p.display(), "Prelude found");
        } else {
            warn!("Prelude not found");
        }
        result
    }

    #[instrument(skip(self), fields(path = %path.display()), level = "info")]
    pub(super) fn parse_file_recursive(&mut self, path: &Path) -> Result<Ast> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        trace!(canonical = %canon.display(), visited = self.visited.len(), "Checking visited set");
        if self.visited.contains(&canon) {
            debug!(path = %canon.display(), "File already visited — returning empty AST");
            return Ok(Ast {
                exprs: Arena::new(),
                strings: Rodeo::default(),
                statements: Vec::new(),
                sources: HashMap::new(),
                line_indexes: HashMap::new(),
                file_offsets: Vec::new(),
            });
        }
        self.visited.insert(canon.clone());
        info!(path = %canon.display(), "Parsing file");

        let src = if let Some(text) = self
            .overlays
            .get(&canon)
            .or_else(|| self.overlays.get(path))
        {
            text.clone()
        } else {
            std::fs::read_to_string(&canon).map_err(|e| {
                error!(error = %e, path = %canon.display(), "Failed to read file");
                let suggestion = if !path.starts_with("std") {
                    let rel_path = path.strip_prefix(&self.root_dir).unwrap_or(path);
                    let std_cand1 = self.std_lib_dir.join("std").join(rel_path);
                    let std_cand2 = self.std_lib_dir.join(rel_path);
                    if std_cand1.exists()
                        || std_cand1.with_extension("jc").exists()
                        || std_cand2.exists()
                        || std_cand2.with_extension("jc").exists()
                    {
                        format!(" (did you mean 'std/{}'?)", rel_path.display())
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                JmccError::Generic(format!(
                    "Failed to read file '{}': {}{suggestion}",
                    canon.display(),
                    e
                ))
            })?
        };
        trace!(bytes = src.len(), "File read");

        let offset = self.global_offset;
        let mut ast = parse_string(&src, &canon.display().to_string(), self.edition, offset)
            .map_err(|e| {
                error!(error = ?e, path = %canon.display(), "Failed to parse file");
                e
            })?;
        self.global_offset += src.len() + 1;

        let module_name = self.get_module_name(&canon)?;
        if self.edition >= 2026 && !module_name.is_empty() {
            debug!(module = %module_name, "Applying name mangling");
            Self::apply_mangling(&mut ast, &module_name);
        }

        let import_map = self.resolve_file_imports(&mut ast, &canon)?;

        if self.edition >= 2026 && !import_map.is_empty() {
            debug!(entries = import_map.len(), "Applying import map");
            Self::apply_import_map(&mut ast, &import_map);
        }

        info!(path = %canon.display(), statements = ast.statements.len(), "Finished parsing file");
        Ok(ast)
    }

    fn resolve_file_imports(
        &mut self,
        ast: &mut Ast,
        canonical_path: &Path,
    ) -> Result<HashMap<String, String>> {
        let is_prelude = matches!(canonical_path.file_name(), Some(file) if PRELUDE_NAMES.iter().any(|name| file == *name));
        if !is_prelude {
            trace!("Injecting prelude import");
            let prelude_id = ast.strings.get_or_intern("prelude");
            ast.statements.insert(
                0,
                Statement::Import(ImportStmt {
                    path: prelude_id,
                    kind: ImportKind::SideEffect,
                    span: 0..0,
                }),
            );
        }

        let directory = canonical_path.parent().unwrap_or_else(|| Path::new("."));
        let old_statements = std::mem::take(&mut ast.statements);
        let mut new_statements = Vec::new();
        let mut import_map = HashMap::new();
        let import_count = old_statements
            .iter()
            .filter(|statement| matches!(statement, Statement::Import(_)))
            .count();
        debug!(
            imports = import_count,
            total = old_statements.len(),
            "Processing statements"
        );

        for statement in old_statements {
            let Statement::Import(import) = statement else {
                new_statements.push(statement);
                continue;
            };
            let path_string = ast.strings.resolve(&import.path).to_owned();
            let Some(full_path) = self.import_path(directory, &path_string) else {
                continue;
            };
            let resolved_paths = self.resolve_import_path(&full_path, &path_string)?;
            debug!(import = %path_string, candidates = resolved_paths.len(), "Resolved import candidates");

            for file_path in resolved_paths {
                let imported_ast = self.parse_file_recursive(&file_path)?;
                let imported = if self.edition >= 2026 {
                    let module_name = self.get_module_name(&file_path)?;
                    self.filter_imported_statements(
                        ast,
                        imported_ast,
                        &import.kind,
                        &module_name,
                        &mut import_map,
                    )
                } else {
                    self.merge_ast(ast, imported_ast)
                };
                let before = new_statements.len();
                new_statements.extend(imported);
                trace!(
                    added = new_statements.len() - before,
                    "Appended imported statements"
                );
            }
        }
        ast.statements = new_statements;
        Ok(import_map)
    }

    fn import_path(&self, directory: &Path, path: &str) -> Option<PathBuf> {
        if PRELUDE_NAMES.contains(&path) {
            trace!(import = %path, "Resolving prelude import");
            let resolved = self.find_prelude_path();
            if resolved.is_none() {
                warn!(import = %path, "Prelude path not found — skipping");
            }
            resolved
        } else if path == "std" || path.starts_with("std/") {
            debug!(import = %path, "Resolving std import");
            Some(self.std_lib_dir.join(path))
        } else if directory.starts_with(self.std_lib_dir.join("std")) {
            // Internal imports inside standard library files always resolve relative to std
            debug!(import = %path, "Resolving internal std import");
            Some(directory.join(path))
        } else if let Some((pkg, rest)) = path.split_once('/') {
            self.package_roots.get(pkg).map_or_else(
                || {
                    debug!(import = %path, "Resolving local import");
                    Some(directory.join(path))
                },
                |pkg_root| {
                    debug!(pkg, rest, "Resolving package sub-path import");
                    Some(pkg_root.join(rest))
                },
            )
        } else if let Some(pkg_root) = self.package_roots.get(path) {
            debug!(pkg = path, "Resolving package root import");
            let lib_candidates = [
                pkg_root.join("src").join("lib.jc"),
                pkg_root.join("lib.jc"),
                pkg_root.join("src").join("main.jc"),
                pkg_root.join("main.jc"),
            ];
            let target = lib_candidates
                .into_iter()
                .find(|p| p.is_file())
                .unwrap_or_else(|| {
                    let src_dir = pkg_root.join("src");
                    if src_dir.is_dir() {
                        src_dir
                    } else {
                        pkg_root.clone()
                    }
                });
            Some(target)
        } else {
            debug!(import = %path, "Resolving local import");
            Some(directory.join(path))
        }
    }

    #[instrument(skip(self), fields(full_path = %full_path.display(), original_path = %original_path), level = "debug")]
    fn resolve_import_path(&self, full_path: &Path, original_path: &str) -> Result<Vec<PathBuf>> {
        if PRELUDE_NAMES.contains(&original_path) {
            let result = self
                .find_prelude_path()
                .map(|p| vec![p])
                .unwrap_or_default();
            trace!(count = result.len(), "Resolved prelude import path");
            return Ok(result);
        }
        if full_path.is_dir() {
            debug!(dir = %full_path.display(), "Import path is directory — collecting .jc files");
            let mut files = Vec::new();
            Self::collect_jc_files(full_path, &mut files)?;
            trace!(
                collected = files.len(),
                "Collected .jc files from directory"
            );
            Ok(files)
        } else {
            let mut path = full_path.to_path_buf();
            if path.extension().is_none() {
                path.set_extension("jc");
            }
            trace!(resolved = %path.display(), "Resolved single import file");
            Ok(vec![path])
        }
    }

    #[instrument(skip(out), fields(dir = %dir.display()), level = "trace")]
    fn collect_jc_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        // Traversal order is observable: sort_by_file_name ensures deterministic preorder traversal.
        for entry in WalkDir::new(dir).follow_links(true).sort_by_file_name() {
            // Directory errors are fatal.
            let entry = entry.map_err(|e| {
                let failed = e.path().unwrap_or(dir);
                error!(error = %e, dir = %failed.display(), "Failed to read directory");
                JmccError::Generic(format!("Failed to read dir '{}': {}", failed.display(), e))
            })?;
            if entry.file_type().is_file()
                && entry.path().extension().is_some_and(|ext| ext == "jc")
            {
                trace!(file = %entry.path().display(), "Found .jc file");
                out.push(entry.into_path());
            }
        }
        Ok(())
    }
}

fn find_std_via_env() -> Option<PathBuf> {
    for env_var in ["JMCC_STD_PATH", "JMCC_SYSROOT"] {
        if let Ok(val) = std::env::var(env_var) {
            let p = PathBuf::from(val);
            if p.join("std").is_dir() {
                debug!(dir = %p.display(), env = env_var, "Found std dir via environment variable");
                return Some(p);
            }
            if (p.join("prelude_2026.jc").is_file() || p.join("prelude_2023.jc").is_file())
                && let Some(parent) = p.parent()
            {
                debug!(dir = %parent.display(), env = env_var, "Found std dir via environment variable (prelude parent)");
                return Some(parent.to_path_buf());
            }
            if p.is_dir() {
                debug!(dir = %p.display(), env = env_var, "Using environment variable path directly for std");
                return Some(p);
            }
        }
    }
    None
}

fn find_std_via_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    debug!(exe = %exe.display(), "Searching for std dir relative to executable");
    loop {
        if dir.join("std").is_dir() {
            debug!(dir = %dir.display(), "Found std dir relative to exe");
            return Some(dir);
        }
        if dir.join("jmcc").join("std").is_dir() {
            debug!(dir = %dir.display(), "Found jmcc/std dir relative to exe");
            return Some(dir.join("jmcc"));
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn find_std_via_ancestors(root_dir: &Path) -> Option<PathBuf> {
    for dir in root_dir.ancestors() {
        if dir.join("std").is_dir() {
            debug!(dir = %dir.display(), "Found std dir via root_dir ancestors");
            return Some(dir.to_path_buf());
        }
        if dir.join("jmcc").join("std").is_dir() {
            debug!(dir = %dir.display(), "Found jmcc/std dir via root_dir ancestors");
            return Some(dir.join("jmcc"));
        }
    }
    None
}

fn find_std_via_vscode_extension() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    for ext_base in [
        home.join(".vscode/extensions"),
        home.join(".vscode-server/extensions"),
        home.join(".vscode-insiders/extensions"),
    ] {
        if ext_base.is_dir()
            && let Ok(entries) = std::fs::read_dir(&ext_base)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                let matches_ext = path.file_name().is_some_and(|n| {
                    let s = n.to_string_lossy();
                    s.starts_with("jmcc.justcode") || s.starts_with("jmcc.jmc-analyzer")
                });
                if matches_ext && path.join("std").is_dir() {
                    debug!(dir = %path.display(), "Found std dir in installed VS Code extension");
                    return Some(path);
                }
            }
        }
    }
    None
}

#[instrument(skip(root_dir), fields(root_dir = %root_dir.display()), level = "debug")]
pub(super) fn find_std_lib_dir(root_dir: &Path) -> PathBuf {
    let _span = span!(Level::DEBUG, "find_std_lib_dir").entered();

    if let Some(dir) = find_std_via_env() {
        return dir;
    }

    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR").map(PathBuf::from)
        && dir.join("std").is_dir()
    {
        debug!(dir = %dir.display(), "Found std dir via CARGO_MANIFEST_DIR");
        return dir;
    }

    if let Some(dir) = find_std_via_exe() {
        return dir;
    }

    if let Some(dir) = find_std_via_ancestors(root_dir) {
        return dir;
    }

    if let Some(dir) = find_std_via_vscode_extension() {
        return dir;
    }

    warn!(dir = %root_dir.display(), "Falling back to root_dir for std lib");
    root_dir.to_path_buf()
}
