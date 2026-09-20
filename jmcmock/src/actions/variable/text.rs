//! Текст: символ по индексу, длина, разбор числа из текста, регистр, срезы,
//! замена, деление и склейка, кодировки.

use std::borrow::Cow;

use super::*;

/// Текстовые действия. `None` — действие не из этой группы.
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
        ActionId::SetVariableText => join_values(rt, stream, op),
        ActionId::SetVariableTextCase => case(rt, stream, op),
        ActionId::SetVariableTrimText => trim(rt, stream, op),
        ActionId::SetVariableReplaceText => replace(rt, stream, op),
        ActionId::SetVariableRepeatText => repeat(rt, stream, op),
        ActionId::SetVariableSplitText => split(rt, stream, op),
        ActionId::SetVariableSplitTextByLength => split_by_length(rt, stream, op),
        ActionId::SetVariableJoinText => join(rt, stream, op),
        ActionId::SetVariableStripText => strip(rt, stream, op),
        ActionId::SetVariableClearColorCodes => clear_color_codes(rt, stream, op),
        ActionId::SetVariableTextToChars => to_chars(rt, stream, op),
        ActionId::SetVariableTextToBytes => to_bytes(rt, stream, op),
        ActionId::SetVariableBytesToText => from_bytes(rt, stream, op),
        ActionId::SetVariableRemoveText => remove(rt, stream, op),
        ActionId::SetVariableParseToComponent => parse_to_component(rt, stream, op),
        _ => return None,
    })
}

/// `set_variable_remove_text`: убирает подтексты из текста.
///
/// # Errors
///
/// Возвращает [`RuntimeError::Unimplemented`] на регулярных выражениях, которых
/// у мока нет, и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn remove<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let mut text = rt.text_arg(stream, args, "text")?.into_owned();
        if rt.optional_enum_arg(stream, args, "regex")? == Some("TRUE") {
            rt.unimplemented("regular expressions in 'set_variable_remove_text'")?;
            return Ok(Some(value::text(text)));
        }
        for needle in rt.values_arg(stream, args, "remove")? {
            let needle = value::display(&needle);
            if !needle.is_empty() {
                text = text.replace(&needle, "");
            }
        }
        Ok(Some(value::text(text)))
    })
}

/// `set_variable_text`: склеивает значения в один текст.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме
/// склейки и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn join_values<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items: Vec<String> = rt
            .values_arg(stream, args, "text")?
            .iter()
            .map(|value| value::text_of(value, &at_op(args, "text")).map(Cow::into_owned))
            .collect::<Result<_>>()?;
        let separator = match rt.optional_enum_arg(stream, args, "merging")? {
            Some("SPACES") => " ",
            Some("SEPARATE_LINES") => "\n",
            _ => "",
        };
        Ok(Some(value::text(items.join(separator))))
    })
}

/// `set_variable_text_case`: регистр текста.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn case<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let result = match rt.enum_arg(stream, args, "case_type")? {
            "UPPER" => text.to_uppercase(),
            "LOWER" => text.to_lowercase(),
            "INVERT" => text
                .chars()
                .flat_map(|character| {
                    if character.is_uppercase() {
                        character.to_lowercase().collect::<Vec<_>>()
                    } else {
                        character.to_uppercase().collect::<Vec<_>>()
                    }
                })
                .collect(),
            "PROPER" => proper_case(&text),
            "RANDOM" => random_case(rt, &text),
            other => return Err(invalid_enum(args, "case_type", other.to_owned())),
        };
        Ok(Some(value::text(result)))
    })
}

/// Первая буква каждого слова — заглавная.
#[tracing::instrument(level = "trace", fields(text = %text))]
fn proper_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_start = true;
    for character in text.chars() {
        if character.is_alphanumeric() {
            if at_start {
                out.extend(character.to_uppercase());
            } else {
                out.extend(character.to_lowercase());
            }
            at_start = false;
        } else {
            out.push(character);
            at_start = true;
        }
    }
    out
}

/// Случайный регистр каждой буквы.
#[tracing::instrument(level = "trace", skip(rt), fields(text = %text))]
fn random_case(rt: &Runtime<'_>, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if rt.world().next_unit() < 0.5 {
            out.extend(character.to_lowercase());
        } else {
            out.extend(character.to_uppercase());
        }
    }
    out
}

