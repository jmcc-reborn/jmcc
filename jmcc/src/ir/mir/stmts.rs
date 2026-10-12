//! Lowering MIR statements: blocks, branching (`if`/`else`), loops,
//! `return`/`break`, function/process/event declarations, and `let` bindings.

use super::*;

impl MirLowerer<'_> {
    #[instrument(skip(self), level = "trace")]
    fn repeat_forever(&mut self, body: Id) -> Id {
        self.action("repeat", "forever", vec![], body)
    }

    #[instrument(skip(self), level = "trace")]
    fn try_invert_action(&mut self, act: Id) -> Option<Id> {
        let Mir::Action(ids) = self.nodes.as_slice()[usize::from(act)].clone() else {
            return None;
        };
        let Mir::Str(name_sym) = &self.nodes.as_slice()[usize::from(ids[1])] else {
            return None;
        };
        let inv_name = match name_sym.0.as_str() {
            "equals" => "not_equals",
            "not_equals" => "equals",
            "greater" => "less_or_equals",
            "less" => "greater_or_equals",
            "greater_or_equals" => "less",
            "less_or_equals" => "greater",
            _ => return None,
        };
        let inv_name_id = self.str_id(inv_name);
        let mut new_ids = ids.to_vec();
        new_ids[1] = inv_name_id;
        Some(self.add(Mir::Action(new_ids.into_boxed_slice())))
    }

    #[instrument(skip(self), level = "trace")]
    fn make_if_else(&mut self, cond: Id, then_block: Id, else_block: Option<Id>) -> Id {
        let true_val = self.add(Mir::Num(1.0.into()));
        let if_action = self.cmp_action("equals", cond, true_val, then_block);
        let mut ops = vec![if_action];
        if let Some(else_b) = else_block {
            if ends_with_terminator(self.nodes.as_slice(), then_block) {
                if let Mir::Block(children) = &self.nodes.as_slice()[usize::from(else_b)].clone() {
                    ops.extend_from_slice(children);
                } else {
                    ops.push(else_b);
                }
            } else {
                ops.push(self.else_action(else_b));
            }
        }
        match ops.as_slice() {
            [single] => *single,
            _ => self.add(Mir::Block(ops.into_boxed_slice())),
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn peel_atomic_let_block(
        &mut self,
        mut curr: Id,
        local_substs: &mut Vec<Id>,
        local_named_substs: &mut Vec<((ScopeKind, Symbol), Option<Id>)>,
    ) -> Id {
        loop {
            let mut progressed = false;
            while let Hir::Let([var, val, body]) = &self.hir[curr] {
                if self.is_atomic_hir(*val) {
                    self.var_subst.insert(*var, *val);
                    local_substs.push(*var);
                    if let Some(key) = self.extract_var_key(*var) {
                        let old = self.named_var_subst.insert(key, *val);
                        local_named_substs.push((key, old));
                    }
                    curr = *body;
                    progressed = true;
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
                progressed = true;
            }

            if !progressed {
                break;
            }
        }

        curr
    }

    #[instrument(skip(self), level = "trace")]
    fn peel_cond_lets(
        &mut self,
        mut curr: Id,
        local_substs: &mut Vec<Id>,
        local_named_substs: &mut Vec<((ScopeKind, Symbol), Option<Id>)>,
        pre_ops: &mut Vec<Id>,
    ) -> Result<Id, MirError> {
        loop {
            let mut progressed = false;
            while let Hir::Let([var, val, body]) = &self.hir[curr] {
                let var = *var;
                let val = *val;
                let body = *body;
                if self.is_atomic_hir(val) {
                    self.var_subst.insert(var, val);
                    local_substs.push(var);
                    if let Some(key) = self.extract_var_key(var) {
                        let old = self.named_var_subst.insert(key, val);
                        local_named_substs.push((key, old));
                    }
                } else {
                    let set_op = self.lower_set(var, val)?;
                    pre_ops.push(set_op);
                }
                curr = body;
                progressed = true;
            }

            if let Hir::Block(ids) = &self.hir[curr]
                && ids.len() == 2
                && let (Hir::Set([ret1, expr]), ret2) = (&self.hir[ids[0]], ids[1])
                && *ret1 == ret2
            {
                curr = *expr;
                progressed = true;
            }

            if !progressed {
                break;
            }
        }

        Ok(curr)
    }

    #[instrument(skip(self), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        reason = "Comprehensive matching of HIR condition variants and dual operators"
    )]
    fn lower_condition_action(&mut self, cond_hir: Id) -> Result<Option<(Id, bool)>, MirError> {
        let mut local_substs = Vec::new();
        let mut local_named_substs = Vec::new();
        let curr = self.peel_atomic_let_block(cond_hir, &mut local_substs, &mut local_named_substs);

        let res = match &self.hir[curr] {
            Hir::Not(inner) => {
                let inner_peeled =
                    self.peel_atomic_let_block(*inner, &mut local_substs, &mut local_named_substs);
                match &self.hir[inner_peeled] {
                    Hir::Eq([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("not_equals", a, b, nop), false)))
                    }
                    Hir::Ne([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("equals", a, b, nop), false)))
                    }
                    Hir::Lt([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((
                            self.cmp_action("greater_or_equals", a, b, nop),
                            false,
                        )))
                    }
                    Hir::Le([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("greater", a, b, nop), false)))
                    }
                    Hir::Gt([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("less_or_equals", a, b, nop), false)))
                    }
                    Hir::Ge([a, b]) => {
                        let a = self.lower_expr(*a)?;
                        let b = self.lower_expr(*b)?;
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("less", a, b, nop), false)))
                    }
                    Hir::Line(_) | Hir::Local(_) | Hir::Game(_) | Hir::Save(_) | Hir::Var(_) => {
                        let var = self.lower_expr(inner_peeled)?;
                        let zero = self.add(Mir::Num(0.0.into()));
                        let nop = self.nop();
                        Ok(Some((self.cmp_action("equals", var, zero, nop), false)))
                    }
                    _ => {
                        if let Some((act, is_inv)) = self.lower_condition_action(*inner)? {
                            if !is_inv && let Some(inv_act) = self.try_invert_action(act) {
                                Ok(Some((inv_act, false)))
                            } else {
                                Ok(Some((act, !is_inv)))
                            }
                        } else {
                            Ok(None)
                        }
                    }
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
                Ok(Some((self.cmp_action("not_equals", a, b, nop), false)))
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
            Hir::Line(_) | Hir::Local(_) | Hir::Game(_) | Hir::Save(_) | Hir::Var(_) => {
                let var = self.lower_expr(curr)?;
                let one = self.add(Mir::Num(1.0.into()));
                let nop = self.nop();
                Ok(Some((self.cmp_action("equals", var, one, nop), false)))
            }
            Hir::Action(ids) if is_bool_action(self.hir, ids)? => {
                let act = self.lower_action(ids, true)?;
                Ok(Some((act, false)))
            }
            _ => Ok(None),
        };

        for var in local_substs {
            self.var_subst.remove(&var);
        }
        for (key, old) in local_named_substs.into_iter().rev() {
            if let Some(prev) = old {
                self.named_var_subst.insert(key, prev);
            } else {
                self.named_var_subst.remove(&key);
            }
        }

        res
    }

    #[instrument(skip(self), level = "trace")]
    fn lower_if(&mut self, cond: Id, then_ops: Id, else_ops: Option<Id>) -> Result<Id, MirError> {
        let mut local_substs = Vec::new();
        let mut local_named_substs = Vec::new();
        let mut pre_ops = Vec::new();
        let curr = self.peel_cond_lets(
            cond,
            &mut local_substs,
            &mut local_named_substs,
            &mut pre_ops,
        )?;

        let res = match &self.hir[curr] {
            Hir::And([left, right]) if else_ops.is_none() => {
                let left = *left;
                let right = *right;
                let inner = self.lower_if(right, then_ops, None)?;
                self.lower_if(left, inner, None)
            }
            Hir::Or([left, right])
                if else_ops.is_none() && ends_with_terminator(self.nodes.as_slice(), then_ops) =>
            {
                let left = *left;
                let right = *right;
                let first_if = self.lower_if(left, then_ops, None)?;
                let second_if = self.lower_if(right, then_ops, None)?;
                Ok(self.add(Mir::Block(vec![first_if, second_if].into_boxed_slice())))
            }
            _ => self.lower_if_leaf(curr, then_ops, else_ops),
        };

        for var in local_substs {
            self.var_subst.remove(&var);
        }
        for (key, old) in local_named_substs.into_iter().rev() {
            if let Some(prev) = old {
                self.named_var_subst.insert(key, prev);
            } else {
                self.named_var_subst.remove(&key);
            }
        }

        let res = res?;
        if pre_ops.is_empty() {
            Ok(res)
        } else {
            pre_ops.push(res);
            Ok(self.add(Mir::Block(pre_ops.into_boxed_slice())))
        }
    }

    fn lower_if_leaf(
        &mut self,
        cond: Id,
        then_ops: Id,
        else_ops: Option<Id>,
    ) -> Result<Id, MirError> {
        if let Some((cond_act, is_inverted)) = self.lower_condition_action(cond)? {
            let actual_then = if is_inverted {
                else_ops.unwrap_or_else(|| self.nop())
            } else {
                then_ops
            };
            let actual_else = if !is_inverted {
                else_ops
            } else {
                Some(then_ops)
            };
            let actual_else = actual_else
                .filter(|&id| !matches!(&self.nodes.as_slice()[usize::from(id)], Mir::Nop));
            if let Some(block_id) = self.conditional_bool_block(cond_act, actual_then, actual_else)
            {
                return Ok(block_id);
            }
        }

        let cond_mir = self.lower_expr(cond)?;
        let actual_else =
            else_ops.filter(|&id| !matches!(&self.nodes.as_slice()[usize::from(id)], Mir::Nop));
        Ok(self.make_if_else(cond_mir, then_ops, actual_else))
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
            if ends_with_terminator(self.nodes.as_slice(), actual_then) {
                if let Mir::Block(children) = &self.nodes.as_slice()[usize::from(else_b)].clone() {
                    ops.extend_from_slice(children);
                } else {
                    ops.push(else_b);
                }
            } else {
                ops.push(self.else_action(else_b));
            }
        }
        Some(match ops.as_slice() {
            [single] => *single,
            _ => self.add(Mir::Block(ops.into_boxed_slice())),
        })
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
                let val_id = self.bind_in_place_target(val_id, var_id).unwrap_or(val_id);
                let body_id = self.lower_stmt(*body)?;
                self.add(Mir::Let([var_id, val_id, body_id]))
            }
            Hir::If([c, t, el]) => {
                let then_ops = self.lower_stmt(*t)?;
                let else_ops = self.lower_stmt(*el)?;
                let else_opt = if is_empty_mir(self.nodes.as_slice(), else_ops) {
                    None
                } else {
                    Some(else_ops)
                };
                self.lower_if(*c, then_ops, else_opt)?
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
        let mut ref_params = HashSet::new();
        if let Hir::List(param_ids) = &self.hir[params] {
            for &p_id in param_ids.as_ref() {
                let p_name = match &self.hir[p_id] {
                    Hir::List(pair) if pair.len() >= 2 => extract_str_hir(self.hir, pair[0]).ok(),
                    _ => extract_str_hir(self.hir, p_id).ok(),
                };
                if let Some(p_name) = p_name {
                    let (name, is_ref) = crate::utils::split_param_ref(&p_name);
                    let (stripped, _) = crate::utils::split_param_spread(name);
                    if is_ref {
                        ref_params.insert(stripped.to_owned());
                    }
                }
            }
        }

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

        let prev_ref_params = std::mem::replace(&mut self.current_ref_params, ref_params);
        let body = self.lower_stmt(body);
        self.current_ref_params = prev_ref_params;
        let body = body?;
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
        match &self.hir[body] {
            Hir::Block(ids) => {
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
            Hir::FuncDecl(_) | Hir::ProcDecl(_) => self.lower_expr(body),
            _ => Ok(self.nop()),
        }
    }

    pub(super) fn lower_let(&mut self, [variable, value, body]: [Id; 3]) -> Result<Id, MirError> {
        let variable = self.lower_expr(variable)?;
        let value = self.lower_expr(value)?;
        let value = self.bind_in_place_target(value, variable).unwrap_or(value);
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
            let set = self
                .bind_in_place_target(value, ret)
                .unwrap_or_else(|| self.make_set_var(ret, value));
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

fn is_empty_mir(nodes: &[Mir], id: Id) -> bool {
    match &nodes[usize::from(id)] {
        Mir::Nop => true,
        Mir::Block(children) => {
            children.is_empty() || children.iter().all(|c| is_empty_mir(nodes, *c))
        }
        _ => false,
    }
}

fn ends_with_terminator(nodes: &[Mir], id: Id) -> bool {
    match &nodes[usize::from(id)] {
        Mir::ReturnFunc(_) | Mir::Break | Mir::Continue => true,
        Mir::Block(children) => {
            if let Some(&last) = children.iter().rev().find(|&&c| !is_empty_mir(nodes, c)) {
                ends_with_terminator(nodes, last)
            } else {
                false
            }
        }
        Mir::Let([_, _, body]) => ends_with_terminator(nodes, *body),
        _ => false,
    }
}
