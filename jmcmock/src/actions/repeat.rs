//! Resumable state for `repeat` containers.
//!
//! Preparing a loop snapshots its arguments; advancing it assigns iteration
//! variables without executing its body. The scheduler owns body execution and
//! handles stop, skip, return, and suspension without recursive Rust calls.

use jmcdata::generated::ActionId;
use jmcdata::module::{Conditional, Op, Value, VariableScope};

use crate::actions::code::optional_target;
use crate::error::Result;
use crate::interp::{Args, Flow, at_op, invalid_enum};
use crate::run::{Runtime, Stream};
use crate::value::{self, Rt, count_as_number};

type Target = Option<(VariableScope, String)>;

/// Saved progress of a loop whose body is executed by the scheduler.
#[derive(Debug)]
pub enum LoopState<'a> {
    Empty,
    Forever,
    MultiTimes {
        target: Target,
        value: f64,
        amount: f64,
    },
    Range {
        target: Target,
        value: f64,
        end: f64,
        step: f64,
    },
    /// `repeat_on_circle`: the iteration variable is a location on a circle.
    Circle {
        target: Target,
        center: [f64; 5],
        radius: f64,
        points: f64,
        start: f64,
        step: f64,
        basis_u: [f64; 3],
        basis_v: [f64; 3],
        index: f64,
    },
    List {
        index: Target,
        value: Target,
        items: std::iter::Enumerate<std::vec::IntoIter<Rt<'a>>>,
    },
    Map {
        key: Target,
        value: Target,
        entries: std::vec::IntoIter<(String, Value<'a>)>,
    },
    /// A ready list of locations: `repeat_on_path`, `repeat_on_sphere`,
    /// `repeat_adjacently`.
    Locations {
        target: Target,
        points: std::vec::IntoIter<[f64; 5]>,
    },
    /// `repeat_on_grid`: every block of an axis-aligned box, lazily.
    Grid {
        target: Target,
        start: [f64; 5],
        end: [f64; 5],
        current: [f64; 5],
        done: bool,
    },
    While {
        op: &'a Op<'a>,
        conditional: Conditional,
    },
}

/// Evaluates a loop's arguments once, before its first iteration.
///
/// # Errors
///
/// Returns argument evaluation errors or an unsupported-action error.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub fn prepare<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<LoopState<'a>> {
    let args = Args::of(op);
    match op.action {
        ActionId::RepeatForever => Ok(LoopState::Forever),
        ActionId::RepeatMultiTimes => {
            let target = optional_target(rt, stream, args, "variable")?;
            let amount = rt
                .optional_number_arg(stream, args, "amount")?
                .unwrap_or(0.0);
            Ok(LoopState::MultiTimes {
                target,
                value: 1.0,
                amount,
            })
        }
        ActionId::RepeatOnRange => {
            let target = optional_target(rt, stream, args, "variable")?;
            let start = rt
                .optional_number_arg(stream, args, "start")?
                .unwrap_or(0.0);
            let end = rt.optional_number_arg(stream, args, "end")?.unwrap_or(0.0);
            let step = rt.optional_number_arg(stream, args, "interval")?;
            let step = match step {
                Some(step) if step != 0.0 => step,
                _ if end < start => -1.0,
                _ => 1.0,
            };
            Ok(LoopState::Range {
                target,
                value: start,
                end,
                step,
            })
        }
        ActionId::RepeatOnCircle => prepare_circle(rt, stream, args),
        ActionId::RepeatOnGrid => prepare_grid(rt, stream, args),
        ActionId::RepeatOnPath => prepare_path(rt, stream, args),
        ActionId::RepeatOnSphere => prepare_sphere(rt, stream, args),
        ActionId::RepeatAdjacently => prepare_adjacently(rt, stream, args),
        ActionId::RepeatForEachInList => {
            let index = optional_target(rt, stream, args, "index_variable")?;
            let value = optional_target(rt, stream, args, "value_variable")?;
            let items = rt.list_arg(stream, args, "list")?.into_iter().enumerate();
            Ok(LoopState::List {
                index,
                value,
                items,
            })
        }
        ActionId::RepeatForEachMapEntry => {
            let key = optional_target(rt, stream, args, "key_variable")?;
            let value = optional_target(rt, stream, args, "value_variable")?;
            let entries = rt.map_arg(stream, args, "map")?;
            // Keep the map's actual iteration order and snapshot its values.
            let entries = entries
                .iter()
                .map(|(key, value)| (key.0.clone(), value.clone()))
                .collect::<Vec<_>>()
                .into_iter();
            Ok(LoopState::Map {
                key,
                value,
                entries,
            })
        }
        ActionId::RepeatWhile => {
            if let Some(conditional) = op.conditional {
                Ok(LoopState::While { op, conditional })
            } else {
                rt.unimplemented_op(stream, op)?;
                Ok(LoopState::Empty)
            }
        }
        ActionId::RepeatDummy => Ok(LoopState::Empty),
        _other => {
            rt.unimplemented_op(stream, op)?;
            Ok(LoopState::Empty)
        }
    }
}

