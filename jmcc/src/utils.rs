//! Shared utilities for the JMCC compiler.

use jmcdata::generated::{ActionArg, ActionDef};
use std::collections::HashSet;
use text_components::Modifier as _;

use crate::ir::codegen::CodegenError;

/// Resolves a named or positional argument against an action definition.
#[tracing::instrument(level = "trace")]
pub fn resolve_action_arg_def<'a>(
    arg_name: Option<&str>,
    def: Option<&'a ActionDef>,
    used_params: &mut HashSet<String>,
    positional_idx: &mut usize,
) -> Option<&'a ActionArg> {
    if let Some(name) = arg_name {
        let d = def?;
        d.args.iter().find(|a| a.id == name)
    } else {
        let mut found = None;
        if let Some(def) = def {
            while *positional_idx < def.args.len() {
                let a = &def.args[*positional_idx];
                *positional_idx += 1;
                if used_params.insert(a.id.to_owned()) {
                    found = Some(a);
                    break;
                }
            }
        }
        found
    }
}

/// Splits leading spread markers (`*`/`**`) from a parameter name.
///
/// Returns the stripped name and the marker count (0, 1, or 2).
#[must_use]
pub fn split_param_spread(name: &str) -> (&str, u8) {
    if let Some(stripped) = name.strip_prefix("**") {
        return (stripped, 2);
    }
    if let Some(stripped) = name.strip_prefix('*') {
        return (stripped, 1);
    }
    (name, 0)
}

/// The marker that tells a parameter is passed by reference.
///
/// A parameter travels through the compiler as a plain name, so the `ref` of
/// `.jc` has to ride along with it. A space cannot appear in an identifier, so
/// the prefix cannot be confused with a real name; [`split_param_ref`] takes it
/// off again everywhere the name itself is used.
pub const REF_PARAM_PREFIX: &str = "ref ";

/// Splits the `ref` marker off a parameter name.
///
/// Returns the name without the marker and whether the parameter is a reference.
/// [`split_param_spread`] is applied to the result: the two markers combine, as
/// in `ref *args`, though `ref` on a variadic parameter has no separate meaning.
#[must_use]
pub fn split_param_ref(name: &str) -> (&str, bool) {
    name.strip_prefix(REF_PARAM_PREFIX)
        .map_or((name, false), |rest| (rest, true))
}

/// Returns true if the parameter name is a `self` receiver (`self`, `сам`, or `себя`).
#[must_use]
pub fn is_self_param(name: &str) -> bool {
    matches!(name, "self" | "сам" | "себя")
}

/// # Errors
///
/// Returns an error if gzip compression cannot write or finish the output stream.
#[tracing::instrument(level = "trace")]
pub fn gzip_base64_encode(data: &[u8]) -> Result<String, CodegenError> {
    use base64::Engine as _;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write as _;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&compressed))
}

#[tracing::instrument(level = "trace")]
fn parse_legacy_color(c: &str) -> text_components::format::Color {
    use text_components::format::Color;
    match c {
        "black" => Color::Black,
        "dark_blue" => Color::DarkBlue,
        "dark_green" => Color::DarkGreen,
        "dark_aqua" => Color::DarkAqua,
        "dark_red" => Color::DarkRed,
        "dark_purple" => Color::DarkPurple,
        "gold" => Color::Gold,
        "gray" => Color::Gray,
        "dark_gray" => Color::DarkGray,
        "blue" => Color::Blue,
        "green" => Color::Green,
        "aqua" => Color::Aqua,
        "red" => Color::Red,
        "light_purple" => Color::LightPurple,
        "yellow" => Color::Yellow,
        _ => Color::White,
    }
}

struct LegacyTextStyle {
    color: String,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl Default for LegacyTextStyle {
    fn default() -> Self {
        Self {
            color: "white".to_owned(),
            bold: false,
            italic: false,
            underlined: false,
            strikethrough: false,
            obfuscated: false,
        }
    }
}

impl LegacyTextStyle {
    fn component(&self, message: &str) -> text_components::RawTextComponent<'static> {
        use text_components::RawTextComponent;

        let mut component = RawTextComponent::plain(message.to_owned());
        if self.color.starts_with('#') {
            if let Some(color) = text_components::format::Color::from_hex(&self.color) {
                component = component.color(color);
            }
        } else {
            component = component.color(parse_legacy_color(&self.color));
        }
        if self.bold {
            component = component.bold(true);
        }
        if self.italic {
            component = component.italic(true);
        }
        if self.underlined {
            component = component.underlined(true);
        }
        if self.strikethrough {
            component = component.strikethrough(true);
        }
        if self.obfuscated {
            component = component.obfuscated(true);
        }
        component
    }
}

