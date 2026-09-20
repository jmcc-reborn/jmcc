//! Dead Code Elimination passes for HIR.

pub mod dfe;
#[path = "dce.rs"]
mod pass;

pub use dfe::DeadFunctionEliminationPass;
pub use pass::DeadCodeEliminationPass;