/// Evaluates `repeat_on_circle`'s arguments into a [`LoopState::Circle`].
///
/// `circle_points` counts points, so the angular pitch is a full turn divided by
/// it; with no points there is nothing to walk and the loop ends at once.
///
/// # Errors
///
/// Returns argument evaluation errors.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn prepare_circle<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<LoopState<'a>> {
    let target = optional_target(rt, stream, args, "variable")?;
    let center = rt.location_arg(stream, args, "center")?;
    let radius = rt
        .optional_number_arg(stream, args, "radius")?
        .unwrap_or(0.0);
    let points = rt
        .optional_number_arg(stream, args, "circle_points")?
        .unwrap_or(0.0);
    let normal = rt.vector_arg(stream, args, "perpendicular_to_plane")?;
    let start_angle = rt
        .optional_number_arg(stream, args, "start_angle")?
        .unwrap_or(0.0);
    let step = if points > 0.0 {
        std::f64::consts::TAU / points
    } else {
        0.0
    };
    let start = match rt.optional_enum_arg(stream, args, "angle_unit")? {
        Some("RADIANS") => start_angle,
        _ => start_angle.to_radians(),
    };
    let (basis_u, basis_v) = circle_basis(normal);
    Ok(LoopState::Circle {
        target,
        center,
        radius,
        points,
        start,
        step,
        basis_u,
        basis_v,
        index: 0.0,
    })
}

/// Evaluates `repeat_on_grid`'s arguments into a [`LoopState::Grid`].
///
/// The box is taken by whole blocks: both corners are rounded down, and both are
/// included, so a 1×1×1 box gives exactly one iteration.
///
/// # Errors
///
/// Returns argument evaluation errors.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn prepare_grid<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<LoopState<'a>> {
    let target = optional_target(rt, stream, args, "variable")?;
    let start = rt.location_arg(stream, args, "start")?;
    let end = rt.location_arg(stream, args, "end")?;
    // The corners may arrive in any order; the box is the same either way.
    let mut first = floor_location(start);
    let mut second = floor_location(end);
    for axis in 0..3 {
        if first[axis] > second[axis] {
            (first[axis], second[axis]) = (second[axis], first[axis]);
        }
    }
    Ok(LoopState::Grid {
        target,
        start: first,
        end: second,
        current: first,
        done: false,
    })
}

/// `repeat_on_path`: walks a polyline with a fixed step.
///
/// The samples are computed once: the path never changes while the loop runs,
/// and the distance between two samples is the same on every iteration.
///
/// # Errors
///
/// Returns argument evaluation errors.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn prepare_path<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<LoopState<'a>> {
    let target = optional_target(rt, stream, args, "variable")?;
    let step = rt.optional_number_arg(stream, args, "step")?.unwrap_or(1.0);
    let rotation = matches!(
        rt.optional_enum_arg(stream, args, "rotation")?,
        Some("TRUE")
    );
    let mut points = Vec::new();
    for value in rt.values_arg(stream, args, "locations")? {
        points.push(value::location_of(&value, &at_op(args, "locations"))?);
    }
    Ok(LoopState::Locations {
        target,
        points: path_points(&points, step, rotation).into_iter(),
    })
}

