//! Математика сверх арифметики: тригонометрия, статистика, шум, время, цвета.
//!
//! Сюда попало всё числовое из блока `variable`, у чего нет своего дома в
//! [`super::arithmetic`]: обратные тригонометрические функции, средние и
//! медиана, гамма-функция и нормальное распределение, интерполяция и
//! перекладывание диапазона, шум, разбор и сборка JSON, время и цвета.
//!
//! # Где мок считает по-своему
//!
//! Три действия — `set_variable_perlin_noise_3d`, `set_variable_simplex_noise_3d`
//! и `set_variable_voronoi_noise_3d` — в `JustMC` считаются закрытой
//! реализацией, воспроизвести которую по схеме нельзя. Мок считает их своим
//! детерминированным шумом: форма результата та же (диапазон, октавы, частота,
//! нормализация соблюдены), а числа другие. Об этом сказано в документации
//! [`noise`]: шум годится, чтобы прогнать ветку кода, но не чтобы сверить
//! значение с сервером.
//!
//! # Чего здесь нет
//!
//! `set_variable_hash`, `set_variable_get_text_width`, `set_variable_to_rgb` и
//! родственные им, `set_variable_code_bytes`, регулярные выражения: у мока нет
//! ни криптографии, ни таблиц ширины глифов, ни кодировок цвета, ни `zlib`, ни
//! движка регулярных выражений. Выдуманное значение здесь хуже отказа: его
//! примут за настоящее.

use super::*;

/// Числовые действия. `None` — действие не из этой группы.
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
        ActionId::SetVariableAtan2 => atan2(rt, stream, op),
        ActionId::SetVariableTangent => tangent(rt, stream, op, true),
        ActionId::SetVariableCotangent => tangent(rt, stream, op, false),
        ActionId::SetVariableAverage => average(rt, stream, op),
        ActionId::SetVariableMedian => median(rt, stream, op),
        ActionId::SetVariableMathematicalExpectation => expectation(rt, stream, op),
        ActionId::SetVariableGammaFunction => gamma(rt, stream, op),
        ActionId::SetVariableGaussianDistribution => gaussian(rt, stream, op),
        ActionId::SetVariableLerpNumber => lerp(rt, stream, op),
        ActionId::SetVariableMapRange => map_range(rt, stream, op),
        ActionId::SetVariableWarp => warp(rt, stream, op),
        ActionId::SetVariableFindNearestNumber => nearest_number(rt, stream, op),
        ActionId::SetVariableConvertNumberToText => number_to_text(rt, stream, op),
        ActionId::SetVariableCharToNumber => char_to_number(rt, stream, op),
        ActionId::SetVariableToChar => to_char(rt, stream, op),
        ActionId::SetVariableGetTextSimilarity => text_similarity(rt, stream, op),
        ActionId::SetVariableGetIndexOfSubtext => index_of_subtext(rt, stream, op),
        ActionId::SetVariableParseJson => parse_json(rt, stream, op),
        ActionId::SetVariableToJson => to_json(rt, stream, op),
        ActionId::SetVariableFormatTimestamp => format_timestamp(rt, stream, op),
        ActionId::SetVariablePerlinNoise3d => noise(rt, stream, op, Noise::Perlin),
        ActionId::SetVariableSimplexNoise3d => noise(rt, stream, op, Noise::Simplex),
        ActionId::SetVariableVoronoiNoise3d => noise(rt, stream, op, Noise::Voronoi),
        _ => return None,
    })
}

/// `set_variable_atan2`: угол вектора `(x, y)`.
///
/// Угол возвращается в градусах: единиц измерения у этого действия в схеме нет,
/// а весь остальной `JustMC` показывает углы в градусах.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn atan2<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let y = rt.number_arg(stream, args, "y")?;
        let x = rt.number_arg(stream, args, "x")?;
        Ok(Some(value::number(y.atan2(x).to_degrees())))
    })
}

