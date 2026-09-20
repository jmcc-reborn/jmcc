//! Условия над переменными: равенство и порядок, диапазоны, списки, словари,
//! текст, локации.
//!
//! Условия живут отдельно от остальных действий: `if_*` — не эффект, а выбор
//! ветки, и выполняет их [`Runtime::condition`](crate::Runtime::condition).
//! Сюда вынесены все условия объекта `variable`; те, что требуют предметов,
//! блоков и контейнеров, остаются нереализованными — у мока нет их модели.
//!
//! Многозначные аргументы (`compare`, `values`, `key`, `check`) в схеме помечены
//! как `array`: условие выполняется, если подходит **любой** из значений, кроме
//! тех действий, где режим проверки задан отдельно (`check_mode = ALL` требует
//! все).

use jmcdata::generated::ActionId;
use jmcdata::module::{Op, TextValue, Value};

use crate::error::{Result, RuntimeError};
use crate::interp::{Args, at_op, invalid_enum, matches_type};
use crate::run::{Runtime, Stream};
use crate::value::{self, Rt};

/// Относится ли условие к этой группе.
#[must_use]
pub const fn handles(condition: ActionId) -> bool {
    matches!(
        condition,
        ActionId::IfVariableDummy
            | ActionId::IfVariableEquals
            | ActionId::IfVariableNotEquals
            | ActionId::IfVariableGreater
            | ActionId::IfVariableGreaterOrEquals
            | ActionId::IfVariableLess
            | ActionId::IfVariableLessOrEquals
            | ActionId::IfVariableInRange
            | ActionId::IfVariableNumberInRange
            | ActionId::IfVariableExists
            | ActionId::IfVariableIsType
            | ActionId::IfVariableTextMatches
            | ActionId::IfVariableTextContains
            | ActionId::IfVariableTextStartsWith
            | ActionId::IfVariableTextEndsWith
            | ActionId::IfVariableListIsEmpty
            | ActionId::IfVariableListContainsValue
            | ActionId::IfVariableListValueEquals
            | ActionId::IfVariableMapHasKey
            | ActionId::IfVariableMapValueEquals
            | ActionId::IfVariableLocationInRange
            | ActionId::IfVariableLocationIsNear
            | ActionId::IfVariableRangeIntersectsRange
    )
}

/// Вычисляет условие группы.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов и нечислового значения там, где
/// нужно число.
#[tracing::instrument(level = "debug", skip(rt, stream, op), fields(condition = ?condition))]
pub fn evaluate<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    condition: ActionId,
    op: &'a Op<'a>,
) -> Result<bool> {
    let args = Args::of_condition(op, condition);
    match condition {
        ActionId::IfVariableDummy => Ok(false),
        ActionId::IfVariableEquals | ActionId::IfVariableNotEquals => {
            let left = rt.arg(stream, args, "value")?;
            let equal = rt
                .values_arg(stream, args, "compare")?
                .iter()
                .any(|candidate| value::equals(&left, candidate));
            Ok(equal == (condition == ActionId::IfVariableEquals))
        }
        ActionId::IfVariableGreater => {
            compare_numbers(rt, stream, args, |left, right| left > right)
        }
        ActionId::IfVariableGreaterOrEquals => {
            compare_numbers(rt, stream, args, |left, right| left >= right)
        }
        ActionId::IfVariableLess => compare_numbers(rt, stream, args, |left, right| left < right),
        ActionId::IfVariableLessOrEquals => {
            compare_numbers(rt, stream, args, |left, right| left <= right)
        }
        ActionId::IfVariableInRange => {
            // The action's bounds and its value are of any type, and the check
            // follows them. A location is the location region; for anything else
            // the comparison is numeric, and there an empty value counts as
            // zero: `not x.in_range(0, 0)` is how `tests/pvp` asks whether a
            // variable has been set at all. A value that is not a number — an
            // item, a text — is outside every range.
            let value = rt.optional_arg(stream, args, "value")?.flatten();
            let min = rt.optional_arg(stream, args, "min")?.flatten();
            let max = rt.optional_arg(stream, args, "max")?.flatten();
            if [&value, &min, &max]
                .iter()
                .any(|bound| matches!(bound, Some(Value::Location { .. })))
            {
                return location_in_range(rt, stream, args);
            }
            let number = |bound: &Rt<'a>| match bound {
                None | Some(Value::Error) => 0.0,
                Some(bound) => value::as_number(bound).unwrap_or(f64::NAN),
            };
            let (value, min, max) = (number(&value), number(&min), number(&max));
            Ok(value >= min && value <= max)
        }
        ActionId::IfVariableNumberInRange => number_in_range(rt, stream, args),
        ActionId::IfVariableExists => {
            let (scope, name) = rt.target_of(stream, args, "variable")?;
            let value = rt.scope_store(stream, scope).peek(&name).flatten();
            Ok(!value::is_unset(&value))
        }
        ActionId::IfVariableIsType => {
            let value = rt.arg(stream, args, "value")?;
            let wanted = rt.enum_arg(stream, args, "variable_type")?;
            Ok(matches_type(&value, wanted))
        }
        ActionId::IfVariableTextMatches => text_matches(rt, stream, args),
        ActionId::IfVariableTextContains => text_compare(rt, stream, args, text_contains),
        ActionId::IfVariableTextStartsWith => text_compare(rt, stream, args, text_starts_with),
        ActionId::IfVariableTextEndsWith => text_compare(rt, stream, args, text_ends_with),
        ActionId::IfVariableListIsEmpty => {
            let list = rt.arg(stream, args, "list")?;
            Ok(value::list_of(&list, &at_op(args, "list"))?.is_empty())
        }
        ActionId::IfVariableListContainsValue => list_contains(rt, stream, args),
        ActionId::IfVariableListValueEquals => list_value_equals(rt, stream, args),
        ActionId::IfVariableMapHasKey => map_has_key(rt, stream, args),
        ActionId::IfVariableMapValueEquals => map_value_equals(rt, stream, args),
        ActionId::IfVariableLocationInRange => location_in_range(rt, stream, args),
        ActionId::IfVariableLocationIsNear => location_is_near(rt, stream, args),
        ActionId::IfVariableRangeIntersectsRange => range_intersects(rt, stream, args),
        // `handles` has already checked the action; the arm exists so that the
        // match stays exhaustive without a catch-all that would hide a missing
        // condition.
        other => Err(RuntimeError::Unimplemented {
            what: format!("the condition '{}'", crate::schema::name(other)),
        }),
    }
}