fn flush_legacy_message(
    extras: &mut Vec<text_components::RawTextComponent<'static>>,
    current_message: &mut String,
    style: &LegacyTextStyle,
) {
    if !current_message.is_empty() {
        extras.push(style.component(current_message));
        current_message.clear();
    }
}

const fn legacy_color_code(symbol: char) -> Option<&'static str> {
    match symbol {
        '0' => Some("black"),
        '1' => Some("dark_blue"),
        '2' => Some("dark_green"),
        '3' => Some("dark_aqua"),
        '4' => Some("dark_red"),
        '5' => Some("dark_purple"),
        '6' => Some("gold"),
        '7' => Some("gray"),
        '8' => Some("dark_gray"),
        '9' => Some("blue"),
        'a' => Some("green"),
        'b' => Some("aqua"),
        'c' => Some("red"),
        'd' => Some("light_purple"),
        'e' => Some("yellow"),
        'f' => Some("white"),
        _ => None,
    }
}

#[must_use]
#[tracing::instrument(level = "trace")]
pub fn parse_legacy_text(text: &str) -> text_components::RawTextComponent<'static> {
    use text_components::RawTextComponent;

    let mut style = LegacyTextStyle::default();
    let mut current_msg = String::new();
    let mut extras: Vec<RawTextComponent<'static>> = Vec::new();

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '&' && i + 1 < chars.len() {
            let sym = chars[i + 1];

            if sym == '#' && i + 7 < chars.len() {
                let hex: String = chars[i + 1..i + 8].iter().collect();
                flush_legacy_message(&mut extras, &mut current_msg, &style);
                style.color = hex;
                i += 8;
                continue;
            } else if sym == 'r' {
                flush_legacy_message(&mut extras, &mut current_msg, &style);
                style = LegacyTextStyle::default();
                i += 2;
                continue;
            } else {
                match sym {
                    'l' => {
                        flush_legacy_message(&mut extras, &mut current_msg, &style);
                        style.bold = true;
                    }
                    'o' => {
                        flush_legacy_message(&mut extras, &mut current_msg, &style);
                        style.italic = true;
                    }
                    'n' => {
                        flush_legacy_message(&mut extras, &mut current_msg, &style);
                        style.underlined = true;
                    }
                    'm' => {
                        flush_legacy_message(&mut extras, &mut current_msg, &style);
                        style.strikethrough = true;
                    }
                    'k' => {
                        flush_legacy_message(&mut extras, &mut current_msg, &style);
                        style.obfuscated = true;
                    }
                    _ => {}
                }
                if let Some(color) = legacy_color_code(sym) {
                    flush_legacy_message(&mut extras, &mut current_msg, &style);
                    style = LegacyTextStyle {
                        color: color.to_owned(),
                        ..LegacyTextStyle::default()
                    };
                    i += 2;
                    continue;
                }
            }
        }
        current_msg.push(chars[i]);
        i += 1;
    }

    if current_msg.is_empty() && extras.is_empty() {
        extras.push(style.component(""));
    } else {
        flush_legacy_message(&mut extras, &mut current_msg, &style);
    }

    let mut root = extras.remove(0);
    if !extras.is_empty() {
        root = root.add_children(extras);
    }
    root
}

/// Parses formatted text into an NBT compound.
#[must_use]
#[tracing::instrument(level = "trace")]
pub fn parse_text_to_nbt(
    text: &str,
    parsing: jmcdata::module::TextParsing,
) -> simdnbt::owned::NbtCompound {
    use simdnbt::owned::{NbtCompound, NbtTag};
    use text_components::{RawTextComponent, nbt::NbtBuilder, resolving::NoResolutor};

    if text.is_empty() {
        let mut c = NbtCompound::new();
        c.insert("text", NbtTag::String("".into()));
        return c;
    }

    let mut component = match parsing {
        jmcdata::module::TextParsing::MiniMessage => RawTextComponent::minimessage(text),
        jmcdata::module::TextParsing::Legacy => parse_legacy_text(text),
        _ => RawTextComponent::plain(text),
    };

    if component.format.italic.is_none() {
        component.format.italic = Some(false);
    }

    let nbt = component.build(&NoResolutor, NbtBuilder);

    if let NbtTag::Compound(comp) = nbt {
        comp
    } else {
        let mut c = NbtCompound::new();
        c.insert("text", NbtTag::String(text.into()));
        c
    }
}

/// Parses an SNBT string into an `NbtTag`.
///
/// # Errors
///
/// Returns an error if the input contains malformed or unsupported SNBT.
#[tracing::instrument(level = "trace")]
pub fn parse_snbt(input: &str) -> Result<simdnbt::owned::NbtTag, CodegenError> {
    let mut chars = input.trim().chars().peekable();
    parse_value(&mut chars)
}

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use std::iter::Peekable;
use std::str::Chars;

