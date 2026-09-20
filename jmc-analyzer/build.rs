//! Assembles the VS Code client from `data/` + `vscode/` and packs a `.vsix`.
//!
//! Grammar and language configuration live in `data/` so the highlighter and
//! the future LSP share one source. `vscode/` holds the editor client. This
//! script copies the data files into `vscode/`, then writes
//! `out/justcode-lang-<version>.vsix`.

#[path = "build/vsix.rs"]
mod vsix;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=vscode");
    println!("cargo:rerun-if-changed=build/vsix.rs");
    println!("cargo:rerun-if-changed=../jmcc/std");

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sync_data_into_vscode(&manifest)?;
    sync_std_into_vscode(&manifest)?;

    let version = env!("CARGO_PKG_VERSION");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let crate_out = manifest.join("out");
    fs::create_dir_all(&crate_out)?;

    let name = format!("justcode-lang-{version}.vsix");
    vsix::pack(&manifest, &crate_out.join(&name))?;
    vsix::pack(&manifest, &out_dir.join(&name))?;

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("FileDescription", "JustMC Language Server & Analyzer");
        res.set("ProductName", "jmc-analyzer");
        res.set("OriginalFilename", "jmc-analyzer.exe");
        res.set("LegalCopyright", "Copyright (c) 2026 suprohub");
        res.set("CompanyName", "JustMC");
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="asInvoker" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
        <!-- Windows 10 and Windows 11 -->
        <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
        <!-- Windows 8.1 -->
        <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
        <!-- Windows 8 -->
        <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
        <!-- Windows 7 -->
        <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
    </application>
</compatibility>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resources: {e}");
        }
    }

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

fn sync_std_into_vscode(manifest: &Path) -> Result<(), Box<dyn Error>> {
    let std_src = manifest
        .parent()
        .ok_or("no parent directory for manifest")?
        .join("jmcc")
        .join("std");
    let std_dst = manifest.join("vscode").join("std");
    copy_jc_files_recursive(&std_src, &std_dst)?;
    Ok(())
}

fn copy_jc_files_recursive(src: &Path, dst: &Path) -> Result<(), Box<dyn Error>> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let target = dst.join(file_name);
        if path.is_dir() {
            copy_jc_files_recursive(&path, &target)?;
        } else if path.extension().is_some_and(|ext| ext == "jc") {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
