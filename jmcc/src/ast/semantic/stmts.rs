//! Statements: var/assign/return/if/block, reassigning implicit variables,
//! and distributing multiple assignments across action slots.

use super::*;

impl Analyzer<'_> {
    #[instrument(skip(self, stmt), level = "trace")]
    pub(super) fn analyze_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VarDecl(d) => self.analyze_var_decl(d),
            Statement::Assign(a) => self.analyze_assign(a),
            Statement::Return(r) => self.analyze_return(r),
            Statement::While(w) => self.analyze_while(w),
            Statement::For(f) => self.analyze_for(f),
            Statement::Break(b) => {
                if self.loop_depth == 0 {
                    self.error(SemanticErrorKind::BreakOutsideLoop, b.span.clone());
                } else if let Some(target_label) = b.label
                    && !self.loop_labels.contains(&Some(target_label))
                {
                    let name = self.ast.strings.resolve(&target_label).to_owned();
                    self.error(SemanticErrorKind::UnknownLoopLabel { name }, b.span.clone());
                }
            }
            Statement::Continue(c) => {
                if self.loop_depth == 0 {
                    self.error(SemanticErrorKind::ContinueOutsideLoop, c.span.clone());
                } else if let Some(target_label) = c.label
                    && !self.loop_labels.contains(&Some(target_label))
                {
                    let name = self.ast.strings.resolve(&target_label).to_owned();
                    self.error(SemanticErrorKind::UnknownLoopLabel { name }, c.span.clone());
                }
            }
            Statement::If(i) => self.analyze_if(i),
            Statement::Match(m) => self.analyze_match_stmt(m),
            Statement::TryCatch(tc) => self.analyze_try_catch(tc),
            Statement::Throw(th) => self.analyze_throw(th),
            Statement::Function(f) => self.analyze_function(f),
            Statement::Process(p) => self.analyze_process(p),
            Statement::Event(e) => self.analyze_event(e),
            Statement::Class(c) => self.analyze_class(c),
            Statement::Interface(i) => self.analyze_interface(i),
            Statement::Enum(_) | Statement::Import(_) | Statement::TypeAlias(_) => {}
            Statement::Expr(eid) => {
                let prev = self.statement_context;
                let expr = &self.ast.exprs[*eid];
                self.statement_context = matches!(expr, Expr::Action(..) | Expr::Call(..));
                self.analyze_expr(*eid);
                self.statement_context = prev;
            }
            Statement::Cfg(_, inner) => self.analyze_stmt(inner),
        }
    }

    #[instrument(skip(self, d), level = "trace")]
    fn analyze_var_decl(&mut self, d: &VarDecl) {
        let val_ty = match d.value {
            Some(v) => self.analyze_expr(v),
            None => self.unifier.new_var(),
        };

        // Destructuring spreads the value over the action's slots, otherwise over array elements.
        let slot_types = (d.names.len() > 1)
            .then(|| self.multi_assign_slot_types(d.value?))
            .flatten()
            .filter(|slots| d.names.len() <= slots.len());

        let elem_ty = if d.names.len() > 1 {
            if let Type::Class(id, args) = &val_ty {
                let array_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
                if *id == array_id {
                    args.first().cloned().unwrap_or(Type::Unknown)
                } else {
                    Type::Unknown
                }
            } else {
                Type::Unknown
            }
        } else {
            val_ty
        };

        for i in 0..d.names.len() {
            let name = text_value_to_string(self.ast, &d.names[i]);
            let declared_ty = d
                .tys
                .get(i)
                .copied()
                .flatten()
                .map(|t| self.parse_decl_type(&self.str(t), d.span.clone()));

            let name_ty = slot_types
                .as_ref()
                .map_or_else(|| elem_ty.clone(), |slots| slots[i].clone());

            let final_ty = declared_ty.as_ref().map_or_else(
                || name_ty.clone(),
                |decl_ty| {
                    self.require_type(&name_ty, decl_ty, d.span.clone(), |expected, actual| {
                        SemanticErrorKind::TypeMismatchVarDecl { expected, actual }
                    });
                    decl_ty.clone()
                },
            );

            let scope = d
                .scopes
                .get(i)
                .copied()
                .flatten()
                .unwrap_or(self.default_scope);

            if scope == VarScope::Inline {
                self.declare(
                    name,
                    Symbol::Var {
                        ty: final_ty,
                        scope: VarScope::Inline,
                        declared: declared_ty.is_some(),
                    },
                );
                continue;
            }

            if let Some(existing) = self.scopes.last().and_then(|s| s.get(&name))
                && !matches!(existing, Symbol::Var { .. })
            {
                self.error(
                    SemanticErrorKind::AlreadyDeclared { name: name.clone() },
                    d.span.clone(),
                );
            }
            if self.callable_depth > 0
                && !name.starts_with('_')
                && !name.contains('%')
                && scope != VarScope::Save
                && scope != VarScope::Game
            {
                self.var_declarations.push(VarDeclRecord {
                    name: name.clone(),
                    span: d.span.clone(),
                    scope,
                    used: false,
                    callable_depth: self.callable_depth,
                });
            }
            self.declare(
                name,
                Symbol::Var {
                    ty: final_ty,
                    scope,
                    declared: declared_ty.is_some(),
                },
            );
        }
    }

    /// Types of the action's `assign` slots, if the expression is a call with several of them.
    ///
    /// Destructuring fills the slots in declaration order, so a target takes the type of its
    /// slot rather than of the whole returned array. An `any` slot yields `Unknown` — binding
    /// to the `any` class would break every later method call on the variable.
    fn multi_assign_slot_types(&self, eid: ExprId) -> Option<Vec<Type>> {
        let def = self.action_def_of_expr(eid)?;
        let assigns = def.assign?;
        if assigns.len() < 2 {
            return None;
        }
        Some(
            assigns
                .iter()
                .map(|arg| {
                    if arg.arg_type == "any" {
                        Type::Unknown
                    } else {
                        self.type_from_action_arg(arg.arg_type, false)
                    }
                })
                .collect(),
        )
    }

    /// Re-binds the type of an undeclared variable to the value being assigned.
    ///
    /// A platform variable is an untyped slot, so `northDir = value::eye_location` is a plain
    /// `set_value`, not a typed declaration: exported projects reuse one variable for values of
    /// different kinds. An annotated variable (`var x: number = 0`) keeps its type and the
    /// assignment is still checked.
    ///
    /// Returns `true` if the variable was rebound and the assignment needs no further check.
    fn retype_implicit_var(&mut self, target: ExprId, val_ty: &Type) -> bool {
        let Some(name) = self.action_variable_name(target) else {
            return false;
        };
        let Some(Symbol::Var {
            ty,
            scope,
            declared: false,
        }) = self.lookup(&name)
        else {
            return false;
        };

        let current = self.unifier.find(&ty);
        let actual = self.unifier.find(val_ty);
        // An unresolved type goes through the normal path, where `require_type` unifies the vars.
        let is_open = |ty: &Type| {
            matches!(
                ty,
                Type::Unknown | Type::InferVar(_) | Type::Param(_) | Type::Never
            )
        };
        if is_open(&current) || is_open(&actual) || self.is_assignable_to(&actual, &current) {
            return false;
        }

        // A fresh var unifies with anything, so the error is unreachable.
        let fresh = self.unifier.new_var();
        let _unify = self.unifier.unify(&fresh, &actual);
        self.declare(
            name,
            Symbol::Var {
                ty: fresh,
                scope,
                declared: false,
            },
        );
        true
    }

    #[instrument(skip(self, a), level = "trace")]
    fn analyze_assign(&mut self, a: &AssignStmt) {
        let val_ty = self.analyze_expr(a.value);
        let slot_types = self.multi_assign_slot_types(a.value);
        let op_str = a.op.map(|s| self.str(s)).unwrap_or_default();
        let is_simple = op_str == "=";
        for (i, &target) in a.targets.iter().enumerate() {
            // Assignment introduces an inferred variable before its target is analyzed. The
            // scope comes from the target: `y = 5` takes the default one, `g"y" = 5` the
            // written one, without which the target would look undeclared.
            if let Some(name_str) = self.action_variable_name(target)
                && self.lookup(&name_str).is_none()
            {
                let scope = match &self.ast.exprs[target] {
                    Expr::Variable(variable) => variable.scope,
                    _ => self.default_scope,
                };
                let new_var = self.unifier.new_var();
                self.declare(
                    name_str,
                    Symbol::Var {
                        ty: new_var,
                        scope,
                        declared: false,
                    },
                );
            }

            let target_ty = self.analyze_expr(target);

            // A destructured name gets its slot's type, any other name the whole expression's.
            let expected_ty = match &slot_types {
                Some(slots) if a.targets.len() <= slots.len() => slots[i].clone(),
                _ => val_ty.clone(),
            };

            if is_simple && self.retype_implicit_var(target, &expected_ty) {
                continue;
            }

            if is_simple {
                self.require_type(
                    &expected_ty,
                    &target_ty,
                    a.span.clone(),
                    |target, actual| SemanticErrorKind::TypeMismatchAssign { target, actual },
                );
            } else {
                let dunders = compound_assign_dunders(&op_str);
                let dunder_result = dunders.and_then(|(i_op, op)| {
                    if self.has_binary_dunder(i_op, &target_ty) {
                        self.lookup_binary_dunder(i_op, &target_ty, &val_ty, &a.span)
                    } else if self.has_binary_dunder(op, &target_ty) {
                        self.lookup_binary_dunder(op, &target_ty, &val_ty, &a.span)
                    } else {
                        None
                    }
                });

                if let Some(res_ty) = dunder_result {
                    self.require_type(&res_ty, &target_ty, a.span.clone(), |target, actual| {
                        SemanticErrorKind::TypeMismatchAssign { target, actual }
                    });
                } else {
                    if !self.is_numeric(&val_ty) {
                        self.error(
                            SemanticErrorKind::CompoundAssignRhs {
                                op: op_str.clone(),
                                actual: self.format_type(&val_ty),
                            },
                            a.span.clone(),
                        );
                    }
                    if !self.is_numeric(&target_ty) {
                        self.error(
                            SemanticErrorKind::CompoundAssignTarget {
                                op: op_str.clone(),
                                target: self.format_type(&target_ty),
                            },
                            a.span.clone(),
                        );
                    }
                }
            }
        }
    }

    #[instrument(skip(self, r), level = "trace")]
    fn analyze_return(&mut self, r: &ReturnStmt) {
        if !self.in_function && !self.in_process && self.current_event.is_none() {
            self.error(SemanticErrorKind::ReturnOutsideCallable, r.span.clone());
            return;
        }
        if let Some(val) = r.value {
            let val_ty = self.analyze_expr(val);
            if self.in_function {
                if let Some(ret_ty) = self.function_return_type.clone() {
                    self.require_type(&val_ty, &ret_ty, r.span.clone(), |expected, actual| {
                        SemanticErrorKind::TypeMismatchReturn { expected, actual }
                    });
                }
            } else if self.in_process {
                self.error(SemanticErrorKind::ReturnFromProcess, r.span.clone());
            } else {
                self.error(SemanticErrorKind::ReturnFromEvent, r.span.clone());
            }
        } else if self.in_function && self.function_return_type.is_some() {
            self.error(SemanticErrorKind::MissingReturnValue, r.span.clone());
        }
    }

    #[instrument(skip(self, i), level = "trace")]
    fn analyze_if(&mut self, i: &IfStmt) {
        for span in &i.elif_spans {
            self.warning(SemanticErrorKind::DeprecatedElif, span.clone());
        }
        let cond_ty = self.analyze_expr(i.condition);
        if !self.is_truthy(&cond_ty) {
            self.error(
                SemanticErrorKind::InvalidCondition {
                    actual: self.format_type(&cond_ty),
                },
                i.span.clone(),
            );
        }
        self.analyze_block(&i.then_body);
        for (cond, body) in &i.elif_branches {
            let ec_ty = self.analyze_expr(*cond);
            if !self.is_truthy(&ec_ty) {
                self.error(
                    SemanticErrorKind::InvalidElifCondition {
                        actual: self.format_type(&ec_ty),
                    },
                    i.span.clone(),
                );
            }
            self.analyze_block(body);
        }
        if let Some(body) = &i.else_body {
            self.analyze_block(body);
        }
    }

    fn analyze_while(&mut self, w: &WhileStmt) {
        let cond_ty = self.analyze_expr(w.condition);
        if !self.is_truthy(&cond_ty) {
            self.error(
                SemanticErrorKind::InvalidCondition {
                    actual: self.format_type(&cond_ty),
                },
                w.span.clone(),
            );
        }
        let previous_depth = self.loop_depth;
        self.loop_depth += 1;
        self.loop_labels.push(w.label);

        self.check_loop_invariants(&w.body, &[]);
        if w.body.is_empty() && !self.is_std_span(&w.span) {
            self.warning(SemanticErrorKind::DeadLoop, w.span.clone());
        }
        self.analyze_block(&w.body);

        self.loop_labels.pop();
        self.loop_depth = previous_depth;
    }

    fn resolve_iterable_types(&mut self, iterable_ty: &Type) -> Option<(Type, Option<Type>)> {
        let array_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
        let map_id = self.ir_ctx.lang_items.get("map").copied().unwrap_or(0);
        let range_id = self.ir_ctx.lang_items.get("Range").copied().unwrap_or(0);
        let range_inc_id = self
            .ir_ctx
            .lang_items
            .get("RangeInclusive")
            .copied()
            .unwrap_or(0);
        let array_iter_id = self
            .ir_ctx
            .lang_items
            .get("ArrayIterator")
            .copied()
            .or_else(|| {
                self.ir_ctx
                    .get_class_by_name("ArrayIterator")
                    .map(|c| c.def_id)
            })
            .unwrap_or(0);
        let map_iter_id = self
            .ir_ctx
            .lang_items
            .get("MapKeyIterator")
            .copied()
            .or_else(|| {
                self.ir_ctx
                    .get_class_by_name("MapKeyIterator")
                    .map(|c| c.def_id)
            })
            .unwrap_or(0);

        match iterable_ty {
            Type::Class(id, args) if *id == array_id || (*id != 0 && *id == array_iter_id) => {
                Some((args.first().cloned().unwrap_or(Type::Unknown), None))
            }
            Type::Class(id, args) if *id == map_id || (*id != 0 && *id == map_iter_id) => {
                let key = args.first().cloned();
                let val = args.get(1).cloned().unwrap_or(Type::Unknown);
                Some((val, key))
            }
            Type::Class(id, _)
                if (*id != 0 && *id == range_id) || (*id != 0 && *id == range_inc_id) =>
            {
                Some((self.lang_type("number", vec![]), None))
            }
            Type::Class(def_id, args) => {
                let class_info = self.ir_ctx.classes_by_def.get(def_id)?;
                for (iface_id, iface_args) in &class_info.implements {
                    if let Some(iface_info) = self.ir_ctx.classes_by_def.get(iface_id) {
                        let name = iface_info
                            .name
                            .rsplit("::")
                            .next()
                            .unwrap_or(&iface_info.name);
                        if name == "Iterator"
                            || name == "Iterable"
                            || name == "Итератор"
                            || name == "Итерируемый"
                        {
                            let elem = iface_args.first().cloned().unwrap_or(Type::Unknown);
                            return Some((elem, None));
                        }
                    }
                }
                let name = class_info
                    .name
                    .rsplit("::")
                    .next()
                    .unwrap_or(&class_info.name);
                if name == "Iterator"
                    || name == "Iterable"
                    || name == "Итератор"
                    || name == "Итерируемый"
                {
                    let elem = args.first().cloned().unwrap_or(Type::Unknown);
                    return Some((elem, None));
                }
                if let Some(method) = class_info
                    .methods
                    .get("iter")
                    .or_else(|| class_info.methods.get("into_iter"))
                    && let Some(ret_str_id) = method.return_type
                {
                    let ret_str = self.str(ret_str_id);
                    let ret_ty = self.parse_decl_type(&ret_str, method.span.clone());
                    if &ret_ty != iterable_ty {
                        return self.resolve_iterable_types(&ret_ty);
                    }
                }
                if let Some(next_method) = class_info.methods.get("next")
                    && let Some(ret_str_id) = next_method.return_type
                {
                    let ret_str = self.str(ret_str_id);
                    let ret_ty = self.parse_decl_type(&ret_str, next_method.span.clone());
                    return Some((ret_ty, None));
                }
                None
            }
            _ => None,
        }
    }

    fn analyze_for(&mut self, f: &ForStmt) {
        let iterable_ty = self.analyze_expr(f.iterable);
        let any_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);

        let (elem_ty, key_ty) = if let Some(types) = self.resolve_iterable_types(&iterable_ty) {
            types
        } else {
            match &iterable_ty {
                Type::Unknown | Type::InferVar(_) => (Type::Unknown, None),
                Type::Class(id, _) if *id == any_id => (Type::Unknown, None),
                _ => {
                    self.error(
                        SemanticErrorKind::NotIterable {
                            ty: self.format_type(&iterable_ty),
                        },
                        self.ast.exprs[f.iterable].span(),
                    );
                    (Type::Unknown, None)
                }
            }
        };

        let previous_depth = self.loop_depth;
        self.loop_depth += 1;
        self.loop_labels.push(f.label);

        self.push_scope();
        if f.vars.len() == 1 {
            let name = text_value_to_string(self.ast, &f.vars[0]);
            let var_ty = key_ty.unwrap_or(elem_ty);
            let scope = f
                .scopes
                .first()
                .copied()
                .flatten()
                .unwrap_or(self.default_scope);
            self.declare(
                name,
                Symbol::Var {
                    ty: var_ty,
                    scope,
                    declared: true,
                },
            );
        } else if f.vars.len() >= 2 {
            let first_name = text_value_to_string(self.ast, &f.vars[0]);
            let second_name = text_value_to_string(self.ast, &f.vars[1]);
            let first_scope = f
                .scopes
                .first()
                .copied()
                .flatten()
                .unwrap_or(self.default_scope);
            let second_scope = f
                .scopes
                .get(1)
                .copied()
                .flatten()
                .unwrap_or(self.default_scope);
            let first_ty = key_ty.unwrap_or_else(|| self.lang_type("number", vec![]));
            self.declare(
                first_name,
                Symbol::Var {
                    ty: first_ty,
                    scope: first_scope,
                    declared: true,
                },
            );
            self.declare(
                second_name,
                Symbol::Var {
                    ty: elem_ty,
                    scope: second_scope,
                    declared: true,
                },
            );
        }

        let for_var_names: Vec<String> = f
            .vars
            .iter()
            .map(|v| text_value_to_string(self.ast, v))
            .collect();
        self.check_loop_invariants(&f.body, &for_var_names);
        if f.body.is_empty() && !self.is_std_span(&f.span) {
            self.warning(SemanticErrorKind::DeadLoop, f.span.clone());
        }

        self.predeclare_in_block(&f.body);
        for s in &f.body {
            self.analyze_stmt(s);
        }
        self.pop_scope();

        self.loop_labels.pop();
        self.loop_depth = previous_depth;
    }

    fn analyze_match_stmt(&mut self, m: &MatchStmt) {
        let _scrutinee = self.analyze_expr(m.expr);
        self.analyze_match_arms(&m.arms);
    }

    pub(super) fn analyze_match_arms(&mut self, arms: &[MatchArm]) {
        for arm in arms {
            for &pattern in &arm.patterns {
                let _: Type = self.analyze_expr(pattern);
            }
            if let Some(guard) = arm.guard {
                let guard_ty = self.analyze_expr(guard);
                if !self.is_truthy(&guard_ty) {
                    self.error(
                        SemanticErrorKind::InvalidCondition {
                            actual: self.format_type(&guard_ty),
                        },
                        self.ast.exprs[guard].span(),
                    );
                }
            }
            self.analyze_block(&arm.body);
        }
    }

    fn analyze_try_catch(&mut self, tc: &TryCatchStmt) {
        self.analyze_block(&tc.try_body);
        self.push_scope();
        if let Some(var) = &tc.catch_var {
            let name = text_value_to_string(self.ast, var);
            let ty = self.lang_type("text", vec![]);
            self.declare(
                name,
                Symbol::Var {
                    ty,
                    scope: self.default_scope,
                    declared: true,
                },
            );
        }
        self.predeclare_in_block(&tc.catch_body);
        for s in &tc.catch_body {
            self.analyze_stmt(s);
        }
        self.pop_scope();
    }

    fn analyze_throw(&mut self, th: &ThrowStmt) {
        if let Some(v) = th.value {
            let _: Type = self.analyze_expr(v);
        }
    }

    #[instrument(skip(self, body), level = "trace")]
    pub(super) fn analyze_block(&mut self, body: &[Statement]) {
        self.push_scope();
        self.predeclare_in_block(body);
        self.analyze_block_statements(body);
        self.pop_scope();
    }

    #[instrument(skip(self, body), level = "trace")]
    pub(super) fn analyze_block_statements(&mut self, body: &[Statement]) {
        let mut pending_writes: HashMap<String, Span> = HashMap::new();

        for s in body {
            // 1. Mark variables read in this statement as used/read (cancels pending overwritten warning)
            let mut reads = HashSet::new();
            collect_read_vars_in_stmt(self.ast, s, &mut reads);
            for r in &reads {
                pending_writes.remove(r);
            }

            // 2. Check for overwritten assignments before read
            match s {
                Statement::VarDecl(d) => {
                    if d.value.is_some() {
                        for name_tv in &d.names {
                            let name = text_value_to_string(self.ast, name_tv);
                            if let Some(prev_span) = pending_writes.remove(&name)
                                && !self.callable_has_dynamic.last().copied().unwrap_or(false)
                                && !name.starts_with('_')
                                && !name.contains('%')
                                && !self.is_std_span(&d.span)
                            {
                                self.warning(
                                    SemanticErrorKind::OverwrittenAssignment { name: name.clone() },
                                    prev_span,
                                );
                            }
                            pending_writes.insert(name, d.span.clone());
                        }
                    }
                }
                Statement::Assign(a) => {
                    let op_str = a.op.map(|s| self.str(s)).unwrap_or_default();
                    if op_str == "=" {
                        for &target in &a.targets {
                            if let Some(name) = self.action_variable_name(target) {
                                if let Some(prev_span) = pending_writes.remove(&name)
                                    && !self.callable_has_dynamic.last().copied().unwrap_or(false)
                                    && !name.starts_with('_')
                                    && !name.contains('%')
                                    && !self.is_std_span(&a.span)
                                {
                                    self.warning(
                                        SemanticErrorKind::OverwrittenAssignment {
                                            name: name.clone(),
                                        },
                                        prev_span,
                                    );
                                }
                                pending_writes.insert(name, a.span.clone());
                            }
                        }
                    }
                }
                Statement::If(_)
                | Statement::While(_)
                | Statement::For(_)
                | Statement::Match(_)
                | Statement::TryCatch(_) => {
                    pending_writes.clear();
                }
                Statement::Expr(eid) => {
                    if matches!(self.ast.exprs[*eid], Expr::Call(..) | Expr::Action(..)) {
                        pending_writes.clear();
                    }
                }
                _ => {}
            }

            self.analyze_stmt(s);
        }

        // Check for consecutive if statements with identical condition
        for w in body.windows(2) {
            if let (Statement::If(if1), Statement::If(if2)) = (&w[0], &w[1])
                && if1.elif_branches.is_empty()
                && if1.else_body.is_none()
                && if2.elif_branches.is_empty()
                && exprs_structurally_equal(self.ast, if1.condition, if2.condition)
                && !self.callable_has_dynamic.last().copied().unwrap_or(false)
                && !self.is_std_span(&if2.span)
            {
                let mut cond_reads = HashSet::new();
                collect_read_vars_in_expr(self.ast, if1.condition, &mut cond_reads);
                let mut then_mods = HashSet::new();
                collect_modified_vars_in_body(self.ast, &if1.then_body, &mut then_mods);
                if cond_reads.is_disjoint(&then_mods) {
                    self.warning(
                        SemanticErrorKind::ConsecutiveIfSameCondition,
                        if2.span.clone(),
                    );
                }
            }
        }
    }

    fn check_loop_invariants(&mut self, body: &[Statement], extra_modified: &[String]) {
        if self.callable_has_dynamic.last().copied().unwrap_or(false) {
            return;
        }
        let mut modified = HashSet::new();
        modified.extend(extra_modified.iter().cloned());
        collect_modified_vars_in_body(self.ast, body, &mut modified);

        let mut declared_in_loop = HashSet::new();
        for s in body {
            if let Statement::VarDecl(d) = s {
                if let Some(val_eid) = d.value
                    && is_pure_computation(self.ast, val_eid)
                {
                    let mut reads = HashSet::new();
                    collect_read_vars_in_expr(self.ast, val_eid, &mut reads);
                    // If all reads are outside the loop and not modified in the loop:
                    if !reads.is_empty()
                        && reads
                            .iter()
                            .all(|r| !modified.contains(r) && !declared_in_loop.contains(r))
                        && !self.is_std_span(&d.span)
                    {
                        for name_tv in &d.names {
                            let name = text_value_to_string(self.ast, name_tv);
                            if !name.starts_with('_')
                                && !name.contains('%')
                                && !self.is_std_span(&d.span)
                            {
                                self.warning(
                                    SemanticErrorKind::LoopInvariant { name },
                                    d.span.clone(),
                                );
                            }
                        }
                    }
                }
                for name_tv in &d.names {
                    declared_in_loop.insert(text_value_to_string(self.ast, name_tv));
                }
            } else if let Statement::If(i) = s
                && is_pure_computation(self.ast, i.condition)
            {
                let mut reads = HashSet::new();
                collect_read_vars_in_expr(self.ast, i.condition, &mut reads);
                if !reads.is_empty()
                    && reads
                        .iter()
                        .all(|r| !modified.contains(r) && !declared_in_loop.contains(r))
                    && !self.is_std_span(&i.span)
                {
                    self.warning(SemanticErrorKind::LoopInvariantBranch, i.span.clone());
                }
            }
        }
    }
}

