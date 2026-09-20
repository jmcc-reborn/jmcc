//! Векторы: сложение, поворот, проекции, расстояния.
//!
//! Вектор в `JustMC` — три числа `(x, y, z)`. Действия этой группы либо
//! собирают вектор из чисел и других векторов, либо раскладывают его обратно в
//! числа. Формулы — обычная линейная алгебра; там, где `JustMC` оговаривает
//! порядок (углы в градусах или радианах, направление «от куда» и «куда»),
//! порядок берётся из имени аргумента, а не угадывается.
//!
//! Поворот вектора вокруг оси — формула Родрига:
//! `v' = v·cos(θ) + (k × v)·sin(θ) + k·(k·v)·(1 − cos(θ))`.
//!
//! # Ошибки вместо пустого вектора
//!
//! Нулевой вектор — законное значение, а вот деление на нулевую длину или на
//! нулевой вектор — нет: результат такого действия не определён, и подставлять
//! вместо него `(0, 0, 0)` значило бы выдать выдуманное число за настоящее.

use super::*;
use crate::actions::code::optional_target;

/// Векторные действия. `None` — действие не из этой группы: его обрабатывает
/// следующий диспетчер [`super::dispatch`].
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
        ActionId::SetVariableVector => vector(rt, stream, op),
        ActionId::SetVariableAddVectors => add_vectors(rt, stream, op),
        ActionId::SetVariableSubtractVectors => subtract_vectors(rt, stream, op),
        ActionId::SetVariableMultiplyVector => multiply_vector(rt, stream, op),
        ActionId::SetVariableDivideVector => divide_vector(rt, stream, op),
        ActionId::SetVariableVectorDotProduct => dot_product(rt, stream, op),
        ActionId::SetVariableVectorCrossProduct => cross_product(rt, stream, op),
        ActionId::SetVariableHadamardVectorProduct => hadamard_product(rt, stream, op),
        ActionId::SetVariableGetVectorLength => vector_length(rt, stream, op),
        ActionId::SetVariableGetVectorComponent => vector_component(rt, stream, op),
        ActionId::SetVariableGetVectorAllComponents => all_components(rt, stream, op),
        ActionId::SetVariableSetVectorComponent => set_vector_component(rt, stream, op),
        ActionId::SetVariableSetVectorLength => set_vector_length(rt, stream, op),
        ActionId::SetVariableGetAngleBetweenVectors => angle_between(rt, stream, op),
        ActionId::SetVariableRotateVectorAroundAxis => rotate_around_axis(rt, stream, op),
        ActionId::SetVariableRotateVectorAroundVector => rotate_around_vector(rt, stream, op),
        ActionId::SetVariableReflectVectorProduct => reflect(rt, stream, op),
        ActionId::SetVariableGetMidpointBetweenVectors => midpoint(rt, stream, op),
        ActionId::SetVariableGetVectorBetweenLocations => between_locations(rt, stream, op),
        ActionId::SetVariableAlignToAxisVector => align_to_axis(rt, stream, op),
        ActionId::SetVariableVectorToDirectionName => direction_name(rt, stream, op),
        ActionId::SetVariableGetVectorFromBlockFace => from_block_face(rt, stream, op),
        ActionId::SetVariableGetLocationDirection => location_direction(rt, stream, op),
        ActionId::SetVariableSetLocationDirection => set_location_direction(rt, stream, op),
        _ => return None,
    })
}

/// Вектор из трёх чисел: `set_variable_vector`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn vector<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let x = rt.number_arg(stream, args, "x")?;
        let y = rt.number_arg(stream, args, "y")?;
        let z = rt.number_arg(stream, args, "z")?;
        Ok(Some(value::vector(x, y, z)))
    })
}

/// Сложение векторов: `set_variable_add_vectors`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке векторов и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn add_vectors<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    fold_vectors(rt, stream, op, "vectors", |acc, next| {
        [acc[0] + next[0], acc[1] + next[1], acc[2] + next[2]]
    })
}