/// Точки пути через каждые `step` единиц длины.
///
/// Последняя точка пути попадает в список всегда, даже если шаг не уложился в
/// длину сегмента: иначе конец пути терялся бы.
#[expect(
    clippy::while_float,
    reason = "the path is walked by arc length, which is a floating-point number; the loop ends \
              when the remaining length runs out"
)]
#[tracing::instrument(level = "trace", fields(points = ?points, step = ?step, rotation = ?rotation))]
fn path_points(points: &[[f64; 5]], step: f64, rotation: bool) -> Vec<[f64; 5]> {
    if points.is_empty() {
        return Vec::new();
    }
    let step = if step.is_finite() && step > 0.0 {
        step
    } else {
        1.0
    };
    let mut samples = vec![points[0]];
    let mut leftover = 0.0;
    for pair in points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (dx, dy, dz) = (to[0] - from[0], to[1] - from[1], to[2] - from[2]);
        let length = dx.hypot(dy).hypot(dz);
        if length <= f64::EPSILON {
            samples.push(to);
            continue;
        }
        let mut travelled = step - leftover;
        while travelled < length {
            let ratio = travelled / length;
            samples.push(interpolate(from, to, ratio, rotation));
            travelled += step;
        }
        leftover = length - (travelled - step);
        samples.push(to);
    }
    samples
}

/// Точка между двумя локациями; поворот переносится, если он нужен.
#[tracing::instrument(level = "trace", fields(from = ?from, to = ?to, ratio = ?ratio, rotation = ?rotation))]
fn interpolate(from: [f64; 5], to: [f64; 5], ratio: f64, rotation: bool) -> [f64; 5] {
    let mut result = from;
    for index in 0..3 {
        result[index] = ratio.mul_add(to[index] - from[index], from[index]);
    }
    if rotation {
        result[3] = ratio.mul_add(to[3] - from[3], from[3]);
        result[4] = ratio.mul_add(to[4] - from[4], from[4]);
    }
    result
}

/// `repeat_on_sphere`: points on a sphere around a centre.
///
/// The points are spread by the Fibonacci lattice: it gives an even covering
/// without clustering at the poles. `rotate_location` turns the location's gaze
/// away from the centre (`OUTWARDS`) or towards it (`INWARDS`).
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] for an unknown rotation mode and
/// argument evaluation errors.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn prepare_sphere<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<LoopState<'a>> {
    let target = optional_target(rt, stream, args, "variable")?;
    let center = rt.location_arg(stream, args, "center")?;
    let radius = rt
        .optional_number_arg(stream, args, "radius")?
        .unwrap_or(0.0);
    let points = rt
        .optional_number_arg(stream, args, "points")?
        .unwrap_or(0.0);
    let rotation = match rt.enum_arg(stream, args, "rotate_location")? {
        "NO_CHANGES" => None,
        "OUTWARDS" => Some(1.0),
        "INWARDS" => Some(-1.0),
        other => return Err(invalid_enum(args, "rotate_location", other.to_owned())),
    };
    Ok(LoopState::Locations {
        target,
        points: sphere_points(center, radius, points, rotation).into_iter(),
    })
}

/// Точки на сфере по решётке Фибоначчи.
#[tracing::instrument(level = "trace", fields(center = ?center, radius = ?radius, points = ?points, rotation = ?rotation))]
fn sphere_points(
    center: [f64; 5],
    radius: f64,
    points: f64,
    rotation: Option<f64>,
) -> Vec<[f64; 5]> {
    let count = point_count(points);
    let mut result = Vec::with_capacity(count);
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    for index in 0..count {
        #[expect(
            clippy::cast_precision_loss,
            reason = "the number of points on a sphere is small"
        )]
        let index_f = index as f64;
        #[expect(
            clippy::cast_precision_loss,
            reason = "the number of points on a sphere is small"
        )]
        let count_f = count as f64;
        let y = 1.0 - 2.0 * (index_f + 0.5) / count_f;
        let horizontal = (1.0 - y * y).max(0.0).sqrt();
        let angle = golden * index_f;
        let (sin, cos) = angle.sin_cos();
        let (x, z) = (horizontal * cos, horizontal * sin);
        let mut location = [
            radius.mul_add(x, center[0]),
            radius.mul_add(y, center[1]),
            radius.mul_add(z, center[2]),
            center[3],
            center[4],
        ];
        if let Some(direction) = rotation {
            let (yaw, pitch) = gaze([x * direction, y * direction, z * direction]);
            location[3] = yaw;
            location[4] = pitch;
        }
        result.push(location);
    }
    result
}