fn is_pure_computation(ast: &Ast, eid: ExprId) -> bool {
    match &ast.exprs[eid] {
        Expr::Binary(b) => {
            !matches!(b.op, BinOp::Assign)
                && is_pure_expr(ast, b.left)
                && is_pure_expr(ast, b.right)
        }
        Expr::Unary(u) => !matches!(u.op, UnOp::Inc | UnOp::Dec) && is_pure_expr(ast, u.operand),
        Expr::Ternary(t) => {
            is_pure_expr(ast, t.cond)
                && is_pure_expr(ast, t.then_val)
                && is_pure_expr(ast, t.else_val)
        }
        Expr::Subscript(s) => {
            is_pure_expr(ast, s.object)
                && is_pure_expr(ast, s.index)
                && s.end.is_none_or(|e| is_pure_expr(ast, e))
        }
        _ => false,
    }
}

fn is_pure_expr(ast: &Ast, eid: ExprId) -> bool {
    match &ast.exprs[eid] {
        Expr::Number(_) | Expr::Bool(_) | Expr::Ident(..) | Expr::Variable(_) | Expr::Nbt(_) => {
            true
        }
        Expr::Text(t) => t.parts.iter().all(|p| match p {
            TextPart::Literal(_) => true,
            TextPart::Interp(e) => is_pure_expr(ast, *e),
        }),
        Expr::List(l) => l.values.iter().all(|&v| is_pure_expr(ast, v)),
        Expr::Map(m) => {
            m.keys.iter().all(|&k| is_pure_expr(ast, k))
                && m.values.iter().all(|&v| is_pure_expr(ast, v))
        }
        Expr::Binary(b) => {
            !matches!(b.op, BinOp::Assign)
                && is_pure_expr(ast, b.left)
                && is_pure_expr(ast, b.right)
        }
        Expr::Unary(u) => !matches!(u.op, UnOp::Inc | UnOp::Dec) && is_pure_expr(ast, u.operand),
        Expr::Ternary(t) => {
            is_pure_expr(ast, t.cond)
                && is_pure_expr(ast, t.then_val)
                && is_pure_expr(ast, t.else_val)
        }
        Expr::Property(p) => is_pure_expr(ast, p.object),
        Expr::Subscript(s) => {
            is_pure_expr(ast, s.object)
                && is_pure_expr(ast, s.index)
                && s.end.is_none_or(|e| is_pure_expr(ast, e))
        }
        Expr::Cast(c) => is_pure_expr(ast, c.expr),
        _ => false,
    }
}