/// `set_variable_tangent` и `set_variable_cotangent`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном варианте или
/// единицах угла и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op), fields(tangent = ?tangent))]
pub(super) fn tangent<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    tangent: bool,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let variant = rt.enum_arg(stream, args, "variant")?;
        let input = rt.enum_arg(stream, args, "input")?;
        let radius = match input {
            "DEGREES" => number.to_radians(),
            "RADIANS" => number,
            other => return Err(invalid_enum(args, "input", other.to_owned())),
        };
        let value = match (tangent, variant) {
            (true, "TANGENT") => radius.tan(),
            (true, "ARCTANGENT") => radius.atan(),
            (true, "HYPERBOLIC_TANGENT") => radius.tanh(),
            (true, "HYPERBOLIC_ARCTANGENT") => radius.atanh(),
            (false, "COTANGENT") => 1.0 / radius.tan(),
            (false, "ARCCOTANGENT") => std::f64::consts::FRAC_PI_2 - radius.atan(),
            (false, "HYPERBOLIC_COTANGENT") => 1.0 / radius.tanh(),
            (false, "HYPERBOLIC_ARCCOTANGENT") => (radius + 1.0) / (radius - 1.0) / 2.0,
            (_, other) => return Err(invalid_enum(args, "variant", other.to_owned())),
        };
        let value = if variant.starts_with("ARC") && input == "DEGREES" {
            value.to_degrees()
        } else {
            value
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_average`: среднее по списку чисел.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке,
/// [`RuntimeError::NotANumber`] на нечисловом элементе и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn average<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let numbers = rt.numbers_arg(stream, args, "value")?;
        if numbers.is_empty() {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "value",
            });
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "an action argument holds at most a few dozen numbers"
        )]
        let count = numbers.len() as f64;
        let value = match rt.enum_arg(stream, args, "type")? {
            "ARITHMETIC" => numbers.iter().sum::<f64>() / count,
            "GEOMETRIC" => {
                let product: f64 = numbers.iter().product();
                product.powf(1.0 / count)
            }
            "HARMONIC" => count / numbers.iter().map(|number| 1.0 / number).sum::<f64>(),
            "QUADRATIC" => {
                (numbers.iter().map(|number| number * number).sum::<f64>() / count).sqrt()
            }
            other => return Err(invalid_enum(args, "type", other.to_owned())),
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_median`: медиана списка.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn median<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut numbers = rt.numbers_arg(stream, args, "value")?;
        if numbers.is_empty() {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "value",
            });
        }
        numbers.sort_by(f64::total_cmp);
        let middle = numbers.len() / 2;
        let value = if numbers.len() % 2 == 0 {
            f64::midpoint(numbers[middle - 1], numbers[middle])
        } else {
            numbers[middle]
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_mathematical_expectation`: сумма `значение × вероятность`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке, если числа и
/// вероятности не совпадают по длине, и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn expectation<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let values = rt.numbers_arg(stream, args, "values")?;
        let probabilities = rt.numbers_arg(stream, args, "probabilities")?;
        if values.is_empty() || values.len() != probabilities.len() {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "probabilities",
            });
        }
        let value = values
            .iter()
            .zip(probabilities.iter())
            .map(|(value, probability)| value * probability)
            .sum();
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_gamma_function`: гамма-функция, приближение Ланцоша.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn gamma<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        Ok(Some(value::number(gamma_function(number))))
    })
}

/// Гамма-функция через приближение Ланцоша (`g = 7`, 9 коэффициентов).
#[tracing::instrument(level = "trace", fields(value = ?value))]
fn gamma_function(value: f64) -> f64 {
    const COEFFICIENTS: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if value < 0.5 {
        // Reflection: Γ(x)Γ(1−x) = π / sin(πx).
        return std::f64::consts::PI
            / ((std::f64::consts::PI * value).sin() * gamma_function(1.0 - value));
    }
    let shifted = value - 1.0;
    let mut sum = COEFFICIENTS[0];
    for (index, coefficient) in COEFFICIENTS.iter().enumerate().skip(1) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "the index runs over nine coefficients"
        )]
        let divisor = shifted + index as f64;
        sum += coefficient / divisor;
    }
    let t = shifted + 7.5;
    (2.0 * std::f64::consts::PI).sqrt() * t.powf(shifted + 0.5) * (-t).exp() * sum
}

