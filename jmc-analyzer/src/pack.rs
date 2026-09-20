//! Copy the VS Code client and pack it into a `.vsix`.

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::EXTENSION_FILES;

const PUBLISHER: &str = "jmcc";
const EXTENSION_ID: &str = "justcode-lang";

fn sync_grammar(root: &Path) -> Result<(), PackError> {
    let data = root.join("data");
    let vscode = root.join("vscode");
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

fn sync_std(root: &Path) -> Result<(), PackError> {
    if let Some(parent) = root.parent() {
        let std_src = parent.join("jmcc").join("std");
        let std_dst = root.join("vscode").join("std");
        copy_jc_recursive(&std_src, &std_dst)?;
    }
    Ok(())
}

fn copy_jc_recursive(src: &Path, dst: &Path) -> Result<(), PackError> {
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
            copy_jc_recursive(&path, &target)?;
        } else if path.extension().is_some_and(|ext| ext == "jc") {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn pack_dir_recursive(
    dir: &Path,
    root: &Path,
    zip: &mut ZipWriter<File>,
    options: SimpleFileOptions,
) -> Result<(), PackError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            pack_dir_recursive(&path, root, zip, options)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| io::Error::other(e.to_string()))?;
            let suffix = rel
                .strip_prefix("vscode/")
                .map_err(|e| io::Error::other(e.to_string()))?;
            let suffix_str = suffix
                .to_str()
                .ok_or_else(|| io::Error::other("invalid UTF-8 in path"))?;
            zip.start_file(format!("extension/{suffix_str}"), options)?;
            zip.write_all(&fs::read(&path)?)?;
        }
    }
    Ok(())
}

/// Failure while assembling or zipping the VS Code client.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// Filesystem error while reading sources or writing the archive.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The `zip` crate rejected an archive operation.
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    /// A file listed in [`crate::EXTENSION_FILES`] is missing from the crate tree.
    #[error("missing extension file {0}")]
    Missing(String),
}

/// Copy the VS Code client into `dest`, creating the directory if needed.
///
/// # Errors
///
/// Returns [`PackError::Io`] when a source file cannot be read or the
/// destination cannot be written, and [`PackError::Missing`] when a required
/// client file is absent.
///
/// # Panics
///
/// Panics if an [`crate::EXTENSION_FILES`] entry does not start with `vscode/`.
pub fn copy_extension(dest: &Path) -> Result<(), PackError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    sync_grammar(root)?;
    sync_std(root)?;
    fs::create_dir_all(dest)?;
    for rel in EXTENSION_FILES {
        let from = root.join(rel);
        if !from.is_file() {
            return Err(PackError::Missing((*rel).to_owned()));
        }
        let suffix = rel
            .strip_prefix("vscode/")
            .expect("EXTENSION_FILES entries start with vscode/");
        let to = dest.join(suffix);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &to)?;
    }
    let std_src = root.join("vscode").join("std");
    if std_src.is_dir() {
        copy_jc_recursive(&std_src, &dest.join("std"))?;
    }
    Ok(())
}

/// Pack the VS Code client into a `.vsix`.
///
/// When `output` is `None`, the archive is written to
/// `<crate>/out/justcode-lang-<version>.vsix`.
///
/// # Errors
///
/// Returns a [`PackError`] when sources are missing or the archive cannot be
/// written.
///
/// # Panics
///
/// Panics if an [`EXTENSION_FILES`] entry does not start with `vscode/`, which
/// is a crate invariant.
pub fn pack_vsix(output: Option<PathBuf>) -> Result<PathBuf, PackError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    sync_grammar(root)?;
    sync_std(root)?;
    let version = env!("CARGO_PKG_VERSION");
    let dest = output.unwrap_or_else(|| {
        root.join("out")
            .join(format!("{EXTENSION_ID}-{version}.vsix"))
    });
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(&dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;

    zip.start_file("extension.vsixmanifest", options)?;
    zip.write_all(vsix_manifest(version).as_bytes())?;

    for rel in EXTENSION_FILES {
        let from = root.join(rel);
        if !from.is_file() {
            return Err(PackError::Missing((*rel).to_owned()));
        }
        let suffix = rel
            .strip_prefix("vscode/")
            .expect("EXTENSION_FILES entries start with vscode/");
        zip.start_file(format!("extension/{suffix}"), options)?;
        zip.write_all(&fs::read(&from)?)?;
    }

    let std_dir = root.join("vscode").join("std");
    if std_dir.is_dir() {
        pack_dir_recursive(&std_dir, root, &mut zip, options)?;
    }

    zip.finish()?;
    Ok(dest)
}

fn vsix_manifest(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity Language="en-US" Id="{EXTENSION_ID}" Version="{version}" Publisher="{PUBLISHER}" />
    <DisplayName>JustCode for JustMC (JMCC)</DisplayName>
    <Description xml:space="preserve">Full Language Server (diagnostics, IntelliSense, definitions, hover, rename, semantic tokens) and syntax highlighting for JustCode (.jc)</Description>
    <Tags>jmcc,JustCode,JC,justcode,justcode-lang,justmc,minecraft,diamondfire</Tags>
    <Categories>Programming Languages</Categories>
    <GalleryFlags>Public</GalleryFlags>
    <Properties>
      <Property Id="Microsoft.VisualStudio.Code.Engine" Value="^1.84.0" />
      <Property Id="Microsoft.VisualStudio.Code.ExtensionKind" Value="workspace" />
    </Properties>
    <License>extension/LICENSE.txt</License>
    <Icon>extension/media/logo.png</Icon>
  </Metadata>
  <Installation>
    <InstallationTarget Id="Microsoft.VisualStudio.Code"/>
  </Installation>
  <Dependencies/>
  <Assets>
    <Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.Details" Path="extension/README.md" Addressable="true" />
    <Asset Type="Microsoft.VisualStudio.Services.Content.License" Path="extension/LICENSE.txt" Addressable="true" />
    <Asset Type="Microsoft.VisualStudio.Services.Icons.Default" Path="extension/media/logo.png" Addressable="true" />
  </Assets>
</PackageManifest>
"#
    )
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="json" ContentType="application/json" />
  <Default Extension="js" ContentType="application/javascript" />
  <Default Extension="md" ContentType="text/markdown" />
  <Default Extension="txt" ContentType="text/plain" />
  <Default Extension="png" ContentType="image/png" />
  <Default Extension="vsixmanifest" ContentType="text/xml" />
  <Default Extension="xml" ContentType="text/xml" />
</Types>
"#;
