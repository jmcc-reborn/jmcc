//! The `variable` object actions: arithmetic, lists, maps and text.
//!
//! Every action in this group has a target variable — a `Value::Variable` in
//! the `variable` argument. The remaining arguments are the sources of the
//! value. That gives three shapes the handlers are built on:
//!
//! * [`assign`] — compute a value and put it into the target;
//! * [`assign_over`] — read the target's current value and build a new one from
//!   it (incrementing, appending to a list, removing from a map);
//! * [`multiple`] — write several values into several targets
//!   (`set_variable_multiple`, `set_variable_get_all_coordinates`).
//!
//! # Indices count from zero
//!
//! `JustMC` numbers list elements from 0. [`ActionId::SetVariableGetListValue`]
//! stands apart: it has a `default_value` argument, and that means an index
//! past the end is not an error but the default value. Writing by index has no
//! default, so going past the end stops execution there
//! ([`RuntimeError::IndexOutOfRange`]).
//!
//! # What is not here
//!
//! The actions over items (`set_variable_get_item_*`) and over text components
//! (`set_variable_parse_to_component`) rest on models of items and components
//! that the mock does not have. They are reported unimplemented rather than
//! returning an empty value: an empty value in a variable looks like a
//! legitimate result and would send the program down the wrong branch.
//!
//! # Как разложены обработчики
//!
//! Точка входа [`dispatch`] и общие помощники ([`assign`],
//! [`assign_over`], [`write_to`], [`raw_items`], [`index_in`]) живут
//! здесь, а обработчики разложены по смыслу — по имени файла видно,
//! где искать:
//!
//! | Файл | Что в нём |
//! |---|---|
//! | [`arithmetic`] | числа: арифметика, округление, биты, тригонометрия |
//! | [`lists`] | списки: доступ, вставка, удаление, длина, обрезка |
//! | [`maps`] | словари: ключ, значение, размер, список ключей |
//! | [`text`] | текст: символ, длина, разбор числа |
//! | [`state`] | переменные: несколько целей, случайный выбор, очистка |

use jmcdata::generated::ActionId;
use jmcdata::module::{Op, TextParsing, TextValue, Value, VariableScope};

use crate::error::{Result, RuntimeError};
use crate::interp::{Args, Flow, at_op, invalid_enum};
use crate::run::{Runtime, Stream};
use crate::schema;
use crate::value::{self, Rt, count_as_number};

mod arithmetic;
pub mod condition;
mod lists;
mod location;
mod maps;
mod numeric;
mod state;
mod text;
mod vector;

use arithmetic::*;
use lists::*;
use maps::*;
use state::*;
use text::*;

/// Entry point for the `variable` group.
///
/// # Errors
///
/// Returns any runtime error; an unimplemented action gives
/// [`RuntimeError::Unimplemented`].
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    // The groups added by whole files keep this match from growing past what a
    // reader can hold: each answers `None` for everything that is not its own.
    if let Some(flow) = vector::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = location::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = numeric::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = lists::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = maps::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = text::dispatch(rt, stream, op) {
        return flow;
    }
    match op.action {
        ActionId::SetVariableValue => assign(rt, stream, op, |rt, stream, args| {
            rt.arg(stream, args, "value")
        }),
        // The `*_dummy` actions are the template's empty slots: in JustMC they
        // do nothing at all.
        ActionId::SetVariableDummy => Ok(Flow::Continue),
        ActionId::SetVariableMultiple => multiple(rt, stream, op),
        ActionId::SetVariableRandom => random(rt, stream, op),

        ActionId::SetVariableAdd => fold(rt, stream, op, "value", |acc, next| acc + next),
        ActionId::SetVariableSubtract => fold(rt, stream, op, "value", |acc, next| acc - next),
        ActionId::SetVariableMultiply => fold(rt, stream, op, "value", |acc, next| acc * next),
        ActionId::SetVariableDivide => divide(rt, stream, op),
        ActionId::SetVariableIncrement => {
            fold_over(rt, stream, op, "number", |acc, next| acc + next)
        }
        ActionId::SetVariableDecrement => {
            fold_over(rt, stream, op, "number", |acc, next| acc - next)
        }
        ActionId::SetVariableMin => fold(rt, stream, op, "value", f64::min),
        ActionId::SetVariableMax => fold(rt, stream, op, "value", f64::max),

        ActionId::SetVariableAbsolute => unary(rt, stream, op, "number", f64::abs),
        ActionId::SetVariableRound => round(rt, stream, op),
        ActionId::SetVariablePow => binary(rt, stream, op, "base", "power", f64::powf),
        ActionId::SetVariableRoot => root(rt, stream, op),
        // `f64::log` takes the base as its second argument, so the logarithm is
        // an ordinary two-argument computation.
        ActionId::SetVariableLog => binary(rt, stream, op, "number", "base", f64::log),
        ActionId::SetVariableRemainder => remainder(rt, stream, op),
        ActionId::SetVariableClamp => clamp(rt, stream, op),
        ActionId::SetVariableRandomNumber => random_number(rt, stream, op),
        ActionId::SetVariableBitwiseOperation => bitwise(rt, stream, op),
        ActionId::SetVariableSine => trigonometry(rt, stream, op, true),
        ActionId::SetVariableCosine => trigonometry(rt, stream, op, false),

        ActionId::SetVariableCreateList => create_list(rt, stream, op),
        ActionId::SetVariableGetListValue => get_list_value(rt, stream, op),
        ActionId::SetVariableSetListValue => set_list_value(rt, stream, op),
        ActionId::SetVariableInsertListValue => insert_list_value(rt, stream, op),
        ActionId::SetVariableGetListLength => list_length(rt, stream, op),
        ActionId::SetVariableGetListRandomValue => list_random_value(rt, stream, op),
        ActionId::SetVariableRemoveListValue => remove_list_value(rt, stream, op),
        ActionId::SetVariableRemoveListValueAtIndex => remove_list_value_at_index(rt, stream, op),
        ActionId::SetVariableRemoveListDuplicates => remove_list_duplicates(rt, stream, op),
        ActionId::SetVariableAppendValue => append_value(rt, stream, op),
        ActionId::SetVariableAppendList => append_list(rt, stream, op),
        ActionId::SetVariableTrimList => trim_list(rt, stream, op),

        ActionId::SetVariableCreateMapFromValues => create_map_from_values(rt, stream, op),
        ActionId::SetVariableGetMapValue => get_map_value(rt, stream, op),
        ActionId::SetVariableSetMapValue => set_map_value(rt, stream, op),
        ActionId::SetVariableGetMapSize => map_size(rt, stream, op),
        ActionId::SetVariableGetMapKeys => map_keys(rt, stream, op),

        ActionId::SetVariableGetCharAt => get_char_at(rt, stream, op),
        ActionId::SetVariableTextLength => text_length(rt, stream, op),
        ActionId::SetVariableConvertTextToNumber => convert_text_to_number(rt, stream, op),

        ActionId::SetVariablePurge => purge(rt, stream, op),

        _other => rt.unimplemented_op(stream, op),
    }
}