/// `set_variable_gaussian_distribution`: нормальное распределение.
///
/// `NORMAL` даёт величину со средним `mean` и отклонением `deviant`,
/// `FOLDER_NORMAL` — её модуль (свёрнутое нормальное).
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном
/// распределении и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn gaussian<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let deviant = rt.number_arg(stream, args, "deviant")?;
        let mean = rt.number_arg(stream, args, "mean")?;
        let distribution = rt.enum_arg(stream, args, "distribution")?;
        let standard = standard_normal(rt);
        let value = deviant.mul_add(standard, mean);
        let value = match distribution {
            "NORMAL" => value,
            "FOLDER_NORMAL" => value.abs(),
            other => return Err(invalid_enum(args, "distribution", other.to_owned())),
        };
        Ok(Some(value::number(value)))
    })
}

/// Одно нормальное отклонение методом Бокса — Мюллера.
#[tracing::instrument(level = "trace", skip(rt))]
fn standard_normal(rt: &Runtime<'_>) -> f64 {
    let first = rt.world().next_unit().max(f64::MIN_POSITIVE);
    let second = rt.world().next_unit();
    (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
}

/// `set_variable_lerp_number`: линейная интерполяция.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn lerp<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let start = rt.number_arg(stream, args, "start")?;
        let stop = rt.number_arg(stream, args, "stop")?;
        let amount = rt.number_arg(stream, args, "amount")?;
        Ok(Some(value::number(amount.mul_add(stop - start, start))))
    })
}

/// `set_variable_map_range`: перекладывание числа из одного диапазона в другой.
///
/// Результат ограничен целевым диапазоном — как у `JustMC`: за его пределами
/// значение перестаёт что-либо значить.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на пустом исходном диапазоне и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn map_range<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let from_start = rt.number_arg(stream, args, "from_start")?;
        let from_stop = rt.number_arg(stream, args, "from_stop")?;
        let to_start = rt.number_arg(stream, args, "to_start")?;
        let to_stop = rt.number_arg(stream, args, "to_stop")?;
        let span = from_stop - from_start;
        if span == 0.0 {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "from_stop"),
            });
        }
        let ratio = (number - from_start) / span;
        let value = ratio.mul_add(to_stop - to_start, to_start);
        let (low, high) = (to_start.min(to_stop), to_start.max(to_stop));
        Ok(Some(value::number(value.clamp(low, high))))
    })
}

/// `set_variable_warp`: заворачивание числа в диапазон.
///
/// # Errors
///
/// Возвращает [`RuntimeError::DivisionByZero`] на пустом диапазоне и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn warp<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let min = rt.number_arg(stream, args, "min")?;
        let max = rt.number_arg(stream, args, "max")?;
        let span = max - min;
        if span == 0.0 {
            return Err(RuntimeError::DivisionByZero {
                context: at_op(args, "max"),
            });
        }
        Ok(Some(value::number((number - min).rem_euclid(span) + min)))
    })
}

/// `set_variable_find_nearest_number`: ближайшее число из списка.
///
/// # Errors
///
/// Возвращает [`RuntimeError::MissingArgument`] на пустом списке и ошибку
/// вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn nearest_number<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = value::number_of(&rt.arg(stream, args, "number")?, &at_op(args, "number"))?;
        let numbers = rt.numbers_arg(stream, args, "numbers")?;
        let mut best: Option<f64> = None;
        for candidate in numbers {
            let better = best.is_none_or(|best| (candidate - number).abs() < (best - number).abs());
            if better {
                best = Some(candidate);
            }
        }
        let Some(value) = best else {
            return Err(RuntimeError::MissingArgument {
                action: schema::name(args.action()),
                arg: "numbers",
            });
        };
        Ok(Some(value::number(value)))
    })
}

/// `set_variable_convert_number_to_text`: запись целого числа в системе
/// счисления с основанием `radix`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на основании вне `2..=36` и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn number_to_text<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        let radix = radix(rt, stream, args)?;
        Ok(Some(value::text(radix_text(number, radix))))
    })
}

/// Основание системы счисления из аргумента `radix`.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на основании вне `2..=36`
/// или ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn radix<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, args: Args<'a>) -> Result<u32> {
    let radix = rt
        .optional_number_arg(stream, args, "radix")?
        .unwrap_or(10.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the range check below rejects everything that does not fit"
    )]
    let radix = radix.trunc() as i64;
    u32::try_from(radix)
        .ok()
        .filter(|radix| (2..=36).contains(radix))
        .ok_or_else(|| invalid_enum(args, "radix", radix.to_string()))
}