fn collect_read_vars_in_text(ast: &Ast, t: &TextValue, out: &mut HashSet<String>) {
    for part in &t.parts {
        match part {
            TextPart::Literal(s) => {
                let text = ast.strings.resolve(s);
                if text.contains('$') {
                    drop(crate::ir::builder::expand_dollar_vars(
                        text,
                        |var_name, _, _| {
                            out.insert(var_name.to_owned());
                        },
                    ));
                }
            }
            TextPart::Interp(ieid) => {
                collect_read_vars_in_expr(ast, *ieid, out);
            }
        }
    }
}

fn collect_read_vars_in_expr(ast: &Ast, eid: ExprId, out: &mut HashSet<String>) {
    match &ast.exprs[eid] {
        Expr::Variable(v) => {
            let name = text_value_to_string(ast, &v.name);
            out.insert(name);
            for part in &v.name.parts {
                if let TextPart::Interp(ieid) = part {
                    collect_read_vars_in_expr(ast, *ieid, out);
                }
            }
        }
        Expr::Ident(s, _) => {
            out.insert(ast.strings.resolve(s).to_owned());
        }
        Expr::Text(t) => collect_read_vars_in_text(ast, t, out),
        Expr::Binary(b) => {
            collect_read_vars_in_expr(ast, b.left, out);
            collect_read_vars_in_expr(ast, b.right, out);
        }
        Expr::Unary(u) => {
            collect_read_vars_in_expr(ast, u.operand, out);
        }
        Expr::Ternary(t) => {
            collect_read_vars_in_expr(ast, t.cond, out);
            collect_read_vars_in_expr(ast, t.then_val, out);
            collect_read_vars_in_expr(ast, t.else_val, out);
        }
        Expr::List(l) => {
            for &v in &l.values {
                collect_read_vars_in_expr(ast, v, out);
            }
        }
        Expr::Map(m) => {
            for &k in &m.keys {
                collect_read_vars_in_expr(ast, k, out);
            }
            for &v in &m.values {
                collect_read_vars_in_expr(ast, v, out);
            }
        }
        Expr::Property(p) => {
            collect_read_vars_in_expr(ast, p.object, out);
        }
        Expr::Subscript(s) => {
            collect_read_vars_in_expr(ast, s.object, out);
            collect_read_vars_in_expr(ast, s.index, out);
            if let Some(e) = s.end {
                collect_read_vars_in_expr(ast, e, out);
            }
        }
        Expr::Call(c) => {
            collect_read_vars_in_expr(ast, c.target, out);
            for arg in &c.args {
                collect_read_vars_in_expr(ast, arg.value, out);
            }
        }
        Expr::Action(a) => {
            for arg in &a.args {
                collect_read_vars_in_expr(ast, arg.value, out);
            }
        }
        Expr::Constructor(c) => {
            for arg in &c.args {
                collect_read_vars_in_expr(ast, arg.value, out);
            }
        }
        Expr::Cast(c) => {
            collect_read_vars_in_expr(ast, c.expr, out);
        }
        Expr::Match(m) => {
            collect_read_vars_in_expr(ast, m.expr, out);
            for arm in &m.arms {
                for &pat in &arm.patterns {
                    collect_read_vars_in_expr(ast, pat, out);
                }
                if let Some(guard) = arm.guard {
                    collect_read_vars_in_expr(ast, guard, out);
                }
                for s in &arm.body {
                    collect_read_vars_in_stmt(ast, s, out);
                }
            }
        }
        Expr::Lambda(l) => match &l.body {
            LambdaBody::Expr(e) => collect_read_vars_in_expr(ast, *e, out),
            LambdaBody::Block(stmts) => {
                for s in stmts {
                    collect_read_vars_in_stmt(ast, s, out);
                }
            }
        },
        Expr::Number(_) | Expr::Bool(_) | Expr::Nbt(_) => {}
    }
}

