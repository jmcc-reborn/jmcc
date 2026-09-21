//! Runtime values and the operations on them.
//!
//! The mock invents no data model of its own: a runtime value is the same
//! [`Value`] from [`jmcdata::module`] that describes the program. The only
//! difference is that in the compiler `Value::Variable` is a reference to a
//! variable, while in the mock it is an already computed value or `None`.
//!
//! A variable's value is an `Option<Value>`. The three states of a variable are
//! distinguishable and mean different things:
//!
//! * the key is not in [`Vars`] — the variable does not exist, and reading it is
//!   the error [`RuntimeError::UndefinedVariable`];
//! * the key is there, the value is `None` — the variable is declared but holds
//!   no value; reading it is the error [`RuntimeError::UnsetVariable`];
//! * the key is there, the value is `Some` — an ordinary read.
//!
//! The `None` inside a value arises where `JustMC` has no value: reading a
//! missing dictionary key, a selection with no target. Such a value can be
//! carried around and compared, but it cannot be used as a number, text or list
//! — that is a runtime error.

use std::borrow::Cow;

use jmcdata::module::{Number, TextParsing, TextValue, Value};
use litemap::LiteMap;
use ordered_float::OrderedFloat;

use crate::error::{Result, RuntimeError};

/// A variable's value. `None` means the variable exists but holds no value.
pub type Rt<'a> = Option<Value<'a>>;

/// The variable store of one scope. A missing key means the variable does not
/// exist: reading such a variable is an error.
pub type Vars<'a> = std::collections::BTreeMap<String, Rt<'a>>;

/// Whether a value counts as "empty" in the `JustMC` sense.
///
/// An empty value is [`None`] and [`Value::Error`]: the compiler writes `error`
/// where there is no value.
#[must_use]
pub const fn is_unset(value: &Rt<'_>) -> bool {
    matches!(value, None | Some(Value::Error))
}

/// The text a dictionary keys an entry by.
///
/// `JustMC` does not key a dictionary by the key's text: it keys it by the JSON
/// encoding of the key *value*, which is why a module's map literal carries
/// `{"type":"text","text":"a","parsing":"legacy"}` where the program wrote `"a"`.
/// A key written at run time has to be encoded the same way, or it could never be
/// found again — and a map literal in an argument would not match a key the same
/// program inserted.
///
/// An unset key encodes as an empty string, the same as its text form.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn encode_key(value: &Rt<'_>) -> String {
    value.as_ref().map_or_else(String::new, |value| {
        serde_json::to_string(value).unwrap_or_else(|_| display(&Some(value.clone())))
    })
}

/// The value a dictionary key stands for — the inverse of [`encode_key`].
///
/// A key that is not the JSON of a value is taken as plain text: the compiler
/// does not write such keys, but a hand-written module may.
#[must_use]
#[tracing::instrument(level = "trace", fields(key = %key))]
pub fn decode_key(key: &str) -> Value<'static> {
    let held = TextValue(key.to_owned());
    Value::try_from(&held).map_or_else(
        |_| Value::Text {
            text: Cow::Owned(key.to_owned()),
            parsing: TextParsing::Plain,
        },
        owning,
    )
}

