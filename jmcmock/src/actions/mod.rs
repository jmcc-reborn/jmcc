//! `JustMC` actions: what each of them does to the mock's state.
//!
//! Operations arrive here from [`Runtime::exec_op`](crate::interp) already free
//! of branches: `if_*` and `else` are handled there, because they belong to a
//! block rather than to a value.
//!
//! # Groups come from the schema, not from the name
//!
//! The `JustMC` schema has nearly a thousand actions, and sorting them into
//! files by a list would mean keeping a second table that would drift away from
//! the first. So a group is
//! [`ActionDef::object`](jmcdata::generated::ActionDef::object) from `jmcdata` —
//! the very field the schema itself sorts actions by:
//!
//! | Object | Module |
//! |---|---|
//! | `variable` | [`variable`] — computation, lists, dictionaries |
//! | `code` | [`code`] — waiting, returning, calls, `measure_time` |
//! | `repeat` | [`repeat`] — loops |
//! | `select` | [`select`] — selection |
//! | everything else | [`effect`] — actions on the world |
//!
//! A new schema action lands in its group by itself. If the group does not know
//! it, it is not skipped silently but declared unimplemented
//! ([`RuntimeError::Unimplemented`]) — otherwise an action that was supposed to
//! return a value would yield a wrong result with no error at all.

// `code` and `repeat` are reachable from the scheduler: it intercepts the
// suspending operations (`control_wait`, the repeat containers) before
// `dispatch` sees them, and drives their resumable state itself.
pub mod code;
mod effect;
pub mod repeat;
// Visible from `interp`: the `variable` group holds the `if_variable_*`
// conditions, and those are evaluated by `Runtime::condition` where the
// branches are.
pub mod variable;

// Visible from `interp`: a conditional selection is handled where the branches
// are, because its condition lives in `Op::conditional` rather than in the
// action's arguments.
pub mod select;

use jmcdata::module::Op;

use crate::error::Result;
use crate::interp::Flow;
use crate::run::{Runtime, Stream};
use crate::schema;

/// Runs an action that is not a branch.
///
/// # Errors
///
/// Returns any runtime error: a missing variable, a nonexistent function, an
/// unimplemented action.
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    match schema::def(op.action).map(|definition| definition.object) {
        Some("variable") => variable::dispatch(rt, stream, op),
        Some("code" | "controller") => code::dispatch(rt, stream, op),
        Some("repeat") => repeat::dispatch(rt, stream, op),
        Some("select") => select::dispatch(rt, stream, op),
        _ => effect::dispatch(rt, stream, op),
    }
}
