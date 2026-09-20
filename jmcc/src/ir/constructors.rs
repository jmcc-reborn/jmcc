use crate::ir::codegen::{CodeGen, CodegenError};
use crate::ir::mir::Mir;
use egg::{Id, RecExpr};
use jmcdata::module::{Number, Op, TextParsing, Value};
use litemap::LiteMap;
use ordered_float::OrderedFloat;
use simdnbt::owned::{BaseNbt, Nbt, NbtCompound, NbtList, NbtTag};
use std::borrow::Cow;
use std::collections::HashSet;

struct ItemMetadata {
    name: String,
    name_parsing: TextParsing,
    lore: Vec<(String, TextParsing)>,
    custom_tags: LiteMap<String, String>,
}

impl CodeGen {
    #[must_use]
    #[tracing::instrument(skip(self, e, arg_ids), level = "trace")]
    pub fn get_arg_id(
        &self,
        e: &RecExpr<Mir>,
        arg_ids: &[Id],
        name: &str,
        pos: usize,
    ) -> Option<Id> {
        for &id in arg_ids {
            if let Mir::Named([n, v]) = &e[id]
                && let Mir::Str(s) = &e[*n]
                && s.0.as_str() == name
            {
                return Some(*v);
            }
        }
        let mut positional_idx = 0;
        for &id in arg_ids {
            if !matches!(&e[id], Mir::Named(_)) {
                if positional_idx == pos {
                    return Some(id);
                }
                positional_idx += 1;
            }
        }
        None
    }

    /// # Errors
    ///
    /// Returns an error if the selected argument cannot be emitted as text.
    #[tracing::instrument(skip(self, e, arg_ids, ops), level = "trace")]
    pub fn get_string_arg(
        &mut self,
        e: &RecExpr<Mir>,
        arg_ids: &[Id],
        name: &str,
        pos: usize,
        ops: &mut Vec<Op<'static>>,
    ) -> Result<String, CodegenError> {
        if let Some(v_id) = self.get_arg_id(e, arg_ids, name, pos) {
            let v_id = self.resolve_bound_value(e, v_id);
            let val = self.emit_value(e, v_id, ops)?;
            if let Value::Text { text, .. } = val {
                return Ok(text.into_owned());
            }
            if let Value::Number {
                number: Number::Simple(f),
            } = val
            {
                return Ok(f.to_string());
            }
            if self.edition < 2026 {
                tracing::warn!(
                    "Constructor argument '{name}' in legacy edition must be a text string, got {val:?}, defaulting to empty string"
                );
                return Ok(String::new());
            }
            return Err(CodegenError::InvalidConstructorArgument {
                argument: name.to_owned(),
                expected: "text string",
                got: format!("{val:?}"),
            });
        }
        Ok(String::new())
    }

    /// # Errors
    ///
    /// Returns an error if the selected argument cannot be emitted as a number.
    #[tracing::instrument(skip(self, e, arg_ids, ops), level = "trace")]
    pub fn get_number_arg(
        &mut self,
        e: &RecExpr<Mir>,
        arg_ids: &[Id],
        name: &str,
        pos: usize,
        ops: &mut Vec<Op<'static>>,
    ) -> Result<f64, CodegenError> {
        if let Some(v_id) = self.get_arg_id(e, arg_ids, name, pos) {
            let v_id = self.resolve_bound_value(e, v_id);
            let val = self.emit_value(e, v_id, ops)?;
            if let Value::Number {
                number: Number::Simple(f),
            } = val
            {
                return Ok(f.into_inner());
            }
            if self.edition < 2026 {
                tracing::warn!(
                    "Constructor argument '{name}' in legacy edition must be a constant number, got {val:?}, defaulting to 0.0"
                );
                return Ok(0.0);
            }
            return Err(CodegenError::InvalidConstructorArgument {
                argument: name.to_owned(),
                expected: "number",
                got: format!("{val:?}"),
            });
        }
        Ok(0.0)
    }

    /// # Errors
    ///
    /// Returns an error if the constructor is unknown or one of its arguments is invalid.
    #[tracing::instrument(skip(self, e, arg_ids, ops), fields(name), level = "trace")]
    pub fn make_constructor_value(
        &mut self,
        e: &RecExpr<Mir>,
        name: &str,
        arg_ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        tracing::trace!("make_constructor_value called for: {name}");
        Ok(match name {
            "sound" => self.make_sound(e, arg_ids, ops)?,
            "particle" => self.make_particle(e, arg_ids, ops)?,
            "potion" => self.make_potion(e, arg_ids, ops)?,
            "item" => self.make_item(e, arg_ids, ops)?,
            "block" => self.make_block(e, arg_ids, ops)?,
            "value" => self.make_game_value(e, arg_ids, ops)?,
            "enum" => self.make_enum(e, arg_ids, ops)?,
            _ => {
                return Err(CodegenError::UnknownConstructor(name.to_owned()));
            }
        })
    }

