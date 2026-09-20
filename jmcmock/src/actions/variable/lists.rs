//! Списки: элемент по индексу, вставка, удаление, длина, обрезка,
//! случайный элемент, объединение и создание.

use super::*;

/// Списочные действия. `None` — действие не из этой группы.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов или неизвестного значения
/// перечисления.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn dispatch<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Option<Result<Flow>> {
    Some(match op.action {
        ActionId::SetVariableFlattenList => flatten(rt, stream, op),
        ActionId::SetVariableReverseList => reverse(rt, stream, op),
        ActionId::SetVariableSortAnyList => sort(rt, stream, op),
        ActionId::SetVariableRandomizeListOrder => shuffle(rt, stream, op),
        ActionId::SetVariableGetListIndexOfValue => index_of_value(rt, stream, op),
        ActionId::SetVariableGetListVariables => variables(rt, stream, op),
        _ => return None,
    })
}

/// `set_variable_flatten_list`: разворачивает вложенные списки.
///
/// `deep = TRUE` разворачивает и вложенные списки внутри списков, `FALSE` —
/// только верхний уровень.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn flatten<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let deep = matches!(rt.optional_enum_arg(stream, args, "deep")?, Some("TRUE"));
        let mut flat = Vec::with_capacity(items.len());
        for item in items {
            push_flattened(item, deep, &mut flat);
        }
        Ok(Some(Value::Array { values: flat }))
    })
}

/// Кладёт значение в список, разворачивая вложенные списки.
#[tracing::instrument(level = "trace", fields(item = ?item, deep = ?deep, out = ?out))]
fn push_flattened<'a>(item: Rt<'a>, deep: bool, out: &mut Vec<Rt<'a>>) {
    match item {
        Some(Value::Array { values }) => {
            for value in values {
                if deep {
                    push_flattened(value, deep, out);
                } else {
                    out.push(value);
                }
            }
        }
        other => out.push(other),
    }
}

/// `set_variable_reverse_list`: переворачивает список.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn reverse<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = rt.list_arg(stream, args, "list")?;
        items.reverse();
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_sort_any_list`: сортировка списка.
///
/// Числа сравниваются как числа, всё остальное — по текстовому виду: так же,
/// как `JustMC` сравнивает значения в условии.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном порядке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn sort<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = rt.list_arg(stream, args, "list")?;
        let descending = match rt.enum_arg(stream, args, "sort_mode")? {
            "ASCENDING" => false,
            "DESCENDING" => true,
            other => return Err(invalid_enum(args, "sort_mode", other.to_owned())),
        };
        items.sort_by(|left, right| {
            let order = compare_values(left, right);
            if descending { order.reverse() } else { order }
        });
        Ok(Some(Value::Array { values: items }))
    })
}

/// Порядок двух значений: числа — как числа, остальное — как текст.
#[tracing::instrument(level = "trace", fields(left = ?left, right = ?right))]
pub(super) fn compare_values(left: &Rt<'_>, right: &Rt<'_>) -> std::cmp::Ordering {
    if let (Some(Value::Number { .. }), Some(Value::Number { .. })) =
        (left.as_ref(), right.as_ref())
    {
        let left = left.as_ref().and_then(value::as_number).unwrap_or(0.0);
        let right = right.as_ref().and_then(value::as_number).unwrap_or(0.0);
        return left.total_cmp(&right);
    }
    value::display(left).cmp(&value::display(right))
}

/// `set_variable_randomize_list_order`: перемешивание списка.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shuffle<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = rt.list_arg(stream, args, "list")?;
        // Fisher — Yates: every permutation is equally likely, and the draws go
        // through the same generator as the other random actions.
        for position in (1..items.len()).rev() {
            let Some(index) = rt.world().next_index(position + 1) else {
                continue;
            };
            items.swap(position, index);
        }
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_get_list_index_of_value`: позиция значения в списке.
///
/// Значение, которого нет, даёт `-1`: у действия нет аргумента со значением по
/// умолчанию, и это единственный способ сказать «не найдено».
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме поиска и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn index_of_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let value = rt.arg(stream, args, "value")?;
        let found = match rt.enum_arg(stream, args, "search_mode")? {
            "FIRST" => items.iter().position(|item| value::equals(item, &value)),
            "LAST" => items.iter().rposition(|item| value::equals(item, &value)),
            other => return Err(invalid_enum(args, "search_mode", other.to_owned())),
        };
        let index = found.map_or(-1.0, count_as_number);
        Ok(Some(value::number(index)))
    })
}

/// `set_variable_get_list_variables`: имена переменных области как список.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной области и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn variables<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let scope = match rt.enum_arg(stream, args, "scope")? {
            "GAME" => VariableScope::Global,
            "LOCAL" => VariableScope::Local,
            "SAVE" => VariableScope::Save,
            other => return Err(invalid_enum(args, "scope", other.to_owned())),
        };
        let names = rt
            .scope_store(stream, scope)
            .names()
            .into_iter()
            .map(|name| Some(value::text(name)))
            .collect();
        Ok(Some(Value::Array { values: names }))
    })
}

/// `set_variable_create_list`: builds a list out of values.
///
/// The list is created anew rather than appended to: in `JustMC` the action
/// clears an existing list if there was one.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn create_list<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.values_arg(stream, args, "values")?;
        Ok(Some(Value::Array {
            values: items.into_iter().collect(),
        }))
    })
}

