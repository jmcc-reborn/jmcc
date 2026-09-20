//! Running operations: values, branches, variable access.
//!
//! What lives here is what is the same for every action: how a value is
//! computed, how a branch is chosen, where a variable is written and where the
//! target comes from. The actions themselves are in [`crate::actions`], and each
//! comes here for its arguments and for writing its result.
//!
//! # Text is rendered when evaluated, not when written
//!
//! `JustMC` substitutes `%player%` and `%var(x)%` at the moment the value is
//! used. So `set var = "%player%"` puts the already substituted nickname into
//! the variable, and [`Runtime::eval`] renders [`Value::Text`] on the way out.
//!
//! For the same reason variable names go through the substituter
//! ([`Runtime::variable_name`]): `game var %player%_dialog` in `case1.jc` means
//! one variable per player rather than a literal `%` in the name. Otherwise the
//! `player_join` handler would write to one variable while `start_process` read
//! from another.
//!
//! # Arguments do not always belong to the operation carrying them
//!
//! For `if_*` the condition is the operation itself: its `action` is the action
//! being tested. For `repeat_while` and `select_filter_by_conditional` it is the
//! other way round: the operation is a container, and the condition is named
//! separately in [`Op::conditional`], while the condition's arguments live in the
//! container's `values`. [`Args`] separates these two cases, so that both the
//! argument check and the error messages name the action the arguments belong to.
//!
//! # Layout
//!
//! Файл разложен по ответственностям: `exec.rs` ведёт блоки, потоки и
//! ветвления, `eval.rs` вычисляет значения, `args.rs` читает аргументы
//! операций, `select.rs` разрешает цели. Здесь остаётся то, что нужно всем
//! четырём: [`Flow`], [`Args`] и разбор тела операции.

mod args;
mod eval;
mod exec;
mod select;

use std::borrow::Cow;

use jmcdata::generated::{ActionId, GameValueId};
use jmcdata::module::{Conditional, Number, Op, TextValue, Value, VariableScope};
use litemap::LiteMap;

use crate::error::{Result, RuntimeError};
use crate::run::{Runtime, Stream};
use crate::schema;
use crate::value::{self, Rt, count_as_number};
use crate::world::{Position, Target, World};

pub use args::{at_op, invalid_enum};
pub use exec::matches_type;
pub use exec::{body_of, is_branch};

/// What a frame reports to the block after an operation has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flow {
    /// An ordinary continuation: the block's next operation runs.
    Continue,
    /// Leaving a function. The value is already in `local:ret`, and `enter` will
    /// pick it up.
    Return,
    /// The thread is finished entirely — `control_end_thread` or a cancelled
    /// event.
    EndThread,
    /// Break the loop the block is running inside.
    StopRepeat,
    /// Skip the current iteration of the loop.
    SkipIteration,
}

/// The set of arguments of an operation.
///
/// It holds not the values themselves but a reference to the operation's `values`
/// plus the identifier of the action those values belong to. The two do not
/// always coincide: a conditional container carries its condition's arguments
/// (see the module comment), and [`Self::of_condition`] renames the set.
#[derive(Debug, Clone, Copy)]
pub struct Args<'a> {
    action: ActionId,
    values: &'a LiteMap<Cow<'a, str>, Value<'a>>,
}

impl<'a> Args<'a> {
    /// The arguments of the operation itself.
    #[must_use]
    pub const fn of(op: &'a Op<'a>) -> Self {
        Self {
            action: op.action,
            values: &op.values,
        }
    }

    /// The same values, but under the name of the condition action.
    #[must_use]
    pub const fn of_condition(op: &'a Op<'a>, action: ActionId) -> Self {
        Self {
            action,
            values: &op.values,
        }
    }

    /// The action the arguments belong to.
    #[must_use]
    pub const fn action(self) -> ActionId {
        self.action
    }

    /// An argument's value as it is written, without evaluation.
    ///
    /// Needed where the argument is neither a variable reference nor text but a
    /// ready literal with a structure evaluation would not preserve: a sound
    /// ([`Value::Sound`]) carries pitch and volume, and they can be read only
    /// from the value itself.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(name = %name))]
    pub fn raw(self, name: &str) -> Option<&'a Value<'a>> {
        self.values.get(name)
    }
}