    fn make_sound(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        let sound = self.get_string_arg(e, args, "sound", 0, ops)?;
        let volume = self.get_number_arg(e, args, "volume", 1, ops)?;
        let pitch = self.get_number_arg(e, args, "pitch", 2, ops)?;
        let variation = self.get_string_arg(e, args, "variation", 3, ops)?;
        let source = self.get_string_arg(e, args, "source", 4, ops)?;
        Ok(Value::Sound {
            sound: Cow::Owned(sound),
            pitch: OrderedFloat(pitch),
            volume: OrderedFloat(volume),
            variaton: Cow::Owned(variation),
            source: Cow::Owned(if source.is_empty() {
                "MASTER".to_owned()
            } else {
                source
            }),
        })
    }

    fn make_particle(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        Ok(Value::Particle {
            particle_type: Cow::Owned(self.get_string_arg(e, args, "particle", 0, ops)?),
            count: OrderedFloat(self.get_number_arg(e, args, "count", 1, ops)?),
            first_spread: OrderedFloat(self.get_number_arg(e, args, "spread_x", 2, ops)?),
            second_spread: OrderedFloat(self.get_number_arg(e, args, "spread_y", 3, ops)?),
            x_motion: OrderedFloat(self.get_number_arg(e, args, "motion_x", 4, ops)?),
            y_motion: OrderedFloat(self.get_number_arg(e, args, "motion_y", 5, ops)?),
            z_motion: OrderedFloat(self.get_number_arg(e, args, "motion_z", 6, ops)?),
            material: Cow::Owned(self.get_string_arg(e, args, "material", 7, ops)?),
            color: OrderedFloat(self.get_number_arg(e, args, "color", 8, ops)?),
            size: OrderedFloat(self.get_number_arg(e, args, "size", 9, ops)?),
            to_color: OrderedFloat(self.get_number_arg(e, args, "to_color", 10, ops)?),
        })
    }

    fn make_potion(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        Ok(Value::Potion {
            potion: Cow::Owned(self.get_string_arg(e, args, "potion", 0, ops)?),
            amplifier: OrderedFloat(self.get_number_arg(e, args, "amplifier", 1, ops)?),
            duration: OrderedFloat(self.get_number_arg(e, args, "duration", 2, ops)?),
        })
    }

    fn make_item(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        let id = self
            .get_string_arg(e, args, "id", 0, ops)?
            .to_lowercase()
            .replace("minecraft:", "");
        let count = self.item_count(e, args, ops)?;
        let (name, name_parsing) = self.item_name(e, args, ops)?;
        let metadata = ItemMetadata {
            name,
            name_parsing,
            lore: self.item_lore(e, args, ops)?,
            custom_tags: self.item_custom_tags(e, args, ops)?,
        };
        let components = self.item_components(e, args, ops, &metadata)?;
        let mut item = NbtCompound::new();
        item.insert("id", NbtTag::String(format!("minecraft:{id}").into()));
        item.insert("count", NbtTag::Int(count));
        item.insert("DataVersion", NbtTag::Int(3700));
        if !components.is_empty() {
            item.insert("components", NbtTag::Compound(components));
        }
        let mut buffer = Vec::new();
        Nbt::Some(BaseNbt::new("", item)).write(&mut buffer);
        Ok(Value::Item {
            item: Cow::Owned(crate::utils::gzip_base64_encode(&buffer)?),
        })
    }

