//! `JustCode` Language Server Protocol implementation.

pub mod completion;
pub mod diagnostics;
pub mod formatting;
pub mod goto_def;
pub mod highlight;
pub mod hover;
pub mod inlay_hints;
pub mod references;
pub mod rename;
pub mod semantic_tokens;
pub mod server;
pub mod signature_help;
pub mod state;
pub mod symbols;

pub use server::run_server;

/// Entry point to start the LSP server over stdio.
///
/// # Errors
///
/// Returns an error if the server initialization or message loop fails.
pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_server()
}
