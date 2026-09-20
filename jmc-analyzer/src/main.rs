//! CLI for the `JustCode` analyzer crate: pack the VS Code extension, dump
//! syntax data, and (later) run the language server.

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use jmc_analyzer::lsp;
use jmc_analyzer::pack;
use jmc_analyzer::syntax::SyntaxCatalog;

/// `JustCode` language data, extension packer, and future language server.
#[derive(Parser)]
#[command(name = "jmc-analyzer", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the machine-readable syntax catalogue as JSON.
    Syntax,
    /// Copy the VS Code client into `out/` (or `--output`) without packing a vsix.
    Extension {
        /// Destination directory. Defaults to `<crate>/out/vscode`.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Assemble the VS Code client into a `.vsix` archive.
    Pack {
        /// Destination `.vsix` path. Defaults to `out/justcode-lang-<version>.vsix`.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Start the language server over stdio.
    Lsp {
        /// Optional path to the `JustCode` standard library directory (`std`).
        #[arg(long)]
        std_path: Option<PathBuf>,
    },
}

#[expect(
    clippy::let_underscore_must_use,
    reason = "CLI status and syntax dump go to the process streams"
)]
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr(), "{err}");
            ExitCode::FAILURE
        }
    }
}

/// # Errors
///
/// Returns a [`CliError`] when packing fails, JSON cannot be written, or the
/// LSP server reports an error.
fn run() -> Result<(), CliError> {
    match Cli::parse().command {
        Command::Syntax => {
            let catalog = SyntaxCatalog::builtin();
            serde_json::to_writer_pretty(io::stdout(), &DumpCatalog(&catalog))?;
            writeln!(io::stdout())?;
            Ok(())
        }
        Command::Extension { output } => {
            let dest = output
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out/vscode"));
            pack::copy_extension(&dest)?;
            writeln!(io::stderr(), "wrote extension to {}", dest.display())?;
            Ok(())
        }
        Command::Pack { output } => {
            let dest = pack::pack_vsix(output)?;
            writeln!(io::stderr(), "wrote {}", dest.display())?;
            Ok(())
        }
        Command::Lsp { std_path } => {
            lsp::run_with_std_path(std_path).map_err(|e| CliError::Lsp(e.to_string()))
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Pack(#[from] pack::PackError),
    #[error("LSP server error: {0}")]
    Lsp(String),
}

/// Serde wrapper so we can pretty-print the catalog without a Serialize impl
/// that would force every nested type to be public-serialize.
struct DumpCatalog<'a>(&'a SyntaxCatalog);

impl serde::Serialize for DumpCatalog<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let catalog = self.0;
        let mut state = serializer.serialize_struct("SyntaxCatalog", 6)?;
        state.serialize_field("language", &catalog.language)?;
        state.serialize_field("displayName", &catalog.display_name)?;
        state.serialize_field("scopeName", &catalog.scope_name)?;
        state.serialize_field("keywords", &catalog.all_keywords())?;
        state.serialize_field("builtinObjects", &catalog.builtin_objects)?;
        state.serialize_field("lsp", &catalog.lsp.command)?;
        state.end()
    }
}