/// Порядковое сравнение числа со списком значений: сравнение истинно, если
/// подходит любое из значений `compare`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotANumber`] на нечисловом значении и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args, apply))]
fn compare_numbers<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    apply: impl Fn(f64, f64) -> bool,
) -> Result<bool> {
    let left = rt.number_arg(stream, args, "value")?;
    Ok(rt
        .numbers_arg(stream, args, "compare")?
        .iter()
        .any(|candidate| apply(left, *candidate)))
}

/// `if_variable_number_in_range`: попадание числа в диапазон с выбором границ.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn number_in_range<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let value = rt.number_arg(stream, args, "value")?;
    let min = rt.number_arg(stream, args, "min")?;
    let max = rt.number_arg(stream, args, "max")?;
    let (low, high) = (min.min(max), min.max(max));
    let inside = match rt.enum_arg(stream, args, "including")? {
        "NOT_INCLUDE" => value > low && value < high,
        "INCLUDE_FIRST" => value >= low && value < high,
        "INCLUDE_LAST" => value > low && value <= high,
        "INCLUDE_ALL" => value >= low && value <= high,
        other => return Err(invalid_enum(args, "including", other.to_owned())),
    };
    Ok(inside)
}

/// `if_variable_text_matches`: сравнение текста со списком значений.
///
/// # Errors
///
/// Возвращает [`RuntimeError::Unimplemented`] на регулярных выражениях, которых
/// у мока нет, и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn text_matches<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, args: Args<'a>) -> Result<bool> {
    if rt.optional_enum_arg(stream, args, "regular_expressions")? == Some("TRUE") {
        rt.unimplemented("regular expressions in 'if_variable_text_matches'")?;
        return Ok(false);
    }
    let ignore_case = rt.optional_enum_arg(stream, args, "ignore_case")? == Some("TRUE");
    let subject = folded(&rt.text_arg(stream, args, "match")?, ignore_case);
    Ok(rt
        .values_arg(stream, args, "values")?
        .iter()
        .any(|candidate| folded(&value::display(candidate), ignore_case) == subject))
}

/// Проверка вхождения, начала или конца текста.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args, apply))]
fn text_compare<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    apply: impl Fn(&str, &str) -> bool,
) -> Result<bool> {
    let ignore_case = rt.optional_enum_arg(stream, args, "ignore_case")? == Some("TRUE");
    let subject = folded(&rt.text_arg(stream, args, "value")?, ignore_case);
    Ok(rt
        .values_arg(stream, args, "compare")?
        .iter()
        .any(|candidate| {
            let candidate = folded(&value::display(candidate), ignore_case);
            apply(&subject, &candidate)
        }))
}