pub(super) fn collect_read_vars_in_stmt(ast: &Ast, stmt: &Statement, out: &mut HashSet<String>) {
    match stmt {
        Statement::VarDecl(d) => {
            if let Some(v) = d.value {
                collect_read_vars_in_expr(ast, v, out);
            }
        }
        Statement::Assign(a) => {
            collect_read_vars_in_expr(ast, a.value, out);
            for &t in &a.targets {
                if let Expr::Subscript(s) = &ast.exprs[t] {
                    collect_read_vars_in_expr(ast, s.object, out);
                    collect_read_vars_in_expr(ast, s.index, out);
                    if let Some(e) = s.end {
                        collect_read_vars_in_expr(ast, e, out);
                    }
                } else if let Expr::Property(p) = &ast.exprs[t] {
                    collect_read_vars_in_expr(ast, p.object, out);
                }
            }
        }
        Statement::Return(r) => {
            if let Some(v) = r.value {
                collect_read_vars_in_expr(ast, v, out);
            }
        }
        Statement::While(w) => {
            collect_read_vars_in_expr(ast, w.condition, out);
            for s in &w.body {
                collect_read_vars_in_stmt(ast, s, out);
            }
        }
        Statement::For(f) => {
            collect_read_vars_in_expr(ast, f.iterable, out);
            for s in &f.body {
                collect_read_vars_in_stmt(ast, s, out);
            }
        }
        Statement::If(i) => {
            collect_read_vars_in_expr(ast, i.condition, out);
            for s in &i.then_body {
                collect_read_vars_in_stmt(ast, s, out);
            }
            for (c, b) in &i.elif_branches {
                collect_read_vars_in_expr(ast, *c, out);
                for s in b {
                    collect_read_vars_in_stmt(ast, s, out);
                }
            }
            if let Some(b) = &i.else_body {
                for s in b {
                    collect_read_vars_in_stmt(ast, s, out);
                }
            }
        }
        Statement::Match(m) => {
            collect_read_vars_in_expr(ast, m.expr, out);
            for arm in &m.arms {
                if let Some(g) = arm.guard {
                    collect_read_vars_in_expr(ast, g, out);
                }
                for s in &arm.body {
                    collect_read_vars_in_stmt(ast, s, out);
                }
            }
        }
        Statement::TryCatch(tc) => {
            for s in &tc.try_body {
                collect_read_vars_in_stmt(ast, s, out);
            }
            for s in &tc.catch_body {
                collect_read_vars_in_stmt(ast, s, out);
            }
        }
        Statement::Throw(th) => {
            if let Some(v) = th.value {
                collect_read_vars_in_expr(ast, v, out);
            }
        }
        Statement::Expr(e) => {
            collect_read_vars_in_expr(ast, *e, out);
        }
        _ => {}
    }
}