/// Owns a value: every `Cow` that borrows gets its text copied out.
///
/// Needed where a value is built from a string that does not live as long as the
/// program — a dictionary key read back out of a map.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one arm per `Value` variant is the whole of what this function is"
)]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn owning(value: Value<'_>) -> Value<'static> {
    let text = |cow: Cow<'_, str>| Cow::Owned(cow.into_owned());
    match value {
        Value::Array { values } => Value::Array {
            values: values.into_iter().map(|item| item.map(owning)).collect(),
        },
        Value::Block { block } => Value::Block { block: text(block) },
        Value::Enum {
            value,
            variable,
            scope,
        } => Value::Enum {
            value: text(value),
            variable: variable.map(text),
            scope,
        },
        Value::Item { item } => Value::Item { item: text(item) },
        Value::Location {
            x,
            y,
            z,
            yaw,
            pitch,
        } => Value::Location {
            x,
            y,
            z,
            yaw,
            pitch,
        },
        Value::Map { values } => Value::Map {
            values: values
                .into_iter()
                .map(|(key, value)| (key, owning(value)))
                .collect(),
        },
        Value::Number { number } => Value::Number {
            number: match number {
                Number::Simple(number) => Number::Simple(number),
                Number::Calc(expression) => Number::Calc(text(expression)),
            },
        },
        Value::Particle {
            particle_type,
            count,
            first_spread,
            second_spread,
            x_motion,
            y_motion,
            z_motion,
            color,
            material,
            size,
            to_color,
        } => Value::Particle {
            particle_type: text(particle_type),
            count,
            first_spread,
            second_spread,
            x_motion,
            y_motion,
            z_motion,
            color,
            material: text(material),
            size,
            to_color,
        },
        Value::Potion {
            potion,
            amplifier,
            duration,
        } => Value::Potion {
            potion: text(potion),
            amplifier,
            duration,
        },
        Value::Sound {
            sound,
            pitch,
            volume,
            variaton,
            source,
        } => Value::Sound {
            sound: text(sound),
            pitch,
            volume,
            variaton: text(variaton),
            source: text(source),
        },
        Value::Text {
            text: body,
            parsing,
        } => Value::Text {
            text: text(body),
            parsing,
        },
        Value::Variable { variable, scope } => Value::Variable {
            variable: text(variable),
            scope,
        },
        Value::Vector { x, y, z } => Value::Vector { x, y, z },
        Value::GameValue {
            game_value,
            selection,
        } => Value::GameValue {
            game_value,
            selection: text(selection),
        },
        Value::Parameter {
            name,
            desc,
            param_type,
        } => Value::Parameter {
            name: text(name),
            desc: text(desc),
            param_type: owning_parameter(param_type),
        },
        Value::Error => Value::Error,
    }
}

/// [`owning`] for the parameter description a function signature carries.
#[tracing::instrument(level = "trace", skip(parameter))]
fn owning_parameter(
    parameter: jmcdata::module::Parameter<'_>,
) -> jmcdata::module::Parameter<'static> {
    use jmcdata::module::Parameter;
    let text = |cow: Cow<'_, str>| Cow::Owned(cow.into_owned());
    match parameter {
        Parameter::Singular {
            value_type,
            is_required,
            default_value,
            slot,
            description_slot,
        } => Parameter::Singular {
            value_type,
            is_required: text(is_required),
            default_value: text(default_value),
            slot,
            description_slot,
        },
        Parameter::Plural {
            value_type,
            is_required,
            default_value,
            slots,
            description_slots,
            ignore_empty_values,
        } => Parameter::Plural {
            value_type,
            is_required: text(is_required),
            default_value: text(default_value),
            slots: text(slots),
            description_slots: text(description_slots),
            ignore_empty_values: text(ignore_empty_values),
        },
        Parameter::Enum {
            slot,
            elements,
            default_value,
        } => Parameter::Enum {
            slot,
            elements: text(elements),
            default_value: text(default_value),
        },
    }
}

/// Builds a number value.
#[must_use]
pub const fn number<'a>(value: f64) -> Value<'a> {
    Value::Number {
        number: Number::from_f64(value),
    }
}

/// Builds a boolean value the way `JustMC` understands it.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn boolean<'a>(value: bool) -> Value<'a> {
    Value::Text {
        text: if value { "true" } else { "false" }.into(),
        parsing: TextParsing::Plain,
    }
}

/// Builds a text value.
#[must_use]
#[tracing::instrument(level = "trace", skip(value))]
pub fn text<'a>(value: impl Into<Cow<'a, str>>) -> Value<'a> {
    Value::Text {
        text: value.into(),
        parsing: TextParsing::Plain,
    }
}

/// Builds a list from ready values.
#[must_use]
#[tracing::instrument(level = "trace", skip(items))]
pub fn list<'a>(items: impl IntoIterator<Item = Value<'a>>) -> Value<'a> {
    Value::Array {
        values: items.into_iter().map(Some).collect(),
    }
}