/// `set_variable_trim_text`: оставляет символы с `start` по `end` включительно.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn trim<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let start = rt
            .optional_number_arg(stream, args, "start")?
            .unwrap_or(0.0);
        let end = rt.optional_number_arg(stream, args, "end")?;
        let characters: Vec<char> = text.chars().collect();
        let start = index_in(start, characters.len()).unwrap_or(0);
        let end = end.map_or(characters.len(), |end| {
            index_in(end, characters.len()).map_or(0, |end| end + 1)
        });
        Ok(Some(value::text(
            characters[start..end.max(start)].iter().collect::<String>(),
        )))
    })
}

/// `set_variable_replace_text`: замена подтекста.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn replace<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let needle = rt.text_arg(stream, args, "replace")?.into_owned();
        let replacement = rt.text_arg(stream, args, "replacement")?.into_owned();
        let first_only = match rt.enum_arg(stream, args, "first")? {
            "FIRST" => true,
            "ANY" => false,
            other => return Err(invalid_enum(args, "first", other.to_owned())),
        };
        let ignore_case = matches!(
            rt.optional_enum_arg(stream, args, "ignore_case")?,
            Some("TRUE")
        );
        Ok(Some(value::text(replace_text(
            &text,
            &needle,
            &replacement,
            first_only,
            ignore_case,
        ))))
    })
}

/// Замена подтекста с учётом регистра и кратности.
#[tracing::instrument(level = "trace", fields(text = %text, needle = %needle, replacement = %replacement, first_only = ?first_only, ignore_case = ?ignore_case))]
fn replace_text(
    text: &str,
    needle: &str,
    replacement: &str,
    first_only: bool,
    ignore_case: bool,
) -> String {
    if needle.is_empty() {
        return text.to_owned();
    }
    let (haystack, needle_cmp) = if ignore_case {
        (text.to_lowercase(), needle.to_lowercase())
    } else {
        (text.to_owned(), needle.to_owned())
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    while let Some(found) = haystack[rest..].find(&needle_cmp) {
        let at = rest + found;
        out.push_str(&text[rest..at]);
        out.push_str(replacement);
        rest = at + needle.len();
        if first_only {
            break;
        }
    }
    out.push_str(&text[rest..]);
    out
}

/// `set_variable_repeat_text`: повторяет текст.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn repeat<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let count = rt.number_arg(stream, args, "repeat")?;
        Ok(Some(value::text(text.repeat(repeat_count(count)))))
    })
}

/// Число повторов: целое, неотрицательное и в разумных пределах.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into a small non-negative range before the cast"
)]
const fn repeat_count(count: f64) -> usize {
    count.trunc().clamp(0.0, 65_536.0) as usize
}

/// `set_variable_split_text`: делит текст по разделителю.
///
/// Пустой разделитель делит текст на символы.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn split<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let delimiter = rt.text_arg(stream, args, "delimiter")?.into_owned();
        let parts: Vec<Rt<'a>> = if delimiter.is_empty() {
            text.chars()
                .map(|character| Some(value::text(character.to_string())))
                .collect()
        } else {
            text.split(&delimiter)
                .map(|part| Some(value::text(part.to_owned())))
                .collect()
        };
        Ok(Some(Value::Array { values: parts }))
    })
}

/// `set_variable_split_text_by_length`: режет текст на куски по `max_length`
/// символов.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn split_by_length<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let length = rt.number_arg(stream, args, "max_length")?;
        let characters: Vec<char> = text.chars().collect();
        let length = repeat_count(length).max(1);
        let parts: Vec<Rt<'a>> = characters
            .chunks(length)
            .map(|chunk| Some(value::text(chunk.iter().collect::<String>())))
            .collect();
        Ok(Some(Value::Array { values: parts }))
    })
}

