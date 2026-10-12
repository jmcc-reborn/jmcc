//! Lowering invocations: free functions, processes, and mapping their
//! arguments onto declared parameters.

use super::*;

impl MirLowerer<'_> {
    pub(super) fn lower_func_call(&mut self, target: Id, args: Id) -> Result<Id, MirError> {
        let target_id = self.lower_expr(target)?;
        let (ret, ret_arg) = if self.edition >= 2026 {
            let ret = self.fresh_temp();
            (ret, Some(ret))
        } else {
            let ret_raw = self.add(Mir::Var(VarName(Symbol::from("ret"))));
            let ret = self.add(Mir::Local(ret_raw));
            (ret, None)
        };
        let (args, forwarded_refs) = self.build_func_args_map(args, target, ret_arg)?;
        let function = self.named_arg("function_name", target_id);
        let args = self.named_arg("args", args);
        let action = self.make_action("code", "call_function", vec![function, args]);

        if forwarded_refs.is_empty() {
            Ok(self.add(Mir::Let([ret, action, ret])))
        } else {
            let mut ops = Vec::with_capacity(forwarded_refs.len() * 2 + 2);
            for &(orig, temp) in &forwarded_refs {
                ops.push(self.make_set_var(temp, orig));
            }
            ops.push(action);
            for &(orig, temp) in &forwarded_refs {
                ops.push(self.make_set_var(orig, temp));
            }
            ops.push(ret);
            let block = self.add(Mir::Block(ops.into_boxed_slice()));
            Ok(self.add(Mir::Let([ret, block, ret])))
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn lower_proc_call(&mut self, target: Id, args: Id) -> Result<Id, MirError> {
        let target_id = self.lower_expr(target)?;
        let mut prelude_ops = Vec::new();
        let mut target_mode_id = None;
        let mut local_variables_mode_id = None;
        let mut args_map_ids = Vec::new();

        let arg_ids = self.hir_list(args);

        let mut positional_idx = 0;
        for arg_id in arg_ids {
            let (name, val_id) = self.hir_arg(arg_id)?;

            let val = self.lower_expr(val_id)?;

            if let Mir::Enum(_) = &self.nodes.as_slice()[usize::from(val)].clone() {
                let is_tm = name.as_deref() == Some("target_mode")
                    || (name.is_none() && positional_idx == 0);
                let is_lvm = name.as_deref() == Some("local_variables_mode")
                    || (name.is_none() && positional_idx == 1);
                if is_tm {
                    target_mode_id = Some(val);
                    positional_idx += 1;
                    continue;
                } else if is_lvm {
                    local_variables_mode_id = Some(val);
                    positional_idx += 1;
                    continue;
                }
            }

            if let Some(name) = name {
                if self.edition >= 2026 {
                    let key_id = self.str_id(&name);
                    args_map_ids.push(key_id);
                    args_map_ids.push(val);
                } else {
                    let var_raw = self.add(Mir::Var(VarName(Symbol::from(name))));
                    let var_local = self.add(Mir::Local(var_raw));
                    let set_op = self.make_set_var(var_local, val);
                    prelude_ops.push(set_op);
                }
            } else {
                positional_idx += 1;
            }
        }

        let mut named_args = vec![self.named_arg("process_name", target_id)];
        if let Some(tm) = target_mode_id {
            named_args.push(self.named_arg("target_mode", tm));
        }
        if let Some(lvm) = local_variables_mode_id {
            named_args.push(self.named_arg("local_variables_mode", lvm));
        }

        if self.edition >= 2026 && !args_map_ids.is_empty() {
            let args_map = self.add(Mir::Map(args_map_ids.into_boxed_slice()));
            named_args.push(self.named_arg("args", args_map));
        }

        let proc_call_action = self.make_action("code", "start_process", named_args);

        if prelude_ops.is_empty() {
            Ok(proc_call_action)
        } else {
            prelude_ops.push(proc_call_action);
            Ok(self.add(Mir::Block(prelude_ops.into_boxed_slice())))
        }
    }

    #[instrument(skip(self, name), level = "trace")]
    fn find_func_params(&self, name: &str) -> Result<Vec<(String, u8, bool)>, MirError> {
        for idx in 0..self.hir.as_ref().len() {
            let id = Id::from(idx);
            if let Hir::FuncDecl([name_id, params_id, _]) = &self.hir[id] {
                let n = extract_str_hir(self.hir, *name_id)?;
                if n == name {
                    let mut params = Vec::new();
                    if let Hir::List(ids) = &self.hir[*params_id] {
                        for &p_id in ids {
                            let p_name = match &self.hir[p_id] {
                                Hir::List(pair) if pair.len() >= 2 => {
                                    extract_str_hir(self.hir, pair[0])?
                                }
                                _ => extract_str_hir(self.hir, p_id)?,
                            };
                            let (name, is_ref) = crate::utils::split_param_ref(&p_name);
                            let (stripped, spread) = crate::utils::split_param_spread(name);
                            params.push((stripped.to_owned(), spread, is_ref));
                        }
                    }
                    return Ok(params);
                }
            }
        }
        Err(MirError::FunctionNotFound(name.to_owned()))
    }

    fn lower_ref_arg(&mut self, v: Id, param_name: &str) -> Result<Id, MirError> {
        let val = self.lower_expr(v)?;
        if !self.is_variable_mir(val) {
            return Err(MirError::NonVariablePassedToRef(param_name.to_owned()));
        }
        Ok(val)
    }

    fn is_variable_mir(&self, id: Id) -> bool {
        match &self.nodes.as_slice()[usize::from(id)] {
            Mir::Var(_) => true,
            Mir::Line(i) | Mir::Local(i) | Mir::Game(i) | Mir::Save(i) => self.is_variable_mir(*i),
            _ => false,
        }
    }

    fn lower_ref_arg_forwarding(
        &mut self,
        v: Id,
        param_name: &str,
        func_name: &str,
    ) -> Result<(Id, Option<(Id, Id)>), MirError> {
        let val = self.lower_ref_arg(v, param_name)?;
        if let Some(var_name) = self.extract_var_name_mir(val)
            && self.current_ref_params.contains(&var_name)
        {
            Self::emit_nested_ref_warning(&var_name, param_name, func_name);
            let temp = self.fresh_ref_forward_temp();
            return Ok((temp, Some((val, temp))));
        }
        Ok((val, None))
    }

    #[instrument(skip(self), level = "trace")]
    fn build_func_args_map(
        &mut self,
        args_hir_id: Id,
        target_hir_id: Id,
        ret_var: Option<Id>,
    ) -> Result<(Id, Vec<(Id, Id)>), MirError> {
        let func_name = match &self.hir[target_hir_id] {
            Hir::Str(s) => s.0.to_string(),
            Hir::Var(v) => v.0.to_string(),
            _ => return Err(MirError::InvalidFuncCallTarget),
        };

        let params = self.find_func_params(&func_name)?;
        let mut map_ids = Vec::new();
        let mut forwarded_refs = Vec::new();

        let arg_ids = self.hir_list(args_hir_id);

        let mut positional_args = Vec::new();
        let mut named_args = HashMap::new();
        for arg_id in arg_ids {
            let (name, val_id) = self.hir_arg(arg_id)?;
            if let Some(n) = name {
                named_args.insert(n, val_id);
            } else {
                positional_args.push(val_id);
            }
        }

        let mut pos_idx = 0;
        for (p_name, spread, is_ref) in &params {
            if *spread == 1 {
                let mut list_ids = Vec::new();
                while pos_idx < positional_args.len() {
                    list_ids.push(self.lower_expr(positional_args[pos_idx])?);
                    pos_idx += 1;
                }
                let list_id = self.add(Mir::List(list_ids.into_boxed_slice()));
                let key_id = self.str_id(p_name);
                map_ids.push(key_id);
                map_ids.push(list_id);
            } else if *spread == 2 {
                let mut map_map_ids = Vec::new();
                for (name, val_id) in &named_args {
                    let key_id = self.str_id(name);
                    map_map_ids.push(key_id);
                    map_map_ids.push(self.lower_expr(*val_id)?);
                }
                let inner_map_id = self.add(Mir::Map(map_map_ids.into_boxed_slice()));
                let key_id = self.str_id(p_name);
                map_ids.push(key_id);
                map_ids.push(inner_map_id);
            } else {
                let (val, forwarded) = if let Some(v) = named_args.remove(p_name) {
                    if *is_ref {
                        self.lower_ref_arg_forwarding(v, p_name, &func_name)?
                    } else {
                        (self.lower_expr(v)?, None)
                    }
                } else if pos_idx < positional_args.len() {
                    let v = positional_args[pos_idx];
                    pos_idx += 1;
                    if *is_ref {
                        self.lower_ref_arg_forwarding(v, p_name, &func_name)?
                    } else {
                        (self.lower_expr(v)?, None)
                    }
                } else {
                    (self.nop(), None)
                };

                if let Some(forward_pair) = forwarded {
                    forwarded_refs.push(forward_pair);
                }

                let key_id = self.str_id(p_name);
                map_ids.push(key_id);
                map_ids.push(val);
            }
        }

        if let Some(ret) = ret_var {
            let key_id = self.str_id("ret");
            map_ids.push(key_id);
            map_ids.push(ret);
        }

        let map_id = self.add(Mir::Map(map_ids.into_boxed_slice()));
        Ok((map_id, forwarded_refs))
    }
}
