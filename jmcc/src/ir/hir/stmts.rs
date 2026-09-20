//! Statements: blocks, variable declarations, assignments, `return`, and `if`.

use super::*;

impl HirBuilder<'_> {
    pub(super) fn convert_block_inner(&mut self, stmts: &[Statement]) -> Result<Id, IrError> {
        self.push();
        let ids: Vec<Id> = stmts
            .iter()
            .map(|s| self.conv_stmt(s))
            .collect::<Result<_, _>>()?;
        self.pop();
        Ok(if ids.is_empty() {
            self.nop()
        } else {
            self.block_or_single(ids)
        })
    }

    #[instrument(skip(self, stmt), level = "trace")]
    pub(super) fn conv_stmt(&mut self, stmt: &Statement) -> Result<Id, IrError> {
        match stmt {
            Statement::VarDecl(d) => self.conv_var_decl(d),
            Statement::Assign(a) => self.conv_assign(a),
            Statement::Return(r) => self.conv_return(r),
            Statement::While(w) => self.conv_while(w),
            Statement::For(f) => self.conv_for(f),
            Statement::Break(b) => Ok(self.conv_break(b)),
            Statement::If(i) => self.conv_if(i),
            Statement::Match(m) => self.conv_match_stmt(m),
            Statement::TryCatch(tc) => self.conv_try_catch(tc),
            Statement::Throw(th) => self.conv_throw(th),
            Statement::Expr(eid) => {
                let prev = self.is_statement;
                self.is_statement =
                    matches!(&self.ast.exprs[*eid], Expr::Action(..) | Expr::Call(..));
                let res = self.conv_expr(*eid);
                self.is_statement = prev;
                res
            }
            Statement::Function(f) => self.conv_function(f),
            Statement::Process(p) => self.conv_process(p),
            Statement::Event(e) => self.conv_event(e),
            Statement::Class(c) => self.conv_class(c),
            Statement::Enum(e) => Ok(self.conv_enum(e)),
            Statement::Import(_) | Statement::TypeAlias(_) | Statement::Interface(_) => {
                Ok(self.nop())
            }
        }
    }

    #[instrument(skip(self, d), level = "trace")]
    fn conv_var_decl(&mut self, d: &VarDecl) -> Result<Id, IrError> {
        let mut stmts = Vec::new();

        let (val, b) = if let Some(val) = d.value {
            self.atomize(val)?
        } else {
            (self.nop(), Vec::new())
        };

        let has_value = !matches!(self.get(val), Hir::Nop);

        if d.names.len() == 1 {
            let sym = self.eval_var_name(&d.names[0])?;
            let scope = decl_scope(&d.scopes, 0, self.default_scope);

            if has_value {
                if let Some(val_ty) = d.value.and_then(|v| self.types.get(&v)) {
                    self.ir_ctx.record_var_type(sym, val_ty.clone());
                }
            } else if let Some(Some(ty_id)) = d.tys.first() {
                let ty_str = self.ast.strings.resolve(ty_id);
                let ty = self.parse_decl_type_str(ty_str)?;
                self.ir_ctx.record_var_type(sym, ty);
            }

            if scope == VarScope::Inline {
                if has_value {
                    self.inline_vars_mut().insert(sym, val);
                }
                let nop = self.nop();
                return Ok(self.wrap_lets(nop, b));
            }

            self.declare(sym, Binding::Var { name: sym, scope });
            let nid_raw = self.add(Hir::Var(VarName(sym)));
            let nid = self.wrap_scope(nid_raw, scope);
            let vd = self.add(Hir::VarDecl([nid, val]));
            stmts.push(vd);
        } else {
            // `var a, b = action()`: binds the action's output slots, not array elements.
            if has_value
                && let Some((temps, stmt)) = destructure_action_value(self, val, d.names.len())
            {
                stmts.push(stmt);
                for (i, name) in d.names.iter().enumerate() {
                    let sym = self.eval_var_name(name)?;
                    let scope = decl_scope(&d.scopes, i, self.default_scope);

                    if let Some(Some(ty_id)) = d.tys.get(i) {
                        let ty_str = self.ast.strings.resolve(ty_id);
                        let ty = self.parse_decl_type_str(ty_str)?;
                        self.ir_ctx.record_var_type(sym, ty);
                    }

                    if scope == VarScope::Inline {
                        self.inline_vars_mut().insert(sym, temps[i]);
                        continue;
                    }

                    self.declare(sym, Binding::Var { name: sym, scope });
                    let nid_raw = self.add(Hir::Var(VarName(sym)));
                    let nid = self.wrap_scope(nid_raw, scope);
                    let vd = self.add(Hir::VarDecl([nid, temps[i]]));
                    stmts.push(vd);
                }
            } else if has_value {
                let temp_name = self.fresh();
                let temp_var_raw = self.add(Hir::Var(VarName(temp_name)));
                let temp_var = self.wrap_scope(temp_var_raw, self.default_scope);
                stmts.push(self.add(Hir::VarDecl([temp_var, val])));

                for (i, name) in d.names.iter().enumerate() {
                    let sym = self.eval_var_name(name)?;
                    let scope = decl_scope(&d.scopes, i, self.default_scope);

                    if let Some(Some(ty_id)) = d.tys.get(i) {
                        let ty_str = self.ast.strings.resolve(ty_id);
                        let ty = self.parse_decl_type_str(ty_str)?;
                        self.ir_ctx.record_var_type(sym, ty);
                    }

                    let index_id = self.add(Hir::Num(OrderedFloat(i as f64)));
                    let args = self.add(Hir::List(vec![temp_var, index_id].into_boxed_slice()));
                    let act = self.action("variable", Symbol::from("get_list_value"), args);

                    if scope == VarScope::Inline {
                        self.inline_vars_mut().insert(sym, act);
                        continue;
                    }

                    self.declare(sym, Binding::Var { name: sym, scope });
                    let nid_raw = self.add(Hir::Var(VarName(sym)));
                    let nid = self.wrap_scope(nid_raw, scope);
                    let vd = self.add(Hir::VarDecl([nid, act]));
                    stmts.push(vd);
                }
            } else {
                for (i, name) in d.names.iter().enumerate() {
                    let sym = self.eval_var_name(name)?;
                    let scope = decl_scope(&d.scopes, i, self.default_scope);
                    self.declare(sym, Binding::Var { name: sym, scope });
                    let nid_raw = self.add(Hir::Var(VarName(sym)));
                    let nid = self.wrap_scope(nid_raw, scope);
                    let nop = self.nop();
                    stmts.push(self.add(Hir::VarDecl([nid, nop])));
                }
            }
        }

        let block = if stmts.len() == 1 {
            stmts[0]
        } else {
            self.add(Hir::Block(stmts.into_boxed_slice()))
        };
        Ok(self.wrap_lets(block, b))
    }

    #[instrument(skip(self, a), level = "trace")]
    fn conv_assign(&mut self, a: &AssignStmt) -> Result<Id, IrError> {
        if let Some(op_sym) = a.op {
            let op_str = self.ast.strings.resolve(&op_sym);
            if op_str != "=" {
                let (rhs, b) = self.atomize(a.value)?;
                let mut stmts = Vec::with_capacity(a.targets.len());
                for &target in &a.targets {
                    let lhs = self.conv_expr(target)?;
                    let binop = match op_str {
                        "+=" => Hir::Add([lhs, rhs]),
                        "-=" => Hir::Sub([lhs, rhs]),
                        "*=" => Hir::Mul([lhs, rhs]),
                        "/=" => Hir::Div([lhs, rhs]),
                        "%=" => Hir::Mod([lhs, rhs]),
                        "^=" => Hir::Pow([lhs, rhs]),
                        _ => return Err(IrError::InvalidAssignOp(op_str.to_owned())),
                    };
                    let op_id = self.add(binop);
                    match &self.ast.exprs[target] {
                        Expr::Property(p)
                            if self.try_property_assign(p, a.value, op_id, &mut stmts)? =>
                        {
                            continue;
                        }
                        Expr::Subscript(s)
                            if self.try_subscript_assign(s, a.value, &mut stmts)? =>
                        {
                            continue;
                        }
                        _ => {}
                    }
                    let tgt = self.conv_expr(target)?;
                    stmts.push(self.add(Hir::Set([tgt, op_id])));
                }
                let block = self.block_or_single(stmts);
                return Ok(self.wrap_lets(block, b));
            }
        }

        let (val, b) = self.atomize(a.value)?;
        let mut stmts = Vec::with_capacity(a.targets.len());

        // `a, b = action()` destructuring, plain variable targets only (properties/indices below).
        if a.targets.len() > 1
            && a.targets
                .iter()
                .all(|t| matches!(self.ast.exprs[*t], Expr::Ident(..) | Expr::Variable(..)))
            && let Some((temps, stmt)) = destructure_action_value(self, val, a.targets.len())
        {
            stmts.push(stmt);
            for (&target, &temp) in a.targets.iter().zip(&temps) {
                let tgt = self.conv_expr(target)?;
                stmts.push(self.add(Hir::Set([tgt, temp])));
            }
            let block = self.block_or_single(stmts);
            return Ok(self.wrap_lets(block, b));
        }

        for &target in &a.targets {
            match &self.ast.exprs[target] {
                Expr::Property(p) if self.try_property_assign(p, a.value, val, &mut stmts)? => {
                    continue;
                }
                Expr::Subscript(s) if self.try_subscript_assign(s, a.value, &mut stmts)? => {
                    continue;
                }
                _ => {}
            }
            let tgt = self.conv_expr(target)?;
            stmts.push(self.add(Hir::Set([tgt, val])));
        }
        let block = self.block_or_single(stmts);
        Ok(self.wrap_lets(block, b))
    }

    #[instrument(skip(self, r), level = "trace")]
    fn conv_return(&mut self, r: &ReturnStmt) -> Result<Id, IrError> {
        if let Some(ret_var) = self.inline_return_var {
            if let Some(v) = r.value {
                let (atom, b) = self.atomize(v)?;
                let set = self.add(Hir::Set([ret_var, atom]));
                return Ok(self.wrap_lets(set, b));
            }
            return Ok(self.nop());
        }
        if let Some(v) = r.value {
            let (atom, b) = self.atomize(v)?;
            let ret = self.add(Hir::Return(atom));
            Ok(self.wrap_lets(ret, b))
        } else {
            let nop = self.nop();
            Ok(self.add(Hir::Return(nop)))
        }
    }

    #[instrument(skip(self, i), level = "trace")]
    fn conv_if(&mut self, i: &IfStmt) -> Result<Id, IrError> {
        let (cond, mut b) = self.conv_condition(i.condition)?;
        let then_body = self.convert_block_inner(&i.then_body)?;
        let mut else_body = match &i.else_body {
            Some(eb) => self.convert_block_inner(eb)?,
            None => self.nop(),
        };
        for (ec, ebody) in i.elif_branches.iter().rev() {
            let (ec_atom, mut eb) = self.conv_condition(*ec)?;
            b.append(&mut eb);
            let eb_body = self.convert_block_inner(ebody)?;
            else_body = self.add(Hir::If([ec_atom, eb_body, else_body]));
        }
        let cond_final = if i.is_not {
            self.add(Hir::Not(cond))
        } else {
            cond
        };
        let node = self.add(Hir::If([cond_final, then_body, else_body]));
        Ok(self.wrap_lets(node, b))
    }

    pub(super) fn conv_match_stmt(&mut self, m: &MatchStmt) -> Result<Id, IrError> {
        let (scrutinee, b) = self.atomize(m.expr)?;
        let chain = self.lower_match_arms(scrutinee, &m.arms)?;
        Ok(self.wrap_lets(chain, b))
    }

    pub(super) fn conv_match_expr(&mut self, m: &MatchExpr) -> Result<Id, IrError> {
        let (scrutinee, b) = self.atomize(m.expr)?;
        let chain = self.lower_match_arms(scrutinee, &m.arms)?;
        Ok(self.wrap_lets(chain, b))
    }

    fn lower_match_arms(&mut self, scrutinee: Id, arms: &[MatchArm]) -> Result<Id, IrError> {
        let mut else_body = self.nop();
        for arm in arms.iter().rev() {
            let body = self.convert_block_inner(&arm.body)?;
            let cond = self.arm_condition(scrutinee, arm)?;
            else_body = self.add(Hir::If([cond, body, else_body]));
        }
        Ok(else_body)
    }

    fn arm_condition(&mut self, scrutinee: Id, arm: &MatchArm) -> Result<Id, IrError> {
        let mut cond = None;
        for &pattern in &arm.patterns {
            let pat_cond = self.pattern_condition(scrutinee, pattern)?;
            cond = Some(cond.map_or(pat_cond, |prev| self.add(Hir::Or([prev, pat_cond]))));
        }
        let mut cond = cond.unwrap_or_else(|| self.add(Hir::Bool(true)));
        if let Some(guard) = arm.guard {
            let (g, gb) = self.atomize(guard)?;
            let and = self.add(Hir::And([cond, g]));
            cond = self.wrap_lets(and, gb);
        }
        Ok(cond)
    }

    fn pattern_condition(&mut self, scrutinee: Id, pattern: ExprId) -> Result<Id, IrError> {
        if let Expr::Ident(name, _) = &self.ast.exprs[pattern] {
            let ident = self.ast.strings.resolve(name);
            if ident == "_" {
                return Ok(self.add(Hir::Bool(true)));
            }
        }
        let (pat, pb) = self.atomize(pattern)?;
        let eq = self.add(Hir::Eq([scrutinee, pat]));
        Ok(self.wrap_lets(eq, pb))
    }

    fn conv_try_catch(&mut self, tc: &TryCatchStmt) -> Result<Id, IrError> {
        let try_body = self.convert_block_inner(&tc.try_body)?;
        let catch_var = if let Some(var) = &tc.catch_var {
            let sym = self.eval_var_name(var)?;
            self.declare(
                sym,
                Binding::Var {
                    name: sym,
                    scope: self.default_scope,
                },
            );
            let raw = self.add(Hir::Var(VarName(sym)));
            self.wrap_scope(raw, self.default_scope)
        } else {
            let name = self.fresh();
            let raw = self.add(Hir::Var(VarName(name)));
            self.wrap_scope(raw, self.default_scope)
        };
        let kind = match &tc.catch_type {
            Some(ty) => self.ast.strings.resolve(ty).to_ascii_uppercase(),
            None => "ALL".to_owned(),
        };
        let kind = match kind.as_str() {
            "WARNING" | "ERROR" | "ALL" => kind,
            _ => "ALL".to_owned(),
        };
        let var_name = self.str_lit("variable");
        let named_var = self.add(Hir::Named([var_name, catch_var]));
        let type_name = self.str_lit("exception_type");
        let kind_lit = self.str_lit(kind);
        let type_val = self.add(Hir::Enum(kind_lit));
        let named_type = self.add(Hir::Named([type_name, type_val]));
        let args = self.add(Hir::List(vec![named_var, named_type].into_boxed_slice()));
        let obj = self.str_lit("controller");
        let name = self.str_lit("catch_exception");
        let nop = self.nop();
        let action = self.add(Hir::Action(
            vec![obj, name, nop, args, try_body, nop, nop].into_boxed_slice(),
        ));
        let catch_body = self.convert_block_inner(&tc.catch_body)?;
        let exists_obj = self.str_lit("variable");
        let exists_name = self.str_lit("exists");
        let exists_args = self.add(Hir::List(vec![catch_var].into_boxed_slice()));
        let exists = self.add(Hir::Action(
            vec![exists_obj, exists_name, nop, exists_args, nop, nop, nop].into_boxed_slice(),
        ));
        let guarded = self.add(Hir::If([exists, catch_body, nop]));
        Ok(self.add(Hir::Block(vec![action, guarded].into_boxed_slice())))
    }

    fn conv_throw(&mut self, th: &ThrowStmt) -> Result<Id, IrError> {
        let mut named = Vec::new();
        if let Some(v) = th.value {
            let (msg, b) = self.atomize(v)?;
            let key = self.str_lit("message");
            named.push(self.add(Hir::Named([key, msg])));
            let kind = match &th.exception_type {
                Some(ty) => self.ast.strings.resolve(ty).to_ascii_uppercase(),
                None => "ERROR".to_owned(),
            };
            let kind = match kind.as_str() {
                "WARNING" | "ERROR" | "FATAL" => kind,
                _ => "ERROR".to_owned(),
            };
            let type_key = self.str_lit("type");
            let kind_lit = self.str_lit(kind);
            let type_val = self.add(Hir::Enum(kind_lit));
            named.push(self.add(Hir::Named([type_key, type_val])));
            let args = self.add(Hir::List(named.into_boxed_slice()));
            let action = self.action("code", Symbol::from("call_exception"), args);
            return Ok(self.wrap_lets(action, b));
        }
        let kind = match &th.exception_type {
            Some(ty) => self.ast.strings.resolve(ty).to_ascii_uppercase(),
            None => "ERROR".to_owned(),
        };
        let type_key = self.str_lit("type");
        let kind_lit = self.str_lit(kind);
        let type_val = self.add(Hir::Enum(kind_lit));
        named.push(self.add(Hir::Named([type_key, type_val])));
        let args = self.add(Hir::List(named.into_boxed_slice()));
        Ok(self.action("code", Symbol::from("call_exception"), args))
    }

    fn conv_break(&mut self, b: &BreakStmt) -> Id {
        let Some(target_id) = b.label else {
            return self.add(Hir::Break);
        };
        let target_sym = self.sym(target_id);

        let mut target_idx = None;
        for (idx, ctx) in self.loop_stack.iter().enumerate().rev() {
            if ctx.label == Some(target_sym) {
                target_idx = Some(idx);
                break;
            }
        }

        let Some(target_idx) = target_idx else {
            return self.add(Hir::Break);
        };

        if target_idx == self.loop_stack.len() - 1 {
            return self.add(Hir::Break);
        }

        let flag = if let Some(flag) = self.loop_stack[target_idx].break_flag {
            flag
        } else {
            let flag = self.fresh();
            self.loop_stack[target_idx].break_flag = Some(flag);
            flag
        };

        for ctx in &mut self.loop_stack[target_idx + 1..] {
            if !ctx.outer_flags_checked.contains(&flag) {
                ctx.outer_flags_checked.push(flag);
            }
        }

        let flag_var_raw = self.add(Hir::Var(VarName(flag)));
        let flag_var = self.wrap_scope(flag_var_raw, self.default_scope);
        let one = self.add(Hir::Num(1.0.into()));
        let set_flag = self.add(Hir::Set([flag_var, one]));
        let brk = self.add(Hir::Break);
        self.add(Hir::Block(vec![set_flag, brk].into_boxed_slice()))
    }

    fn conv_while(&mut self, w: &WhileStmt) -> Result<Id, IrError> {
        let mut cond = self.conv_expr(w.condition)?;
        if w.is_not {
            cond = self.add(Hir::Not(cond));
        }

        let label_sym = w.label.map(|l| self.sym(l));
        self.loop_stack.push(LoopCtx {
            label: label_sym,
            break_flag: None,
            outer_flags_checked: Vec::new(),
        });

        let body = self.convert_block_inner(&w.body)?;
        let loop_ctx = self.loop_stack.pop().unwrap();

        let while_node = self.add(Hir::While([cond, body]));
        let mut stmts = vec![while_node];

        for flag in &loop_ctx.outer_flags_checked {
            let flag_var_raw = self.add(Hir::Var(VarName(*flag)));
            let flag_var = self.wrap_scope(flag_var_raw, self.default_scope);
            let one = self.add(Hir::Num(1.0.into()));
            let check_cond = self.add(Hir::Eq([flag_var, one]));
            let brk = self.add(Hir::Break);
            let nop = self.nop();
            stmts.push(self.add(Hir::If([check_cond, brk, nop])));

            if let Some(parent_ctx) = self.loop_stack.last_mut()
                && !parent_ctx.outer_flags_checked.contains(flag)
            {
                parent_ctx.outer_flags_checked.push(*flag);
            }
        }

        if let Some(flag) = loop_ctx.break_flag {
            let flag_var_raw = self.add(Hir::Var(VarName(flag)));
            let flag_var = self.wrap_scope(flag_var_raw, self.default_scope);
            let zero = self.add(Hir::Num(0.0.into()));
            let decl = self.add(Hir::VarDecl([flag_var, zero]));
            let set_zero = self.add(Hir::Set([flag_var, zero]));

            let mut wrapped = vec![decl];
            wrapped.extend(stmts);
            wrapped.push(set_zero);
            Ok(self.add(Hir::Block(wrapped.into_boxed_slice())))
        } else if stmts.len() == 1 {
            Ok(stmts[0])
        } else {
            Ok(self.add(Hir::Block(stmts.into_boxed_slice())))
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "for loop lowering with map, range, and array branches"
    )]
    fn conv_for(&mut self, f: &ForStmt) -> Result<Id, IrError> {
        let range_id = self.ir_ctx.lang_items.get("Range").copied().unwrap_or(0);
        let range_inc_id = self
            .ir_ctx
            .lang_items
            .get("RangeInclusive")
            .copied()
            .unwrap_or(0);
        let map_id = self.ir_ctx.lang_items.get("map").copied().unwrap_or(0);
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

        let (range_bin_expr, is_range_class, unwrapped_target, is_array_iter_var, is_map_iter_var) =
            match &self.ast.exprs[f.iterable] {
                Expr::Binary(b) if matches!(b.op, BinOp::Range | BinOp::RangeInclusive) => {
                    (Some(b), false, f.iterable, false, false)
                }
                Expr::Call(c) => {
                    let method_str = self.sym(c.method).to_string();
                    let short_method = method_str.rsplit("::").next().unwrap_or(&method_str);
                    let is_iter = short_method == "iter"
                        || short_method == "into_iter"
                        || short_method == "итератор";
                    if is_iter {
                        if let Expr::Binary(b) = &self.ast.exprs[c.target]
                            && matches!(b.op, BinOp::Range | BinOp::RangeInclusive)
                        {
                            (Some(b), false, c.target, false, false)
                        } else if matches!(self.types.get(&c.target), Some(Type::Class(id, _)) if *id != 0 && (*id == range_id || *id == range_inc_id))
                        {
                            (None, true, c.target, false, false)
                        } else {
                            (None, false, c.target, false, false)
                        }
                    } else {
                        let target_str = match &self.ast.exprs[c.target] {
                            Expr::Ident(name, _) => self.sym(*name).to_string(),
                            _ => String::new(),
                        };
                        let short_target = target_str.rsplit("::").next().unwrap_or(&target_str);
                        let is_iter_ctor = matches!(
                            short_target,
                            "ArrayIterator"
                                | "ИтераторМассива"
                                | "MapKeyIterator"
                                | "ИтераторКлючейКарты"
                                | "ИтераторКлючей"
                        ) && !c.args.is_empty();
                        if is_iter_ctor {
                            (None, false, c.args[0].value, false, false)
                        } else {
                            let is_rc = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && (*id == range_id || *id == range_inc_id));
                            let is_ai = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && *id == array_iter_id);
                            let is_mi = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && *id == map_iter_id);
                            (None, is_rc, f.iterable, is_ai, is_mi)
                        }
                    }
                }
                _ => {
                    let is_rc = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && (*id == range_id || *id == range_inc_id));
                    let is_ai = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && *id == array_iter_id);
                    let is_mi = matches!(self.types.get(&f.iterable), Some(Type::Class(id, _)) if *id != 0 && *id == map_iter_id);
                    (None, is_rc, f.iterable, is_ai, is_mi)
                }
            };

        let is_range = range_bin_expr.is_some() || is_range_class;
        let is_map =
            matches!(self.types.get(&unwrapped_target), Some(Type::Class(id, _)) if *id == map_id);

        let (iterable_id, mut b_iter) = if range_bin_expr.is_some() {
            (self.nop(), Vec::new())
        } else if is_array_iter_var || is_map_iter_var {
            let (iter_id, b) = self.atomize(unwrapped_target)?;
            let zero = self.add(Hir::Num(OrderedFloat(0.0)));
            let field_args = self.add(Hir::List(vec![iter_id, zero].into_boxed_slice()));
            let list_val = self.action("variable", Symbol::from("get_list_value"), field_args);
            (list_val, b)
        } else {
            self.atomize(unwrapped_target)?
        };

        let label_sym = f.label.map(|l| self.sym(l));
        self.loop_stack.push(LoopCtx {
            label: label_sym,
            break_flag: None,
            outer_flags_checked: Vec::new(),
        });

        self.push();

        let mut lambda_params = Vec::new();
        let mut named_args = Vec::new();
        let mut range_index_info = None;

        if is_map {
            let key_sym = self.eval_var_name(&f.vars[0])?;
            let key_scope = decl_scope(&f.scopes, 0, self.default_scope);
            self.declare(
                key_sym,
                Binding::Var {
                    name: key_sym,
                    scope: key_scope,
                },
            );
            let key_var_raw = self.add(Hir::Var(VarName(key_sym)));
            let key_var = self.wrap_scope(key_var_raw, key_scope);
            lambda_params.push(key_var);
            let key_arg_name = self.str_lit("key_variable");
            named_args.push(self.add(Hir::Named([key_arg_name, key_var])));

            if f.vars.len() >= 2 {
                let val_sym = self.eval_var_name(&f.vars[1])?;
                let val_scope = decl_scope(&f.scopes, 1, self.default_scope);
                self.declare(
                    val_sym,
                    Binding::Var {
                        name: val_sym,
                        scope: val_scope,
                    },
                );
                let val_var_raw = self.add(Hir::Var(VarName(val_sym)));
                let val_var = self.wrap_scope(val_var_raw, val_scope);
                lambda_params.push(val_var);
                let val_arg_name = self.str_lit("value_variable");
                named_args.push(self.add(Hir::Named([val_arg_name, val_var])));
            }

            let map_arg_name = self.str_lit("map");
            named_args.push(self.add(Hir::Named([map_arg_name, iterable_id])));
        } else if is_range {
            let (start_id, end_id, interval_id) = if let Some(b) = range_bin_expr {
                let is_inclusive = b.op == BinOp::RangeInclusive;
                let (start_val, b_start) = self.atomize(b.left)?;
                let (end_raw, b_end) = self.atomize(b.right)?;
                b_iter.extend(b_start);
                b_iter.extend(b_end);

                let end_val = if is_inclusive {
                    end_raw
                } else if let Expr::Number(num) = &self.ast.exprs[b.right] {
                    self.add(Hir::Num(OrderedFloat(num.value - 1.0)))
                } else {
                    let one = self.add(Hir::Num(OrderedFloat(1.0)));
                    self.add(Hir::Sub([end_raw, one]))
                };
                let interval_val = self.add(Hir::Num(OrderedFloat(1.0)));
                (start_val, end_val, interval_val)
            } else {
                let is_inclusive = matches!(
                    self.types.get(&unwrapped_target),
                    Some(Type::Class(id, _)) if *id == range_inc_id
                );
                let zero = self.add(Hir::Num(OrderedFloat(0.0)));
                let one = self.add(Hir::Num(OrderedFloat(1.0)));
                let three = self.add(Hir::Num(OrderedFloat(3.0)));

                let start_args = self.add(Hir::List(vec![iterable_id, zero].into_boxed_slice()));
                let start_val = self.action("variable", Symbol::from("get_list_value"), start_args);

                let end_args = self.add(Hir::List(vec![iterable_id, one].into_boxed_slice()));
                let end_raw = self.action("variable", Symbol::from("get_list_value"), end_args);

                let step_args = self.add(Hir::List(vec![iterable_id, three].into_boxed_slice()));
                let step_val = self.action("variable", Symbol::from("get_list_value"), step_args);

                let end_val = if is_inclusive {
                    end_raw
                } else {
                    self.add(Hir::Sub([end_raw, step_val]))
                };
                (start_val, end_val, step_val)
            };

            let val_var = if f.vars.len() >= 2 {
                let idx_sym = self.eval_var_name(&f.vars[0])?;
                let idx_scope = decl_scope(&f.scopes, 0, self.default_scope);
                self.declare(
                    idx_sym,
                    Binding::Var {
                        name: idx_sym,
                        scope: idx_scope,
                    },
                );
                let idx_var_raw = self.add(Hir::Var(VarName(idx_sym)));
                let idx_var = self.wrap_scope(idx_var_raw, idx_scope);
                let zero = self.add(Hir::Num(OrderedFloat(0.0)));
                let init_idx = self.add(Hir::VarDecl([idx_var, zero]));

                let val_sym = self.eval_var_name(&f.vars[1])?;
                let val_scope = decl_scope(&f.scopes, 1, self.default_scope);
                self.declare(
                    val_sym,
                    Binding::Var {
                        name: val_sym,
                        scope: val_scope,
                    },
                );
                let val_var_raw = self.add(Hir::Var(VarName(val_sym)));
                let val_wrapped = self.wrap_scope(val_var_raw, val_scope);

                range_index_info = Some((idx_var, init_idx));
                val_wrapped
            } else {
                let val_sym = self.eval_var_name(&f.vars[0])?;
                let val_scope = decl_scope(&f.scopes, 0, self.default_scope);
                self.declare(
                    val_sym,
                    Binding::Var {
                        name: val_sym,
                        scope: val_scope,
                    },
                );
                let val_var_raw = self.add(Hir::Var(VarName(val_sym)));
                self.wrap_scope(val_var_raw, val_scope)
            };

            lambda_params.push(val_var);
            let var_arg_name = self.str_lit("variable");
            named_args.push(self.add(Hir::Named([var_arg_name, val_var])));

            let start_arg_name = self.str_lit("start");
            named_args.push(self.add(Hir::Named([start_arg_name, start_id])));

            let end_arg_name = self.str_lit("end");
            named_args.push(self.add(Hir::Named([end_arg_name, end_id])));

            let interval_arg_name = self.str_lit("interval");
            named_args.push(self.add(Hir::Named([interval_arg_name, interval_id])));
        } else if f.vars.len() >= 2 {
            let idx_sym = self.eval_var_name(&f.vars[0])?;
            let idx_scope = decl_scope(&f.scopes, 0, self.default_scope);
            self.declare(
                idx_sym,
                Binding::Var {
                    name: idx_sym,
                    scope: idx_scope,
                },
            );
            let idx_var_raw = self.add(Hir::Var(VarName(idx_sym)));
            let idx_var = self.wrap_scope(idx_var_raw, idx_scope);
            lambda_params.push(idx_var);
            let idx_arg_name = self.str_lit("index_variable");
            named_args.push(self.add(Hir::Named([idx_arg_name, idx_var])));

            let val_sym = self.eval_var_name(&f.vars[1])?;
            let val_scope = decl_scope(&f.scopes, 1, self.default_scope);
            self.declare(
                val_sym,
                Binding::Var {
                    name: val_sym,
                    scope: val_scope,
                },
            );
            let val_var_raw = self.add(Hir::Var(VarName(val_sym)));
            let val_var = self.wrap_scope(val_var_raw, val_scope);
            lambda_params.push(val_var);
            let val_arg_name = self.str_lit("value_variable");
            named_args.push(self.add(Hir::Named([val_arg_name, val_var])));

            let list_arg_name = self.str_lit("list");
            named_args.push(self.add(Hir::Named([list_arg_name, iterable_id])));
        } else {
            let val_sym = self.eval_var_name(&f.vars[0])?;
            let val_scope = decl_scope(&f.scopes, 0, self.default_scope);
            self.declare(
                val_sym,
                Binding::Var {
                    name: val_sym,
                    scope: val_scope,
                },
            );
            let val_var_raw = self.add(Hir::Var(VarName(val_sym)));
            let val_var = self.wrap_scope(val_var_raw, val_scope);
            lambda_params.push(val_var);
            let val_arg_name = self.str_lit("value_variable");
            named_args.push(self.add(Hir::Named([val_arg_name, val_var])));

            let list_arg_name = self.str_lit("list");
            named_args.push(self.add(Hir::Named([list_arg_name, iterable_id])));
        }

        let mut body_ids: Vec<Id> = f
            .body
            .iter()
            .map(|s| self.conv_stmt(s))
            .collect::<Result<_, _>>()?;
        if let Some((idx_var, _)) = range_index_info {
            let one = self.add(Hir::Num(OrderedFloat(1.0)));
            let next_idx = self.add(Hir::Add([idx_var, one]));
            let inc = self.add(Hir::Set([idx_var, next_idx]));
            body_ids.push(inc);
        }
        self.pop();

        let body = if body_ids.is_empty() {
            self.nop()
        } else {
            self.block_or_single(body_ids)
        };

        let loop_ctx = self.loop_stack.pop().unwrap();

        let obj_sym = Symbol::from("repeat");
        let name_sym = Symbol::from(if is_map {
            "for_each_map_entry"
        } else if is_range {
            "on_range"
        } else {
            "for_each_in_list"
        });

        let sel_id = self.nop();
        let args_id = self.add(Hir::List(named_args.into_boxed_slice()));
        let lambda_id = self.add(Hir::List(lambda_params.into_boxed_slice()));
        let cond_id = self.nop();

        let action_id = self.action_with(
            obj_sym,
            name_sym,
            [sel_id, args_id, body, lambda_id, cond_id],
        );

        let mut stmts = if let Some((_, init_idx)) = range_index_info {
            vec![init_idx, self.wrap_lets(action_id, b_iter)]
        } else {
            vec![self.wrap_lets(action_id, b_iter)]
        };

        for flag in &loop_ctx.outer_flags_checked {
            let flag_var_raw = self.add(Hir::Var(VarName(*flag)));
            let flag_var = self.wrap_scope(flag_var_raw, self.default_scope);
            let one = self.add(Hir::Num(1.0.into()));
            let check_cond = self.add(Hir::Eq([flag_var, one]));
            let brk = self.add(Hir::Break);
            let nop = self.nop();
            stmts.push(self.add(Hir::If([check_cond, brk, nop])));

            if let Some(parent_ctx) = self.loop_stack.last_mut()
                && !parent_ctx.outer_flags_checked.contains(flag)
            {
                parent_ctx.outer_flags_checked.push(*flag);
            }
        }

        if let Some(flag) = loop_ctx.break_flag {
            let flag_var_raw = self.add(Hir::Var(VarName(flag)));
            let flag_var = self.wrap_scope(flag_var_raw, self.default_scope);
            let zero = self.add(Hir::Num(0.0.into()));
            let decl = self.add(Hir::VarDecl([flag_var, zero]));
            let set_zero = self.add(Hir::Set([flag_var, zero]));

            let mut wrapped = vec![decl];
            wrapped.extend(stmts);
            wrapped.push(set_zero);
            Ok(self.add(Hir::Block(wrapped.into_boxed_slice())))
        } else if stmts.len() == 1 {
            Ok(stmts[0])
        } else {
            Ok(self.add(Hir::Block(stmts.into_boxed_slice())))
        }
    }
}
