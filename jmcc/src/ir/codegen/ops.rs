//! Lowering MIR nodes into `Op`/`Module`: statements, `let` bindings, platform actions,
//! argument assignment (`Named`), and conditional branches.

use super::*;

impl CodeGen {
    #[tracing::instrument(skip(self, e), fields(node = ?e[id]), level = "trace")]
    pub(super) fn emit_op(&mut self, e: &RecExpr<Mir>, id: Id) -> CgResult<Vec<Op<'static>>> {
        let mut ops = Vec::new();
        match &e[id] {
            Mir::FuncDecl(_) | Mir::ProcDecl(_) | Mir::EventDecl(_) => {
                self.emit_root_stmt(e, id)?;
            }
            Mir::Block(ids) => {
                for &c in ids {
                    ops.extend(self.emit_op(e, c)?);
                }
            }
            Mir::Let([var, val, body]) => {
                self.emit_let_binding(e, *var, *val, &mut ops)?;
                ops.extend(self.emit_op(e, *body)?);
            }
            Mir::Action(_) => {
                let op = self.emit_action_op(e, id, None, &mut ops)?;
                ops.push(op);
            }
            Mir::Set([target, val]) => {
                let target = self.emit_variable(e, *target)?;
                let val = self.emit_value(e, *val, &mut ops)?;
                ops.push(Op::variable_set_value(target, val));
            }
            Mir::VarDecl([name, val]) => {
                let var_val = self.emit_variable(e, *name)?;
                if !matches!(&e[*val], Mir::Nop) {
                    let val = self.emit_value(e, *val, &mut ops)?;
                    ops.push(Op::variable_set_value(var_val, val));
                }
            }
            // `break` leaves the repeat it stands in, and the `.jc` `break`
            // keyword and the compiler's own loop lowering both mean exactly
            // that. The action the schema *names* `break` is `control_end_thread`
            // — end the whole thread — which is what `code::break()` calls; the
            // keyword is not that.
            Mir::Break => ops.push(Op::code_stop_repeat()),
            Mir::Continue => ops.push(Op::code_skip_iteration()),
            Mir::ReturnFunc(_) => ops.push(Op::code_return_function()),
            Mir::Nop => {}
            _ => {
                let _: Value<'static> = self.emit_value(e, id, &mut ops)?;
            }
        }
        Ok(ops)
    }

    #[tracing::instrument(skip(self, e, ops), fields(var = ?e[var], val = ?e[val]), level = "trace")]
    pub(super) fn emit_let_binding(
        &mut self,
        e: &RecExpr<Mir>,
        var: Id,
        val: Id,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<()> {
        let var_val = self.emit_variable(e, var)?;
        match &e[val] {
            Mir::Action(ids) => {
                let obj = extract_str(e, ids[0])?;
                let name = extract_str(e, ids[1])?;
                if Self::is_conditional(&obj, &name) {
                    let mut if_op = self.emit_action_op(e, val, None, ops)?;
                    if_op.operations = Some(vec![Op::variable_set_value(var_val, ONE.clone())]);
                    ops.push(if_op);
                } else {
                    let op = self.emit_action_op(e, val, Some(var_val), ops)?;
                    ops.push(op);
                }
            }
            Mir::Nop => {}
            _ => {
                let val = self.emit_value(e, val, ops)?;
                let val = self.resolve_alias(val);
                let is_target_temp = if let Value::Variable {
                    variable: target_name,
                    ..
                } = &var_val
                {
                    crate::ir::opt::mir::common::is_plain_temp(target_name, &["__ct", "__mt"])
                } else {
                    false
                };
                let is_src_temp = if let Value::Variable {
                    variable: src_name, ..
                } = &val
                {
                    crate::ir::opt::mir::common::is_temp_var(src_name, &["__ct", "__mt"])
                } else {
                    false
                };
                if is_target_temp && is_src_temp && val != var_val {
                    if let Value::Variable {
                        variable: target_name,
                        ..
                    } = &var_val
                    {
                        self.var_aliases.insert(target_name.to_string(), val);
                    }
                } else if val != var_val {
                    ops.push(Op::variable_set_value(var_val, val));
                }
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip(self, e, target_var, ops), fields(node = ?e[id]), level = "trace")]
    pub(super) fn emit_action_op(
        &mut self,
        e: &RecExpr<Mir>,
        id: Id,
        target_var: Option<Value<'static>>,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<Op<'static>> {
        let Mir::Action(ids) = &e[id] else {
            return Err(CodegenError::InvalidAction(e[id].clone()));
        };
        let obj = extract_str(e, ids[0])?;
        let name = extract_str(e, ids[1])?;
        tracing::trace!(obj, name, "emit_action_op");

        let action_id = get_action_id(&obj, &name)
            .ok_or_else(|| CodegenError::UnknownAction(obj.clone(), name.clone()))?;
        let mut builder = OpBuilder::new(action_id);

        if !matches!(&e[ids[2]], Mir::Nop) {
            let Mir::Sel(s_id) = &e[ids[2]] else {
                return Err(CodegenError::InvalidSelector(e[ids[2]].clone()));
            };
            builder = builder.with_selection(Selection {
                selection_type: Cow::Owned(extract_str(e, *s_id)?),
            });
        }

        let action_def = get_action_def(&obj, &name);
        let assign_ids: HashSet<String> = action_def
            .and_then(|d| d.assign)
            .map(|a| a.iter().map(|x| x.id.to_owned()).collect())
            .unwrap_or_default();

        let (mut final_values, mut used) =
            self.emit_action_arguments(e, ids[3], action_def, ops)?;
        self.add_assign_targets(
            &assign_ids,
            target_var.as_ref(),
            &mut final_values,
            &mut used,
        );

        for (k, v) in final_values {
            builder = builder.with_value(k, v);
        }

        if !matches!(&e[ids[4]], Mir::Nop) {
            builder = builder.with_operations(self.emit_op(e, ids[4])?);
        }

        let mut op = builder.build();
        self.apply_action_condition(e, ids, &mut op, ops)?;
        Ok(op)
    }

    fn emit_action_arguments(
        &mut self,
        e: &RecExpr<Mir>,
        args_id: Id,
        action_def: Option<&ActionDef>,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<EmittedActionArguments> {
        let mut values = LiteMap::new();
        let mut used = HashSet::new();
        let Mir::List(arg_ids) = &e[args_id] else {
            return Ok((values, used));
        };
        for &arg_id in arg_ids {
            let (key, value_id) = match &e[arg_id] {
                Mir::Named([name, value]) => (extract_str(e, *name)?, *value),
                _ => return Err(CodegenError::InvalidAction(e[arg_id].clone())),
            };
            let arg_def = action_def.and_then(|def| def.args.iter().find(|arg| arg.id == key));
            used.insert(key.clone());
            let value = self.emit_value(e, value_id, ops)?;
            let value = Self::coerce_enum_value(value, arg_def)?;
            values.insert(Cow::Owned(key), value);
        }
        Ok((values, used))
    }

    fn coerce_enum_value(
        value: Value<'static>,
        arg_def: Option<&ActionArg>,
    ) -> CgResult<Value<'static>> {
        let Some(arg_def) = arg_def.filter(|arg| arg.arg_type == "enum") else {
            return Ok(value);
        };
        Ok(match value {
            Value::Number {
                number: Number::Simple(number),
            } => {
                let values = arg_def.values.ok_or(CodegenError::MissingEnumValue(0))?;
                let index = number.into_inner() as usize;
                let value = values
                    .get(index)
                    .ok_or(CodegenError::MissingEnumValue(index))?;
                Value::Enum {
                    value: Cow::Borrowed(*value),
                    variable: None,
                    scope: None,
                }
            }
            Value::Text { text, .. } => Value::Enum {
                value: text,
                variable: None,
                scope: None,
            },
            Value::Variable { variable, scope } => {
                let value = arg_def
                    .values
                    .and_then(|values| values.first())
                    .ok_or(CodegenError::MissingEnumValue(0))?;
                Value::Enum {
                    value: Cow::Owned((*value).to_owned()),
                    variable: Some(variable),
                    scope: Some(scope),
                }
            }
            other => other,
        })
    }

    fn add_assign_targets(
        &mut self,
        assign_ids: &HashSet<String>,
        target: Option<&Value<'static>>,
        values: &mut LiteMap<Cow<'static, str>, Value<'static>>,
        used: &mut HashSet<String>,
    ) {
        let Some(target) = target else {
            return;
        };
        let mut current = Some(target.clone());
        for assign_id in assign_ids {
            if used.insert(assign_id.clone()) {
                let value = current.take().unwrap_or_else(|| self.fresh_temp());
                values.insert(Cow::Owned(assign_id.clone()), value);
            }
        }
    }

    fn apply_action_condition(
        &mut self,
        e: &RecExpr<Mir>,
        ids: &[Id],
        op: &mut Op<'static>,
        ops: &mut Vec<Op<'static>>,
    ) -> CgResult<()> {
        let condition_index = ids.len() - 1;
        if condition_index <= 4 || matches!(&e[ids[condition_index]], Mir::Nop) {
            return Ok(());
        }
        let (is_inverted, condition_id) = match &e[ids[condition_index]] {
            Mir::Not(id) => (true, *id),
            _ => (false, ids[condition_index]),
        };
        if matches!(&e[condition_id], Mir::Action(_)) {
            let condition = self.emit_action_op(e, condition_id, None, ops)?;
            op.values = condition.values;
            op.conditional = Some(jmcdata::module::Conditional {
                action: condition.action,
                is_inverted,
            });
        }
        Ok(())
    }
}
