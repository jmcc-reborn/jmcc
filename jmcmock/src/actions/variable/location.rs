//! Локации: координаты, повороты, расстояния, выравнивание по блокам.
//!
//! Локация в `JustMC` — пять чисел `(x, y, z, yaw, pitch)`: позиция и поворот
//! взгляда. Действия этой группы либо собирают локацию из чисел, либо
//! раскладывают её обратно, либо считают по ней что-то — расстояние, ближайшую
//! из списка, точку перед взглядом.
//!
//! Углы — в градусах, как их и показывает `JustMC`. Позиция и поворот
//! независимы: `set_variable_face_location` меняет только поворот, а
//! `align_location` умеет стирать поворот (`rotation_mode = REMOVE`).
//!
//! # Целые координаты
//!
//! В Minecraft блок занимает куб от `n` до `n + 1`. `align_mode = CORNER`
//! ставит локацию в угол блока (`floor`), `BLOCK_CENTER` — в его центр
//! (`floor + 0.5`).

use super::vector::face_vector;
use super::*;
use crate::actions::code::optional_target;

/// Действия над локациями. `None` — действие не из этой группы.
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
        ActionId::SetVariableAlignLocation => align(rt, stream, op),
        ActionId::SetVariableCenterLocation => center(rt, stream, op),
        ActionId::SetVariableClampLocation => clamp(rt, stream, op),
        ActionId::SetVariableFaceLocation => face(rt, stream, op),
        ActionId::SetVariableFindNearestLocation => nearest(rt, stream, op),
        ActionId::SetVariableLocationRelative => relative(rt, stream, op),
        ActionId::SetVariableLocationsDistance => distance(rt, stream, op),
        ActionId::SetVariableRandomLocation => random(rt, stream, op),
        ActionId::SetVariableGetCoordinate => get_coordinate(rt, stream, op),
        ActionId::SetVariableSetCoordinate => set_coordinate(rt, stream, op),
        ActionId::SetVariableShiftCoordinate => shift_coordinate(rt, stream, op),
        ActionId::SetVariableGetAllCoordinates => get_all(rt, stream, op),
        ActionId::SetVariableSetAllCoordinates => set_all(rt, stream, op),
        ActionId::SetVariableShiftAllCoordinates => shift_all(rt, stream, op),
        ActionId::SetVariableShiftLocationInDirection => shift_in_direction(rt, stream, op),
        ActionId::SetVariableShiftLocationOnVector => shift_on_vector(rt, stream, op),
        ActionId::SetVariableShiftLocationTowardsLocation => shift_towards(rt, stream, op),
        _ => return None,
    })
}

/// `set_variable_shift_location_in_direction`: сдвиг вдоль взгляда.
///
/// `FORWARD` — по направлению взгляда, `SIDEWAYS` — вбок от него, `UPWARD` —
/// вверх; поворот локации не меняется.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном направлении
/// и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shift_in_direction<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some([x, y, z, yaw, pitch]) = rt.optional_location_arg(stream, args, "location")?
        else {
            return Ok(None);
        };
        let shift = rt.number_arg(stream, args, "shift")?;
        let radians = yaw.to_radians();
        let (sin, cos) = radians.sin_cos();
        let (dx, dy, dz) = match rt.enum_arg(stream, args, "direction")? {
            "FORWARD" => (-sin * shift, 0.0, cos * shift),
            "SIDEWAYS" => (cos * shift, 0.0, sin * shift),
            "UPWARD" => (0.0, shift, 0.0),
            other => return Err(invalid_enum(args, "direction", other.to_owned())),
        };
        Ok(Some(value::location(x + dx, y + dy, z + dz, yaw, pitch)))
    })
}

