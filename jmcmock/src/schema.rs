//! The `JustMC` action schema, taken from `jmcdata`.
//!
//! The mock keeps no table of actions of its own: both the identifiers and the
//! argument sets come from [`jmcdata::generated`] — the same generated
//! description the compiler builds operations from. The reverse index
//! `ActionId -> ActionDef` lives there too, behind
//! [`get_action_def_by_id`], because the generated tables are keyed by name
//! while operations refer to an action by enum variant.
//!
//! Three things are taken from the schema:
//!
//! * the action identifier for error messages ([`name`]);
//! * the list of allowed arguments — to notice an argument the action does not
//!   have ([`check_arguments`]);
//! * properties of an individual argument — the set of values of an enum
//!   ([`argument_values`]) and whether it accepts several values
//!   ([`argument_is_array`]).
//!
//! Requiredness is not derived from the schema: `jmcdata::generated` has no
//! "required" flag. It is not needed either — requiredness shows where the
//! argument is used: an action handler that needs an argument asks for it with
//! `require` and gets [`RuntimeError::MissingArgument`], while optional
//! arguments (`merging` on messages, `interval` on a range, `default_value` on
//! reading a list element) the compiler is free not to write, and the handler
//! simply does not ask for them.

use jmcdata::generated::{ActionArg, ActionDef, ActionId, get_action_def_by_id};
use jmcdata::module::Op;

use crate::error::{Result, RuntimeError};

/// The description of an action in the `JustMC` schema.
///
/// The reverse index lives in `jmcdata` (`get_action_def_by_id`), keyed by the
/// action id: the generated `ACTION_DEF_MAP` is keyed by `(object, name)` while
/// operations refer to an action by enum variant.
#[must_use]
#[tracing::instrument(level = "trace", fields(action = ?action))]
pub fn def(action: ActionId) -> Option<&'static ActionDef> {
    get_action_def_by_id(action)
}

/// The canonical action identifier — for example `set_variable_get_list_length`.
///
/// It is what ends up in error messages too, so an unknown action has a
/// stand-in rather than a panic.
#[must_use]
#[tracing::instrument(level = "trace", fields(action = ?action))]
pub fn name(action: ActionId) -> &'static str {
    def(action).map_or("<unknown action>", |def| def.id)
}

/// The description of an argument according to the schema.
#[must_use]
#[tracing::instrument(level = "trace", fields(action = ?action, arg = %arg))]
pub fn argument(action: ActionId, arg: &str) -> Option<&'static ActionArg> {
    def(action)?
        .args
        .iter()
        .find(|candidate| candidate.id == arg)
}

/// The allowed values of an enum argument.
///
/// `None` means the argument is not an enum, not that it has no allowed values:
/// an enum's list is always non-empty, otherwise there would be nothing to
/// check.
#[must_use]
#[tracing::instrument(level = "trace", fields(action = ?action, arg = %arg))]
pub fn argument_values(action: ActionId, arg: &str) -> Option<&'static [&'static str]> {
    argument(action, arg)?.values
}

/// Whether the schema marks the argument as accepting several values.
///
/// The flag is needed where a single JSON value does not tell a list from one
/// value: the compiler writes `compare: any[21]` both as an array and as a
/// single number.
#[must_use]
#[tracing::instrument(level = "trace", fields(action = ?action, arg = %arg))]
pub fn argument_is_array(action: ActionId, arg: &str) -> bool {
    argument(action, arg).is_some_and(|candidate| candidate.array.is_some())
}

/// Checks that an operation has no arguments the action does not have.
///
/// The action is passed separately from the operation because the two do not
/// always coincide: for `repeat_while` and `select_filter_by_conditional` the
/// operation is a container, and its `values` belong to the condition in
/// `Op::conditional`.
///
/// The reverse check — that every needed argument is present — is impossible
/// here: the schema does not mark arguments as required. The action handler
/// does it when it reaches a concrete argument.
///
/// # Errors
///
/// Returns [`RuntimeError::UnknownArgument`] if the operation carries an
/// argument that is not in the action's schema.
#[tracing::instrument(level = "trace", skip(op), fields(action = ?action))]
pub fn check_arguments(action: ActionId, op: &Op<'_>) -> Result<()> {
    let Some(def) = def(action) else {
        return Ok(());
    };
    for key in op.values.keys() {
        if !def.args.iter().any(|arg| arg.id == key.as_ref()) {
            return Err(RuntimeError::UnknownArgument {
                action: def.id,
                arg: key.to_string(),
            });
        }
    }
    Ok(())
}
