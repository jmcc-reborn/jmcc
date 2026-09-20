//! Assembling handlers: MIR root traversal, function, process, and event declarations,
//! handler parameters, and synthetic `world_start` handler.

use super::*;

#[tracing::instrument(level = "trace")]
fn parse_param_name(name: String) -> (String, bool, bool) {
    // `ref x` marks a parameter passed by reference; `*x`/`**x` mark spread.
    let (name, is_ref) = crate::utils::split_param_ref(&name);
    let (stripped, spread) = crate::utils::split_param_spread(name);
    (stripped.to_owned(), spread == 1, is_ref)
}

/// Converts an AST type into the JSON module's [`ArgType`].
///
/// A leading `self` parameter is always represented as [`ArgType::Variable`]: a
/// method's receiver is the caller's instance, not a copy of it. `ref` and a
/// declared type of `variable` ask for the same thing, and `JustMC` reads it
/// from exactly this field — a `variable` parameter is bound to the caller's
/// variable, so writes to it are visible outside the call.
fn map_type_to_arg_type(param_name: &str, type_str: &str, _idx: usize) -> ArgType {
    if crate::utils::is_self_param(param_name) {
        return ArgType::Variable;
    }
    let type_str = type_str.trim();

    let base = type_str.split('<').next().unwrap_or(type_str).trim();
    let lower = base.to_lowercase();
    match lower.as_str() {
        "number" | "num" | "int" | "float" | "double" => ArgType::Number,
        "text" | "string" | "str" => ArgType::Text,
        "list" | "array" => ArgType::Array,
        "map" | "dict" => ArgType::Map,
        "item" => ArgType::Item,
        "block" => ArgType::Block,
        "location" => ArgType::Location,
        "particle" => ArgType::Particle,
        "potion" => ArgType::Potion,
        "sound" => ArgType::Sound,
        "vector" => ArgType::Vector,
        "enum" => ArgType::Enum,
        // `ref x: variable` is how a parameter is spelled when it holds
        // "some variable" rather than a value of a known type.
        "variable" => ArgType::Variable,
        _ => ArgType::Any,
    }
}

#[tracing::instrument(level = "trace")]
fn make_param(
    name: String,
    i: usize,
    is_plural: bool,
    desc_slot: i32,
    value_type: ArgType,
) -> Value<'static> {
    let desc = Cow::Borrowed("{\"translations\":{}}");
    if is_plural {
        Value::Parameter {
            name: Cow::Owned(name),
            desc,
            param_type: Parameter::Plural {
                value_type,
                is_required: Cow::Borrowed("true"),
                default_value: Cow::Borrowed("[]"),
                slots: Cow::Owned(format!("[{}, {}]", i * 2, i * 2 + 1)),
                description_slots: Cow::Owned(format!("[{}]", i + 9)),
                ignore_empty_values: Cow::Borrowed("true"),
            },
        }
    } else {
        Value::Parameter {
            name: Cow::Owned(name),
            desc,
            param_type: Parameter::Singular {
                value_type,
                is_required: Cow::Borrowed("true"),
                default_value: Cow::Borrowed("{}"),
                slot: i as u32,
                description_slot: desc_slot,
            },
        }
    }
}

impl CodeGen {
    #[tracing::instrument(skip(self, e), fields(node = ?e[id]), level = "trace")]
    pub(super) fn emit_root(&mut self, e: &RecExpr<Mir>, id: Id) -> CgResult<()> {
        if let Mir::Block(ids) = &e[id] {
            for &c in ids {
                self.emit_root_stmt(e, c)?;
            }
            Ok(())
        } else {
            self.emit_root_stmt(e, id)
        }
    }

    #[tracing::instrument(skip(e), fields(node = ?e[params_id]), level = "trace")]
    fn emit_params(
        e: &RecExpr<Mir>,
        params_id: Id,
        desc_slot_base: Option<i32>,
    ) -> CgResult<LiteMap<Cow<'static, str>, Value<'static>>> {
        let mut values = LiteMap::new();
        let Mir::List(param_ids) = &e[params_id] else {
            return Ok(values);
        };
        if param_ids.is_empty() {
            return Ok(values);
        }
        let param_values: Vec<_> = param_ids
            .iter()
            .enumerate()
            .map(|(i, &p_id)| -> CgResult<_> {
                let (raw_name, ty_str) = match &e[p_id] {
                    Mir::List(pair) if pair.len() >= 2 => {
                        let name = extract_str(e, pair[0])?;
                        let ty = extract_str(e, pair[1])?;
                        (name, ty)
                    }
                    _ => unreachable!(),
                };
                let (clean, is_plural, is_ref) = parse_param_name(raw_name);
                let desc_slot = desc_slot_base.map_or(-1, |b| b + i as i32);
                let value_type = if is_ref {
                    ArgType::Variable
                } else {
                    map_type_to_arg_type(&clean, &ty_str, i)
                };
                Ok(Some(make_param(clean, i, is_plural, desc_slot, value_type)))
            })
            .collect::<CgResult<_>>()?;
        values.insert(
            Cow::Borrowed("parameters"),
            Value::Array {
                values: param_values,
            },
        );
        Ok(values)
    }

    #[tracing::instrument(skip(self, e), fields(node = ?e[id]), level = "trace")]
    pub(super) fn emit_root_stmt(&mut self, e: &RecExpr<Mir>, id: Id) -> CgResult<()> {
        match &e[id] {
            Mir::FuncDecl([name_id, params_id, body_id]) => {
                self.var_aliases.clear();
                let name = extract_str(e, *name_id)?;
                tracing::trace!(name, "FuncDecl");
                let mut values = Self::emit_params(e, *params_id, None)?;
                values.insert(
                    Cow::Borrowed("description"),
                    Value::Array {
                        values: vec![Some(Value::Text {
                            text: Cow::Owned(format!("Function: {name}")),
                            parsing: TextParsing::Legacy,
                        })],
                    },
                );
                let ops = self.emit_op(e, *body_id)?;
                self.handlers.push(Line {
                    line_type: LineType::Function,
                    position: 1337,
                    operations: ops,
                    line_value: LineValue::Fn {
                        values,
                        name: Cow::Owned(name),
                    },
                });
            }
            Mir::ProcDecl([name_id, params_id, body_id]) => {
                self.var_aliases.clear();
                let name = extract_str(e, *name_id)?;
                tracing::trace!(name, "ProcDecl");
                let values = if self.edition >= 2026 {
                    Self::emit_params(e, *params_id, Some(9))?
                } else {
                    LiteMap::new()
                };
                let ops = self.emit_op(e, *body_id)?;
                self.handlers.push(Line {
                    line_type: LineType::Process,
                    position: 1337,
                    operations: ops,
                    line_value: LineValue::Fn {
                        values,
                        name: Cow::Owned(name),
                    },
                });
            }
            Mir::EventDecl([name_id, body_id]) => {
                self.var_aliases.clear();
                let name = extract_str(e, *name_id)?;
                tracing::trace!(name, "EventDecl");
                let event = serde_json::from_str(&format!("\"{name}\""))
                    .map_err(|e| CodegenError::InvalidEvent(name.clone(), e))?;
                let ops = self.emit_op(e, *body_id)?;
                self.handlers.push(Line {
                    line_type: LineType::Event,
                    position: 1337,
                    operations: ops,
                    line_value: LineValue::Event { event },
                });
            }
            _ => {
                let ops = self.emit_op(e, id)?;
                self.world_start_ops.extend(ops);
            }
        }
        Ok(())
    }
}