fn skip_whitespace(chars: &mut Peekable<Chars<'_>>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

fn parse_value(chars: &mut Peekable<Chars<'_>>) -> Result<NbtTag, CodegenError> {
    skip_whitespace(chars);
    match chars.peek() {
        Some(&'{') => parse_compound(chars),
        Some(&'[') => parse_list(chars),
        Some(&'"' | &'\'') => Ok(parse_string(chars)),
        _ => parse_primitive(chars),
    }
}

fn parse_key(chars: &mut Peekable<Chars<'_>>) -> Result<String, CodegenError> {
    if let Some(&'"' | &'\'') = chars.peek() {
        let s = parse_string(chars);
        if let NbtTag::String(s) = s {
            Ok(s.to_string())
        } else {
            unreachable!()
        }
    } else {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                key.push(c);
                chars.next();
            } else {
                break;
            }
        }
        Ok(key)
    }
}

/// Parses comma-separated values until the closing character `close`.
fn parse_separated<'a, T>(
    chars: &mut Peekable<Chars<'a>>,
    close: char,
    on_error: &str,
    parse_item: impl Fn(&mut Peekable<Chars<'a>>) -> Result<T, CodegenError>,
) -> Result<Vec<T>, CodegenError> {
    let mut items = Vec::new();
    if chars.peek() == Some(&close) {
        chars.next();
        return Ok(items);
    }
    loop {
        items.push(parse_item(chars)?);
        skip_whitespace(chars);
        match chars.next() {
            Some(',') => skip_whitespace(chars),
            Some(c) if c == close => break,
            _ => return Err(CodegenError::Snbt(on_error.to_owned())),
        }
    }
    Ok(items)
}

fn parse_compound(chars: &mut Peekable<Chars<'_>>) -> Result<NbtTag, CodegenError> {
    chars.next();
    let mut compound = NbtCompound::new();
    skip_whitespace(chars);
    let pairs = parse_separated(
        chars,
        '}',
        "Expected ',' or '}' in SNBT compound",
        |chars| {
            skip_whitespace(chars);
            let key = parse_key(chars)?;
            skip_whitespace(chars);
            if chars.next() != Some(':') {
                return Err(CodegenError::Snbt("Expected ':' in SNBT compound".into()));
            }
            Ok((key, parse_value(chars)?))
        },
    )?;
    for (key, value) in pairs {
        compound.insert(key, value);
    }
    Ok(NbtTag::Compound(compound))
}

fn parse_list(chars: &mut Peekable<Chars<'_>>) -> Result<NbtTag, CodegenError> {
    chars.next();
    skip_whitespace(chars);

    let mut prefix = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphabetic() {
            prefix.push(c);
            chars.next();
        } else {
            break;
        }
    }

    if !prefix.is_empty() && chars.peek() == Some(&';') {
        chars.next();
        skip_whitespace(chars);
        return parse_typed_array(chars, &prefix);
    }

    let list = parse_separated(chars, ']', "Expected ',' or ']' in SNBT list", parse_value)?;

    Ok(NbtTag::List(build_nbt_list(list)?))
}

fn parse_typed_array_values<T>(
    chars: &mut Peekable<Chars<'_>>,
    invalid_type: &'static str,
    convert: impl Fn(NbtTag) -> Option<T>,
) -> Result<Vec<T>, CodegenError> {
    parse_separated(chars, ']', "Expected ',' or ']' in array", |chars| {
        convert(parse_primitive(chars)?).ok_or_else(|| CodegenError::Snbt(invalid_type.to_owned()))
    })
}

fn parse_typed_array(
    chars: &mut Peekable<Chars<'_>>,
    prefix: &str,
) -> Result<NbtTag, CodegenError> {
    match prefix {
        "B" => parse_typed_array_values(chars, "Invalid type in byte array", |tag| match tag {
            NbtTag::Byte(value) => Some(value as u8),
            NbtTag::Int(value) => Some(value as u8),
            _ => None,
        })
        .map(NbtTag::ByteArray),
        "I" => parse_typed_array_values(chars, "Invalid type in int array", |tag| match tag {
            NbtTag::Int(value) => Some(value),
            NbtTag::Long(value) => Some(value as i32),
            _ => None,
        })
        .map(NbtTag::IntArray),
        "L" => parse_typed_array_values(chars, "Invalid type in long array", |tag| match tag {
            NbtTag::Long(value) => Some(value),
            NbtTag::Int(value) => Some(i64::from(value)),
            _ => None,
        })
        .map(NbtTag::LongArray),
        _ => Err(CodegenError::Snbt(format!("Unknown array type {prefix}"))),
    }
}

