//! Assembling `Value` instances from MIR nodes: numbers, strings, variables,
//! collections, constructors, enums, and actions used as values.

use super::*;

impl CodeGen {
    #[tracing::instrument(skip(self, e), fields(node = ?e[id]), level = "trace")]
    pub(super) fn emit_variable(&mut self, e: &RecExpr<Mir>, id: Id) -> CgResult<Value<'static>> {
        let (name, scope) = self.get_var_name_scope(e, id)?;
        self.var_scopes.insert(name.clone(), scope);
        self.var_aliases.remove(&name);
        Ok(Value::Variable {
            variable: Cow::Owned(name),
            scope,
        })
    }

    #[tracing::instrument(skip(self, e, ops), fields(node = ?e[id]), level = "trace")]
    fn emit_text_content(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<String> {
        Ok(match &e[id] {
            Mir::Str(s) => s.0.to_string(),
            Mir::Num(n) => n.to_string(),
            Mir::Concat(ids) => {
                let mut s = String::new();
                for &c in ids {
                    s.push_str(&self.emit_text_content(e, c, ops)?);
                }
                s
            }
            Mir::Text([_, c]) => self.emit_text_content(e, *c, ops)?,
            Mir::Nbt(c) => extract_str(e, *c)?,
            _ => {
                let val = self.emit_value(e, id, ops)?;
                match val {
                    Value::Text { text, .. } => text.into_owned(),
                    Value::Number {
                        number: Number::Simple(f),
                    } => f.to_string(),
                    Value::Enum { value, .. } => value.into_owned(),
                    Value::Variable { variable, scope } => match scope {
                        VariableScope::Local => format!("%var_local({variable})"),
                        VariableScope::Line => format!("%var_line({variable})"),
                        VariableScope::Global => format!("%var({variable})"),
                        VariableScope::Save => format!("%var_save({variable})"),
                    },
                    _ => {
                        let temp_val = self.fresh_temp();
                        let Value::Variable { variable, scope } = &temp_val else {
                            return Err(CodegenError::InvalidVariable(e[id].clone()));
                        };
                        let temp_name = variable.to_string();
                        ops.push(Op::variable_set_value(temp_val.clone(), val));
                        match scope {
                            VariableScope::Local => format!("%var_local({temp_name})"),
                            VariableScope::Line => format!("%var_line({temp_name})"),
                            VariableScope::Global => format!("%var({temp_name})"),
                            VariableScope::Save => format!("%var_save({temp_name})"),
                        }
                    }
                }
            }
        })
    }

    /// # Errors
    ///
    /// Returns an error if the MIR node cannot be encoded as a runtime value.
    #[tracing::instrument(skip(self, e, ops), fields(node = ?e[id]), level = "trace")]
    pub fn emit_value(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        match &e[id] {
            Mir::Num(n) => Ok(Value::Number {
                number: Number::Simple(*n),
            }),
            Mir::Bool(b) => Ok(if *b { ONE.clone() } else { ZERO.clone() }),
            Mir::Str(s) => Ok(Value::Text {
                text: Cow::Owned(s.0.to_string()),
                parsing: TextParsing::Plain,
            }),
            Mir::Var(v) => Ok(self.resolve_alias(Value::Variable {
                variable: Cow::Owned(v.0.to_string()),
                scope: self
                    .var_scopes
                    .get(v.0.as_str())
                    .copied()
                    .unwrap_or(self.default_scope),
            })),
            Mir::Local(_) | Mir::Game(_) | Mir::Save(_) | Mir::Line(_) => {
                let v = self.emit_scoped_var(e, id, ops)?;
                Ok(self.resolve_alias(v))
            }
            Mir::Let([var, val, body]) => {
                self.emit_let_binding(e, *var, *val, ops)?;
                self.emit_value(e, *body, ops)
            }
            Mir::Action(ids) => {
                let obj = extract_str(e, ids[0])?;
                let name = extract_str(e, ids[1])?;
                if obj == "code" && name == "start_process" {
                    return Err(CodegenError::ProcessAsValue);
                }
                self.emit_action_value(e, id, ops)
            }
            Mir::Block(ids) => self.emit_block_value(e, ids, ops),
            Mir::Text([t, c]) => {
                let parsing = match extract_str(e, *t)?.as_str() {
                    "legacy" => TextParsing::Legacy,
                    "minimessage" => TextParsing::MiniMessage,
                    "json" => TextParsing::Json,
                    _ => TextParsing::Plain,
                };
                Ok(Value::Text {
                    text: Cow::Owned(self.emit_text_content(e, *c, ops)?),
                    parsing,
                })
            }
            Mir::Nbt(c) => Ok(Value::Text {
                text: Cow::Owned(extract_str(e, *c)?),
                parsing: TextParsing::Plain,
            }),
            Mir::GameValue(ids) => Self::emit_game_value(e, *ids),
            Mir::List(ids) => self.emit_array_value(e, ids, ops),
            Mir::Map(ids) => self.emit_map_value(e, ids, ops),
            Mir::Concat(ids) => self.emit_concat_value(e, ids, ops),
            Mir::Ctor(ids) => self.emit_constructor_value(e, ids, ops),
            Mir::Named([_, v]) => self.emit_value(e, *v, ops),
            Mir::Sel(s) => self.emit_value(e, *s, ops),
            Mir::Enum(value) => self.emit_enum_value(e, *value, ops),
            Mir::Not(value) => self.emit_not_value(e, *value, ops),
            Mir::Nop => Ok(ZERO.clone()),
            other => Err(CodegenError::UnsupportedValue(other.clone())),
        }
    }

    fn emit_block_value(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        for (index, &id) in ids.iter().enumerate() {
            if index + 1 == ids.len() {
                return self.emit_value(e, id, ops);
            }
            ops.extend(self.emit_op(e, id)?);
        }
        Ok(ZERO.clone())
    }

    fn emit_array_value(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let values = ids
            .iter()
            .map(|id| self.emit_value(e, *id, ops).map(Some))
            .collect::<CgResult<Vec<_>>>()?;
        Ok(Value::Array { values })
    }

    fn emit_map_value(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let mut values = LiteMap::new();
        for pair in ids.chunks_exact(2) {
            let key = self.emit_value(e, pair[0], ops)?;
            let key = TextValue(serde_json::to_string(&key)?);
            values.insert(key, self.emit_value(e, pair[1], ops)?);
        }
        Ok(Value::Map { values })
    }

    fn emit_concat_value(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let mut text = String::new();
        for &id in ids {
            text.push_str(&self.emit_text_content(e, id, ops)?);
        }
        Ok(Value::Text {
            text: Cow::Owned(text),
            parsing: TextParsing::Plain,
        })
    }

    fn emit_constructor_value(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let name = extract_str(e, ids[0])?;
        let args = match &e[ids[1]] {
            Mir::List(args) => args.as_ref(),
            _ => &[],
        };
        self.make_constructor_value(e, &name, args, ops)
    }

    fn emit_enum_value(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        match self.emit_value(e, id, ops)? {
            Value::Text { text, .. } => Ok(Value::Enum {
                value: text,
                variable: None,
                scope: None,
            }),
            _ => Err(CodegenError::InvalidEnumValue),
        }
    }

    fn emit_not_value(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let value = self.emit_value(e, id, ops)?;
        let temp = self.fresh_temp();
        ops.push(Op::variable_equals(
            value,
            ZERO.clone(),
            vec![Op::variable_set_value(temp.clone(), ONE.clone())],
        ));
        Ok(temp)
    }

    #[tracing::instrument(skip(self, e, ops), fields(node = ?e[id]), level = "trace")]
    fn emit_scoped_var(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let (inner_id, scope) = match &e[id] {
            Mir::Game(v) => (*v, VariableScope::Global),
            Mir::Save(v) => (*v, VariableScope::Save),
            Mir::Line(v) => (*v, VariableScope::Line),
            Mir::Local(v) => (*v, VariableScope::Local),
            _ => return Err(CodegenError::InvalidVariable(e[id].clone())),
        };
        let inner = self.emit_value(e, inner_id, ops)?;
        match inner {
            Value::Variable { variable, .. } => Ok(Value::Variable { variable, scope }),
            Value::Text { text, .. } => Ok(Value::Variable {
                variable: text,
                scope,
            }),
            _ => Ok(inner),
        }
    }

    #[tracing::instrument(skip(self, e, ops), fields(node = ?e[id]), level = "trace")]
    fn emit_action_value(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Value<'static>> {
        let Mir::Action(ids) = &e[id] else {
            return Err(CodegenError::InvalidAction(e[id].clone()));
        };
        let obj = extract_str(e, ids[0])?;
        let name = extract_str(e, ids[1])?;
        tracing::trace!(obj, name, "emit_action_value");

        if obj == "value" {
            let selection = if !matches!(&e[ids[2]], Mir::Nop) {
                let Mir::Sel(s_id) = &e[ids[2]] else {
                    return Err(CodegenError::InvalidSelector(e[ids[2]].clone()));
                };
                let sel_str = extract_str(e, *s_id)?;
                serde_json::to_string(&serde_json::json!({"type": sel_str}))?
            } else {
                serde_json::to_string(&serde_json::json!({"type": "default"}))?
            };
            let gv_id = serde_json::from_str(&format!("\"{name}\""))
                .map_err(|e| CodegenError::InvalidGameValue(name, e))?;
            return Ok(Value::GameValue {
                game_value: gv_id,
                selection: Cow::Owned(selection),
            });
        }

        let temp_val = self.fresh_temp();
        if Self::is_conditional(&obj, &name) {
            ops.push(Op::variable_set_value(temp_val.clone(), ZERO.clone()));

            let mut if_op = self.emit_action_op(e, id, None, ops)?;
            if_op.operations = Some(vec![Op::variable_set_value(temp_val.clone(), ONE.clone())]);
            ops.push(if_op);
        } else {
            let op = self.emit_action_op(e, id, Some(temp_val.clone()), ops)?;
            let output = Self::action_output(&op, get_action_def(&obj, &name), &temp_val)
                .unwrap_or(temp_val);
            ops.push(op);
            return Ok(self.resolve_alias(output));
        }
        Ok(self.resolve_alias(temp_val))
    }

    /// Value the action wrote to its output slot. Usually `temp`, but where the `variable` arg is
    /// explicit, [`Self::add_assign_targets`] skips that slot and the write lands there instead.
    fn action_output(
        op: &Op<'static>,
        action_def: Option<&ActionDef>,
        temp: &Value<'static>,
    ) -> Option<Value<'static>> {
        for arg in action_def?.assign? {
            let value = op.values.get(arg.id)?;
            if value != temp {
                return Some(value.clone());
            }
        }
        None
    }
}