/// Приводит текст к нижнему регистру, если сравнение безрегистровое.
#[tracing::instrument(level = "trace", fields(text = %text, ignore_case = ?ignore_case))]
fn folded(text: &str, ignore_case: bool) -> String {
    if ignore_case {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// Содержит ли текст подтекст.
#[tracing::instrument(level = "trace", fields(haystack = %haystack, needle = %needle))]
fn text_contains(haystack: &str, needle: &str) -> bool {
    haystack.contains(needle)
}

/// Начинается ли текст с подтекста.
#[tracing::instrument(level = "trace", fields(haystack = %haystack, needle = %needle))]
fn text_starts_with(haystack: &str, needle: &str) -> bool {
    haystack.starts_with(needle)
}

/// Заканчивается ли текст подтекстом.
#[tracing::instrument(level = "trace", fields(haystack = %haystack, needle = %needle))]
fn text_ends_with(haystack: &str, needle: &str) -> bool {
    haystack.ends_with(needle)
}

/// `if_variable_list_contains_value`: есть ли значения в списке.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn list_contains<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let items = rt.list_arg(stream, args, "list")?;
    let wanted = rt.values_arg(stream, args, "values")?;
    let found = |wanted: &Rt<'a>| items.iter().any(|item| value::equals(item, wanted));
    // The mode may be left out: `tests/pvp` asks whether a map has a key
    // without naming one, and `ALL` is the stricter reading — every key asked
    // for has to be there.
    match rt
        .optional_enum_arg(stream, args, "check_mode")?
        .unwrap_or("ALL")
    {
        "ALL" => Ok(wanted.iter().all(found)),
        "ANY" => Ok(wanted.iter().any(found)),
        other => Err(invalid_enum(args, "check_mode", other.to_owned())),
    }
}

/// `if_variable_list_value_equals`: равенство элемента списка значению.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn list_value_equals<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let items = rt.list_arg(stream, args, "list")?;
    let index = rt.number_arg(stream, args, "index")?;
    let Some(item) = super::index_in(index, items.len()).and_then(|index| items.get(index)) else {
        // An index past the end is not an error: the condition is simply false.
        return Ok(false);
    };
    let item = item.clone();
    Ok(rt
        .values_arg(stream, args, "values")?
        .iter()
        .any(|candidate| value::equals(&item, candidate)))
}

/// `if_variable_map_has_key`: есть ли ключи в словаре.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn map_has_key<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, args: Args<'a>) -> Result<bool> {
    let map = rt.map_arg(stream, args, "map")?;
    let keys = rt.values_arg(stream, args, "key")?;
    let found = |key: &Rt<'a>| map.contains_key(&TextValue(value::encode_key(key)));
    // See `list_contains`: an omitted mode means `ALL`.
    match rt
        .optional_enum_arg(stream, args, "check_mode")?
        .unwrap_or("ALL")
    {
        "ALL" => Ok(keys.iter().all(found)),
        "ANY" => Ok(keys.iter().any(found)),
        other => Err(invalid_enum(args, "check_mode", other.to_owned())),
    }
}

/// `if_variable_map_value_equals`: равенство значения по ключу.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAMap`] на не-словаре и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn map_value_equals<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let map = rt.map_arg(stream, args, "map")?;
    let key = TextValue(value::encode_key(&rt.arg(stream, args, "key")?));
    let Some(stored) = map.get(&key) else {
        return Ok(false);
    };
    let stored = Some(stored.clone());
    Ok(rt
        .values_arg(stream, args, "values")?
        .iter()
        .any(|candidate| value::equals(&stored, candidate)))
}

/// `if_variable_location_in_range`: попадание локации в область.
///
/// Режим границ необязателен: у общего `if_variable_in_range` его нет вовсе, а
/// там `BLOCK` — то, что ожидает `tests/pvp` (углы области стоят на половинах
/// координат, и блок внутри области должен попадать в неё).
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме
/// границ и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn location_in_range<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    // A value about a target the mock does not have — the victim of a fight —
    // is empty, and an empty location is in no region.
    let Some(value) = rt.optional_location_arg(stream, args, "value")? else {
        return Ok(false);
    };
    let first = rt.location_arg(stream, args, "min")?;
    let second = rt.location_arg(stream, args, "max")?;
    let mode = rt
        .optional_enum_arg(stream, args, "border_handling")?
        .unwrap_or("BLOCK");
    let round = |mut point: [f64; 5]| {
        if mode == "EXACT" {
            return point;
        }
        for coordinate in &mut point[..3] {
            *coordinate = coordinate.floor();
        }
        point
    };
    let value = round(value);
    let first = round(first);
    let second = round(second);
    Ok((0..3).all(|axis| {
        let (low, high) = (first[axis].min(second[axis]), first[axis].max(second[axis]));
        value[axis] >= low && value[axis] <= high
    }))
}