/// Вычитание векторов: `set_variable_subtract_vectors`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке векторов и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn subtract_vectors<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    fold_vectors(rt, stream, op, "vectors", |acc, next| {
        [acc[0] - next[0], acc[1] - next[1], acc[2] - next[2]]
    })
}

/// Свёртка списка векторов слева направо.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op, apply), fields(arg = %arg))]
fn fold_vectors<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    arg: &'static str,
    apply: impl Fn([f64; 3], [f64; 3]) -> [f64; 3],
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut vectors = vector_items(rt, stream, args, arg)?.into_iter();
        let Some(first) = vectors.next() else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg,
            });
        };
        let result = vectors.fold(first, apply);
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Умножение вектора на число: `set_variable_multiply_vector`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn multiply_vector<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let multiplier = rt.number_arg(stream, args, "multiplier")?;
        Ok(Some(scale(vector, multiplier)))
    })
}

/// Покомпонентное деление вектора на вектор: `set_variable_divide_vector`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на нулевой компоненте делителя и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn divide_vector<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let divider = rt.vector_arg(stream, args, "divider")?;
        let mut result = [0.0; 3];
        for (index, (value, divisor)) in vector.iter().zip(divider.iter()).enumerate() {
            if *divisor == 0.0 {
                return Err(RuntimeError::DivisionByZero {
                    context: at_op(args, "divider"),
                });
            }
            result[index] = value / divisor;
        }
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Скалярное произведение: `set_variable_vector_dot_product`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn dot_product<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = rt.vector_arg(stream, args, "vector_1")?;
        let right = rt.vector_arg(stream, args, "vector_2")?;
        Ok(Some(value::number(dot(left, right))))
    })
}

/// Векторное произведение: `set_variable_vector_cross_product`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn cross_product<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = rt.vector_arg(stream, args, "vector_1")?;
        let right = rt.vector_arg(stream, args, "vector_2")?;
        let result = cross(left, right);
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Покомпонентное произведение: `set_variable_hadamard_vector_product`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn hadamard_product<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = rt.vector_arg(stream, args, "vector_1")?;
        let right = rt.vector_arg(stream, args, "vector_2")?;
        Ok(Some(value::vector(
            left[0] * right[0],
            left[1] * right[1],
            left[2] * right[2],
        )))
    })
}

/// Длина вектора: `set_variable_get_vector_length`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном `length_type`
/// и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn vector_length<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let length = dot(vector, vector);
        let result = match rt.enum_arg(stream, args, "length_type")? {
            "LENGTH" => length.sqrt(),
            "LENGTH_SQUARED" => length,
            other => return Err(invalid_enum(args, "length_type", other.to_owned())),
        };
        Ok(Some(value::number(result)))
    })
}

/// Одна компонента вектора: `set_variable_get_vector_component`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной компоненте и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn vector_component<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let component = component_of(rt.enum_arg(stream, args, "vector_component")?, args)?;
        Ok(Some(value::number(vector[component])))
    })
}

/// Все три компоненты вектора разом: `set_variable_get_vector_all_components`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn all_components<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let vector = rt.vector_arg(stream, args, "vector")?;
    for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
        if let Some((scope, target)) = optional_target(rt, stream, args, name)? {
            rt.scope_store(stream, scope)
                .set(&target, Some(value::number(vector[index])));
        }
    }
    Ok(Flow::Continue)
}

/// Замена одной компоненты вектора: `set_variable_set_vector_component`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной компоненте и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_vector_component<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut vector = rt.vector_or_zero(stream, args, "vector")?;
        let value = rt.number_arg(stream, args, "value")?;
        let component = component_of(rt.enum_arg(stream, args, "vector_component")?, args)?;
        vector[component] = value;
        Ok(Some(value::vector(vector[0], vector[1], vector[2])))
    })
}

