//! Instruction combination and peephole optimization module for MIR.
//!
//! Submodules:
//! - [`set_variable_folding`]: Merges consecutive `set_value` actions into `set_values`.
//! - [`coordinate_folding`]: Collapses component-wise location/vector operations into `set_all_coordinates` etc.
//! - [`peephole`]: Specializes actions (`increment`, `decrement`) and merges adjacent `send_message` / `append` operations.
//! - [`constant_folding`]: Compile-time evaluation of operations on strings, lists, maps, vectors, and locations.

pub mod constant_folding;
pub mod coordinate_folding;
pub mod peephole;
pub mod set_variable_folding;

pub use constant_folding::MirConstantFoldingPass;
pub use coordinate_folding::CoordinateFoldingPass;
pub use peephole::ActionPeepholePass;
pub use set_variable_folding::SetVariableFoldingPass;
