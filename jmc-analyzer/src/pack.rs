//! Copy the VS Code client and pack it into a `.vsix`.

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::EXTENSION_FILES;

const PUBLISHER: &str = "jmcc";
const EXTENSION_ID: &str = "jmc-analyzer";

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
    Ok(())
}

/// Pack the VS Code client into a `.vsix`.
///
/// When `output` is `None`, the archive is written to
/// `<crate>/out/jmc-analyzer-<version>.vsix`.
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

    zip.finish()?;
    Ok(dest)
}

fn vsix_manifest(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity Language="en-US" Id="{EXTENSION_ID}" Version="{version}" Publisher="{PUBLISHER}" />
    <DisplayName>JMC Analyzer</DisplayName>
    <Description xml:space="preserve">Full Language Server (diagnostics, IntelliSense, definitions, hover, rename, semantic tokens) and syntax highlighting for JustCode (.jc)</Description>
    <Tags>jmcc,JustCode,JC,jmc-analyzer,justmc,minecraft,diamondfire</Tags>
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