/// Приведение длины вектора: `set_variable_set_vector_length`.
///
/// Нулевой вектор остаётся нулевым: направления у него нет, и растягивать его
/// некуда.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_vector_length<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let length = rt.number_arg(stream, args, "length")?;
        let current = dot(vector, vector).sqrt();
        if current <= f64::EPSILON {
            return Ok(Some(value::vector(0.0, 0.0, 0.0)));
        }
        let factor = length / current;
        Ok(Some(scale(vector, factor)))
    })
}

/// Угол между векторами: `set_variable_get_angle_between_vectors`.
///
/// У нулевого вектора направления нет, и угол с ним не определён: мок отвечает
/// нулём, а не останавливает прогон. Такой вектор получается, когда значение
/// взято у цели, которой у мока нет (жертвы боя), и останавливаться на этом
/// значило бы не дать программе дойти до её собственной логики.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестных единицах и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn angle_between<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = rt.vector_or_zero(stream, args, "vector_1")?;
        let right = rt.vector_or_zero(stream, args, "vector_2")?;
        let lengths = dot(left, left).sqrt() * dot(right, right).sqrt();
        let angle = if lengths <= f64::EPSILON {
            0.0
        } else {
            (dot(left, right) / lengths).clamp(-1.0, 1.0).acos()
        };
        let result = match rt.enum_arg(stream, args, "angle_units")? {
            "DEGREES" => angle.to_degrees(),
            "RADIANS" => angle,
            other => return Err(invalid_enum(args, "angle_units", other.to_owned())),
        };
        Ok(Some(value::number(result)))
    })
}

/// Поворот вектора вокруг оси координат: `set_variable_rotate_vector_around_axis`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной оси или
/// единицах угла и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn rotate_around_axis<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let angle = angle_arg(rt, stream, args, "angle")?;
        let axis = match rt.enum_arg(stream, args, "axis")? {
            "X" => [1.0, 0.0, 0.0],
            "Y" => [0.0, 1.0, 0.0],
            "Z" => [0.0, 0.0, 1.0],
            other => return Err(invalid_enum(args, "axis", other.to_owned())),
        };
        let result = rotate(vector, axis, angle);
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Поворот вектора вокруг другого вектора:
/// `set_variable_rotate_vector_around_vector`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на нулевой оси,
/// [`RuntimeError::InvalidEnumArgument`] на неизвестных единицах угла и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn rotate_around_vector<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "rotating_vector")?;
        let axis = rt.vector_arg(stream, args, "axis_vector")?;
        let angle = angle_arg(rt, stream, args, "angle")?;
        let length = dot(axis, axis).sqrt();
        if length <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "axis_vector"),
            });
        }
        let result = rotate(vector, scaled(axis, 1.0 / length), angle);
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Отражение вектора: `set_variable_reflect_vector_product`.
///
/// `vector_2` — нормаль плоскости отражения, `bounce` — доля отражённой
/// составляющей: `0` оставляет вектор как есть, `1` отражает полностью.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на нулевой нормали и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn reflect<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector_1")?;
        let normal = rt.vector_arg(stream, args, "vector_2")?;
        let bounce = rt.number_arg(stream, args, "bounce")?;
        let squared = dot(normal, normal);
        if squared <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "vector_2"),
            });
        }
        let factor = (1.0 + bounce) * dot(vector, normal) / squared;
        let result = [
            vector[0] - factor * normal[0],
            vector[1] - factor * normal[1],
            vector[2] - factor * normal[2],
        ];
        Ok(Some(value::vector(result[0], result[1], result[2])))
    })
}

/// Середина между векторами: `set_variable_get_midpoint_between_vectors`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn midpoint<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = rt.vector_arg(stream, args, "vector_1")?;
        let right = rt.vector_arg(stream, args, "vector_2")?;
        Ok(Some(value::vector(
            f64::midpoint(left[0], right[0]),
            f64::midpoint(left[1], right[1]),
            f64::midpoint(left[2], right[2]),
        )))
    })
}