/// `set_variable_join_text`: склеивает список в текст.
///
/// `limit` ограничивает число элементов, а `truncated` дописывается, если
/// элементы остались за пределом.
///
/// # Errors
///
/// Возвращает [`RuntimeError::NotAList`] на не-списке и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn join<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let items = rt.list_arg(stream, args, "list")?;
        let separator = rt.text_arg(stream, args, "separator")?.into_owned();
        let prefix = rt.text_arg(stream, args, "prefix")?.into_owned();
        let postfix = rt.text_arg(stream, args, "postfix")?.into_owned();
        let limit = rt.optional_number_arg(stream, args, "limit")?;
        let truncated = rt.optional_arg(stream, args, "truncated")?;
        let total = items.len();
        let shown = limit.map_or(total, |limit| {
            index_in(limit, total.saturating_add(1)).map_or(total, |limit| limit.min(total))
        });
        let mut parts = Vec::with_capacity(shown);
        for item in items.into_iter().take(shown) {
            parts.push(value::display(&item));
        }
        let mut text = format!("{prefix}{}{postfix}", parts.join(&separator));
        if shown < total
            && let Some(truncated) = truncated
        {
            text.push_str(&value::display(&truncated));
        }
        Ok(Some(value::text(text)))
    })
}

/// `set_variable_strip_text`: убирает пробелы по краям или отступ.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном режиме и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn strip<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let result = match rt.enum_arg(stream, args, "strip_type")? {
            "ALL" => text.trim().to_owned(),
            "START" => text.trim_start().to_owned(),
            "END" => text.trim_end().to_owned(),
            "INDENT" => strip_indent(&text),
            other => return Err(invalid_enum(args, "strip_type", other.to_owned())),
        };
        Ok(Some(value::text(result)))
    })
}

/// Убирает общий отступ у всех непустых строк.
#[tracing::instrument(level = "trace", fields(text = %text))]
fn strip_indent(text: &str) -> String {
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| line.get(indent..).unwrap_or(line).to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `set_variable_clear_color_codes`: убирает устаревшие цветовые коды.
///
/// # Errors
///
/// Возвращает ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn clear_color_codes<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        Ok(Some(value::text(crate::text::strip_legacy_codes(&text))))
    })
}

/// `set_variable_text_to_chars`: текст как список символов или их кодов.
///
/// # Errors
///
/// Возвращает [`RuntimeError::Unimplemented`] на вариантах с кодовыми точками
/// и ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn to_chars<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let mode = rt.enum_arg(stream, args, "chars_type")?;
        let items: Vec<Rt<'a>> = match mode {
            "CHARS" => text
                .chars()
                .map(|character| Some(value::text(character.to_string())))
                .collect(),
            "CODES" => text
                .chars()
                .map(|character| Some(value::number(f64::from(u32::from(character)))))
                .collect(),
            _ => {
                rt.unimplemented(format!("the '{mode}' mode of 'set_variable_text_to_chars'"))?;
                Vec::new()
            }
        };
        Ok(Some(Value::Array { values: items }))
    })
}

/// Кодировка текста.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    /// UTF-8: от одного до четырёх байтов на символ.
    Utf8,
    /// UTF-16: по два байта на единицу кода.
    Utf16,
    /// ASCII: только младшие семь битов.
    Ascii,
}

/// Разбирает кодировку из имени аргумента.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной кодировке.
#[tracing::instrument(level = "trace", skip(args), fields(name = %name))]
fn charset(name: &str, args: Args<'_>) -> Result<Charset> {
    match name {
        "UTF_8" => Ok(Charset::Utf8),
        "UTF_16" => Ok(Charset::Utf16),
        "ASCII" => Ok(Charset::Ascii),
        other => Err(invalid_enum(args, "charset", other.to_owned())),
    }
}

/// `set_variable_text_to_bytes`: байты текста в выбранной кодировке.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной кодировке и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn to_bytes<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let charset = charset(rt.enum_arg(stream, args, "charset")?, args)?;
        let bytes: Vec<Rt<'a>> = match charset {
            Charset::Utf8 => text
                .bytes()
                .map(|byte| Some(value::number(f64::from(byte))))
                .collect(),
            Charset::Utf16 => text
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .map(|byte| Some(value::number(f64::from(byte))))
                .collect(),
            Charset::Ascii => text
                .chars()
                .map(|character| Some(value::number(f64::from(character as u32 & 0x7f))))
                .collect(),
        };
        Ok(Some(Value::Array { values: bytes }))
    })
}