/// Целая часть числа в заданной системе счисления.
#[tracing::instrument(level = "trace", fields(number = ?number, radix = ?radix))]
fn radix_text(number: f64, radix: u32) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the number is brought to an integer on purpose: JustMC writes whole numbers in \
                  another radix"
    )]
    let integer = number.trunc() as i64;
    let negative = integer < 0;
    let mut value = integer.unsigned_abs();
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the remainder is below the radix, which is at most 36"
        )]
        let digit = (value % u64::from(radix)) as u8;
        digits.push(char::from_digit(u32::from(digit), radix).unwrap_or('?'));
        value /= u64::from(radix);
    }
    if negative {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// `set_variable_char_to_number`: код символа.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotText`] на пустом тексте и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn char_to_number<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "char")?;
        let Some(character) = text.chars().next() else {
            return Err(RuntimeError::NotText {
                context: at_op(args, "char"),
                actual: "an empty text".to_owned(),
            });
        };
        Ok(Some(value::number(f64::from(u32::from(character)))))
    })
}

/// `set_variable_to_char`: символ по его коду.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotANumber`], если кода нет ни у какого символа, и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn to_char<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let number = rt.number_arg(stream, args, "number")?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a code point is a non-negative integer; the checks below reject the rest"
        )]
        let code = number.trunc() as i64;
        let character = u32::try_from(code)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| RuntimeError::NotANumber {
                context: at_op(args, "number"),
                actual: format!("{number} is not a character code"),
            })?;
        Ok(Some(value::text(character.to_string())))
    })
}

/// `set_variable_get_text_similarity`: расстояние Левенштейна и его норма.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном типе значения
/// и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn text_similarity<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let first = rt.text_arg(stream, args, "text_1")?.into_owned();
        let second = rt.text_arg(stream, args, "text_2")?.into_owned();
        let distance = levenshtein(&first, &second);
        let value = match rt.enum_arg(stream, args, "value_type")? {
            "LEVENSHTEIN_DISTANCE" => value::count_as_number(distance),
            "SIMILARITY" => {
                let longest = first.chars().count().max(second.chars().count());
                if longest == 0 {
                    1.0
                } else {
                    1.0 - value::count_as_number(distance) / value::count_as_number(longest)
                }
            }
            other => return Err(invalid_enum(args, "value_type", other.to_owned())),
        };
        Ok(Some(value::number(value)))
    })
}

/// Расстояние Левенштейна между двумя текстами, в символах.
#[tracing::instrument(level = "trace", fields(first = %first, second = %second))]
fn levenshtein(first: &str, second: &str) -> usize {
    let left: Vec<char> = first.chars().collect();
    let right: Vec<char> = second.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, left_char) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_char != right_char);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// `set_variable_get_index_of_subtext`: позиция подтекста.
///
/// Позиция считается в символах и равна `-1`, если подтекст не найден, — так же,
/// как в `JustMC`.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn index_of_subtext<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text: Vec<char> = rt.text_arg(stream, args, "text")?.chars().collect();
        let subtext: Vec<char> = rt.text_arg(stream, args, "subtext")?.chars().collect();
        let start = rt
            .optional_number_arg(stream, args, "start_index")?
            .unwrap_or(0.0);
        let mode = rt.enum_arg(stream, args, "search_mode")?;
        let start = char_index(start, text.len());
        let found = match mode {
            "FIRST" => find_from(&text, &subtext, start),
            "LAST" => rfind_before(&text, &subtext, start),
            other => return Err(invalid_enum(args, "search_mode", other.to_owned())),
        };
        let value = found.map_or(-1.0, value::count_as_number);
        Ok(Some(value::number(value)))
    })
}

/// Индекс символа, ограниченный длиной текста.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the index is clamped into the text's length after the conversion"
)]
#[tracing::instrument(level = "trace", fields(index = ?index, len = ?len))]
fn char_index(index: f64, len: usize) -> usize {
    (index.trunc().max(0.0) as usize).min(len)
}