/// `set_variable_shift_location_on_vector`: сдвиг на вектор заданной длины.
///
/// Длина необязательна: без неё сдвиг идёт ровно на вектор (`tests/pvp`
/// вызывает действие с тремя аргументами, и `length` там не задан).
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на нулевом векторе (у него нет
/// направления) и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shift_on_vector<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some([x, y, z, yaw, pitch]) = rt.optional_location_arg(stream, args, "location")?
        else {
            return Ok(None);
        };
        let [vx, vy, vz] = rt.vector_arg(stream, args, "vector")?;
        let length = vx.mul_add(vx, vy.mul_add(vy, vz * vz)).sqrt();
        if length <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "vector"),
            });
        }
        let distance = rt
            .optional_number_arg(stream, args, "length")?
            .unwrap_or(1.0);
        let factor = distance / length;
        Ok(Some(value::location(
            factor.mul_add(vx, x),
            factor.mul_add(vy, y),
            factor.mul_add(vz, z),
            yaw,
            pitch,
        )))
    })
}

/// `set_variable_shift_location_towards_location`: сдвиг в сторону цели.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на совпадающих точках и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shift_towards<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(from) = rt.optional_location_arg(stream, args, "location_from")? else {
            return Ok(None);
        };
        let Some(to) = rt.optional_location_arg(stream, args, "location_to")? else {
            return Ok(None);
        };
        let distance = rt.number_arg(stream, args, "distance")?;
        let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
        let length = dx.hypot(dy).hypot(dz);
        if length <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "location_to"),
            });
        }
        let factor = distance / length;
        Ok(Some(value::location(
            factor.mul_add(dx, from[0]),
            factor.mul_add(dy, from[1]),
            factor.mul_add(dz, from[2]),
            from[3],
            from[4],
        )))
    })
}

