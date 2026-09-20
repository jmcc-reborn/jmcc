//! Словари: значение по ключу, запись пар ключ-значение, размер и
//! список ключей.

use super::lists::compare_values;
use super::*;
use crate::actions::code::optional_target;
use litemap::LiteMap;

/// Словарные действия. `None` — действие не из этой группы.
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
        ActionId::SetVariableAppendMap => append(rt, stream, op),
        ActionId::SetVariableClearMap => clear(rt, stream, op),
        ActionId::SetVariableCreateMap => create(rt, stream, op),
        ActionId::SetVariableGetMapKeyByIndex => key_by_index(rt, stream, op),
        ActionId::SetVariableGetMapValueByIndex => value_by_index(rt, stream, op),
        ActionId::SetVariableGetMapKeysByValue => keys_by_value(rt, stream, op),
        ActionId::SetVariableGetMapValues => map_values(rt, stream, op),
        ActionId::SetVariableSortAnyMap => sort(rt, stream, op),
        ActionId::SetVariableRemoveMapEntry => remove_entry(rt, stream, op),
        _ => return None,
    })
}

/// `set_variable_append_map`: сливает два словаря в третий.
///
/// Ключи второго словаря перекрывают одноимённые ключи первого.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn append<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut map = rt.map_arg(stream, args, "map")?;
        for (key, value) in rt.map_arg(stream, args, "other_map")?.iter() {
            map.insert(key.clone(), value.clone());
        }
        Ok(Some(Value::Map { values: map }))
    })
}

/// `set_variable_clear_map`: очищает словарь в самой переменной.
///
/// Цель названа аргументом `map`, а не `variable`: так записано в схеме.
///
/// # Errors
///
/// Возвращает [`RuntimeError::ExpectedVariable`] на не-переменной в `map` и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn clear<'a>(rt: &mut Runtime<'a>, stream: &Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let (scope, name) = rt.target_of(stream, args, "map")?;
    rt.scope_store(stream, scope).set(
        &name,
        Some(Value::Map {
            values: LiteMap::new(),
        }),
    );
    Ok(Flow::Continue)
}

/// `set_variable_create_map`: словарь из двух параллельных списков.
///
/// # Errors
///
/// Возвращает [`RuntimeError::UnexpectedValue`], если списки разной длины, и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn create<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let keys = rt.optional_list_arg(stream, args, "keys")?;
        let values = rt.optional_list_arg(stream, args, "values")?;
        if keys.len() != values.len() {
            return Err(RuntimeError::UnexpectedValue {
                context: format!(
                    "action '{}': {} key(s) for {} value(s)",
                    schema::name(args.action()),
                    keys.len(),
                    values.len()
                ),
                actual: "a different number of values than keys".to_owned(),
            });
        }
        let mut map = LiteMap::new();
        for (key, value) in keys.into_iter().zip(values) {
            if let Some(value) = value {
                map.insert(TextValue(value::encode_key(&key)), value);
            }
        }
        Ok(Some(Value::Map { values: map }))
    })
}

/// `set_variable_get_map_key_by_index`: ключ по его месту в словаре.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn key_by_index<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        let index = rt.number_arg(stream, args, "index")?;
        Ok(match index_in(index, map.len()) {
            Some(index) => map
                .iter()
                .nth(index)
                .map(|(key, _)| value::decode_key(&key.0)),
            None => rt.optional_arg(stream, args, "default_value")?.flatten(),
        })
    })
}

/// `set_variable_get_map_value_by_index`: значение по месту в словаре.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn value_by_index<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        let index = rt.number_arg(stream, args, "index")?;
        Ok(match index_in(index, map.len()) {
            Some(index) => map.iter().nth(index).map(|(_, value)| value.clone()),
            None => rt.optional_arg(stream, args, "default_value")?.flatten(),
        })
    })
}

/// `set_variable_get_map_keys_by_value`: ключи, под которыми лежит значение.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме поиска и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn keys_by_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        let wanted = rt.arg(stream, args, "value")?;
        let mode = rt.enum_arg(stream, args, "find_mode")?;
        let matching: Vec<Rt<'a>> = map
            .iter()
            .filter(|(_, value)| value::equals(&Some((*value).clone()), &wanted))
            .map(|(key, _)| Some(value::decode_key(&key.0)))
            .collect();
        let found: Vec<Rt<'a>> = match mode {
            "ALL" => matching,
            "FIRST" => matching.into_iter().take(1).collect(),
            "LAST" => matching.into_iter().last().into_iter().collect(),
            other => return Err(invalid_enum(args, "find_mode", other.to_owned())),
        };
        if found.is_empty() {
            // An empty list means "nothing matched"; the default value, if the
            // compiler wrote one, says what to use instead.
            if let Some(default) = rt.optional_arg(stream, args, "default_value")? {
                return Ok(Some(value::list(default)));
            }
        }
        Ok(Some(Value::Array { values: found }))
    })
}

/// `set_variable_get_map_values`: значения словаря как список.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn map_values<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        Ok(Some(Value::Array {
            values: map.values().map(|value| Some(value.clone())).collect(),
        }))
    })
}