/// `repeat_adjacently`: the neighbouring blocks of an origin.
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] for an unknown pattern and
/// argument evaluation errors.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn prepare_adjacently<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<LoopState<'a>> {
    let target = optional_target(rt, stream, args, "variable")?;
    let origin = rt.location_arg(stream, args, "origin")?;
    let change_rotation = matches!(
        rt.optional_enum_arg(stream, args, "change_rotation")?,
        Some("TRUE")
    );
    let include_self = matches!(
        rt.optional_enum_arg(stream, args, "include_self")?,
        Some("TRUE")
    );
    let pattern = rt.enum_arg(stream, args, "pattern")?;
    Ok(LoopState::Locations {
        target,
        points: adjacent_points(origin, pattern, change_rotation, include_self, args)?.into_iter(),
    })
}

/// Смещения соседей и точки вокруг `origin`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном образце.
#[tracing::instrument(level = "trace", skip(args), fields(origin = ?origin, pattern = %pattern, change_rotation = ?change_rotation, include_self = ?include_self))]
fn adjacent_points(
    origin: [f64; 5],
    pattern: &str,
    change_rotation: bool,
    include_self: bool,
    args: Args<'_>,
) -> Result<Vec<[f64; 5]>> {
    let offsets: &[[f64; 3]] = match pattern {
        // Six faces.
        "ADJACENT" => &[
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ],
        // The four horizontal neighbours.
        "CARDINAL" => &[
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ],
        // The whole 3×3×3 block around the origin.
        "CUBE" => &CUBE_OFFSETS,
        // The eight horizontal neighbours.
        "SQUARE" => &[
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
            [1.0, 0.0, -1.0],
            [-1.0, 0.0, 1.0],
            [-1.0, 0.0, -1.0],
        ],
        other => return Err(invalid_enum(args, "pattern", other.to_owned())),
    };
    let mut result = Vec::with_capacity(offsets.len() + usize::from(include_self));
    if include_self {
        result.push(origin);
    }
    for offset in offsets {
        let mut location = [
            origin[0] + offset[0],
            origin[1] + offset[1],
            origin[2] + offset[2],
            origin[3],
            origin[4],
        ];
        if change_rotation {
            let (yaw, pitch) = gaze(*offset);
            location[3] = yaw;
            location[4] = pitch;
        }
        result.push(location);
    }
    Ok(result)
}

/// Двадцать шесть смещений вокруг точки.
static CUBE_OFFSETS: [[f64; 3]; 26] = cube_offsets();

/// Строит смещения 3×3×3 без нулевого.
const fn cube_offsets() -> [[f64; 3]; 26] {
    let mut offsets = [[0.0; 3]; 26];
    let mut index = 0;
    let mut x = -1;
    while x <= 1 {
        let mut y = -1;
        while y <= 1 {
            let mut z = -1;
            while z <= 1 {
                if !(x == 0 && y == 0 && z == 0) {
                    offsets[index] = [x as f64, y as f64, z as f64];
                    index += 1;
                }
                z += 1;
            }
            y += 1;
        }
        x += 1;
    }
    offsets
}

/// Углы взгляда по направлению.
#[tracing::instrument(level = "trace", fields(direction = ?direction))]
fn gaze(direction: [f64; 3]) -> (f64, f64) {
    let [x, y, z] = direction;
    let length = x.mul_add(x, y.mul_add(y, z * z)).sqrt();
    if length <= f64::EPSILON {
        return (0.0, 0.0);
    }
    (
        (-x).atan2(z).to_degrees(),
        (-y / length).asin().to_degrees(),
    )
}