/// Computes a value and writes it into the target variable.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, compute))]
fn assign<'a, F>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    compute: F,
) -> Result<Flow>
where
    F: FnOnce(&mut Runtime<'a>, &mut Stream<'a>, Args<'a>) -> Result<Rt<'a>>,
{
    let args = Args::of(op);
    let (scope, name) = rt.target_of(stream, args, "variable")?;
    let value = compute(rt, stream, args)?;
    rt.scope_store(stream, scope).set(&name, value);
    Ok(Flow::Continue)
}

/// Builds a new value for the target out of its current value.
///
/// # Errors
///
/// Returns [`RuntimeError::UndefinedVariable`] if the target does not exist
/// yet, and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, compute))]
fn assign_over<'a, F>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    compute: F,
) -> Result<Flow>
where
    F: FnOnce(&mut Runtime<'a>, &mut Stream<'a>, Args<'a>, Rt<'a>) -> Result<Rt<'a>>,
{
    let args = Args::of(op);
    let (scope, name) = rt.target_of(stream, args, "variable")?;
    let store = rt.scope_store(stream, scope);
    // The previous value may legitimately be absent: a `game` variable another
    // handler is yet to write reads as empty.
    let current = store.read_or_empty(&name, scope)?;
    let value = compute(rt, stream, args, current)?;
    store.set(&name, value);
    Ok(Flow::Continue)
}

/// Writes a value into the variable named by a `Value::Variable`.
///
/// # Errors
///
/// Returns [`RuntimeError::ExpectedVariable`] if the value is not a variable.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(action = ?action, arg = %arg, value = ?value, target = ?target))]
fn write_to<'a>(
    rt: &mut Runtime<'a>,
    stream: &Stream<'a>,
    action: ActionId,
    arg: &'static str,
    value: &'a Value<'a>,
    target: Rt<'a>,
) -> Result<()> {
    let (scope, name) = rt.variable_target(stream, value, action, arg)?;
    rt.scope_store(stream, scope).set(&name, target);
    Ok(())
}

/// The raw elements of an argument, without evaluating them.
///
/// Needed where the elements are not values but variable names
/// (`set_variable_multiple`) or key/value pairs
/// (`set_variable_set_map_value`): evaluating them would mean reading the
/// variables the action is about to write.
///
/// # Errors
///
/// Returns [`RuntimeError::MissingArgument`] if the argument is absent.
#[tracing::instrument(level = "trace", skip(args, op), fields(name = %name))]
fn raw_items<'a>(args: Args<'a>, op: &'a Op<'a>, name: &'static str) -> Result<Vec<&'a Value<'a>>> {
    match op.values.get(name) {
        Some(Value::Array { values }) => Ok(values.iter().flatten().collect()),
        Some(other) => Ok(vec![other]),
        None => Err(RuntimeError::MissingArgument {
            action: schema::name(args.action()),
            arg: name,
        }),
    }
}

/// An index within `0..len`, if the number looks like one.
///
/// A negative index, a fractional number and an index past the end give `None`:
/// the caller decides what that means — a default value or an error.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a list index in JustMC is an integer; the fraction is dropped"
)]
#[tracing::instrument(level = "trace", fields(index = ?index, len = ?len))]
fn index_in(index: f64, len: usize) -> Option<usize> {
    let index = index.trunc() as i64;
    usize::try_from(index).ok().filter(|index| *index < len)
}
