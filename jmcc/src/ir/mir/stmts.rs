//! Lowering MIR statements: blocks, branching (`if`/`else`), loops,
//! `return`/`break`, function/process/event declarations, and `let` bindings.

use super::*;

impl MirLowerer<'_> {
    #[instrument(skip(self), level = "trace")]
    fn repeat_forever(&mut self, body: Id) -> Id {
        self.action("repeat", "forever", vec![], body)
    }

    #[instrument(skip(self), level = "trace")]
    fn make_if_else(&mut self, cond: Id, then_block: Id, else_block: Option<Id>) -> Id {
        let true_val = self.add(Mir::Num(1.0.into()));
        let if_action = self.cmp_action("equals", cond, true_val, then_block);
        let mut ops = vec![if_action];
        if let Some(else_b) = else_block {
            ops.push(self.else_action(else_b));
        }
        self.add(Mir::Block(ops.into_boxed_slice()))
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_condition_action(&mut self, cond_hir: Id) -> Result<Option<(Id, bool)>, MirError> {
        let mut curr = cond_hir;
        let mut local_substs = Vec::new();

        while let Hir::Let([var, val, body]) = &self.hir[curr] {
            if self.is_atomic_hir(*val) {
                self.var_subst.insert(*var, *val);
                local_substs.push(*var);
                curr = *body;
            } else {
                break;
            }
        }

        if let Hir::Block(ids) = &self.hir[curr]
            && ids.len() == 2
            && let (Hir::Set([ret1, expr]), ret2) = (&self.hir[ids[0]], ids[1])
            && *ret1 == ret2
        {
            curr = *expr;
        }

        let res = match &self.hir[curr] {
            Hir::Not(inner) => {
                if let Some((act, is_inv)) = self.lower_condition_action(*inner)? {
                    Ok(Some((act, !is_inv)))
                } else {
                    Ok(None)
                }
            }
            Hir::Eq([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((self.cmp_action("equals", a, b, nop), false)))
            }
            Hir::Ne([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((self.cmp_action("equals", a, b, nop), true)))
            }
            Hir::Lt([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((self.cmp_action("less", a, b, nop), false)))
            }
            Hir::Le([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((self.cmp_action("less_or_equals", a, b, nop), false)))
            }
            Hir::Gt([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((self.cmp_action("greater", a, b, nop), false)))
            }
            Hir::Ge([a, b]) => {
                let a = self.lower_expr(*a)?;
                let b = self.lower_expr(*b)?;
                let nop = self.nop();
                Ok(Some((
                    self.cmp_action("greater_or_equals", a, b, nop),
                    false,
                )))
            }
            Hir::Action(ids) if is_bool_action(self.hir, ids)? => {
                let act = self.lower_expr(curr)?;
                Ok(Some((act, false)))
            }
            _ => Ok(None),
        };

        for var in local_substs {
            self.var_subst.remove(&var);
        }

        res
    }

    #[instrument(skip(self), level = "trace")]
    fn conditional_bool_block(
        &mut self,
        cond: Id,
        actual_then: Id,
        actual_else: Option<Id>,
    ) -> Option<Id> {
        let act_ids = match &self.nodes.as_slice()[usize::from(cond)] {
            Mir::Action(ids) => ids.clone(),
            _ => return None,
        };
        let mut new_act_ids = act_ids.to_vec();
        new_act_ids[4] = actual_then;
        let if_action = self.add(Mir::Action(new_act_ids.into_boxed_slice()));
        let mut ops = vec![if_action];
        if let Some(else_b) = actual_else {
            ops.push(self.else_action(else_b));
        }
        Some(self.add(Mir::Block(ops.into_boxed_slice())))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn lower_stmt(&mut self, id: Id) -> Result<Id, MirError> {
        let hir_node = &self.hir[id];
        trace!(?hir_node, "Lowering stmt");
        let res = match hir_node {
            Hir::Block(ids) => {
                let new_ids = ids
                    .iter()
                    .map(|i| self.lower_stmt(*i))
                    .collect::<Result<Vec<_>, _>>()?;
                self.add(Mir::Block(new_ids.into_boxed_slice()))
            }
            Hir::Let([var, val, body]) => {
                let var_id = self.lower_expr(*var)?;
                let val_id = self.lower_expr(*val)?;
                let body_id = self.lower_stmt(*body)?;
                self.add(Mir::Let([var_id, val_id, body_id]))
            }
            Hir::If([c, t, el]) => {
                let then_ops = self.lower_stmt(*t)?;
                let else_ops = self.lower_stmt(*el)?;
                let is_empty_else =
                    matches!(&self.nodes.as_slice()[usize::from(else_ops)], Mir::Nop);

                if let Some((cond, is_inverted)) = self.lower_condition_action(*c)? {
                    let actual_then = if is_inverted { else_ops } else { then_ops };
                    let actual_else = if !is_inverted && !is_empty_else {
                        Some(else_ops)
                    } else if is_inverted
                        && !matches!(&self.nodes.as_slice()[usize::from(then_ops)], Mir::Nop)
                    {
                        Some(then_ops)
                    } else {
                        None
                    };
                    if let Some(block_id) =
                        self.conditional_bool_block(cond, actual_then, actual_else)
                    {
                        return Ok(block_id);
                    }
                }

                let cond = self.lower_expr(*c)?;
                let else_block = if !is_empty_else { Some(else_ops) } else { None };
                self.make_if_else(cond, then_ops, else_block)
            }
            Hir::While([c, b]) => self.lower_while_loop(*c, *b)?,
            Hir::Action(ids) => self.lower_action(ids, true)?,
            _ => self.lower_expr(id)?,
        };
        Ok(res)
    }

    pub(super) fn lower_function_decl(
        &mut self,
        [name, params, body]: [Id; 3],
        kind: FunctionKind,
    ) -> Result<Id, MirError> {
        let name = self.lower_expr(name)?;
        let mut params = self.lower_expr(params)?;
        if self.edition >= 2026 && matches!(kind, FunctionKind::Function) {
            let existing = if let Mir::List(existing) = &self.nodes.as_slice()[usize::from(params)]
            {
                existing.clone()
            } else {
                Box::new([])
            };
            let ret_name = self.str_id("ref ret");
            let ret_ty = self.str_id("variable");
            let ret_param = self.add(Mir::List(vec![ret_name, ret_ty].into_boxed_slice()));
            let mut param_ids = Vec::with_capacity(1 + existing.len());
            param_ids.push(ret_param);
            param_ids.extend_from_slice(&existing);
            params = self.add(Mir::List(param_ids.into_boxed_slice()));
        }
        let body = self.lower_stmt(body)?;
        Ok(match kind {
            FunctionKind::Function => self.add(Mir::FuncDecl([name, params, body])),
            FunctionKind::Process => self.add(Mir::ProcDecl([name, params, body])),
        })
    }

    pub(super) fn lower_event_decl(&mut self, [name, body]: [Id; 2]) -> Result<Id, MirError> {
        let name = self.lower_expr(name)?;
        let body = self.lower_stmt(body)?;
        Ok(self.add(Mir::EventDecl([name, body])))
    }

    pub(super) fn lower_class_decl(&mut self, body: Id) -> Result<Id, MirError> {
        let Hir::Block(ids) = &self.hir[body] else {
            return Ok(self.nop());
        };
        let declarations = ids
            .iter()
            .filter(|&&id| matches!(self.hir[id], Hir::FuncDecl(_) | Hir::ProcDecl(_)))
            .map(|&id| self.lower_expr(id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(if declarations.is_empty() {
            self.nop()
        } else {
            self.add(Mir::Block(declarations.into_boxed_slice()))
        })
    }

    pub(super) fn lower_let(&mut self, [variable, value, body]: [Id; 3]) -> Result<Id, MirError> {
        let variable = self.lower_expr(variable)?;
        let value = self.lower_expr(value)?;
        let body = self.lower_expr(body)?;
        Ok(self.add(Mir::Let([variable, value, body])))
    }

    pub(super) fn lower_if_expr(
        &mut self,
        [condition, then, otherwise]: [Id; 3],
    ) -> Result<Id, MirError> {
        let condition = self.lower_expr(condition)?;
        let temp = self.fresh_temp();
        let then = self.lower_expr(then)?;
        let set_then = self.make_set_var(temp, then);
        let otherwise = self.lower_expr(otherwise)?;
        let mut operations = Vec::new();
        if matches!(&self.nodes.as_slice()[usize::from(otherwise)], Mir::Nop) {
            let zero = self.add(Mir::Num(0.0.into()));
            let init_temp = self.make_set_var(temp, zero);
            operations.push(init_temp);
        }
        let true_value = self.add(Mir::Num(1.0.into()));
        operations.push(self.cmp_action("equals", condition, true_value, set_then));
        if !matches!(&self.nodes.as_slice()[usize::from(otherwise)], Mir::Nop) {
            let set_otherwise = self.make_set_var(temp, otherwise);
            let else_action = self.else_action(set_otherwise);
            operations.push(else_action);
        }
        operations.push(temp);
        let block = self.add(Mir::Block(operations.into_boxed_slice()));
        Ok(self.add(Mir::Let([temp, block, temp])))
    }

    pub(super) fn lower_while_loop(&mut self, condition: Id, body: Id) -> Result<Id, MirError> {
        let body = self.lower_stmt(body)?;
        if let Some((act, is_inverted)) = self.lower_condition_action(condition)? {
            let cond_id = if is_inverted {
                self.add(Mir::Not(act))
            } else {
                act
            };
            let obj_id = self.str_id("repeat");
            let name_id = self.str_id("while");
            let sel = self.nop();
            let args_list = self.add(Mir::List(Box::new([])));
            let lambda = self.nop();
            Ok(self.add(Mir::Action(
                vec![obj_id, name_id, sel, args_list, body, lambda, cond_id].into_boxed_slice(),
            )))
        } else {
            let cond_val = self.lower_expr(condition)?;
            let break_op = self.add(Mir::Break);
            let zero_val = self.add(Mir::Num(0.0.into()));
            let if_exit = self.cmp_action("equals", cond_val, zero_val, break_op);
            let new_body = self.add(Mir::Block(vec![if_exit, body].into_boxed_slice()));
            Ok(self.repeat_forever(new_body))
        }
    }

    pub(super) fn lower_while_expr(&mut self, [condition, body]: [Id; 2]) -> Result<Id, MirError> {
        self.lower_while_loop(condition, body)
    }

    pub(super) fn lower_return(&mut self, value: Id) -> Result<Id, MirError> {
        let value = self.lower_expr(value)?;
        let mut operations = Vec::new();
        if !matches!(self.nodes.as_slice()[usize::from(value)], Mir::Nop) {
            let ret_raw = self.add(Mir::Var(VarName(Symbol::from("ret"))));
            let ret = if self.edition >= 2026 {
                self.add(Mir::Line(ret_raw))
            } else {
                self.add(Mir::Local(ret_raw))
            };
            let set = self.make_set_var(ret, value);
            operations.push(set);
        }
        let nop = self.nop();
        let return_op = self.add(Mir::ReturnFunc(nop));
        operations.push(return_op);
        Ok(match operations.as_slice() {
            [operation] => *operation,
            _ => self.add(Mir::Block(operations.into_boxed_slice())),
        })
    }

    pub(super) fn lower_block_expr(&mut self, ids: &[Id]) -> Result<Id, MirError> {
        let mut lowered = Vec::with_capacity(ids.len());
        for (index, &id) in ids.iter().enumerate() {
            let id = if index + 1 == ids.len() {
                self.lower_expr(id)?
            } else {
                self.lower_stmt(id)?
            };
            lowered.push(id);
        }
        Ok(self.add(Mir::Block(lowered.into_boxed_slice())))
    }
}
