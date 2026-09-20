//! Text values the way `JustMC` understands them.
//!
//! A text value carries a parsing mode: `plain`, `legacy`, `minimessage` or
//! `json`. The mock draws nothing on screen, but it still has to:
//!
//! * substitute variable and game-value placeholders (`%var(x)`, `%player%`,
//!   `%random%`, …) — otherwise the log would show something other than what
//!   actually happened;
//! * strip `legacy` colour codes, so comparisons and length counts mean
//!   something;
//! * report `%math()` as unimplemented: its evaluator is not part of the mock,
//!   and quietly returning the raw text would hide the gap.
//!
//! # A placeholder is a variable read
//!
//! `%var(x)%` reads a variable exactly like `Value::Variable` in an action
//! argument, so a missing variable is the same error
//! ([`RuntimeError::UndefinedVariable`](crate::RuntimeError::UndefinedVariable)).
//! An unknown placeholder is a different thing: `%something%` is left alone,
//! because it may be a game value the mock does not model, while `%var(x)%` is
//! unambiguously a variable read.
//!
//! # Nested placeholders
//!
//! A variable name may itself be a placeholder: the compiler emits
//! `game var %player%_dialog` — one variable per player. So a `%` inside
//! parentheses does not close the placeholder, and the argument is substituted
//! recursively: `%var(%player%_dialog)%` reads `Dev_dialog`.
//!
//! # The closing `%` may be missing
//!
//! The compiler writes a variable's value into text as `%var(x)` — with no
//! closing `%`. That is how `"${x}"` expands inside a literal:
//! `"${owner}_victim"` becomes `%var(owner)_victim`. So besides the main
//! `%name%` form the mock also accepts the call form `%name(argument)` up to
//! its closing parenthesis.

use std::borrow::Cow;

use jmcdata::module::{TextParsing, Value, VariableScope};

use crate::error::Result;
use crate::run::{Runtime, Stream};

/// Renders a text value: substitutes placeholders and strips colour codes
/// according to `parsing`.
///
/// # Errors
///
/// Returns [`RuntimeError::UndefinedVariable`](crate::RuntimeError::UndefinedVariable)
/// if a placeholder reads a variable that does not exist, and
/// [`RuntimeError::Unimplemented`](crate::RuntimeError::Unimplemented) if the
/// text contains `%math()`.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(raw = %raw, parsing = ?parsing))]
pub(crate) fn render<'a>(
    rt: &mut Runtime<'a>,
    stream: &Stream<'a>,
    raw: &str,
    parsing: TextParsing,
) -> Result<String> {
    if raw.contains("%math(") {
        rt.unimplemented("the %math() expression")?;
        return Ok(raw.to_owned());
    }
    let substituted = substitute_placeholders(rt, stream, raw)?;
    Ok(match parsing {
        TextParsing::Legacy => strip_legacy_codes(&substituted),
        TextParsing::Plain | TextParsing::MiniMessage | TextParsing::Json => substituted,
    })
}

/// Substitutes `JustMC` placeholders, except `%math()`, which is handled by the
/// caller.
///
/// # Errors
///
/// Returns [`RuntimeError::UndefinedVariable`](crate::RuntimeError::UndefinedVariable)
/// if a placeholder reads a variable that does not exist.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(raw = %raw))]
pub(crate) fn substitute_placeholders<'a>(
    rt: &Runtime<'a>,
    stream: &Stream<'a>,
    raw: &str,
) -> Result<String> {
    if !raw.contains('%') {
        return Ok(raw.to_owned());
    }

    let mut out = String::with_capacity(raw.len());
    let mut index = 0;

    while index < raw.len() {
        let Some(open) = raw[index..].find('%').map(|offset| index + offset) else {
            out.push_str(&raw[index..]);
            break;
        };
        out.push_str(&raw[index..open]);

        // The call form, `%name(argument)`, has no closing `%`.
        if let Some((token, mut end)) = call_placeholder(raw, open) {
            let replacement = resolve_placeholder(rt, stream, token)?;
            if raw[end..].starts_with('%')
                && call_placeholder(raw, end).is_none()
                && closing_percent(raw, end).is_none()
            {
                end += 1;
            }
            push_placeholder(&mut out, replacement, token, false);
            index = end;
            continue;
        }

        // The main form, `%name%`.
        if let Some(close) = closing_percent(raw, open) {
            let token = &raw[open + 1..close];
            push_placeholder(
                &mut out,
                resolve_placeholder(rt, stream, token)?,
                token,
                true,
            );
            index = close + 1;
            continue;
        }

        // An unclosed `%` is plain text: that is what a percent sign in a
        // program looks like, not a placeholder.
        out.push_str(&raw[open..]);
        break;
    }

    Ok(out)
}

/// Appends a substitution, or the unknown placeholder back as it was: that is
/// what `JustMC` does.
///
/// `terminated` records whether the placeholder had a closing `%`: the call
/// form did not, and adding one would change the program's text.
#[tracing::instrument(level = "trace", fields(out = %out, replacement = ?replacement, token = %token, terminated = ?terminated))]
fn push_placeholder(out: &mut String, replacement: Option<String>, token: &str, terminated: bool) {
    if let Some(value) = replacement {
        out.push_str(&value);
        return;
    }
    out.push('%');
    out.push_str(token);
    if terminated {
        out.push('%');
    }
}

