//! Арифметика и логика над числами: сложение и вычитание, деление,
//! округление, остаток, побитовые операции, тригонометрия.

use super::*;

/// Adds, subtracts, multiplies or compares the values of the argument, writing
/// the result into the target.
///
/// The target is not read: `JustMC` seeds the fold with the first number of the
/// argument ("Add Numbers" *sets* the variable, unlike "Increment Number" —
/// [`fold_over`]).
///
/// # Errors
///
/// Returns [`RuntimeError::MissingArgument`] if the argument carries no numbers,
/// and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, apply), fields(arg = %arg))]
pub(super) fn fold<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    arg: &'static str,
    apply: impl Fn(f64, f64) -> f64,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut numbers = rt.numbers_arg(stream, args, arg)?.into_iter();
        let Some(first) = numbers.next() else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg,
            });
        };
        Ok(Some(value::number(numbers.fold(first, apply))))
    })
}

/// Changes the target's current value by every value of the argument
/// (`set_variable_increment`, `set_variable_decrement`).
///
/// Unlike [`fold`], the target seeds the fold instead of being replaced by it.
///
/// # Errors
///
/// Returns [`RuntimeError::ExpectedNumber`] if the target is not a number, and
/// any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, apply), fields(arg = %arg))]
pub(super) fn fold_over<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    arg: &'static str,
    apply: impl Fn(f64, f64) -> f64,
) -> Result<Flow> {
    assign_over(rt, stream, op, |rt, stream, args, current| {
        let mut accumulator = value::number_of(&current, &at_op(args, "variable"))?;
        for next in rt.numbers_arg(stream, args, arg)? {
            accumulator = apply(accumulator, next);
        }
        Ok(Some(value::number(accumulator)))
    })
}

/// Applies a function to the argument, leaving the target's current value
/// alone.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, apply), fields(arg = %arg))]
pub(super) fn unary<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    arg: &'static str,
    apply: impl Fn(f64) -> f64,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let value = rt.number_arg(stream, args, arg)?;
        Ok(Some(value::number(apply(value))))
    })
}

/// Applies a function to two arguments, leaving the target's current value
/// alone.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op, apply), fields(left = %left, right = %right))]
pub(super) fn binary<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    left: &'static str,
    right: &'static str,
    apply: impl Fn(f64, f64) -> f64,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let a = rt.number_arg(stream, args, left)?;
        let b = rt.number_arg(stream, args, right)?;
        Ok(Some(value::number(apply(a, b))))
    })
}

/// `set_variable_divide`: sets the target to the quotient of the numbers in
/// `value`, dividing left to right.
///
/// As in [`fold`], the target's current value does not take part.
///
/// Division by zero stops execution: `f64` would return infinity or `NaN`, and
/// that value would then travel on as an ordinary number.
///
/// # Errors
///
/// Returns [`RuntimeError::DivisionByZero`] if a divisor is zero,
/// [`RuntimeError::MissingArgument`] if `value` carries no numbers, and any
/// error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn divide<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let mode = {
        let args = Args::of(op);
        rt.optional_enum_arg(stream, args, "division_mode")?
    };
    assign(rt, stream, op, |rt, stream, args| {
        let mut numbers = rt.numbers_arg(stream, args, "value")?.into_iter();
        let Some(mut accumulator) = numbers.next() else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "value",
            });
        };
        for next in numbers {
            if next == 0.0 {
                return Err(RuntimeError::DivisionByZero {
                    context: at_op(args, "value"),
                });
            }
            accumulator /= next;
        }
        Ok(Some(value::number(round_like(accumulator, mode))))
    })
}

/// Rounds a division result according to the `division_mode` argument.
#[tracing::instrument(level = "trace", fields(value = ?value, mode = ?mode))]
fn round_like(value: f64, mode: Option<&str>) -> f64 {
    match mode {
        Some("CEIL") => value.ceil(),
        Some("FLOOR") => value.floor(),
        Some("ROUND_TO_INT") => value.round(),
        _ => value,
    }
}

/// `set_variable_round`: rounds to `precision` digits after the point.
///
/// Precision and mode may be left out — `tests/pvp` writes
/// `variable::round(number, 1)` — and then the number is rounded to the nearest
/// integer, the ordinary meaning of "round".
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn round<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let precision = rt
            .optional_number_arg(stream, args, "precision")?
            .unwrap_or(0.0);
        let mode = rt
            .optional_enum_arg(stream, args, "round_type")?
            .unwrap_or("ROUND");
        let scale = 10_f64.powi(exponent(precision));
        let scaled = number * scale;
        let rounded = match mode {
            "CEIL" => scaled.ceil(),
            "FLOOR" => scaled.floor(),
            _ => scaled.round(),
        };
        Ok(Some(value::number(rounded / scale)))
    })
}

/// The rounding precision as a power of ten.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the rounding precision in JustMC is an integer; the fraction is dropped"
)]
const fn exponent(precision: f64) -> i32 {
    precision.trunc() as i32
}

/// `set_variable_root`: the `root`-th root of `base`.
///
/// # Errors
///
/// Returns [`RuntimeError::DivisionByZero`] for a zero degree, and any error
/// from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn root<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let base = rt.number_arg(stream, args, "base")?;
        let degree = rt.number_arg(stream, args, "root")?;
        if degree == 0.0 {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "root"),
            });
        }
        Ok(Some(value::number(base.powf(1.0 / degree))))
    })
}