/// Первое вхождение подтекста начиная с символа `start`.
#[tracing::instrument(level = "trace", fields(text = ?text, subtext = ?subtext, start = ?start))]
fn find_from(text: &[char], subtext: &[char], start: usize) -> Option<usize> {
    if subtext.is_empty() {
        return Some(start.min(text.len()));
    }
    (start..=text.len().saturating_sub(subtext.len()))
        .find(|at| text[*at..at + subtext.len()] == *subtext)
}

/// Последнее вхождение подтекста не позже символа `start`.
#[tracing::instrument(level = "trace", fields(text = ?text, subtext = ?subtext, start = ?start))]
fn rfind_before(text: &[char], subtext: &[char], start: usize) -> Option<usize> {
    if subtext.is_empty() {
        return Some(start.min(text.len()));
    }
    let last = (start + 1).min(text.len());
    (0..=last.saturating_sub(subtext.len()))
        .rev()
        .find(|at| text[*at..at + subtext.len()] == *subtext)
}

/// `set_variable_parse_json`: разбор JSON в значение.
///
/// Значение укладывается в переменную как есть: текст с разбором `json`,
/// список, словарь или число.
///
/// # Errors
///
/// Возвращает [`RuntimeError::Json`] на неразобранном JSON и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn parse_json<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let json = rt.text_arg(stream, args, "json")?.into_owned();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).map_err(|source| RuntimeError::Json {
                context: at_op(args, "json"),
                source,
            })?;
        Ok(Some(json_to_value(parsed)))
    })
}

/// Разобранный JSON как значение мока.
///
/// Простые числа и строки становятся числами и текстом, списки и объекты —
/// списком и словарём; `null` — пустое значение.
#[tracing::instrument(level = "trace", skip(value))]
fn json_to_value<'a>(value: serde_json::Value) -> Value<'a> {
    match value {
        serde_json::Value::Null => Value::Error,
        serde_json::Value::Bool(value) => value::boolean(value),
        serde_json::Value::Number(number) => value::number(number.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(text) => value::text(text),
        serde_json::Value::Array(items) => value::list(items.into_iter().map(json_to_value)),
        serde_json::Value::Object(entries) => value::dict(
            entries
                .into_iter()
                .map(|(key, value)| (key, json_to_value(value))),
        ),
    }
}

/// `set_variable_to_json`: значение как JSON-текст.
///
/// # Errors
///
/// Возвращает [`RuntimeError::Serialize`], если значение не удаётся записать в
/// JSON, и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn to_json<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let value = rt.arg(stream, args, "value")?;
        let pretty = matches!(
            rt.optional_enum_arg(stream, args, "pretty_print")?,
            Some("TRUE")
        );
        let json = if pretty {
            serde_json::to_string_pretty(&value)
        } else {
            serde_json::to_string(&value)
        };
        let json = json.map_err(|source| RuntimeError::Serialize {
            context: at_op(args, "value"),
            source,
        })?;
        Ok(Some(value::text(json)))
    })
}

/// `set_variable_format_timestamp`: время как текст.
///
/// Время — секунды эпохи, зона — UTC: базы часовых поясов у мока нет, и
/// подставлять вместо неё что-то своё значило бы показывать чужое время.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном формате и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn format_timestamp<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let seconds = rt.number_arg(stream, args, "time")?;
        let pattern = rt.optional_arg(stream, args, "pattern")?;
        let format = rt.optional_enum_arg(stream, args, "format")?;
        let pattern = pattern
            .as_ref()
            .map(|value| value::text_of(value, &at_op(args, "pattern")))
            .transpose()?;
        let moment = Civil::from_epoch(seconds);
        let text = match (format, pattern) {
            // Only the custom format reads the pattern; the named ones are the
            // pattern.
            (Some("CUSTOM") | None, Some(pattern)) => moment.render(&pattern),
            (Some(named), _) => moment.render_named(named),
            (None, None) => moment.render_named("YYYY_MM_DD_HH_MM_S"),
        };
        Ok(Some(value::text(text)))
    })
}

