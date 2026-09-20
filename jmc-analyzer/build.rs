//! Assembles the VS Code client from `data/` + `vscode/` and packs a `.vsix`.
//!
//! Grammar and language configuration live in `data/` so the highlighter and
//! the future LSP share one source. `vscode/` holds the editor client. This
//! script copies the data files into `vscode/`, then writes
//! `out/jmc-analyzer-<version>.vsix`.

#[path = "build/vsix.rs"]
mod vsix;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=vscode");
    println!("cargo:rerun-if-changed=build/vsix.rs");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sync_data_into_vscode(&manifest)?;

    let version = env!("CARGO_PKG_VERSION");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let crate_out = manifest.join("out");
    fs::create_dir_all(&crate_out)?;

    let name = format!("jmc-analyzer-{version}.vsix");
    vsix::pack(&manifest, &crate_out.join(&name))?;
    vsix::pack(&manifest, &out_dir.join(&name))?;

    Ok(())
}

fn sync_data_into_vscode(manifest: &Path) -> Result<(), Box<dyn Error>> {
    let data = manifest.join("data");
    let vscode = manifest.join("vscode");
    fs::create_dir_all(vscode.join("syntaxes"))?;
    fs::copy(
        data.join("justcode.tmLanguage.json"),
        vscode.join("syntaxes/justcode.tmLanguage.json"),
    )?;
    fs::copy(
        data.join("language-configuration.json"),
        vscode.join("language-configuration.json"),
    )?;
    Ok(())
}