/// Builds a dictionary from key-value pairs.
#[must_use]
#[tracing::instrument(level = "trace", skip(entries))]
pub fn dict<'a>(entries: impl IntoIterator<Item = (String, Value<'a>)>) -> Value<'a> {
    let mut values: LiteMap<TextValue, Value<'a>> = LiteMap::new();
    for (key, value) in entries {
        values.insert(TextValue(key), value);
    }
    Value::Map { values }
}

/// Builds a vector.
#[must_use]
pub const fn vector<'a>(x: f64, y: f64, z: f64) -> Value<'a> {
    Value::Vector {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

/// Builds a location.
#[must_use]
pub const fn location<'a>(x: f64, y: f64, z: f64, yaw: f64, pitch: f64) -> Value<'a> {
    Value::Location {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
        yaw: OrderedFloat(yaw),
        pitch: OrderedFloat(pitch),
    }
}

/// The kind of a value, for error messages.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn kind(value: &Value<'_>) -> String {
    match value {
        Value::Variable { .. } => "variable reference".to_owned(),
        other => other.type_name().into_owned(),
    }
}

/// Converts a value to a number.
///
/// Numbers are the numbers proper, plus text that parses as a number in full:
/// `JustMC` allows arithmetic on text, and the compiler relies on that.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn as_number(value: &Value<'_>) -> Option<f64> {
    match value {
        Value::Number { number } => Some(match number {
            Number::Simple(f) => f.0,
            // `Number::Calc` is a `%math()` expression, which the compiler never
            // produces; the mock cannot evaluate it.
            Number::Calc(_) => return None,
        }),
        Value::Text { text, .. } => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// A count — a length, an index, a step counter — as a floating-point number.
#[expect(
    clippy::cast_precision_loss,
    reason = "every count the mock keeps — a list or text length, an index, the step counter, a \
              selection size — is far below 2^53"
)]
pub(crate) const fn count_as_number(count: usize) -> f64 {
    count as f64
}

/// Converts a value to a number or reports that it is not one.
///
/// # Errors
///
/// Returns [`RuntimeError::NotANumber`] if the value is not a number.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_number(value: &Value<'_>, context: &str) -> Result<f64> {
    as_number(value).ok_or_else(|| RuntimeError::NotANumber {
        context: context.to_owned(),
        actual: kind(value),
    })
}

/// Converts a value to a string the way `%var%` in text would.
///
/// The conversion is partial: compound values have no text form here. That is
/// what is wanted where equality is decided or where a value has to fit an
/// enumeration. For an action argument there is [`argument_text`].
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn as_text<'a>(value: &Value<'a>) -> Option<Cow<'a, str>> {
    match value {
        Value::Text { text, .. } => Some(text.clone()),
        Value::Number { number } => Some(match number {
            Number::Simple(f) => Cow::Owned(format_number(f.0)),
            Number::Calc(_) => return None,
        }),
        Value::Enum { value, .. } => Some(value.clone()),
        Value::Item { item } => Some(item.clone()),
        Value::Block { block } => Some(block.clone()),
        _ => None,
    }
}

/// A number the way `JustMC` writes it: an integer without a fractional part.
#[tracing::instrument(level = "trace", fields(value = ?value))]
fn format_number(value: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the fractional part is already known to be zero; any such number fits in i64"
    )]
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// The text of an action argument.
///
/// Inside actions `JustMC` converts any value to text: the argument is declared
/// as text, and whatever landed in it is read as text. That is why a value of
/// class kind — and the compiler describes an instance as a list of fields —
/// that reached a text argument through `as text` is not an error. `as` in
/// `JustMC` does not convert a value: it marks the type for the compiler, while
/// the action itself does the conversion.
///
/// None of this concerns user code: function parameters, value comparison and
/// writing to a variable do not change the type, and there the value stays
/// itself.
///
/// Compound values are written in a form the mock makes up: it has no server
/// representation of a list, dictionary, location or vector, and cannot repeat
/// one. This is the only place in the mock where the text depends on the mock
/// rather than on the program.
///
/// # Errors
///
/// Returns [`RuntimeError::UnexpectedValue`] if the value has no text form at
/// all, as an unevaluated `%math()` does not.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn argument_text<'a>(value: &Value<'a>, context: &str) -> Result<Cow<'a, str>> {
    render_text(value)
        .map(Cow::Owned)
        .ok_or_else(|| RuntimeError::UnexpectedValue {
            context: context.to_owned(),
            actual: no_text_reason(value),
        })
}