/// Форма области для `if_variable_location_is_near`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Шар: расстояние по трём осям.
    Sphere,
    /// Круг в горизонтальной плоскости.
    Circle,
    /// Куб: покоординатное расстояние.
    Cube,
    /// Квадрат в горизонтальной плоскости.
    Square,
}

/// `if_variable_location_is_near`: есть ли локация рядом с данной.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной форме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn location_is_near<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let center = rt.location_arg(stream, args, "location")?;
    let radius = rt
        .optional_number_arg(stream, args, "radius")?
        .unwrap_or(0.0);
    let shape = match rt.enum_arg(stream, args, "shape")? {
        "SPHERE" => Shape::Sphere,
        "CIRCLE" => Shape::Circle,
        "CUBE" => Shape::Cube,
        "SQUARE" => Shape::Square,
        other => return Err(invalid_enum(args, "shape", other.to_owned())),
    };
    let mut candidates = Vec::new();
    for value in rt.values_arg(stream, args, "check")? {
        candidates.push(value::location_of(&value, &at_op(args, "check"))?);
    }
    Ok(candidates
        .into_iter()
        .any(|candidate| within(center, candidate, radius, shape)))
}

/// Попадание `candidate` в область вокруг `center`.
#[tracing::instrument(level = "trace", skip(shape), fields(center = ?center, candidate = ?candidate, radius = ?radius))]
fn within(center: [f64; 5], candidate: [f64; 5], radius: f64, shape: Shape) -> bool {
    let (dx, dy, dz) = (
        candidate[0] - center[0],
        candidate[1] - center[1],
        candidate[2] - center[2],
    );
    match shape {
        Shape::Sphere => dx.hypot(dy).hypot(dz) <= radius,
        Shape::Circle => dx.hypot(dz) <= radius,
        Shape::Cube => dx.abs() <= radius && dy.abs() <= radius && dz.abs() <= radius,
        Shape::Square => dx.abs() <= radius && dz.abs() <= radius,
    }
}

/// `if_variable_range_intersects_range`: содержит ли одна область другую или
/// пересекается ли она с ней.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn range_intersects<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<bool> {
    let first_min = rt.location_arg(stream, args, "min1")?;
    let first_max = rt.location_arg(stream, args, "max1")?;
    let second_min = rt.location_arg(stream, args, "min2")?;
    let second_max = rt.location_arg(stream, args, "max2")?;
    match rt.enum_arg(stream, args, "check_type")? {
        "CONTAINS" => Ok(contains(first_min, first_max, second_min, second_max)
            || contains(second_min, second_max, first_min, first_max)),
        "OVERLAPS" => Ok(overlaps(first_min, first_max, second_min, second_max)),
        other => Err(invalid_enum(args, "check_type", other.to_owned())),
    }
}

/// Границы области по оси, от меньшей к большей.
const fn bounds(min: [f64; 5], max: [f64; 5], axis: usize) -> (f64, f64) {
    (min[axis].min(max[axis]), min[axis].max(max[axis]))
}

/// Содержит ли первая область вторую целиком.
#[tracing::instrument(level = "trace", fields(outer_min = ?outer_min, outer_max = ?outer_max, inner_min = ?inner_min, inner_max = ?inner_max))]
fn contains(
    outer_min: [f64; 5],
    outer_max: [f64; 5],
    inner_min: [f64; 5],
    inner_max: [f64; 5],
) -> bool {
    (0..3).all(|axis| {
        let (low, high) = bounds(outer_min, outer_max, axis);
        let (inner_low, inner_high) = bounds(inner_min, inner_max, axis);
        inner_low >= low && inner_high <= high
    })
}

/// Пересекаются ли области.
#[tracing::instrument(level = "trace", fields(first_min = ?first_min, first_max = ?first_max, second_min = ?second_min, second_max = ?second_max))]
fn overlaps(
    first_min: [f64; 5],
    first_max: [f64; 5],
    second_min: [f64; 5],
    second_max: [f64; 5],
) -> bool {
    (0..3).all(|axis| {
        let (a_low, a_high) = bounds(first_min, first_max, axis);
        let (b_low, b_high) = bounds(second_min, second_max, axis);
        a_low <= b_high && b_low <= a_high
    })
}