/// Finds the `%` closing the placeholder opened at `open`.
///
/// A `%` inside parentheses does not close it: a nested `%var(…)%` sits there
/// and is substituted recursively. Without that, `%var(%player%_dialog)%` would
/// fall apart into three pieces and never read `Dev_dialog`.
#[tracing::instrument(level = "trace", fields(raw = %raw, open = ?open))]
fn closing_percent(raw: &str, open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (offset, byte) in raw.as_bytes()[open + 1..].iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'%' if depth == 0 => return Some(open + 1 + offset),
            _ => {}
        }
    }
    None
}

/// Finds the call form `%name(argument)`, which has no closing `%`.
///
/// Returns the placeholder without its leading `%` and the position just past
/// its closing parenthesis.
#[tracing::instrument(level = "trace", fields(raw = %raw, open = ?open))]
fn call_placeholder(raw: &str, open: usize) -> Option<(&str, usize)> {
    let rest = &raw[open + 1..];
    let paren = rest.find('(')?;
    let name = &rest[..paren];
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    let mut depth = 0_usize;
    for (offset, byte) in raw[open + 1 + paren..].bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = open + paren + offset + 2;
                    return Some((&raw[open + 1..end], end));
                }
            }
            _ => {}
        }
    }
    None
}

/// Resolves one placeholder. `None` means the placeholder is unknown and should
/// stay in the text as it is: that is what `JustMC` does.
///
/// # Errors
///
/// Returns [`RuntimeError::UndefinedVariable`](crate::RuntimeError::UndefinedVariable)
/// if a placeholder of the form `%var*(…)` reads a variable that does not
/// exist.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(token = %token))]
fn resolve_placeholder<'a>(
    rt: &Runtime<'a>,
    stream: &Stream<'a>,
    token: &str,
) -> Result<Option<String>> {
    let (name, argument) = split_call(token);

    let scope = match name {
        "var" => Some(VariableScope::Global),
        "var_line" => Some(VariableScope::Line),
        "var_local" => Some(VariableScope::Local),
        "var_save" => Some(VariableScope::Save),
        _ => None,
    };

    if let (Some(scope), Some(argument)) = (scope, argument) {
        let name = substitute_placeholders(rt, stream, argument)?;
        let value = rt.scope_store(stream, scope).read_or_empty(&name, scope)?;
        return Ok(Some(crate::value::display(&value)));
    }

    // `%player%` and friends look at the frame's targets, not at the world:
    // inside a `select` block "the player" is the selection, not the first
    // player on the server.
    let target = stream
        .targets
        .first()
        .copied()
        .or_else(|| rt.world().primary_target());
    let world = rt.world();
    let selected = stream.selection.first().copied();
    Ok(match name {
        "player" | "entity" | "display_name" => target.map(|t| world.target_name(t)),
        "selected" => selected.map(|t| world.target_name(t)),
        "player_uuid" | "entity_uuid" | "uuid" | "victim_uuid" | "damager_uuid"
        | "shooter_uuid" | "killer_uuid" | "selected_uuid" => {
            target.map(|t| world.target_uuid(t).to_owned())
        }
        // The mock has no victim, damager or killer: it does not simulate
        // damage, and putting the frame's target here would pass it off as a
        // participant in the event. `empty` is empty by definition.
        "victim" | "damager" | "shooter" | "killer" | "empty" => Some(String::new()),
        "space" => Some(" ".to_owned()),
        "random" => Some(world.next_random().to_string()),
        "random_uuid" => Some(world.next_random_uuid()),
        "player_amount" | "online" | "global_online" => Some(world.players().len().to_string()),
        "entity_count" | "entity_amount" => Some(world.entities().len().to_string()),
        "time" => Some(world.tick().to_string()),
        "worlds" => Some(world.world_name().to_owned()),
        "damage" => Some("0".to_owned()),
        _ => None,
    })
}

/// Splits `var(x)` into a name and an argument.
#[tracing::instrument(level = "trace", fields(token = %token))]
fn split_call(token: &str) -> (&str, Option<&str>) {
    match token.find('(') {
        Some(open) if token.ends_with(')') => {
            (&token[..open], Some(&token[open + 1..token.len() - 1]))
        }
        _ => (token, None),
    }
}

/// Strips `legacy` colour codes: `&a`, `&l`, `&#RRGGBB`.
#[must_use]
#[tracing::instrument(level = "trace", fields(raw = %raw))]
pub fn strip_legacy_codes(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'#') {
            // `&#RRGGBB` — six hexadecimal digits.
            let mut hex = String::new();
            for _ in 0..6 {
                match chars.peek() {
                    Some(digit) if digit.is_ascii_hexdigit() => {
                        hex.push(*digit);
                        chars.next();
                    }
                    _ => break,
                }
            }
            if hex.len() == 6 {
                continue;
            }
            out.push('&');
            out.push('#');
            out.push_str(&hex);
            continue;
        }
        match chars.peek() {
            Some(code) if is_legacy_code(*code) => {
                chars.next();
            }
            _ => out.push('&'),
        }
    }
    out
}

/// Reports whether `code` is a `legacy` formatting code.
const fn is_legacy_code(code: char) -> bool {
    matches!(
        code.to_ascii_lowercase(),
        '0'..='9' | 'a'..='f' | 'k'..='o' | 'r'
    )
}

/// Builds a `JustMC` text value with the given parsing mode.
#[must_use]
#[tracing::instrument(level = "trace", skip(value), fields(parsing = ?parsing))]
pub fn text_value(value: impl Into<String>, parsing: TextParsing) -> Value<'static> {
    Value::Text {
        text: Cow::Owned(value.into()),
        parsing,
    }
}