/// Округляет координаты локации вниз, до целого блока.
#[tracing::instrument(level = "trace", fields(location = ?location))]
fn floor_location(location: [f64; 5]) -> [f64; 5] {
    let mut result = location;
    for coordinate in &mut result[..3] {
        *coordinate = coordinate.floor();
    }
    result
}

/// Шаг по сетке блока: `z` внутри `y`, `y` внутри `x`.
#[tracing::instrument(level = "trace", fields(current = ?current, start = ?start, end = ?end, done = ?done))]
fn advance_grid(current: &mut [f64; 5], start: [f64; 5], end: [f64; 5], done: &mut bool) {
    current[2] += 1.0;
    if current[2] <= end[2] {
        return;
    }
    current[2] = start[2];
    current[1] += 1.0;
    if current[1] <= end[1] {
        return;
    }
    current[1] = start[1];
    current[0] += 1.0;
    if current[0] > end[0] {
        *done = true;
    }
}

/// Число точек как `usize`, ограниченное разумным пределом.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into a small non-negative range before the cast"
)]
const fn point_count(points: f64) -> usize {
    points.trunc().clamp(0.0, 65_536.0) as usize
}

impl<'a> LoopState<'a> {
    /// Assigns iteration variables and reports whether the body should run.
    ///
    /// Every attempt consumes a step, including a terminating attempt, so even
    /// an empty infinite body is bounded by the configured step limit.
    ///
    /// # Errors
    ///
    /// Returns step-limit errors and errors evaluating a while condition.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per loop kind reads as the table of what each loop yields; splitting it \
                  would scatter the comparison of the kinds"
    )]
    #[tracing::instrument(level = "trace", skip(self, rt, stream))]
    pub(crate) fn next(&mut self, rt: &mut Runtime<'a>, stream: &mut Stream<'a>) -> Result<bool> {
        rt.step()?;
        match self {
            Self::Empty => Ok(false),
            Self::Forever => Ok(true),
            Self::MultiTimes {
                target,
                value,
                amount,
            } => {
                // The counter is one-based: `repeat::multi_times(n)` counts
                // `1..=n`, the way `JustMC` numbers it. Programs lean on that —
                // `std/ai/nn.jc` subtracts one from the counter to index a list.
                // Keep the original floating-point comparison (including NaN).
                if *value <= *amount {
                    assign(rt, stream, target, Some(value::number(*value)));
                    *value += 1.0;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Self::Range {
                target,
                value,
                end,
                step,
            } => {
                let forward = *step > 0.0;
                if (forward && *value > *end) || (!forward && *value < *end) {
                    return Ok(false);
                }
                assign(rt, stream, target, Some(value::number(*value)));
                *value += *step;
                Ok(true)
            }
            Self::Circle {
                target,
                center,
                radius,
                points,
                start,
                step,
                basis_u,
                basis_v,
                index,
            } => {
                if *index >= *points {
                    return Ok(false);
                }
                assign(
                    rt,
                    stream,
                    target,
                    value::rt(circle_point(
                        *center, *radius, *start, *step, *index, *basis_u, *basis_v,
                    )),
                );
                *index += 1.0;
                Ok(true)
            }
            Self::List {
                index,
                value,
                items,
            } => {
                let Some((position, item)) = items.next() else {
                    return Ok(false);
                };
                assign(
                    rt,
                    stream,
                    index,
                    Some(value::number(count_as_number(position))),
                );
                assign(rt, stream, value, item);
                Ok(true)
            }
            Self::Map {
                key,
                value,
                entries,
            } => {
                let Some((entry_key, entry_value)) = entries.next() else {
                    return Ok(false);
                };
                assign(rt, stream, key, value::rt(value::decode_key(&entry_key)));
                assign(rt, stream, value, Some(entry_value));
                Ok(true)
            }
            Self::Locations { target, points } => {
                let Some([x, y, z, yaw, pitch]) = points.next() else {
                    return Ok(false);
                };
                assign(
                    rt,
                    stream,
                    target,
                    value::rt(value::location(x, y, z, yaw, pitch)),
                );
                Ok(true)
            }
            Self::Grid {
                target,
                start,
                end,
                current,
                done,
            } => {
                if *done {
                    return Ok(false);
                }
                assign(
                    rt,
                    stream,
                    target,
                    value::rt(value::location(
                        current[0], current[1], current[2], current[3], current[4],
                    )),
                );
                advance_grid(current, *start, *end, done);
                Ok(true)
            }
            Self::While { op, conditional } => {
                Ok(rt.condition(stream, conditional.action, op)? != conditional.is_inverted)
            }
        }
    }
}

/// The `index`-th point of a circle:
/// `center + radius * (cos(angle) * u + sin(angle) * v)`.
#[tracing::instrument(level = "trace", fields(center = ?center, radius = ?radius, start = ?start, step = ?step, index = ?index, basis_u = ?basis_u, basis_v = ?basis_v))]
fn circle_point<'a>(
    center: [f64; 5],
    radius: f64,
    start: f64,
    step: f64,
    index: f64,
    basis_u: [f64; 3],
    basis_v: [f64; 3],
) -> Value<'a> {
    let angle = step.mul_add(index, start);
    let (sin, cos) = angle.sin_cos();
    let [cx, cy, cz, yaw, pitch] = center;
    value::location(
        radius.mul_add(cos.mul_add(basis_u[0], sin * basis_v[0]), cx),
        radius.mul_add(cos.mul_add(basis_u[1], sin * basis_v[1]), cy),
        radius.mul_add(cos.mul_add(basis_u[2], sin * basis_v[2]), cz),
        yaw,
        pitch,
    )
}