/// Момент времени по Гринвичу, разложенный на части.
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Civil {
    /// Раскладывает секунды эпохи в календарную дату.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the calendar arithmetic below works on whole days and seconds"
    )]
    const fn from_epoch(seconds: f64) -> Self {
        let total = seconds.floor() as i64;
        let days = total.div_euclid(86_400);
        let time = total.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (time / 3600) as u32,
            minute: (time % 3600 / 60) as u32,
            second: (time % 60) as u32,
        }
    }

    /// Подставляет части в шаблон.
    ///
    /// Понимаются те же обозначения, что у `SimpleDateFormat`: `yyyy`, `MM`,
    /// `dd`, `HH`, `mm`, `ss`, `EEE`, `EEEE` и одиночные `H`, `m`, `s`, `d`.
    #[tracing::instrument(level = "trace", skip(self), fields(pattern = %pattern))]
    fn render(&self, pattern: &str) -> String {
        let mut out = String::with_capacity(pattern.len());
        let mut rest = pattern;
        while let Some(character) = rest.chars().next() {
            if !"yMdHmsEa".contains(character) {
                out.push(character);
                rest = &rest[character.len_utf8()..];
                continue;
            }
            let length = rest.chars().take_while(|other| *other == character).count();
            out.push_str(&self.token(character, length));
            rest = &rest[length * character.len_utf8()..];
        }
        out
    }

    /// Рендерит именованный формат из схемы.
    #[tracing::instrument(level = "trace", skip(self), fields(format = %format))]
    fn render_named(&self, format: &str) -> String {
        let pattern = match format {
            "DD_MM_YYYY_HH_MM_S" => "dd.MM.yyyy HH:mm:ss",
            "YYYY_MM_DD" => "yyyy-MM-dd",
            "YYYY_MM_DD_HH_MM_S" => "yyyy-MM-dd HH:mm:ss",
            "HH_MM_SS" => "HH:mm:ss",
            "H_H_M_M_S_S" => "H:H:m:m:s:s",
            "H_MM_A" => "H:mm a",
            "S_S" => "s.s",
            "EEEE" => "EEEE",
            "EEE_D_MMMM" => "EEE d MMMM",
            "EEE_MMMM_D" => "EEE MMMM d",
            _ => "dd.MM.yyyy",
        };
        self.render(pattern)
    }

    /// Одно обозначение шаблона.
    #[tracing::instrument(level = "trace", skip(self), fields(token = ?token, length = ?length))]
    fn token(&self, token: char, length: usize) -> String {
        match token {
            'y' => format!("{:04}", self.year),
            'M' => pad_or_name(self.month, length, &MONTHS),
            'd' => format!("{:0length$}", self.day),
            'H' => format!("{:0length$}", self.hour),
            'm' => format!("{:0length$}", self.minute),
            's' => format!("{:0length$}", self.second),
            'E' => weekday_name(self.days_since_epoch(), length),
            _ => if self.hour < 12 { "AM" } else { "PM" }.to_owned(),
        }
    }

    /// Сколько дней прошло с эпохи — нужно для дня недели.
    #[tracing::instrument(level = "trace", skip(self))]
    fn days_since_epoch(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
}

/// Названия месяцев.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Названия дней недели, начиная с воскресенья (как в Java).
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Название дня недели; `length` выбирает краткую или полную форму.
#[tracing::instrument(level = "trace", fields(days = ?days, length = ?length))]
fn weekday_name(days: i64, length: usize) -> String {
    let index = (days + 4).rem_euclid(7) as usize;
    let name = WEEKDAYS[index];
    if length >= 4 {
        name.to_owned()
    } else {
        name[..3].to_owned()
    }
}

/// Значение месяца или его название по длине обозначения.
#[tracing::instrument(level = "trace", fields(value = ?value, length = ?length, names = ?names))]
fn pad_or_name(value: u32, length: usize, names: &[&str; 12]) -> String {
    if length >= 3 {
        let index = value.saturating_sub(1) as usize;
        let name = names.get(index).copied().unwrap_or("");
        if length >= 4 {
            name.to_owned()
        } else {
            name[..3.min(name.len())].to_owned()
        }
    } else {
        format!("{value:0length$}")
    }
}

