//! Переменные: запись нескольких значений, случайный выбор из значений
//! и очистка переменных по именам.

use super::*;

/// `set_variable_multiple`: several assignments in one operation.
///
/// There must be as many targets as values: the compiler produces this
/// operation from `a, b = x, y` with the counts already matched, so a
/// discrepancy means the module was built wrong — and guessing what goes with
/// what is not an option.
///
/// # Errors
///
/// Returns [`RuntimeError::UnexpectedValue`] if the counts differ, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn multiple<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let targets = raw_items(args, op, "variables")?;
    let values = rt.values_arg(stream, args, "values")?;
    if targets.len() != values.len() {
        return Err(RuntimeError::UnexpectedValue {
            context: format!(
                "action '{}': {} target(s) for {} value(s)",
                schema::name(op.action),
                targets.len(),
                values.len()
            ),
            actual: "a different number of values than variables".to_owned(),
        });
    }
    for (target, value) in targets.iter().zip(values) {
        write_to(rt, stream, op.action, "variables", target, value)?;
    }
    Ok(Flow::Continue)
}

/// `set_variable_random`: a value picked from a list at random.
///
/// An empty list gives an empty value: there is nothing to choose from, and
/// that is not an error — the server behaves the same way.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn random<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let candidates = rt.values_arg(stream, args, "values")?;
        let Some(index) = rt.world().next_index(candidates.len()) else {
            return Ok(None);
        };
        Ok(candidates.into_iter().nth(index).flatten())
    })
}

/// `set_variable_purge`: removes the variables matching the given names.
///
/// Nothing but the names is required: `tests/pvp` purges a list of per-player
/// variables with no scope, mode or case flag at all, and the server then reads
/// them in the `GAME` scope, matching exactly and case-sensitively. Line
/// variables are never among the targets: a line variable lives for one frame,
/// and there is nothing to clean.
///
/// # Errors
///
/// Returns [`RuntimeError::Unimplemented`] for the `PART_CONTAINS` mode, which
/// the mock does not implement.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn purge<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let patterns: Vec<String> = rt
        .values_arg(stream, args, "names")?
        .iter()
        .map(value::display)
        .collect();
    let scope = match rt.optional_enum_arg(stream, args, "scope")? {
        Some("LOCAL") => VariableScope::Local,
        Some("SAVE") => VariableScope::Save,
        _ => VariableScope::Global,
    };
    let comparison = rt
        .optional_enum_arg(stream, args, "match")?
        .unwrap_or("EQUALS");
    let ignore_case = matches!(
        rt.optional_enum_arg(stream, args, "ignore_case")?,
        Some("TRUE")
    );
    if comparison == "PART_CONTAINS" {
        rt.unimplemented("the 'PART_CONTAINS' mode of 'set_variable_purge'")?;
        return Ok(Flow::Continue);
    }
    let store = rt.scope_store(stream, scope);
    for name in store.names() {
        let folded = if ignore_case {
            name.to_lowercase()
        } else {
            name.clone()
        };
        let matched = patterns.iter().any(|pattern| {
            let pattern = if ignore_case {
                pattern.to_lowercase()
            } else {
                pattern.clone()
            };
            match comparison {
                "EQUALS" => folded == pattern,
                "STARTS_WITH" => folded.starts_with(&pattern),
                "ENDS_WITH" => folded.ends_with(&pattern),
                _ => folded.contains(&pattern),
            }
        });
        if matched {
            store.remove(&name);
        }
    }
    Ok(Flow::Continue)
}