/// Why a value has no text form.
#[tracing::instrument(level = "trace", fields(value = ?value))]
fn no_text_reason(value: &Value<'_>) -> String {
    match value {
        Value::Number {
            number: Number::Calc(_),
        } => "an unevaluated %math() expression".to_owned(),
        other => format!("a value of kind '{}' with no text form", kind(other)),
    }
}

/// The text of a value; `None` means the value has no text form.
#[tracing::instrument(level = "trace", fields(value = ?value))]
fn render_text(value: &Value<'_>) -> Option<String> {
    match value {
        Value::Array { values } => {
            let mut out = String::from("[");
            for (index, item) in values.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&render_text(item.as_ref()?)?);
            }
            out.push(']');
            Some(out)
        }
        Value::Map { values } => {
            let mut out = String::from("{");
            for (index, (key, item)) in values.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                // A key is stored as the JSON of a value; what a program sees is
                // the value.
                out.push_str(&render_text(&decode_key(&key.0))?);
                out.push_str(": ");
                out.push_str(&render_text(item)?);
            }
            out.push('}');
            Some(out)
        }
        Value::Location {
            x,
            y,
            z,
            yaw,
            pitch,
        } => Some(format!(
            "({}, {}, {}, {}, {})",
            x.0, y.0, z.0, yaw.0, pitch.0
        )),
        Value::Vector { x, y, z } => Some(format!("({}, {}, {})", x.0, y.0, z.0)),
        // An empty variable and `error` are an absence of value rather than a
        // value: they give an empty string, as `%var%` with an empty variable
        // does.
        Value::Error => Some(String::new()),
        other => as_text(other).map(Cow::into_owned),
    }
}

/// Converts a value to a string or reports that it is not text.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`] if the value does not convert to text.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_text<'a>(value: &Value<'a>, context: &str) -> Result<Cow<'a, str>> {
    as_text(value).ok_or_else(|| RuntimeError::NotText {
        context: context.to_owned(),
        actual: kind(value),
    })
}

/// Converts a value to a list.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the value is not a list.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_list<'a>(value: &Value<'a>, context: &str) -> Result<Vec<Rt<'a>>> {
    match value {
        Value::Array { values } => Ok(values.clone()),
        _ => Err(RuntimeError::NotAList {
            context: context.to_owned(),
            actual: kind(value),
        }),
    }
}

/// Converts a value to a dictionary.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAMap`] if the value is not a dictionary.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_map<'a>(value: &Value<'a>, context: &str) -> Result<LiteMap<TextValue, Value<'a>>> {
    match value {
        Value::Map { values } => Ok(values.clone()),
        _ => Err(RuntimeError::NotAMap {
            context: context.to_owned(),
            actual: kind(value),
        }),
    }
}

/// Converts a value to a location.
///
/// # Errors
///
/// Returns [`RuntimeError::NotALocation`] if the value is not a location.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_location(value: &Value<'_>, context: &str) -> Result<[f64; 5]> {
    match value {
        Value::Location {
            x,
            y,
            z,
            yaw,
            pitch,
        } => Ok([x.0, y.0, z.0, yaw.0, pitch.0]),
        Value::Array { values } => {
            if let Some(Some(first)) = values.first() {
                expect_location(first, context)
            } else {
                Err(RuntimeError::NotALocation {
                    context: context.to_owned(),
                    actual: "an empty array".to_owned(),
                })
            }
        }
        _ => Err(RuntimeError::NotALocation {
            context: context.to_owned(),
            actual: kind(value),
        }),
    }
}

/// Converts a value to a vector.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAVector`] if the value is not a vector.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn expect_vector(value: &Value<'_>, context: &str) -> Result<[f64; 3]> {
    match value {
        Value::Vector { x, y, z } => Ok([x.0, y.0, z.0]),
        _ => Err(RuntimeError::NotAVector {
            context: context.to_owned(),
            actual: kind(value),
        }),
    }
}