/// Дни с эпохи по календарной дате (алгоритм Говарда Хиннанта).
#[tracing::instrument(level = "trace", fields(year = ?year, month = ?month, day = ?day))]
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = i64::from((month + 9) % 12);
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Календарная дата по дням с эпохи.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the algorithm works in whole days; the casts are bounded by it"
)]
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = (if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Какой шум считать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Noise {
    /// Гладкий градиентный шум.
    Perlin,
    /// Тот же градиентный шум с другим смешиванием.
    Simplex,
    /// Расстояние до ближайшей случайной точки решётки.
    Voronoi,
}

/// `set_variable_perlin_noise_3d`, `..._simplex_noise_3d` и
/// `set_variable_voronoi_noise_3d`.
///
/// Октавы суммируются как `Σ амплитуда × шум(частота × точка)`, где частота и
/// амплитуда умножаются на `frequency`/`amplitude` на каждой следующей октаве.
/// `FULL_RANGE` оставляет результат в `[-1, 1]`, `ZERO_TO_ONE` сдвигает его в
/// `[0, 1]`, `normalized` делит на сумму амплитуд.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном диапазоне и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op, kind))]
pub(super) fn noise<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    kind: Noise,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
        let seed = rt.optional_number_arg(stream, args, "seed")?.unwrap_or(0.0);
        let loc_frequency = rt
            .optional_number_arg(stream, args, "loc_frequency")?
            .unwrap_or(1.0);
        let octaves = rt
            .optional_number_arg(stream, args, "octaves")?
            .unwrap_or(1.0);
        let frequency = rt
            .optional_number_arg(stream, args, "frequency")?
            .unwrap_or(2.0);
        let amplitude = rt
            .optional_number_arg(stream, args, "amplitude")?
            .unwrap_or(0.5);
        let mode = rt.enum_arg(stream, args, "range_mode")?;
        let normalized = matches!(
            rt.optional_enum_arg(stream, args, "normalized")?,
            Some("TRUE")
        );
        let value = noise_value(
            kind,
            [x * loc_frequency, y * loc_frequency, z * loc_frequency],
            seed,
            octaves,
            frequency,
            amplitude,
            normalized,
        );
        let value = match mode {
            "FULL_RANGE" => value,
            "ZERO_TO_ONE" => f64::midpoint(value, 1.0),
            other => return Err(invalid_enum(args, "range_mode", other.to_owned())),
        };
        Ok(Some(value::number(value)))
    })
}

/// Сумма октав шума в `[-1, 1]`.
#[tracing::instrument(level = "trace", skip(kind), fields(point = ?point, seed = ?seed, octaves = ?octaves, frequency = ?frequency, amplitude = ?amplitude, normalized = ?normalized))]
fn noise_value(
    kind: Noise,
    point: [f64; 3],
    seed: f64,
    octaves: f64,
    frequency: f64,
    amplitude: f64,
    normalized: bool,
) -> f64 {
    let octaves = octave_count(octaves);
    let seed = seed.trunc() as i64;
    let mut total = 0.0;
    let mut amplitude_sum = 0.0;
    let mut current_frequency = 1.0;
    let mut current_amplitude = 1.0;
    for octave in 0..octaves {
        let scaled = [
            point[0] * current_frequency,
            point[1] * current_frequency,
            point[2] * current_frequency,
        ];
        total += current_amplitude
            * match kind {
                Noise::Perlin => gradient_noise(scaled, seed.wrapping_add(i64::from(octave))),
                Noise::Simplex => gradient_noise(
                    scaled,
                    seed.wrapping_add(i64::from(octave)).wrapping_mul(31),
                ),
                Noise::Voronoi => voronoi_noise(scaled, seed.wrapping_add(i64::from(octave))),
            };
        amplitude_sum += current_amplitude;
        current_frequency *= frequency;
        current_amplitude *= amplitude;
    }
    if normalized && amplitude_sum > 0.0 {
        total / amplitude_sum
    } else {
        total
    }
}

/// Число октав: целое в разумных пределах.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 1..=32 before the cast"
)]
const fn octave_count(octaves: f64) -> u32 {
    octaves.trunc().clamp(1.0, 32.0) as u32
}

