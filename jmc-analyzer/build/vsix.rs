//! Minimal `.vsix` writer used by `build.rs`. Kept separate from `src/pack.rs`
//! because build scripts cannot link the crate they are building.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::Path;

use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const FILES: &[&str] = &[
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

pub fn pack(manifest: &Path, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let version = env!("CARGO_PKG_VERSION");
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;
    zip.start_file("extension.vsixmanifest", options)?;
    zip.write_all(manifest_xml(version).as_bytes())?;

    for rel in FILES {
        let from = manifest.join(rel);
        let suffix = rel
            .strip_prefix("vscode/")
            .expect("vsix file list entries start with vscode/");
        zip.start_file(format!("extension/{suffix}"), options)?;
        zip.write_all(&fs::read(from)?)?;
    }

    zip.finish()?;
    Ok(())
}

fn manifest_xml(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity Language="en-US" Id="jmc-analyzer" Version="{version}" Publisher="jmcc" />
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