/// `set_variable_get_list_value`: the element of a list at an index.
///
/// An index past the end gives `default_value` if the compiler wrote one, and
/// an empty value if not: that is what the argument exists for.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn get_list_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let index = rt.number_arg(stream, args, "number")?;
        Ok(match index_in(index, items.len()) {
            Some(index) => items.into_iter().nth(index).flatten(),
            None => rt.optional_arg(stream, args, "default_value")?.flatten(),
        })
    })
}

/// `set_variable_set_list_value`: replaces the element of a list at an index.
///
/// Writing has no default value, so an index past the end is an error:
/// returning the list unchanged would hide that nothing was written.
///
/// # Errors
///
/// Returns [`RuntimeError::IndexOutOfRange`] if the index is outside the list,
/// and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_list_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = rt.list_arg(stream, args, "list")?;
        let index = rt.number_arg(stream, args, "number")?;
        let value = rt.arg(stream, args, "value")?;
        let index = value::expect_index(index, items.len(), &at_op(args, "number"))?;
        items[index] = value;
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_insert_list_value`: inserts a value, shifting the tail right.
///
/// An index equal to the length of the list is a legitimate append.
///
/// # Errors
///
/// Returns [`RuntimeError::IndexOutOfRange`] if the index is outside
/// `0..=len`, and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn insert_list_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = rt.list_arg(stream, args, "list")?;
        let index = rt.number_arg(stream, args, "number")?;
        let value = rt.arg(stream, args, "value")?;
        let index = index_in(index, items.len().saturating_add(1)).ok_or_else(|| {
            RuntimeError::IndexOutOfRange {
                context: at_op(args, "number"),
                index: index.trunc() as i64,
                len: items.len(),
            }
        })?;
        items.insert(index, value);
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_get_list_length`: how many elements a list has.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn list_length<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        Ok(Some(value::number(count_as_number(items.len()))))
    })
}

/// `set_variable_get_list_random_value`: a random element of a list.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn list_random_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        if items.is_empty() {
            return Ok(None);
        }
        let unit = rt.world().next_unit();
        let index = index_in((unit * count_as_number(items.len())).floor(), items.len());
        Ok(index
            .and_then(|index| items.into_iter().nth(index))
            .flatten())
    })
}

/// `set_variable_remove_list_value`: removes the values equal to the given one.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remove_list_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let value = rt.arg(stream, args, "value")?;
        let mode = rt.optional_enum_arg(stream, args, "remove_mode")?;
        let position = |items: &[Rt<'a>]| match mode {
            Some("FIRST") => items
                .iter()
                .position(|candidate| value::equals(candidate, &value)),
            Some("LAST") => items
                .iter()
                .rposition(|candidate| value::equals(candidate, &value)),
            _ => None,
        };
        let kept = if mode == Some("FIRST") || mode == Some("LAST") {
            let mut kept = items;
            if let Some(index) = position(&kept) {
                kept.remove(index);
            }
            kept
        } else {
            items
                .into_iter()
                .filter(|candidate| !value::equals(candidate, &value))
                .collect()
        };
        Ok(Some(Value::Array { values: kept }))
    })
}

/// `set_variable_remove_list_value_at_index`: removes the element at an index
/// and also hands back what was removed.
///
/// # Errors
///
/// Returns [`RuntimeError::IndexOutOfRange`] if the index is outside the list,
/// and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remove_list_value_at_index<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let removed = {
        let args = Args::of(op);
        let (scope, name) = rt.target_of(stream, args, "removed_value")?;
        (scope, name)
    };
    assign_over(rt, stream, op, move |rt, stream, args, _current| {
        let mut items = rt.list_arg(stream, args, "list")?;
        let index = rt.number_arg(stream, args, "index")?;
        let index = value::expect_index(index, items.len(), &at_op(args, "index"))?;
        let value = items.remove(index);
        rt.scope_store(stream, removed.0).set(&removed.1, value);
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_remove_list_duplicates`: keeps the first occurrence of each
/// value, preserving the order.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remove_list_duplicates<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let mut unique: Vec<Rt<'a>> = Vec::with_capacity(items.len());
        for item in items {
            if !unique.iter().any(|kept| value::equals(kept, &item)) {
                unique.push(item);
            }
        }
        Ok(Some(Value::Array { values: unique }))
    })
}

/// `set_variable_append_value`: appends values to the end of the list that is
/// in the target variable itself.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the target is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn append_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign_over(rt, stream, op, |rt, stream, args, current| {
        let mut items = value::list_of(&current, &at_op(args, "variable"))?;
        items.extend(rt.values_arg(stream, args, "values")?);
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_append_list`: glues two lists into a third.
///
/// The compiler writes one argument when the call passes a single list —
/// `variable::append_list([a, b])` — so the second list is optional.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if either argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn append_list<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut items = if args.raw("list_1").is_some() {
            rt.list_arg(stream, args, "list_1")?
        } else {
            Vec::new()
        };
        if args.raw("list_2").is_some() {
            items.extend(rt.list_arg(stream, args, "list_2")?);
        }
        Ok(Some(Value::Array { values: items }))
    })
}

/// `set_variable_trim_list`: keeps the elements from `start` to `end`,
/// both inclusive.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAList`] if the argument is not a list, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn trim_list<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let start = rt.number_arg(stream, args, "start")?;
        let end = rt.number_arg(stream, args, "end")?;
        let Some(start) = index_in(start, items.len()) else {
            return Ok(Some(Value::Array { values: Vec::new() }));
        };
        let end = index_in(end, items.len()).map_or(items.len(), |end| end + 1);
        let kept = items
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        Ok(Some(Value::Array { values: kept }))
    })
}