/// Expands `match` on the first tag into corresponding `NbtList` variants.
macro_rules! nbt_list_arms {
    ($tags:expr, $first:expr, $($variant:ident),+ $(,)?) => {
        match $first {
            $(
                NbtTag::$variant(_) => NbtList::$variant(
                    $tags
                        .into_iter()
                        .filter_map(|tag| match tag {
                            NbtTag::$variant(value) => Some(value),
                            _ => None,
                        })
                        .collect(),
                ),
            )+
            _ => {
                return Err(CodegenError::Snbt(
                    "Unsupported list type in SNBT".to_owned(),
                ));
            }
        }
    };
}

fn build_nbt_list(tags: Vec<NbtTag>) -> Result<NbtList, CodegenError> {
    let Some(first) = tags.first() else {
        return Ok(NbtList::Empty);
    };
    // Order of variants matches NbtTag.
    let list = nbt_list_arms!(
        tags, first, Byte, Short, Int, Long, Float, Double, String, Compound, List
    );
    Ok(list)
}

fn parse_string(chars: &mut Peekable<Chars<'_>>) -> NbtTag {
    let quote = chars.next().unwrap();
    let mut s = String::new();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(escaped) = chars.next() {
                s.push(escaped);
            }
        } else if c == quote {
            break;
        } else {
            s.push(c);
        }
    }
    NbtTag::String(s.into())
}

fn parse_primitive(chars: &mut Peekable<Chars<'_>>) -> Result<NbtTag, CodegenError> {
    let mut s = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '.' || c == '-' || c == '+' || c == '_' {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if s.is_empty() {
        return Err(CodegenError::Snbt("Unexpected end of SNBT".into()));
    }

    let last_char = s
        .chars()
        .last()
        .unwrap()
        .to_lowercase()
        .next()
        .unwrap_or(' ');
    let num_str = &s[..s.len() - 1];
    match last_char {
        'b' => {
            if let Ok(n) = num_str.parse::<i8>() {
                return Ok(NbtTag::Byte(n));
            }
            if let Ok(n) = s.parse::<i8>() {
                return Ok(NbtTag::Byte(n));
            }
        }
        's' => {
            if let Ok(n) = num_str.parse::<i16>() {
                return Ok(NbtTag::Short(n));
            }
            if let Ok(n) = s.parse::<i16>() {
                return Ok(NbtTag::Short(n));
            }
        }
        'l' => {
            if let Ok(n) = num_str.parse::<i64>() {
                return Ok(NbtTag::Long(n));
            }
            if let Ok(n) = s.parse::<i64>() {
                return Ok(NbtTag::Long(n));
            }
        }
        'f' => {
            if let Ok(n) = num_str.parse::<f32>() {
                return Ok(NbtTag::Float(n));
            }
            if let Ok(n) = s.parse::<f32>() {
                return Ok(NbtTag::Float(n));
            }
        }
        'd' => {
            if let Ok(n) = num_str.parse::<f64>() {
                return Ok(NbtTag::Double(n));
            }
            if let Ok(n) = s.parse::<f64>() {
                return Ok(NbtTag::Double(n));
            }
        }
        _ => {}
    }
    if let Ok(n) = s.parse::<i32>() {
        return Ok(NbtTag::Int(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        return Ok(NbtTag::Double(n));
    }
    Ok(NbtTag::String(s.into()))
}

/// Computes the Levenshtein edit distance between two strings.
#[must_use]
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }
    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row = vec![0; b_len + 1];

    let b_chars: Vec<char> = b.chars().collect();
    for (i, ca) in a.chars().enumerate() {
        curr_row[0] = i + 1;
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr_row[j + 1] = (curr_row[j] + 1)
                .min(prev_row[j + 1] + 1)
                .min(prev_row[j] + cost);
        }
        prev_row.copy_from_slice(&curr_row);
    }
    prev_row[b_len]
}

/// Finds the closest candidate among `candidates` to `target`.
/// Returns `Some(candidate)` if a sufficiently close match is found.
#[must_use]
pub fn did_you_mean<'a, I>(target: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let target_lower = target.to_lowercase();
    let max_dist = match target.len() {
        0..=3 => 1,
        4..=8 => 2,
        _ => 3,
    };
    let mut best: Option<(&'a str, usize)> = None;
    for cand in candidates {
        let cand_lower = cand.to_lowercase();
        let dist = levenshtein_distance(&target_lower, &cand_lower);
        if dist <= max_dist {
            if let Some((_, best_dist)) = best {
                if dist < best_dist {
                    best = Some((cand, dist));
                }
            } else {
                best = Some((cand, dist));
            }
        }
    }
    best.map(|(cand, _)| cand)
}

/// Suggests a close action name for a given object, if one exists.
#[must_use]
pub fn suggest_action(object: &str, name: &str) -> Option<&'static str> {
    let candidates = jmcdata::generated::ACTION_DEF_MAP
        .keys()
        .copied()
        .filter_map(|(obj, act_name)| if obj == object { Some(act_name) } else { None });
    did_you_mean(name, candidates)
}