    fn item_count(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<i32, CodegenError> {
        let Some(id) = self.get_arg_id(e, args, "count", 2) else {
            return Ok(1);
        };
        Ok(match self.emit_value(e, id, ops)? {
            Value::Number {
                number: Number::Simple(number),
            } => number.into_inner() as i32,
            _ => 1,
        })
    }

    fn item_name(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<(String, TextParsing), CodegenError> {
        let Some(id) = self.get_arg_id(e, args, "name", 1) else {
            return Ok((String::new(), TextParsing::Legacy));
        };
        Ok(match self.emit_value(e, id, ops)? {
            Value::Text { text, parsing } => (text.into_owned(), parsing),
            _ => (String::new(), TextParsing::Legacy),
        })
    }

    fn item_lore(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Vec<(String, TextParsing)>, CodegenError> {
        let Some(id) = self.get_arg_id(e, args, "lore", 3) else {
            return Ok(Vec::new());
        };
        let Value::Array { values } = self.emit_value(e, id, ops)? else {
            return Ok(Vec::new());
        };
        Ok(values
            .iter()
            .flatten()
            .filter_map(|value| match value {
                Value::Text { text, parsing } => Some((text.to_string(), *parsing)),
                _ => None,
            })
            .collect())
    }

    fn item_custom_tags(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<LiteMap<String, String>, CodegenError> {
        let Some(id) = self.get_arg_id(e, args, "custom_tags", 5) else {
            return Ok(LiteMap::new());
        };
        let id = self.resolve_bound_value(e, id);
        let value = self.emit_value(e, id, ops)?;
        let Value::Map { values } = value else {
            if let Value::Text { text, .. } = &value {
                if text == "{}" || text.is_empty() {
                    return Ok(LiteMap::new());
                }
                if let Ok(serde_json::Value::Object(map)) =
                    serde_json::from_str::<serde_json::Value>(text)
                {
                    let mut tags = LiteMap::new();
                    for (k, v) in map {
                        let s = match v {
                            serde_json::Value::String(s) => s,
                            other => other.to_string(),
                        };
                        tags.insert(k, s);
                    }
                    return Ok(tags);
                }
            }
            return Ok(LiteMap::new());
        };
        let mut tags = LiteMap::new();
        for (key, value) in values.iter() {
            let key = serde_json::from_str::<serde_json::Value>(&key.0)
                .ok()
                .and_then(|json| json.get("text")?.as_str().map(String::from))
                .unwrap_or_default();
            if !key.is_empty() {
                tags.insert(key, Self::custom_tag_value(value));
            }
        }
        Ok(tags)
    }

    fn resolve_bound_value(&self, e: &RecExpr<Mir>, mut id: Id) -> Id {
        let mut visited = HashSet::new();
        while let Mir::Var(var_name) = &e[Self::unwrapped_variable_static(e, id)] {
            if visited.insert(var_name.0)
                && let Some(&bound) = self.let_bindings.get(&var_name.0)
            {
                id = bound;
            } else {
                break;
            }
        }
        id
    }

    fn custom_tag_value(value: &Value<'_>) -> String {
        match value {
            Value::Text { text, .. } => text.to_string(),
            Value::Number {
                number: Number::Simple(number),
            } => number.to_string(),
            Value::Enum { value, .. } => value.to_string(),
            _ => String::new(),
        }
    }

    fn item_components(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
        metadata: &ItemMetadata,
    ) -> Result<NbtCompound, CodegenError> {
        let mut components = self.item_nbt(e, args, ops)?;
        if !metadata.name.is_empty() {
            let name = crate::utils::parse_text_to_nbt(&metadata.name, metadata.name_parsing);
            components.insert("minecraft:custom_name", NbtTag::Compound(name));
        }
        if !metadata.lore.is_empty() {
            let lore = metadata
                .lore
                .iter()
                .map(|(text, parsing)| crate::utils::parse_text_to_nbt(text, *parsing))
                .collect();
            components.insert("minecraft:lore", NbtTag::List(NbtList::Compound(lore)));
        }
        if !metadata.custom_tags.is_empty() {
            let mut values = NbtCompound::new();
            for (key, value) in metadata.custom_tags.iter() {
                values.insert(
                    format!("justcreativeplus:{key}"),
                    NbtTag::String(value.clone().into()),
                );
            }
            let mut custom_data = NbtCompound::new();
            custom_data.insert("PublicBukkitValues", NbtTag::Compound(values));
            components.insert("minecraft:custom_data", NbtTag::Compound(custom_data));
        }
        Ok(components)
    }

    fn item_nbt(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<NbtCompound, CodegenError> {
        let Some(id) = self.get_arg_id(e, args, "nbt", 4) else {
            return Ok(NbtCompound::new());
        };
        let Value::Text { text, .. } = self.emit_value(e, id, ops)? else {
            return Ok(NbtCompound::new());
        };
        if text.is_empty() {
            return Ok(NbtCompound::new());
        }
        let text = text
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\t", "\t");
        match crate::utils::parse_snbt(&text) {
            Ok(NbtTag::Compound(compound)) => Ok(compound),
            Ok(other) => {
                tracing::warn!(?other, "NBT parsed but is not a compound");
                Ok(NbtCompound::new())
            }
            Err(error) => {
                tracing::warn!(%error, "Failed to parse NBT");
                Ok(NbtCompound::new())
            }
        }
    }

    fn make_block(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        Ok(Value::Block {
            block: Cow::Owned(self.get_string_arg(e, args, "id", 0, ops)?),
        })
    }

    fn make_game_value(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        let name = self.get_string_arg(e, args, "value", 0, ops)?;
        let game_value = serde_json::from_str(&format!("\"{name}\"")).map_err(|source| {
            CodegenError::InvalidGameValueId {
                value: name.clone(),
                source,
            }
        })?;
        Ok(Value::GameValue {
            game_value,
            selection: Cow::Borrowed("null"),
        })
    }

    fn make_enum(
        &mut self,
        e: &RecExpr<Mir>,
        args: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> Result<Value<'static>, CodegenError> {
        Ok(Value::Enum {
            value: Cow::Owned(self.get_string_arg(e, args, "value", 0, ops)?),
            variable: None,
            scope: None,
        })
    }
}