/// `set_variable_remainder`: the remainder of a division.
///
/// `MODULO` gives a result with the divisor's sign (like `%` in `JustMC`),
/// `REMAINDER` with the dividend's — the latter is Rust's `%`, the former is
/// `rem_euclid`. The mode may be left out: `tests/pvp` calls the action with a
/// dividend and a divisor alone, and `MODULO` is what the `%` operator itself
/// asks for.
///
/// # Errors
///
/// Returns [`RuntimeError::DivisionByZero`] for a zero divisor, and any error
/// from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remainder<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let dividend = rt.number_arg(stream, args, "dividend")?;
        let divisor = rt.number_arg(stream, args, "divisor")?;
        if divisor == 0.0 {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "divisor"),
            });
        }
        let mode = rt.optional_enum_arg(stream, args, "remainder_mode")?;
        let value = match mode {
            Some("REMAINDER") => dividend % divisor,
            _ => dividend.rem_euclid(divisor),
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_clamp`: limits a number from below and above.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn clamp<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let min = rt.number_arg(stream, args, "min")?;
        let max = rt.number_arg(stream, args, "max")?;
        Ok(Some(value::number(number.clamp(min, max))))
    })
}

/// `set_variable_random_number`: a random number from a range.
///
/// The source of randomness is [`World::next_unit`](crate::World::next_unit),
/// the same linear congruential generator as `%random%` in text: a mock run has
/// to be reproducible.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn random_number<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let min = rt.number_arg(stream, args, "min")?;
        let max = rt.number_arg(stream, args, "max")?;
        let integer = matches!(rt.optional_enum_arg(stream, args, "integer")?, Some("TRUE"));
        let unit = rt.world().next_unit();
        let value = if integer {
            (min + (unit * (max - min + 1.0)).floor()).min(max)
        } else {
            unit.mul_add(max - min, min)
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_bitwise_operation`: a bitwise operation over two numbers.
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] for an unknown operator, and
/// any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn bitwise<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let left = integer(rt.number_arg(stream, args, "operand1")?);
        let right = integer(rt.number_arg(stream, args, "operand2")?);
        let operator = rt.enum_arg(stream, args, "operator")?;
        let value = match operator {
            "AND" => left & right,
            "OR" => left | right,
            "XOR" => left ^ right,
            "LEFT_SHIFT" => left.wrapping_shl(shift(right)),
            "RIGHT_SHIFT" => left.wrapping_shr(shift(right)),
            // In JustMC this is an unsigned shift: a signed number shifted
            // logically.
            "UNSIGNED_RIGHT_SHIFT" => unsigned_shr(left, shift(right)),
            "NOT" => !left,
            other => return Err(invalid_enum(args, "operator", other.to_owned())),
        };
        Ok(Some(value::number(value as f64)))
    })
}

/// Brings a number to an integer for a bitwise operation.
#[expect(
    clippy::cast_possible_truncation,
    reason = "bitwise operations in JustMC are defined over integers; the fraction is dropped"
)]
const fn integer(value: f64) -> i64 {
    value.trunc() as i64
}

/// Brings a shift amount into the range `wrapping_shl` understands.
///
/// No `#[expect]` for the cast: `rem_euclid` returns a value in `0..64`, so the
/// `as u32` cannot lose a sign. That holds for a negative amount too — `-1`
/// lands on `63`, exactly the unsigned reading of the shift.
const fn shift(value: i64) -> u32 {
    value.rem_euclid(64) as u32
}

/// A logical right shift: the high bits are filled with zeros, not with the
/// sign.
#[expect(
    clippy::cast_sign_loss,
    reason = "the shift is meant to be unsigned; the sign bit must not replicate"
)]
const fn unsigned_shr(value: i64, amount: u32) -> i64 {
    ((value as u64) >> amount) as i64
}

/// `set_variable_sine` and `set_variable_cosine`: direct and inverse functions,
/// with a choice of degrees or radians.
///
/// # Errors
///
/// Returns [`RuntimeError::InvalidEnumArgument`] for a variant that does not
/// belong to the action, and any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op), fields(sine = ?sine))]
pub(super) fn trigonometry<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    sine: bool,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let variant = rt.enum_arg(stream, args, "variant")?;
        let input = rt.enum_arg(stream, args, "input")?;
        let radius = match input {
            "DEGREES" => number.to_radians(),
            _ => number,
        };
        let value = match (sine, variant) {
            (true, "SINE") => radius.sin(),
            (true, "ARCSINE") => radius.asin(),
            (true, "HYPERBOLIC_SINE") => radius.sinh(),
            (true, "HYPERBOLIC_ARCSINE") => radius.asinh(),
            (false, "COSINE") => radius.cos(),
            (false, "ARCCOSINE") => radius.acos(),
            (false, "HYPERBOLIC_COSINE") => radius.cosh(),
            (false, "HYPERBOLIC_ARCCOSINE") => radius.acosh(),
            (_, other) => return Err(invalid_enum(args, "variant", other.to_owned())),
        };
        // The inverse functions in JustMC return degrees when the angle is
        // given in degrees: the answer has to be in the same unit as the
        // question.
        let value = if (variant.ends_with("ARCOSINE") || variant.ends_with("ARCSINE"))
            && input == "DEGREES"
        {
            value.to_degrees()
        } else {
            value
        };
        Ok(Some(value::number(value)))
    })
}