/// `set_variable_bytes_to_text`: текст из байтов в выбранной кодировке.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестной кодировке,
/// [`RuntimeError::NotANumber`] на нечисловом байте и ошибку вычисления
/// аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn from_bytes<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let charset = charset(rt.enum_arg(stream, args, "charset")?, args)?;
        let mut numbers = Vec::new();
        for value in rt.values_arg(stream, args, "bytes")? {
            numbers.push(value::number_of(&value, &at_op(args, "bytes"))?);
        }
        let text = match charset {
            Charset::Utf8 => String::from_utf8_lossy(&to_bytes_vec(&numbers)).into_owned(),
            Charset::Utf16 => {
                let units: Vec<u16> = numbers
                    .chunks_exact(2)
                    .map(|pair| {
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "a byte is masked into 0..=255 before the cast"
                        )]
                        let low = (pair[0] as i64 & 0xff) as u16;
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "a byte is masked into 0..=255 before the cast"
                        )]
                        let high = (pair[1] as i64 & 0xff) as u16;
                        (high << 8) | low
                    })
                    .collect();
                String::from_utf16_lossy(&units)
            }
            Charset::Ascii => numbers
                .iter()
                .map(|byte| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a code is masked into 0..=127 before the cast"
                    )]
                    let code = (*byte as i64 & 0x7f) as u8;
                    char::from(code)
                })
                .collect(),
        };
        Ok(Some(value::text(text)))
    })
}

/// Байты как `u8`.
#[tracing::instrument(level = "trace", fields(numbers = ?numbers))]
fn to_bytes_vec(numbers: &[f64]) -> Vec<u8> {
    numbers
        .iter()
        .map(|byte| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a byte is masked into 0..=255 before the cast"
            )]
            let value = (*byte as i64 & 0xff) as u8;
            value
        })
        .collect()
}

/// `set_variable_parse_to_component`: текст с выбранным разбором.
///
/// Компонентов у мока нет, но разбор — часть значения, и он сохраняется: так
/// текст, разобранный как `MiniMessage`, не смешивается с обычным.
///
/// # Errors
///
/// Возвращает [`RuntimeError::InvalidEnumArgument`] на неизвестном разборе и
/// ошибку вычисления аргументов.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn parse_to_component<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let parsing = match rt.enum_arg(stream, args, "parsing")? {
            "PLAIN" => TextParsing::Plain,
            "LEGACY" => TextParsing::Legacy,
            "MINIMESSAGE" => TextParsing::MiniMessage,
            "JSON" => TextParsing::Json,
            other => return Err(invalid_enum(args, "parsing", other.to_owned())),
        };
        Ok(Some(Value::Text {
            text: text.into(),
            parsing,
        }))
    })
}

/// `set_variable_get_char_at`: the character of a text at an index from zero.
///
/// An index past the end gives an empty text: this action has no
/// default-value argument, and stopping the program over a read past the end of
/// a string would trip on every walk through text.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn get_char_at<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?.into_owned();
        let index = rt.number_arg(stream, args, "index")?;
        let character = index_in(index, text.chars().count())
            .and_then(|index| text.chars().nth(index))
            .map_or_else(String::new, String::from);
        Ok(Some(value::text(character)))
    })
}

/// `set_variable_text_length`: the length of a text in characters.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn text_length<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?;
        Ok(Some(value::number(count_as_number(text.chars().count()))))
    })
}

/// `set_variable_convert_text_to_number`: parses a text as a number in the
/// given radix.
///
/// # Errors
///
/// Returns any error from evaluating the arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
pub(super) fn convert_text_to_number<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    assign(rt, stream, op, |rt, stream, args| {
        let text = rt.text_arg(stream, args, "text")?;
        let radix = rt.optional_number_arg(stream, args, "radix")?;
        let radix = radix.map_or(10_u32, radix_of);
        let parsed = i64::from_str_radix(text.trim(), radix).map_or_else(
            |_| f64::NAN,
            |value| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "i64 carries more digits than an f64, but the JustMC result is \
                              rounded to a double anyway"
                )]
                let parsed = value as f64;
                parsed
            },
        );
        Ok(Some(value::number(parsed)))
    })
}

/// The radix for parsing a text.
///
/// `from_str_radix` does not accept a radix below two, and a non-numeric
/// argument value means "not given", so both extremes fall back to decimal.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a radix is an integer from 2 to 36; the fraction is dropped"
)]
#[tracing::instrument(level = "trace", fields(radix = ?radix))]
fn radix_of(radix: f64) -> u32 {
    let radix = radix.trunc();
    if radix.is_finite() && (2.0..=36.0).contains(&radix) {
        radix as u32
    } else {
        10
    }
}