fn collect_modified_vars_in_body(ast: &Ast, body: &[Statement], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Statement::Assign(a) => {
                for &t in &a.targets {
                    match &ast.exprs[t] {
                        Expr::Variable(v) => {
                            out.insert(text_value_to_string(ast, &v.name));
                        }
                        Expr::Ident(id, _) => {
                            out.insert(ast.strings.resolve(id).to_owned());
                        }
                        _ => {}
                    }
                }
            }
            Statement::While(w) => collect_modified_vars_in_body(ast, &w.body, out),
            Statement::For(f) => {
                for v in &f.vars {
                    out.insert(text_value_to_string(ast, v));
                }
                collect_modified_vars_in_body(ast, &f.body, out);
            }
            Statement::If(i) => {
                collect_modified_vars_in_body(ast, &i.then_body, out);
                for (_, b) in &i.elif_branches {
                    collect_modified_vars_in_body(ast, b, out);
                }
                if let Some(b) = &i.else_body {
                    collect_modified_vars_in_body(ast, b, out);
                }
            }
            Statement::TryCatch(tc) => {
                collect_modified_vars_in_body(ast, &tc.try_body, out);
                collect_modified_vars_in_body(ast, &tc.catch_body, out);
            }
            Statement::Match(m) => {
                for arm in &m.arms {
                    collect_modified_vars_in_body(ast, &arm.body, out);
                }
            }
            _ => {}
        }
    }
}

