use heck::ToUpperCamelCase as _;
use litemap::LiteMap;
use ordered_float::OrderedFloat as F;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error, Visitor},
    ser::SerializeMap as _,
    ser::SerializeSeq as _,
};
use std::borrow::Cow;
use std::fmt;

use crate::generated::{ActionId, ArgType, EventId, GameValueId};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Module<'a> {
    #[serde(borrow, deserialize_with = "deserialize_handlers")]
    pub handlers: Vec<Line<'a>>,
}

impl Module<'_> {
    /// Serializes the module to compact JSON.
    ///
    /// # Errors
    ///
    /// Fails if serialization does not succeed, e.g. on a non-finite `f64`
    /// in one of the values.
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serializes the module to indented, human-readable JSON.
    ///
    /// # Errors
    ///
    /// Fails if serialization does not succeed, e.g. on a non-finite `f64`
    /// in one of the values.
    pub fn to_string_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Line<'a> {
    #[serde(rename = "type")]
    pub line_type: LineType,
    pub position: u16,
    pub operations: Vec<Op<'a>>,
    #[serde(flatten, borrow)]
    pub line_value: LineValue<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum LineType {
    Event,
    Process,
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", untagged, bound(deserialize = "'de: 'a"))]
#[repr(u8)]
pub enum LineValue<'a> {
    Event {
        event: EventId,
    },
    Fn {
        #[serde(
            serialize_with = "serialize_litemap_as_array",
            deserialize_with = "deserialize_litemap_from_array"
        )]
        values: LiteMap<Cow<'a, str>, Value<'a>>,
        name: Cow<'a, str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub struct Op<'a> {
    pub action: ActionId,
    #[serde(
        serialize_with = "serialize_litemap_as_array",
        deserialize_with = "deserialize_litemap_from_array"
    )]
    pub values: LiteMap<Cow<'a, str>, Value<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<Op<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional: Option<Conditional>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_inverted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Selection<'a> {
    #[serde(rename = "type")]
    pub selection_type: Cow<'a, str>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub struct Conditional {
    pub action: ActionId,
    pub is_inverted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextValue(pub String);

impl TryFrom<Value<'_>> for TextValue {
    type Error = serde_json::Error;

    fn try_from(value: Value<'_>) -> Result<Self, Self::Error> {
        Ok(Self(serde_json::to_string(&value)?))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(
    rename_all = "snake_case",
    tag = "type",
    bound(deserialize = "'de: 'a")
)]
pub enum Value<'a> {
    Array {
        #[serde(serialize_with = "serialize_vec_of_options")]
        values: Vec<Option<Value<'a>>>,
    },
    Block {
        block: Cow<'a, str>,
    },
    Enum {
        #[serde(rename = "enum", deserialize_with = "deserialize_upper_camel")]
        value: Cow<'a, str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        variable: Option<Cow<'a, str>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope: Option<VariableScope>,
    },
    Item {
        item: Cow<'a, str>,
    },
    Location {
        x: F<f64>,
        y: F<f64>,
        z: F<f64>,
        yaw: F<f64>,
        pitch: F<f64>,
    },
    Map {
        #[serde(
            serialize_with = "serialize_litemap_as_object",
            deserialize_with = "deserialize_litemap_from_object"
        )]
        values: LiteMap<TextValue, Value<'a>>,
    },
    Number {
        number: Number<'a>,
    },
    Particle {
        particle_type: Cow<'a, str>,
        count: F<f64>,
        first_spread: F<f64>,
        second_spread: F<f64>,
        x_motion: F<f64>,
        y_motion: F<f64>,
        z_motion: F<f64>,
        color: F<f64>,
        material: Cow<'a, str>,
        size: F<f64>,
        to_color: F<f64>,
    },
    Potion {
        potion: Cow<'a, str>,
        amplifier: F<f64>,
        duration: F<f64>,
    },
    Sound {
        sound: Cow<'a, str>,
        pitch: F<f64>,
        volume: F<f64>,
        variaton: Cow<'a, str>,
        source: Cow<'a, str>,
    },
    Text {
        text: Cow<'a, str>,
        parsing: TextParsing,
    },
    Variable {
        variable: Cow<'a, str>,
        scope: VariableScope,
    },
    Vector {
        x: F<f64>,
        y: F<f64>,
        z: F<f64>,
    },
    GameValue {
        game_value: GameValueId,
        selection: Cow<'a, str>,
    },
    Parameter {
        name: Cow<'a, str>,
        #[serde(rename = "description")]
        desc: Cow<'a, str>,
        #[serde(flatten)]
        param_type: Parameter<'a>,
    },
    #[default]
    Error,
}