/// `set_variable_sort_any_map`: сортировка словаря по ключам или значениям.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном порядке или поле
/// сортировки и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn sort<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        let descending = match rt.enum_arg(stream, args, "sort_order")? {
            "ASCENDING" => false,
            "DESCENDING" => true,
            other => return Err(invalid_enum(args, "sort_order", other.to_owned())),
        };
        let by_values = match rt.enum_arg(stream, args, "sort_type")? {
            "KEYS" => false,
            "VALUES" => true,
            other => return Err(invalid_enum(args, "sort_type", other.to_owned())),
        };
        let mut entries: Vec<(TextValue, Value<'a>)> = map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        entries.sort_by(|left, right| {
            let order = if by_values {
                compare_values(&Some(left.1.clone()), &Some(right.1.clone()))
            } else {
                left.0.0.cmp(&right.0.0)
            };
            if descending { order.reverse() } else { order }
        });
        let mut sorted = LiteMap::new();
        for (key, value) in entries {
            sorted.insert(key, value);
        }
        Ok(Some(Value::Map { values: sorted }))
    })
}

/// `set_variable_remove_map_entry`: удаляет записи по ключам.
///
/// Ключи приходят и поодиночке (`key`), и списком (`values`); удалённое
/// значение попадает в переменную `removed_value`, если компилятор её написал.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remove_entry<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let removed = {
        let args = Args::of(op);
        optional_target(rt, stream, args, "removed_value")?
    };
    assign(rt, stream, op, move |rt, stream, args| {
        let mut map = rt.map_arg(stream, args, "map")?;
        let mut keys = Vec::new();
        if let Some(key) = rt.optional_arg(stream, args, "key")? {
            keys.push(value::encode_key(&key));
        }
        for key in rt.optional_values_arg(stream, args, "values")? {
            keys.push(value::encode_key(&key));
        }
        let mut last: Option<Value<'a>> = None;
        for key in keys {
            if let Some(value) = map.remove(&TextValue(key)) {
                last = Some(value);
            }
        }
        if let Some((scope, name)) = &removed {
            rt.scope_store(stream, *scope).set(name, last.clone());
        }
        Ok(Some(Value::Map { values: map }))
    })
}

/// `set_variable_create_map_from_values`: builds a map from parallel key/value
/// arrays. Missing values are omitted, matching map assignment semantics.
///
/// # Errors
///
/// Returns [`RuntimeError::UnexpectedValue`] if keys and values have different
/// lengths, and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn create_map_from_values<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let keys = rt.values_arg(stream, args, "keys")?;
        let values = rt.values_arg(stream, args, "values")?;
        if keys.len() != values.len() {
            return Err(RuntimeError::UnexpectedValue {
                context: format!(
                    "action '{}': {} key(s) for {} value(s)",
                    schema::name(args.action()),
                    keys.len(),
                    values.len()
                ),
                actual: "a different number of values than keys".to_owned(),
            });
        }
        let mut map = LiteMap::new();
        for (key, value) in keys.into_iter().zip(values) {
            if let Some(value) = value {
                map.insert(TextValue(value::encode_key(&key)), value);
            }
        }
        Ok(Some(Value::Map { values: map }))
    })
}

/// `set_variable_get_map_value`: the value of a map by key.
///
/// A missing key is `default_value` if one was written, and an empty value if
/// not: the same as reading a list element.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAMap`] if the argument is not a map, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn get_map_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        // The key is a value of any type, and the dictionary is keyed by its
        // JSON encoding — see [`value::encode_key`].
        let key = rt.arg(stream, args, "key")?;
        Ok(match map.get(&TextValue(value::encode_key(&key))) {
            Some(value) => Some(value.clone()),
            None => rt.optional_arg(stream, args, "default_value")?.flatten(),
        })
    })
}

/// `set_variable_set_map_value`: writes key/value pairs.
///
/// # Errors
///
/// Returns [`RuntimeError::UnexpectedValue`] if there are not as many values as
/// keys, and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_map_value<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut map = rt.map_arg(stream, args, "map")?;
        let keys = rt.values_arg(stream, args, "key")?;
        let values = rt.values_arg(stream, args, "value")?;
        if keys.len() != values.len() {
            return Err(RuntimeError::UnexpectedValue {
                context: format!(
                    "action '{}': {} key(s) for {} value(s)",
                    schema::name(args.action()),
                    keys.len(),
                    values.len()
                ),
                actual: "a different number of values than keys".to_owned(),
            });
        }
        for (key, value) in keys.into_iter().zip(values) {
            match value {
                Some(value) => {
                    map.insert(TextValue(value::encode_key(&key)), value);
                }
                // An empty value erases the key: in JustMC `map[key] = null`
                // removes the entry rather than storing emptiness under the key.
                None => {
                    map.remove(&TextValue(value::encode_key(&key)));
                }
            }
        }
        Ok(Some(Value::Map { values: map }))
    })
}

/// `set_variable_get_map_size`: how many entries a map has.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAMap`] if the argument is not a map, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn map_size<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        Ok(Some(value::number(count_as_number(map.len()))))
    })
}

/// `set_variable_get_map_keys`: the keys of a map as a list.
///
/// # Errors
///
/// Returns [`RuntimeError::NotAMap`] if the argument is not a map, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn map_keys<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let map = rt.map_arg(stream, args, "map")?;
        let keys = map
            .keys()
            .map(|key| Some(value::decode_key(&key.0)))
            .collect();
        Ok(Some(Value::Array { values: keys }))
    })
}