/// Градиентный шум на решётке единичного куба.
#[tracing::instrument(level = "trace", fields(point = ?point, seed = ?seed))]
fn gradient_noise(point: [f64; 3], seed: i64) -> f64 {
    let cell = point.map(f64::floor);
    let local = [point[0] - cell[0], point[1] - cell[1], point[2] - cell[2]];
    let fade =
        local.map(|value| value * value * value * value.mul_add(value.mul_add(6.0, -15.0), 10.0));
    let mut result = 0.0;
    for corner in 0..8 {
        let offsets = [
            f64::from(corner & 1),
            f64::from((corner >> 1) & 1),
            f64::from((corner >> 2) & 1),
        ];
        let weight = (0..3)
            .map(|axis| {
                if offsets[axis] > 0.5 {
                    fade[axis]
                } else {
                    1.0 - fade[axis]
                }
            })
            .product::<f64>();
        let gradient =
            gradient(seed ^ (corner as i64).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64));
        let dot = gradient[0].mul_add(
            local[0] - offsets[0],
            gradient[1].mul_add(local[1] - offsets[1], gradient[2] * (local[2] - offsets[2])),
        );
        result += weight * dot;
    }
    result
}

/// Расстояние до ближайшей точки решётки со случайным смещением.
#[tracing::instrument(level = "trace", fields(point = ?point, seed = ?seed))]
fn voronoi_noise(point: [f64; 3], seed: i64) -> f64 {
    let cell = point.map(f64::floor);
    let mut best = f64::INFINITY;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let offset = [f64::from(x), f64::from(y), f64::from(z)];
                let neighbour = [
                    cell[0] + offset[0],
                    cell[1] + offset[1],
                    cell[2] + offset[2],
                ];
                let feature = hash3(neighbour, seed);
                let distance = (0..3)
                    .map(|axis| {
                        let delta = point[axis] - (neighbour[axis] + feature[axis]);
                        delta * delta
                    })
                    .sum::<f64>()
                    .sqrt();
                best = best.min(distance);
            }
        }
    }
    // Distance to the nearest feature point is at most ~1.7, so the range is
    // brought back to [-1, 1] by that bound.
    (best / 1.7).mul_add(2.0, -1.0).clamp(-1.0, 1.0)
}

/// Случайная точка внутри ячейки: по числу в `[0, 1)` на каждую ось.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the cell coordinates are brought to whole numbers for hashing"
)]
#[tracing::instrument(level = "trace", fields(cell = ?cell, seed = ?seed))]
fn hash3(cell: [f64; 3], seed: i64) -> [f64; 3] {
    let cell = cell.map(|value| value as i64);
    let mut result = [0.0; 3];
    for (axis, value) in result.iter_mut().enumerate() {
        let mixed = mix(cell[0]
            .wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)
            .wrapping_add(cell[1].wrapping_mul(0xC2B2_AE3D_27D4_EB4F_u64 as i64))
            .wrapping_add(cell[2].wrapping_mul(0x1656_67B1_9E37_79F9_u64 as i64))
            .wrapping_add(seed)
            .wrapping_add(axis as i64));
        *value = unit_from_hash(mixed);
    }
    result
}

/// Число в `[0, 1)` из хеша: старшие 53 бита — это ровно мантисса `f64`.
#[expect(
    clippy::cast_precision_loss,
    reason = "the shifted hash has exactly 53 significant bits, which an f64 holds"
)]
const fn unit_from_hash(hash: i64) -> f64 {
    (hash.unsigned_abs() >> 11) as f64 / 9_007_199_254_740_992.0
}

/// Один из восьми градиентов единичного куба.
const fn gradient(key: i64) -> [f64; 3] {
    const GRADIENTS: [[f64; 3]; 8] = [
        [1.0, 1.0, 0.0],
        [-1.0, 1.0, 0.0],
        [1.0, -1.0, 0.0],
        [-1.0, -1.0, 0.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
        [0.0, 1.0, -1.0],
        [0.0, -1.0, -1.0],
    ];
    let index = key.rem_euclid(8) as usize;
    GRADIENTS[index]
}

/// Финальное перемешивание битов (как в `splitmix64`).
const fn mix(value: i64) -> i64 {
    let first = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9_u64 as i64);
    let second = (first ^ (first >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB_u64 as i64);
    second ^ (second >> 31)
}