const fn compound_assign_dunders(op: &str) -> Option<(&'static str, &'static str)> {
    if op.len() < 2 {
        return None;
    }
    match op.as_bytes() {
        b"+=" => Some(("__iadd__", "__add__")),
        b"-=" => Some(("__isubtract__", "__subtract__")),
        b"*=" => Some(("__imultiply__", "__multiply__")),
        b"/=" => Some(("__idivide__", "__divide__")),
        b"%=" => Some(("__iremainder__", "__remainder__")),
        b"^=" => Some(("__ipow__", "__pow__")),
        _ => None,
    }
}

fn exprs_structurally_equal(ast: &Ast, a: ExprId, b: ExprId) -> bool {
    match (&ast.exprs[a], &ast.exprs[b]) {
        (Expr::Number(n1), Expr::Number(n2)) => (n1.value - n2.value).abs() < f64::EPSILON,
        (Expr::Bool(b1), Expr::Bool(b2)) => b1.value == b2.value,
        (Expr::Ident(s1, _), Expr::Ident(s2, _)) => s1 == s2,
        (Expr::Variable(v1), Expr::Variable(v2)) => {
            v1.scope == v2.scope
                && text_value_to_string(ast, &v1.name) == text_value_to_string(ast, &v2.name)
        }
        (Expr::Binary(b1), Expr::Binary(b2)) => {
            b1.op == b2.op
                && exprs_structurally_equal(ast, b1.left, b2.left)
                && exprs_structurally_equal(ast, b1.right, b2.right)
        }
        (Expr::Unary(u1), Expr::Unary(u2)) => {
            u1.op == u2.op && exprs_structurally_equal(ast, u1.operand, u2.operand)
        }
        (Expr::Property(p1), Expr::Property(p2)) => {
            p1.property == p2.property && exprs_structurally_equal(ast, p1.object, p2.object)
        }
        (Expr::Subscript(s1), Expr::Subscript(s2)) => {
            exprs_structurally_equal(ast, s1.object, s2.object)
                && exprs_structurally_equal(ast, s1.index, s2.index)
                && match (s1.end, s2.end) {
                    (Some(e1), Some(e2)) => exprs_structurally_equal(ast, e1, e2),
                    (None, None) => true,
                    _ => false,
                }
        }
        _ => false,
    }
}