impl<'a> TryFrom<&'a TextValue> for Value<'a> {
    type Error = serde_json::Error;

    fn try_from(
        value: &'a TextValue,
    ) -> Result<Self, <Value<'a> as TryFrom<&'a TextValue>>::Error> {
        serde_json::from_str(&value.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case", tag = "type_key")]
pub enum Parameter<'a> {
    Singular {
        value_type: ArgType,
        is_required: Cow<'a, str>,
        default_value: Cow<'a, str>,
        slot: u32,
        description_slot: i32,
    },
    Plural {
        value_type: ArgType,
        is_required: Cow<'a, str>,
        default_value: Cow<'a, str>,
        slots: Cow<'a, str>,
        description_slots: Cow<'a, str>,
        ignore_empty_values: Cow<'a, str>,
    },
    Enum {
        slot: u8,
        elements: Cow<'a, str>,
        default_value: Cow<'a, str>,
    },
}

impl<'a, const T: usize> From<[Value<'a>; T]> for Value<'a> {
    #[inline]
    fn from(v: [Value<'a>; T]) -> Self {
        Value::Array {
            values: v.into_iter().map(Some).collect(),
        }
    }
}

/// Все текстовые конверсии делают одно и то же: кладут текст в режим `plain`.
/// Отличается только тип-источник, поэтому impl'ы порождает макрос.
macro_rules! impl_text_from {
    ($lt:lifetime, $($ty:ty),* $(,)?) => {
        $(
            impl<$lt> From<$ty> for Value<$lt> {
                #[inline]
                fn from(v: $ty) -> Self {
                    Value::Text {
                        text: v.into(),
                        parsing: TextParsing::Plain,
                    }
                }
            }
        )*
    };
}

impl_text_from!('a, &'a str, Cow<'a, str>, &'a String, String);

/// Числовые конверсии отличаются только типом-источником; у `f64` каста нет,
/// потому что он и есть целевой тип, — этот impl написан отдельно.
macro_rules! impl_number_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl From<$ty> for Value<'_> {
                #[inline]
                fn from(v: $ty) -> Self {
                    Value::Number {
                        number: Number::Simple(F(v as f64)),
                    }
                }
            }
        )*
    };
}

impl_number_from!(u8, f32, i32, i64, usize, u32, u64);

impl From<f64> for Value<'_> {
    #[inline]
    fn from(v: f64) -> Self {
        Value::Number {
            number: Number::Simple(F(v)),
        }
    }
}

impl<'a> Value<'a> {
    #[must_use]
    pub fn type_name(&'a self) -> Cow<'a, str> {
        match &self {
            Self::Array { .. } => "array".into(),
            Self::Block { .. } => "block".into(),
            Self::Enum { .. } => "enum".into(),
            Self::Item { .. } => "item".into(),
            Self::Location { .. } => "location".into(),
            Self::Map { .. } => "map".into(),
            Self::Number { .. } => "number".into(),
            Self::Particle { .. } => "particle".into(),
            Self::Potion { .. } => "potion".into(),
            Self::Sound { .. } => "sound".into(),
            Self::Text { .. } => "text".into(),
            Self::Variable { variable, scope } => match scope {
                VariableScope::Global => format!("%var({variable})"),
                VariableScope::Line => format!("%var_line({variable})"),
                VariableScope::Local => format!("%var_local({variable})"),
                VariableScope::Save => format!("%var_save({variable})"),
            }
            .into(),
            Self::Parameter { .. } => "parameter".into(),
            Self::Vector { .. } => "vector".into(),
            Self::GameValue { .. } => "gamevalue".into(),
            Self::Error { .. } => "error".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(untagged)]
#[repr(u8)]
pub enum Number<'a> {
    Simple(#[serde(serialize_with = "serialize_float")] F<f64>),
    Calc(Cow<'a, str>),
}

impl Number<'_> {
    #[must_use]
    pub const fn from_f64(v: f64) -> Self {
        Self::Simple(F(v))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum TextParsing {
    Legacy,
    Plain,
    #[serde(rename = "minimessage")]
    MiniMessage,
    Json,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum VariableScope {
    Line,
    Local,
    #[serde(rename = "game")]
    Global,
    Save,
}

impl VariableScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Local => "local",
            Self::Global => "game",
            Self::Save => "save",
        }
    }
}

fn serialize_float<S>(value: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if value.fract() == 0.0 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

fn deserialize_upper_camel<'de: 'a, 'a, D>(deserializer: D) -> Result<Cow<'a, str>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Cow<'de, str> = Deserialize::deserialize(deserializer)?;
    if s.chars().all(|c| c.is_uppercase() || c == '_') {
        Ok(Cow::Owned(s.into_owned()))
    } else {
        Ok(Cow::Owned(s.to_upper_camel_case()))
    }
}

fn deserialize_handlers<'de: 'a, 'a, D>(deserializer: D) -> Result<Vec<Line<'a>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct LineVisitor<'a>(std::marker::PhantomData<&'a ()>);

    impl<'de: 'a, 'a> Visitor<'de> for LineVisitor<'a> {
        type Value = Vec<Line<'a>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a sequence of handler objects")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut lines = Vec::new();
            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                if let serde_json::Value::Object(obj) = value {
                    let line =
                        Line::deserialize(serde_json::Value::Object(obj)).map_err(Error::custom)?;
                    lines.push(line);
                }
            }
            Ok(lines)
        }
    }

    deserializer.deserialize_seq(LineVisitor(std::marker::PhantomData))
}