/// `set_variable_align_location`: привязка координат к сетке блоков.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn align<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some([mut x, mut y, mut z, mut yaw, mut pitch]) =
            rt.optional_location_arg(stream, args, "location")?
        else {
            return Ok(None);
        };
        if rt.enum_arg(stream, args, "rotation_mode")? == "REMOVE" {
            yaw = 0.0;
            pitch = 0.0;
        }
        let center = rt.enum_arg(stream, args, "align_mode")? == "BLOCK_CENTER";
        let align = |value: f64| {
            if center {
                value.floor() + 0.5
            } else {
                value.floor()
            }
        };
        match rt.enum_arg(stream, args, "coordinates_mode")? {
            "ALL" => {
                x = align(x);
                y = align(y);
                z = align(z);
            }
            "X_Z" => {
                x = align(x);
                z = align(z);
            }
            "Y" => y = align(y),
            other => return Err(invalid_enum(args, "coordinates_mode", other.to_owned())),
        }
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_center_location`: середина списка локаций.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn center<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let locations = location_items(rt, stream, args, "locations")?;
        if locations.is_empty() {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "locations",
            });
        }
        let mut sum = [0.0; 5];
        for location in &locations {
            for (index, value) in location.iter().enumerate() {
                sum[index] += value;
            }
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "the number of locations in one action argument is far below 2^53"
        )]
        let count = locations.len() as f64;
        let [x, y, z, yaw, pitch] = sum.map(|value| value / count);
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_clamp_location`: ограничение координат углами области.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn clamp<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(mut location) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let Some(first) = rt.optional_location_arg(stream, args, "corner_1")? else {
            return Ok(None);
        };
        let Some(second) = rt.optional_location_arg(stream, args, "corner_2")? else {
            return Ok(None);
        };
        let mode = rt.enum_arg(stream, args, "coordinates_mode")?;
        let axes: &[usize] = match mode {
            "XYZ" => &[0, 1, 2],
            "X_Z" => &[0, 2],
            "Y" => &[1],
            other => return Err(invalid_enum(args, "coordinates_mode", other.to_owned())),
        };
        for &index in axes {
            let (low, high) = (
                first[index].min(second[index]),
                first[index].max(second[index]),
            );
            location[index] = location[index].clamp(low, high);
        }
        let [x, y, z, yaw, pitch] = location;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_face_location`: поворот локации в сторону цели.
///
/// Позиция берётся у `location`, поворот — из направления на `target`: так же,
/// как `set_variable_set_location_direction` переводит вектор в углы.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на совпадающих точках и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn face<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some([x, y, z, _, _]) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let Some(target) = rt.optional_location_arg(stream, args, "target")? else {
            return Ok(None);
        };
        let (dx, dy, dz) = (target[0] - x, target[1] - y, target[2] - z);
        let length = dx.hypot(dy).hypot(dz);
        if length <= f64::EPSILON {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "target"),
            });
        }
        let yaw = (-dx).atan2(dz).to_degrees();
        let pitch = (-dy / length).asin().to_degrees();
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_find_nearest_location`: ближайшая локация из списка.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn nearest<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(from) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let mode = rt.enum_arg(stream, args, "distance_type")?;
        let locations = location_items(rt, stream, args, "locations")?;
        let mut best: Option<([f64; 5], f64)> = None;
        for candidate in locations {
            let distance = metric(from, candidate, mode);
            if best.is_none_or(|(_, best)| distance < best) {
                best = Some((candidate, distance));
            }
        }
        let Some((location, _)) = best else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "locations",
            });
        };
        let [x, y, z, yaw, pitch] = location;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_location_relative`: сдвиг локации в сторону блока.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной стороне и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn relative<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some([x, y, z, yaw, pitch]) = rt.optional_location_arg(stream, args, "location")?
        else {
            return Ok(None);
        };
        let distance = rt.number_arg(stream, args, "distance")?;
        let face = rt.enum_arg(stream, args, "block_face")?;
        let Some(direction) = face_vector(face) else {
            return Err(invalid_enum(args, "block_face", face.to_owned()));
        };
        Ok(Some(value::location(
            distance.mul_add(direction[0], x),
            distance.mul_add(direction[1], y),
            distance.mul_add(direction[2], z),
            yaw,
            pitch,
        )))
    })
}

/// `set_variable_locations_distance`: расстояние между локациями.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном типе
/// расстояния и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn distance<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(first) = rt.optional_location_arg(stream, args, "location_1")? else {
            return Ok(None);
        };
        let Some(second) = rt.optional_location_arg(stream, args, "location_2")? else {
            return Ok(None);
        };
        let mode = rt.enum_arg(stream, args, "type")?;
        let value = match mode {
            "THREE_D" | "XYZ" => three_d(first, second),
            "TWO_D" | "XZ" => two_d(first, second),
            "ALTITUDE" | "Y" => (first[1] - second[1]).abs(),
            "SQUARED_3D" => three_d(first, second).powi(2),
            "SQUARED_2D" => two_d(first, second).powi(2),
            other => return Err(invalid_enum(args, "type", other.to_owned())),
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_random_location`: случайная точка в области.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn random<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(first) = rt.optional_location_arg(stream, args, "location_1")? else {
            return Ok(None);
        };
        let Some(second) = rt.optional_location_arg(stream, args, "location_2")? else {
            return Ok(None);
        };
        let integer = matches!(rt.optional_enum_arg(stream, args, "integer")?, Some("TRUE"));
        let mut result = first;
        for index in 0..3 {
            let value = rt
                .world()
                .next_unit()
                .mul_add(second[index] - first[index], first[index]);
            result[index] = if integer { value.floor() } else { value };
        }
        let [x, y, z, yaw, pitch] = result;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_get_coordinate`: одна координата локации.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной координате и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn get_coordinate<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(location) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let index = coordinate_of(rt.enum_arg(stream, args, "type")?, args)?;
        Ok(Some(value::number(location[index])))
    })
}

/// `set_variable_set_coordinate`: замена одной координаты локации.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной координате и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_coordinate<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(mut location) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let coordinate = rt.number_arg(stream, args, "coordinate")?;
        let index = coordinate_of(rt.enum_arg(stream, args, "type")?, args)?;
        location[index] = coordinate;
        let [x, y, z, yaw, pitch] = location;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_shift_coordinate`: сдвиг одной координаты локации.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной координате и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shift_coordinate<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(mut location) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        let distance = rt.number_arg(stream, args, "distance")?;
        let index = coordinate_of(rt.enum_arg(stream, args, "type")?, args)?;
        location[index] += distance;
        let [x, y, z, yaw, pitch] = location;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_get_all_coordinates`: все пять чисел локации разом.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn get_all<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let Some(location) = rt.optional_location_arg(stream, args, "location")? else {
        return Ok(Flow::Continue);
    };
    for (index, name) in ["x", "y", "z", "yaw", "pitch"].into_iter().enumerate() {
        if let Some((scope, target)) = optional_target(rt, stream, args, name)? {
            rt.scope_store(stream, scope)
                .set(&target, Some(value::number(location[index])));
        }
    }
    Ok(Flow::Continue)
}

/// `set_variable_set_all_coordinates`: локация из пяти чисел.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn set_all<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let x = rt.number_arg(stream, args, "x")?;
        let y = rt.number_arg(stream, args, "y")?;
        let z = rt.number_arg(stream, args, "z")?;
        let yaw = rt.optional_number_arg(stream, args, "yaw")?.unwrap_or(0.0);
        let pitch = rt
            .optional_number_arg(stream, args, "pitch")?
            .unwrap_or(0.0);
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// `set_variable_shift_all_coordinates`: сдвиг всех пяти чисел локации.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn shift_all<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let Some(mut location) = rt.optional_location_arg(stream, args, "location")? else {
            return Ok(None);
        };
        for (index, name) in ["x", "y", "z", "yaw", "pitch"].into_iter().enumerate() {
            location[index] += rt.optional_number_arg(stream, args, name)?.unwrap_or(0.0);
        }
        let [x, y, z, yaw, pitch] = location;
        Ok(Some(value::location(x, y, z, yaw, pitch)))
    })
}

/// Список локаций из аргумента вида `location[21]`.
///
/// Пустые элементы пропускаются: список может прийти из переменной, часть
/// которой ещё не заполнена, и в `JustMC` это просто отсутствие точки, а не
/// ошибка всего действия.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
fn location_items<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    name: &'static str,
) -> Result<Vec<[f64; 5]>> {
    let mut locations = Vec::new();
    for value in rt.values_arg(stream, args, name)? {
        if value.is_none() || matches!(value, Some(Value::Error)) {
            continue;
        }
        locations.push(value::location_of(&value, &at_op(args, name))?);
    }
    Ok(locations)
}

/// Номер координаты по имени из схемы: `X`, `Y`, `Z`, `YAW`, `PITCH`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном имени.
#[tracing::instrument(level = "trace", skip(args), fields(name = %name))]
fn coordinate_of(name: &str, args: Args<'_>) -> Result<usize> {
    match name {
        "X" => Ok(0),
        "Y" => Ok(1),
        "Z" => Ok(2),
        "YAW" => Ok(3),
        "PITCH" => Ok(4),
        other => Err(invalid_enum(args, "type", other.to_owned())),
    }
}

/// Расстояние по трём осям.
#[tracing::instrument(level = "trace", fields(first = ?first, second = ?second))]
fn three_d(first: [f64; 5], second: [f64; 5]) -> f64 {
    (first[0] - second[0])
        .hypot(first[1] - second[1])
        .hypot(first[2] - second[2])
}

/// Расстояние по горизонтали.
#[tracing::instrument(level = "trace", fields(first = ?first, second = ?second))]
fn two_d(first: [f64; 5], second: [f64; 5]) -> f64 {
    (first[0] - second[0]).hypot(first[2] - second[2])
}

/// Расстояние по выбранной мере.
#[tracing::instrument(level = "trace", fields(from = ?from, to = ?to, mode = %mode))]
fn metric(from: [f64; 5], to: [f64; 5], mode: &str) -> f64 {
    match mode {
        "Y" => (from[1] - to[1]).abs(),
        "XZ" => two_d(from, to),
        _ => three_d(from, to),
    }
}