/// An orthonormal basis `(u, v)` of the plane perpendicular to `normal`.
///
/// The circle is walked as `center + radius * (cos(angle) * u + sin(angle) * v)`.
/// A degenerate normal — a zero vector — leaves the plane undefined; the
/// horizontal plane is used instead of failing, so the loop still yields points
/// rather than silently doing nothing.
#[tracing::instrument(level = "trace", fields(normal = ?normal))]
fn circle_basis(normal: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = normal;
    let length = x.mul_add(x, y.mul_add(y, z * z)).sqrt();
    let n = if length > f64::EPSILON {
        [x / length, y / length, z / length]
    } else {
        [0.0, 1.0, 0.0]
    };
    // The helper must not be parallel to `n`, otherwise the cross product
    // vanishes and the basis is lost.
    let helper = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(helper, n));
    let v = cross(n, u);
    (u, v)
}

/// The cross product of two three-dimensional vectors.
#[tracing::instrument(level = "trace", fields(a = ?a, b = ?b))]
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1].mul_add(b[2], -(a[2] * b[1])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
    ]
}

/// Scales a vector to unit length; a zero vector stays zero.
#[tracing::instrument(level = "trace", fields(v = ?v))]
fn normalize(v: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = v;
    let length = x.mul_add(x, y.mul_add(y, z * z)).sqrt();
    if length > f64::EPSILON {
        [x / length, y / length, z / length]
    } else {
        v
    }
}

#[tracing::instrument(level = "trace", skip(rt, stream), fields(target = ?target, value = ?value))]
fn assign<'a>(rt: &Runtime<'a>, stream: &Stream<'a>, target: &Target, value: Rt<'a>) {
    if let Some((scope, name)) = target {
        rt.scope_store(stream, *scope).set(name, value);
    }
}

/// Fallback for repeat actions not intercepted by the scheduler.
///
/// # Errors
///
/// Returns an unsupported-action error in strict mode.
#[tracing::instrument(level = "debug", skip(rt, _stream, op))]
pub fn dispatch<'a>(
    rt: &mut Runtime<'a>,
    _stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    match op.action {
        ActionId::RepeatDummy => Ok(Flow::Continue),
        _other => rt.unimplemented_op(_stream, op),
    }
}