/// A boolean the way `JustMC` understands it: `true`/`false`, a number other
/// than zero, or non-empty text.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn as_bool(value: &Value<'_>) -> Option<bool> {
    match value {
        Value::Text { text, .. } => Some(matches!(text.as_ref(), "true" | "TRUE")),
        Value::Enum { value, .. } => Some(value.eq_ignore_ascii_case("true")),
        Value::Number { .. } => as_number(value).map(|n| n != 0.0),
        _ => None,
    }
}

/// Takes an enumeration argument and checks that it is in the allowed set.
///
/// Case does not matter: the compiler and the schema spell the values
/// differently, while user-supplied ones arrive already normalized.
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] if the value is not in the set
/// from the `jmcdata` schema.
#[tracing::instrument(level = "trace", fields(value = ?value, action = %action, arg = %arg, allowed = ?allowed))]
pub fn enum_argument(
    value: &Value<'_>,
    action: &'static str,
    arg: &'static str,
    allowed: &[&'static str],
) -> Result<&'static str> {
    let raw = as_text(value).ok_or_else(|| RuntimeError::InvalidEnumArgument {
        action,
        arg,
        value: kind(value),
        allowed: allowed.join(", "),
    })?;
    // `allowed` is always non-empty, otherwise the argument would not be an
    // enumeration.
    allowed
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(&raw))
        .ok_or_else(|| RuntimeError::InvalidEnumArgument {
            action,
            arg,
            value: raw.into_owned(),
            allowed: allowed.join(", "),
        })
}

/// Takes a boolean argument, which `JustMC` writes as `TRUE`/`FALSE`.
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] if the value is not
/// `TRUE`/`FALSE`.
#[tracing::instrument(level = "trace", fields(value = ?value, action = %action, arg = %arg))]
pub fn bool_argument(value: &Value<'_>, action: &'static str, arg: &'static str) -> Result<bool> {
    let raw = enum_argument(value, action, arg, &["TRUE", "FALSE"])?;
    Ok(raw == "TRUE")
}

/// Converts a variable's value to a number.
///
/// An empty variable reads as `0`: in `JustMC` `%var%` in a numeric slot gives
/// `0`, and the compiler relies on that — it freely passes into arithmetic a
/// temporary variable written on only one of the branches.
///
/// # Errors
///
/// Returns [`RuntimeError::NotANumber`] if the value is non-empty and not a
/// number.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn number_of(value: &Rt<'_>, context: &str) -> Result<f64> {
    value
        .as_ref()
        .map_or(Ok(0.0), |value| expect_number(value, context))
}

/// Converts a variable's value to text the way an action does.
///
/// An empty variable reads as empty text — for the same reason as
/// [`number_of`]. A non-empty value is converted by the rules of
/// [`argument_text`]: a text argument accepts any value.
///
/// # Errors
///
/// Returns [`RuntimeError::UnexpectedValue`] if the value has no text form at
/// all.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn text_of<'a>(value: &Rt<'a>, context: &str) -> Result<Cow<'a, str>> {
    value
        .as_ref()
        .map_or(Ok(Cow::Borrowed("")), |value| argument_text(value, context))
}

/// Converts a variable's value to a boolean.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`] if the value is non-empty and does not
/// convert to a boolean.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn bool_of(value: &Rt<'_>, context: &str) -> Result<bool> {
    value.as_ref().map_or(Ok(false), |value| {
        as_bool(value).ok_or_else(|| RuntimeError::NotText {
            context: context.to_owned(),
            actual: kind(value),
        })
    })
}

/// Converts a variable's value to a list.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the value is non-empty and not a list.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn list_of<'a>(value: &Rt<'a>, context: &str) -> Result<Vec<Rt<'a>>> {
    value
        .as_ref()
        .map_or(Ok(Vec::new()), |value| expect_list(value, context))
}

