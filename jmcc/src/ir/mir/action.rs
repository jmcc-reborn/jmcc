//! Lowering platform actions: parsing object and action name,
//! selectors, conditions, `assign` slots, and laying out arguments by `ActionDef`.

use super::*;

impl MirLowerer<'_> {
    #[instrument(skip(self, ids), level = "trace")]
    pub(super) fn lower_action(&mut self, ids: &[Id], is_statement: bool) -> Result<Id, MirError> {
        let obj_str = extract_str_hir(self.hir, ids[0])?;
        let name_str = extract_str_hir(self.hir, ids[1])?;

        if obj_str == "value" {
            let name = self.lower_expr(ids[1])?;
            let sel = self.lower_expr(ids[2])?;
            return Ok(self.add(Mir::GameValue([name, sel])));
        }

        let def = schema::action_def(&obj_str, &name_str);

        let obj = self.lower_expr(ids[0])?;
        let name = self.lower_expr(ids[1])?;
        let sel = self.lower_expr(ids[2])?;

        let cond_id = self.lower_expr(ids[6])?;
        let args_id = ids[3];

        let lambda_id_mir = self.lower_expr(ids[5])?;
        let action_name = format!("{obj_str}::{name_str}");
        let args = self.lower_args(args_id, def, &action_name, is_statement, lambda_id_mir)?;
        let block = self.lower_stmt(ids[4])?;

        let action_id = self.add(Mir::Action(
            vec![obj, name, sel, args, block, lambda_id_mir, cond_id].into_boxed_slice(),
        ));

        if !is_statement && let Some(def) = def {
            return self.bake_assign_args(action_id, def);
        }

        if !is_statement && let Some(def) = def {
            let is_conditional =
                def.action_type == "container" || def.action_type.ends_with("_with_conditional");
            if is_conditional {
                let temp = self.fresh_temp();
                let true_val = self.add(Mir::Num(1.0.into()));
                let set_true = self.make_set_var(temp, true_val);
                let then_block = self.add(Mir::Block(vec![set_true].into_boxed_slice()));

                let zero_val = self.add(Mir::Num(0.0.into()));
                let set_zero = self.make_set_var(temp, zero_val);
                let else_block = self.add(Mir::Block(vec![set_zero].into_boxed_slice()));

                let new_action_ids = vec![obj, name, sel, args, then_block, lambda_id_mir, cond_id];
                let new_action = self.add(Mir::Action(new_action_ids.into_boxed_slice()));
                let else_act = self.else_action(else_block);

                let block = self.add(Mir::Block(
                    vec![new_action, else_act, temp].into_boxed_slice(),
                ));
                return Ok(self.add(Mir::Let([temp, block, temp])));
            }
        }

        Ok(action_id)
    }

    #[instrument(skip(self, def), level = "trace")]
    fn bake_assign_args(
        &mut self,
        action_id: Id,
        def: &jmcdata::generated::ActionDef,
    ) -> Result<Id, MirError> {
        let is_conditional =
            def.action_type == "container" || def.action_type.ends_with("_with_conditional");
        if is_conditional {
            return Ok(action_id);
        }

        let Some(assigns) = def.assign else {
            return Ok(action_id);
        };
        if assigns.is_empty() {
            return Ok(action_id);
        }

        let mut new_args_ids = Vec::new();
        let mut first_temp = None;
        for assign in assigns {
            let temp = self.fresh_temp();
            if first_temp.is_none() {
                first_temp = Some(temp);
            }
            new_args_ids.push(self.named_arg(assign.id, temp));
        }

        let act_ids = match &self.nodes.as_slice()[usize::from(action_id)] {
            Mir::Action(ids) => ids.clone(),
            _ => unreachable!(),
        };
        if let Mir::List(old_args) = &self.nodes.as_slice()[usize::from(act_ids[3])] {
            new_args_ids.extend_from_slice(old_args);
        }

        let new_args = self.add(Mir::List(new_args_ids.into_boxed_slice()));
        let mut new_action_ids = act_ids.to_vec();
        new_action_ids[3] = new_args;
        let new_action = self.add(Mir::Action(new_action_ids.into_boxed_slice()));

        first_temp.map_or(Ok(action_id), |temp| {
            Ok(self.add(Mir::Let([temp, new_action, temp])))
        })
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_args(
        &mut self,
        args_hir_id: Id,
        def: Option<&jmcdata::generated::ActionDef>,
        action_name: &str,
        is_statement: bool,
        lambda_id: Id,
    ) -> Result<Id, MirError> {
        let arg_ids = self.hir_list(args_hir_id);
        self.lower_args_slice(&arg_ids, def, action_name, is_statement, lambda_id)
    }

    #[instrument(skip(self, arg_ids, def), level = "trace")]
    fn lower_args_slice(
        &mut self,
        arg_ids: &[Id],
        def: Option<&jmcdata::generated::ActionDef>,
        action_name: &str,
        is_statement: bool,
        lambda_id: Id,
    ) -> Result<Id, MirError> {
        let mut ids = Vec::new();
        let mut used_params: HashSet<String> = HashSet::new();

        let mut lambda_params_map: HashMap<String, Id> = HashMap::new();
        if let Some(def) = def {
            if let Some(lambdas) = def.lambda
                && let Mir::List(lambda_ids) = &self.nodes.as_slice()[usize::from(lambda_id)]
            {
                for (i, l_def) in lambdas.iter().enumerate() {
                    if let Some(&l_id) = lambda_ids.get(i) {
                        let name = l_def.id.to_owned();
                        // Reserve lambda parameters before resolving positional arguments.
                        lambda_params_map.insert(name.clone(), l_id);
                        used_params.insert(name);
                    }
                }
            }

            // In statement position the operator sets the target, so `assign` slots stay
            // unreserved (see `initial_used_params` in `ir::hir`).
            if !is_statement && let Some(assigns) = def.assign {
                for assign in assigns {
                    used_params.insert(assign.id.to_owned());
                }
            }
        }

        let mut positional_idx = 0;
        for &arg_id in arg_ids {
            let (name, val_id) = self.hir_arg(arg_id)?;

            let mut val = self.lower_expr(val_id)?;

            let mut target_arg =
                resolve_action_arg_def(name.as_deref(), def, &mut used_params, &mut positional_idx);

            if let Some(t_arg) = target_arg
                && t_arg.arg_type == "variable"
            {
                let is_var_expr = matches!(
                    &self.hir[val_id],
                    Hir::Var(_) | Hir::Local(_) | Hir::Game(_) | Hir::Save(_) | Hir::Line(_)
                );
                if !is_var_expr {
                    target_arg = resolve_action_arg_def(
                        name.as_deref(),
                        def,
                        &mut used_params,
                        &mut positional_idx,
                    );
                }
            }

            if let Some(t_arg) = target_arg
                && t_arg.arg_type == "enum"
            {
                match self.nodes.as_slice()[usize::from(val)].clone() {
                    Mir::Num(n) => {
                        if let Some(values) = t_arg.values {
                            let idx = n.0 as usize;
                            if idx < values.len() {
                                let s = self.str_id(values[idx]);
                                val = self.add(Mir::Enum(s));
                            }
                        }
                    }
                    Mir::Text([_, c]) => {
                        val = self.add(Mir::Enum(c));
                    }
                    _ => {}
                }
            }

            let arg_name = match name {
                Some(n) => n,
                None => {
                    if let Some(t_arg) = target_arg {
                        t_arg.id.to_owned()
                    } else {
                        return Err(MirError::UnresolvedArgument {
                            action: action_name.to_owned(),
                            positional_idx,
                        });
                    }
                }
            };

            used_params.insert(arg_name.clone());
            lambda_params_map.remove(&arg_name);
            ids.push(self.named_arg(&arg_name, val));
        }

        for (name, val_id) in lambda_params_map {
            ids.push(self.named_arg(&name, val_id));
        }

        Ok(self.add(Mir::List(ids.into_boxed_slice())))
    }
}