/// Вектор между локациями: `set_variable_get_vector_between_locations`.
///
/// Вектор направлен от `start_location` к `end_location`: так же, как в
/// `set_variable_shift_location_on_vector` он откладывается от локации.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn between_locations<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let end = rt.location_arg(stream, args, "end_location")?;
        let start = rt.location_arg(stream, args, "start_location")?;
        Ok(Some(value::vector(
            end[0] - start[0],
            end[1] - start[1],
            end[2] - start[2],
        )))
    })
}

/// Выравнивание вектора по ближайшей оси: `set_variable_align_to_axis_vector`.
///
/// `normalize` приводит результат к единичной длине.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn align_to_axis<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let dominant = (0..3)
            .max_by(|left, right| {
                vector[*left]
                    .abs()
                    .partial_cmp(&vector[*right].abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        let mut result = [0.0; 3];
        result[dominant] = if vector[dominant] < 0.0 { -1.0 } else { 1.0 };
        let normalize = matches!(
            rt.optional_enum_arg(stream, args, "normalize")?,
            Some("TRUE")
        );
        let length = if normalize {
            1.0
        } else {
            vector[dominant].abs()
        };
        let result = scale(result, length);
        Ok(Some(result))
    })
}

/// Имя направления вектора: `set_variable_vector_to_direction_name`.
///
/// Направление определяется ведущей осью — так же, как `JustMC` показывает
/// стороны света в подсказках.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn direction_name<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let vector = rt.vector_arg(stream, args, "vector")?;
        let [x, y, z] = vector;
        let name = if x.abs() >= y.abs() && x.abs() >= z.abs() {
            if x >= 0.0 { "EAST" } else { "WEST" }
        } else if y.abs() >= z.abs() {
            if y >= 0.0 { "UP" } else { "DOWN" }
        } else if z >= 0.0 {
            "SOUTH"
        } else {
            "NORTH"
        };
        Ok(Some(value::text(name)))
    })
}

/// Вектор из стороны блока: `set_variable_get_vector_from_block_face`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной стороне и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn from_block_face<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let face = rt.text_arg(stream, args, "block_face")?.to_uppercase();
        let Some(vector) = face_vector(&face) else {
            return Err(invalid_enum(args, "block_face", face));
        };
        Ok(Some(value::vector(vector[0], vector[1], vector[2])))
    })
}

/// Единичный вектор стороны блока по её имени.
///
/// Восемнадцать имён из схемы — шесть сторон, восемь горизонтальных и четыре
/// вертикально-горизонтальных направления; `SELF` — отсутствие сдвига. Диагонали
/// нормированы: у них две ненулевые составляющие по `1/√2`.
#[tracing::instrument(level = "trace", fields(name = %name))]
pub(super) fn face_vector(name: &str) -> Option<[f64; 3]> {
    const DIAGONAL: f64 = std::f64::consts::FRAC_1_SQRT_2;
    Some(match name {
        "DOWN" => [0.0, -1.0, 0.0],
        "UP" => [0.0, 1.0, 0.0],
        "NORTH" => [0.0, 0.0, -1.0],
        "SOUTH" => [0.0, 0.0, 1.0],
        "EAST" => [1.0, 0.0, 0.0],
        "WEST" => [-1.0, 0.0, 0.0],
        "NORTH_EAST" => [DIAGONAL, 0.0, -DIAGONAL],
        "NORTH_WEST" => [-DIAGONAL, 0.0, -DIAGONAL],
        "SOUTH_EAST" => [DIAGONAL, 0.0, DIAGONAL],
        "SOUTH_WEST" => [-DIAGONAL, 0.0, DIAGONAL],
        "EAST_NORTH_EAST" => [0.973, 0.0, -0.230],
        "EAST_SOUTH_EAST" => [0.973, 0.0, 0.230],
        "WEST_NORTH_WEST" => [-0.973, 0.0, -0.230],
        "WEST_SOUTH_WEST" => [-0.973, 0.0, 0.230],
        "NORTH_NORTH_EAST" => [0.230, 0.0, -0.973],
        "NORTH_NORTH_WEST" => [-0.230, 0.0, -0.973],
        "SOUTH_SOUTH_EAST" => [0.230, 0.0, 0.973],
        "SOUTH_SOUTH_WEST" => [-0.230, 0.0, 0.973],
        "SELF" => [0.0, 0.0, 0.0],
        _ => return None,
    })
}