fn serialize_vec_of_options<S>(
    vec: &Vec<Option<Value<'_>>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut state = serializer.serialize_seq(Some(vec.len()))?;
    for option in vec {
        match option {
            Some(value) => state.serialize_element(value)?,
            None => state.serialize_element(&serde_json::Value::Null)?,
        }
    }
    state.end()
}

fn serialize_litemap_as_array<'a, S, V>(
    map: &LiteMap<Cow<'a, str>, V>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize + 'a,
{
    let mut seq = serializer.serialize_seq(Some(map.len()))?;
    for (name, value) in map.iter() {
        seq.serialize_element(&NamedValueTemp { name, value })?;
    }
    seq.end()
}

#[derive(Serialize)]
struct NamedValueTemp<'a, V> {
    name: &'a Cow<'a, str>,
    value: &'a V,
}

fn deserialize_litemap_from_array<'a, 'de, D, V>(
    deserializer: D,
) -> Result<LiteMap<Cow<'a, str>, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de> + 'a,
    'de: 'a,
{
    struct LiteMapVisitor<V> {
        marker: std::marker::PhantomData<V>,
    }

    impl<'de, V> Visitor<'de> for LiteMapVisitor<V>
    where
        V: Deserialize<'de>,
    {
        type Value = LiteMap<Cow<'de, str>, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a sequence of named values")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut map = LiteMap::new();
            while let Some(NamedValueDe { name, value }) = seq.next_element()? {
                map.insert(name, value);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_seq(LiteMapVisitor {
        marker: std::marker::PhantomData,
    })
}

fn serialize_litemap_as_object<S>(
    map: &LiteMap<TextValue, Value<'_>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut m = serializer.serialize_map(Some(map.len()))?;
    for (k, v) in map.iter() {
        m.serialize_entry(k, v)?;
    }
    m.end()
}

fn deserialize_litemap_from_object<'de, D>(
    deserializer: D,
) -> Result<LiteMap<TextValue, Value<'de>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MapVisitor;

    impl<'de> Visitor<'de> for MapVisitor {
        type Value = LiteMap<TextValue, Value<'de>>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut litemap = LiteMap::new();
            while let Some((key, value)) = map.next_entry::<TextValue, Value<'_>>()? {
                litemap.insert(key, value);
            }
            Ok(litemap)
        }
    }

    deserializer.deserialize_map(MapVisitor)
}

#[derive(Deserialize)]
struct NamedValueDe<'a, V> {
    name: Cow<'a, str>,
    value: V,
}

#[cfg(test)]
mod tests {
    use super::*;
    use litemap::LiteMap;

    #[test]
    fn map_serializes_as_a_json_object() {
        let empty = Value::Map {
            values: Default::default(),
        };
        assert_eq!(
            serde_json::to_string(&empty).unwrap(),
            r#"{"type":"map","values":{}}"#
        );

        let mut entries = LiteMap::new();
        entries.insert(
            TextValue("key".to_owned()),
            Value::Text {
                text: "val".into(),
                parsing: TextParsing::Plain,
            },
        );
        let map = Value::Map { values: entries };
        assert_eq!(
            serde_json::to_string(&map).unwrap(),
            r#"{"type":"map","values":{"key":{"type":"text","text":"val","parsing":"plain"}}}"#
        );
    }
}
