//! `JustCode` language data and a VS Code extension builder.
//!
//! The crate currently ships:
//! - machine-readable syntax in [`data/syntax.json`](../data/syntax.json);
//! - a `TextMate` grammar and editor configuration consumed by the VS Code client;
//! - a CLI that can pack that client into a `.vsix`;
//! - a placeholder `lsp` subcommand for the future language server.
//!
//! Semantic analysis will live here later; until then the binary only reports
//! that the server is not implemented.

pub mod lsp;
pub mod pack;
pub mod syntax;

use std::path::{Path, PathBuf};

/// Files that make up the VS Code client. Paths are relative to the crate root.
pub const EXTENSION_FILES: &[&str] = &[
    "vscode/package.json",
    "vscode/language-configuration.json",
    "vscode/syntaxes/justcode.tmLanguage.json",
    "vscode/src/extension.js",
    "vscode/README.md",
    "vscode/README_RU.md",
    "vscode/LICENSE.txt",
    "vscode/media/logo.png",
    "vscode/media/icon.png",
    "vscode/media/icon-dark.png",
];

/// Syntax catalogue shipped next to the crate sources.
#[must_use]
pub fn syntax_catalog() -> syntax::SyntaxCatalog {
    syntax::SyntaxCatalog::builtin()
}

/// Absolute path of this crate's `vscode/` directory at compile time.
#[must_use]
pub fn vscode_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("vscode")
}

/// Absolute path of the syntax JSON at compile time.
#[must_use]
pub fn syntax_json_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data/syntax.json")
}