/// Направление взгляда локации: `set_variable_get_location_direction`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn location_direction<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let [_, _, _, yaw, pitch] = rt.location_arg(stream, args, "location")?;
        let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        Ok(Some(value::vector(
            -sin_yaw * cos_pitch,
            -sin_pitch,
            cos_yaw * cos_pitch,
        )))
    })
}

/// Ориентация локации по вектору: `set_variable_set_location_direction`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на нулевом векторе и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_location_direction<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
        let [vx, vy, vz] = rt.vector_arg(stream, args, "vector")?;
        let length = dot([vx, vy, vz], [vx, vy, vz]).sqrt();
        if length <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "vector"),
            });
        }
        let yaw = (-vx).atan2(vz).to_degrees();
        let pitch = (-vy / length).asin().to_degrees();
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// Вектор как последовательность: `vector_items` раскладывает `vector[21]`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
fn vector_items<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    name: &'static str,
) -> Result<Vec<[f64; 3]>> {
    let mut vectors = Vec::new();
    for value in rt.values_arg(stream, args, name)? {
        vectors.push(value::vector_of(&value, &at_op(args, name))?);
    }
    Ok(vectors)
}

/// Номер компоненты по имени из схемы.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном имени.
#[tracing::instrument(level = "trace", skip(args), fields(name = %name))]
fn component_of(name: &str, args: Args<'_>) -> Result<usize> {
    match name {
        "X" => Ok(0),
        "Y" => Ok(1),
        "Z" => Ok(2),
        other => Err(invalid_enum(args, "vector_component", other.to_owned())),
    }
}

/// Угол в радианах по аргументу и его единицам.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестных единицах и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
fn angle_arg<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    name: &'static str,
) -> Result<f64> {
    let angle = rt.number_arg(stream, args, name)?;
    Ok(match rt.enum_arg(stream, args, "angle_units")? {
        "DEGREES" => angle.to_radians(),
        "RADIANS" => angle,
        other => return Err(invalid_enum(args, "angle_units", other.to_owned())),
    })
}

/// Скалярное произведение.
#[tracing::instrument(level = "trace", fields(left = ?left, right = ?right))]
fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

/// Векторное произведение.
#[tracing::instrument(level = "trace", fields(left = ?left, right = ?right))]
fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1].mul_add(right[2], -(left[2] * right[1])),
        left[2].mul_add(right[0], -(left[0] * right[2])),
        left[0].mul_add(right[1], -(left[1] * right[0])),
    ]
}

/// Умножение вектора на число как значение.
#[tracing::instrument(level = "trace", fields(vector = ?vector, factor = ?factor))]
fn scale<'a>(vector: [f64; 3], factor: f64) -> Value<'a> {
    value::vector(vector[0] * factor, vector[1] * factor, vector[2] * factor)
}

/// Умножение вектора на число без упаковки в значение.
#[tracing::instrument(level = "trace", fields(vector = ?vector, factor = ?factor))]
fn scaled(vector: [f64; 3], factor: f64) -> [f64; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

/// Поворот вектора вокруг единичной оси `axis` на угол `angle` (радианы).
#[tracing::instrument(level = "trace", fields(vector = ?vector, axis = ?axis, angle = ?angle))]
fn rotate(vector: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let (sin, cos) = angle.sin_cos();
    let cross_product = cross(axis, vector);
    let projection = dot(axis, vector);
    let mut result = [0.0; 3];
    for (index, value) in result.iter_mut().enumerate() {
        *value = vector[index].mul_add(
            cos,
            cross_product[index].mul_add(sin, axis[index] * projection * (1.0 - cos)),
        );
    }
    result
}