/// Converts a variable's value to a dictionary.
///
/// An empty dictionary is built as an empty [`LiteMap`]; this type has no other
/// way to get one.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAMap`] if the value is non-empty and not a
/// dictionary.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn map_of<'a>(value: &Rt<'a>, context: &str) -> Result<LiteMap<TextValue, Value<'a>>> {
    value
        .as_ref()
        .map_or(Ok(LiteMap::new()), |value| expect_map(value, context))
}

/// Converts a variable's value to a location.
///
/// Unlike numbers and text, an empty variable is an error here: a location has
/// no obvious "zero" value, and substituting wrong coordinates is worse than
/// stopping.
///
/// # Errors
///
/// Returns [`RuntimeError::NotALocation`] both for an empty variable and for a
/// non-empty value that is not a location.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn location_of(value: &Rt<'_>, context: &str) -> Result<[f64; 5]> {
    let value = value.as_ref().ok_or_else(|| RuntimeError::NotALocation {
        context: context.to_owned(),
        actual: "an empty variable".to_owned(),
    })?;
    expect_location(value, context)
}

/// Converts a variable's value to a vector.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAVector`] both for an empty variable and for a
/// non-empty value that is not a vector.
#[tracing::instrument(level = "trace", fields(value = ?value, context = %context))]
pub fn vector_of(value: &Rt<'_>, context: &str) -> Result<[f64; 3]> {
    let value = value.as_ref().ok_or_else(|| RuntimeError::NotAVector {
        context: context.to_owned(),
        actual: "an empty variable".to_owned(),
    })?;
    expect_vector(value, context)
}

/// Compares two values the way `if_variable_equals` does.
///
/// Numbers compare as numbers, everything else by text representation, and if
/// at least one side has no text form (a list, a dictionary, a location) then
/// structurally. So `5` and `"5"` are equal, as in `JustMC`, and so are two
/// identical lists.
#[must_use]
#[tracing::instrument(level = "trace", fields(left = ?left, right = ?right))]
pub fn equals(left: &Rt<'_>, right: &Rt<'_>) -> bool {
    if let (Some(a), Some(b)) = (left, right) {
        if matches!(a, Value::Number { .. }) && matches!(b, Value::Number { .. }) {
            return as_number(a) == as_number(b);
        }
        if let (Some(a), Some(b)) = (as_text(a), as_text(b)) {
            return a == b;
        }
        return a == b;
    }
    if left.is_none() && right.is_none() {
        return true;
    }
    let non_empty = if let Some(a) = left {
        a
    } else if let Some(b) = right {
        b
    } else {
        return true;
    };
    if as_number(non_empty) == Some(0.0) {
        return true;
    }
    if let Some(t) = as_text(non_empty) {
        return t.is_empty();
    }
    false
}

/// The text representation of a value for messages and comparisons. For an
/// empty variable it is an empty string.
///
/// Compound values are rendered by [`render_text`], the same form an action's
/// text argument gets: a list inside `"$var$"` has to read as the list, not as
/// `<array>`, or a program that prints its own data sees nothing useful.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn display(value: &Rt<'_>) -> String {
    value.as_ref().map_or_else(String::new, |value| {
        render_text(value).unwrap_or_else(|| format!("<{}>", kind(value)))
    })
}

/// Turns a ready value into [`Rt`]. Lists and dictionaries are built from ready
/// [`Value`]s, so their elements are always non-empty.
#[must_use]
pub const fn rt(value: Value<'_>) -> Rt<'_> {
    Some(value)
}

/// Converts an integer to a list index.
///
/// # Errors
///
/// Returns [`RuntimeError::IndexOutOfRange`] if the index is negative or beyond
/// the end of the list.
#[tracing::instrument(level = "trace", fields(index = ?index, len = ?len, context = %context))]
pub fn expect_index(index: f64, len: usize, context: &str) -> Result<usize> {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a list index in JustMC is an integer; the fractional part is dropped"
    )]
    let index = index.trunc() as i64;
    if index < 0 || index as u128 >= len as u128 {
        return Err(RuntimeError::IndexOutOfRange {
            context: context.to_owned(),
            index,
            len,
        });
    }
    #[expect(
        clippy::cast_sign_loss,
        reason = "a negative index was ruled out by the check above"
    )]
    Ok(index as usize)
}
