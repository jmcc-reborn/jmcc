//! Properties and indexing: reading and writing class fields,
//! `__subscript__` / `__slice__`, and member resolution along inheritance chains.

use super::*;

impl HirBuilder<'_> {
    pub(super) fn find_in_classes<F, R>(&self, ty: &Type, f: F) -> Option<R>
    where
        F: Fn(&crate::ir::ctx::ClassInfo) -> Option<R>,
    {
        find_in_class_chain(ty, &self.ir_ctx.classes_by_def, f)
    }

    fn resolve_target_ty(&self, eid: ExprId) -> Type {
        let ty = self.types.get(&eid).cloned().unwrap_or(Type::Unknown);
        if !matches!(ty, Type::Unknown | Type::InferVar(_)) {
            return ty;
        }
        if let Expr::Ident(name_id, _) = &self.ast.exprs[eid] {
            let sym = self.sym(*name_id);
            if let Some(&Binding::Var {
                name: bound_sym, ..
            }) = self.lookup(sym)
                && let Some(actual_ty) = self.ir_ctx.var_types.get(&bound_sym).cloned()
            {
                return actual_ty;
            }
        }
        if let Expr::Property(p) = &self.ast.exprs[eid] {
            let obj_ty = self.resolve_target_ty(p.object);
            let prop_sym = self.sym(p.property);
            if let Some((field_ty, _)) =
                self.find_in_classes(&obj_ty, |c| c.fields.get(prop_sym.as_str()).cloned())
            {
                return field_ty;
            }
        }
        ty
    }

    pub(super) fn get_expr_target_var(&self, eid: ExprId) -> Option<Symbol> {
        let sym = match &self.ast.exprs[eid] {
            Expr::Ident(name_id, _) => Some(self.sym(*name_id)),
            Expr::Variable(v) => {
                let s = text_value_to_string(self.ast, &v.name);
                Some(Symbol::from(s))
            }
            _ => None,
        }?;
        let mut cur = sym;
        let mut visited = std::collections::HashSet::new();
        while let Some(&aliased) = self.inline_aliases.get(&cur) {
            if !visited.insert(cur) || aliased == cur {
                break;
            }
            cur = aliased;
        }
        Some(cur)
    }

    pub(super) fn get_or_create_cache_var(&mut self, obj_name: Symbol, prop_name: Symbol) -> Id {
        if let Some(&var_id) = self.cached_field_vars.get(&(obj_name, prop_name)) {
            return var_id;
        }
        let clean_obj = obj_name.as_str().replace(['%', '$'], "");
        let cache_name = Symbol::from(format!("__cache_{clean_obj}_{}", prop_name.as_str()));
        let raw_var = self.add(Hir::Var(VarName(cache_name)));
        let line_var = self.wrap_scope(raw_var, VarScope::Line);
        self.cached_field_vars
            .insert((obj_name, prop_name), line_var);
        line_var
    }

    /// Class field: dictionary-backed, direct single-field, or numeric slot, resolved along inheritance chain.
    pub(super) fn find_field_access(&self, ty: &Type, prop_name: &Symbol) -> Option<FieldAccess> {
        let is_single_field = self.ir_ctx.is_single_field_type(ty);
        self.find_in_classes(ty, |c| {
            if c.is_dict {
                Some(FieldAccess::Dict)
            } else if is_single_field && c.fields.contains_key(prop_name.as_str()) {
                Some(FieldAccess::Direct)
            } else {
                c.fields.get(prop_name.as_str()).map(|(_, i)| {
                    if c.is_field_cached(prop_name.as_str()) {
                        FieldAccess::CachedSlot(*i)
                    } else {
                        FieldAccess::Slot(*i)
                    }
                })
            }
        })
    }

    fn is_zero_or_false(&self, id: Id) -> bool {
        match self.get(id) {
            Hir::Num(n) => n.0 == 0.0,
            Hir::Bool(b) => !b,
            _ => false,
        }
    }

    pub(super) fn try_property_assign(
        &mut self,
        p: &PropertyExpr,
        val: Id,
        stmts: &mut Vec<Id>,
    ) -> Result<bool, IrError> {
        let target_ty = self.resolve_target_ty(p.object);
        let prop_name = self.sym(p.property);

        let member = self.find_in_classes(&target_ty, |c| {
            c.setters
                .get(prop_name.as_str())
                .cloned()
                .map(ClassMember::Setter)
        });

        if let Some(ClassMember::Setter(f)) = member {
            let all_args = vec![positional(p.object)];
            let last_param_name = f
                .params
                .last()
                .map(|p| self.sym(p.name).to_string())
                .unwrap_or_else(|| "value".to_string());
            let predefined = vec![(last_param_name, val)];
            let result =
                self.expand_inline_func_call(&f, &all_args, Vec::new(), predefined, None)?;
            let tgt = self.conv_expr(p.object)?;
            stmts.push(self.add(Hir::Set([tgt, result])));
            return Ok(true);
        }

        let field_info = self.find_field_access(&target_ty, &prop_name);

        if let Some(access) = field_info {
            let is_init_self = self.current_init_self.is_some()
                && self.get_expr_target_var(p.object) == self.current_init_self;
            if is_init_self {
                let slot_idx = match access {
                    FieldAccess::Slot(i) | FieldAccess::CachedSlot(i) => Some(i),
                    _ => None,
                };
                if let Some(idx) = slot_idx {
                    if self.is_zero_or_false(val) && !self.init_written_slots.contains(&idx) {
                        return Ok(true);
                    }
                    self.init_written_slots.insert(idx);
                }
            }

            let target_id = self.conv_expr(p.object)?;
            match access {
                FieldAccess::Direct => {
                    stmts.push(self.add(Hir::Set([target_id, val])));
                }
                FieldAccess::Slot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    let index_node = self.add(Hir::Index([target_id, index_id]));
                    stmts.push(self.add(Hir::Set([index_node, val])));
                }
                FieldAccess::CachedSlot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    let index_node = self.add(Hir::Index([target_id, index_id]));
                    if let Some(obj_name) = self.get_expr_target_var(p.object) {
                        let cache_var =
                            if let Some(&cv) = self.cached_field_vars.get(&(obj_name, prop_name)) {
                                Some(cv)
                            } else if self
                                .active_field_read_counts
                                .get(&(obj_name, prop_name))
                                .copied()
                                .unwrap_or(0)
                                >= 2
                            {
                                Some(self.get_or_create_cache_var(obj_name, prop_name))
                            } else {
                                None
                            };

                        if let Some(cache_var) = cache_var {
                            stmts.push(self.add(Hir::Set([cache_var, val])));
                            stmts.push(self.add(Hir::Set([index_node, cache_var])));
                        } else {
                            stmts.push(self.add(Hir::Set([index_node, val])));
                        }
                    } else {
                        stmts.push(self.add(Hir::Set([index_node, val])));
                    }
                }
                FieldAccess::Dict => {
                    let key_id = self.str_lit(prop_name);
                    let args = self.add(Hir::List(vec![target_id, key_id, val].into_boxed_slice()));
                    let act = self.action("variable", Symbol::from("set_map_value"), args);
                    stmts.push(self.add(Hir::Set([target_id, act])));
                }
            }
            return Ok(true);
        }

        Ok(false)
    }

    pub(super) fn try_subscript_assign(
        &mut self,
        s: &SubscriptExpr,
        val: Id,
        stmts: &mut Vec<Id>,
    ) -> Result<bool, IrError> {
        let target_ty = self.resolve_target_ty(s.object);
        let method_name = if s.end.is_some() {
            "__slice__"
        } else {
            "__subscript__"
        };

        let setter = self.find_in_classes(&target_ty, |c| {
            c.setters.get(method_name).cloned().map(ClassMember::Setter)
        });

        if let Some(ClassMember::Setter(f)) = setter {
            let mut all_args = vec![positional(s.object), positional(s.index)];
            if let Some(end) = s.end {
                all_args.push(positional(end));
            }
            let last_param_name = f
                .params
                .last()
                .map(|p| self.sym(p.name).to_string())
                .unwrap_or_else(|| "value".to_string());
            let predefined = vec![(last_param_name, val)];
            let result =
                self.expand_inline_func_call(&f, &all_args, Vec::new(), predefined, None)?;
            let tgt = self.conv_expr(s.object)?;
            stmts.push(self.add(Hir::Set([tgt, result])));
            return Ok(true);
        }

        if s.end.is_some() {
            return Err(IrError::SliceAssignNotSupported(format!("{target_ty:?}")));
        }

        let array_id = self.lang_item("array");
        let map_id = self.lang_item("map");

        if !matches!(target_ty, Type::Class(id, _) if id == array_id || id == map_id) {
            return Err(IrError::SubscriptAssignNotSupported(format!(
                "{target_ty:?}"
            )));
        }
        Ok(false)
    }

    #[instrument(skip(self, p), level = "trace")]
    pub(super) fn conv_property(&mut self, p: &PropertyExpr) -> Result<Id, IrError> {
        let prop_name = self.sym(p.property);

        if let Expr::Ident(name_id, _) = &self.ast.exprs[p.object] {
            let name = self.ast.strings.resolve(name_id);
            if let Some(def_id) = self.ir_ctx.enums_by_name.get(name)
                && let Some(enum_info) = self.ir_ctx.enums_by_def.get(def_id)
            {
                if enum_info.values.iter().any(|v| v == prop_name.as_str()) {
                    return Ok(self.str_lit(prop_name));
                }
                return Err(IrError::EnumVariantNotFound {
                    enum_name: name.to_owned(),
                    variant: prop_name.to_string(),
                });
            }
        }

        let target_ty = self.resolve_target_ty(p.object);

        let member = self.find_in_classes(&target_ty, |c| {
            c.getters
                .get(prop_name.as_str())
                .cloned()
                .map(ClassMember::Getter)
        });

        if let Some(ClassMember::Getter(f)) = member {
            return self.expand_inline_func_call(
                &f,
                &[positional(p.object)],
                Vec::new(),
                Vec::new(),
                None,
            );
        }

        let field_info = self.find_field_access(&target_ty, &prop_name);

        if let Some(access) = field_info {
            let target_id = self.conv_expr(p.object)?;
            return Ok(match access {
                FieldAccess::Direct => target_id,
                FieldAccess::Slot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    self.add(Hir::Index([target_id, index_id]))
                }
                FieldAccess::CachedSlot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    if let Some(obj_name) = self.get_expr_target_var(p.object)
                        && let Some(&cached_var) =
                            self.cached_field_vars.get(&(obj_name, prop_name))
                    {
                        return Ok(cached_var);
                    }
                    self.add(Hir::Index([target_id, index_id]))
                }
                FieldAccess::Dict => {
                    let key_id = self.str_lit(prop_name);
                    let args = self.add(Hir::List(vec![target_id, key_id].into_boxed_slice()));
                    self.action("variable", Symbol::from("get_map_value"), args)
                }
            });
        }

        let ty_str = self
            .get_class_name(&target_ty)
            .unwrap_or_else(|| format!("{target_ty:?}"));
        Err(IrError::PropertyNotFound {
            property: prop_name.to_string(),
            ty: ty_str,
        })
    }

    #[instrument(skip(self, s), level = "trace")]
    pub(super) fn conv_subscript(&mut self, s: &SubscriptExpr) -> Result<Id, IrError> {
        let target_ty = self.resolve_target_ty(s.object);
        let method_name = if s.end.is_some() {
            "__slice__"
        } else {
            "__subscript__"
        };

        let getter = self.find_in_classes(&target_ty, |c| {
            c.getters.get(method_name).cloned().map(ClassMember::Getter)
        });

        if let Some(ClassMember::Getter(f)) = getter {
            let mut all_args = vec![positional(s.object), positional(s.index)];
            if let Some(end) = s.end {
                all_args.push(positional(end));
            }
            return self.expand_inline_func_call(&f, &all_args, Vec::new(), Vec::new(), None);
        }

        let (obj, mut b) = self.atomize(s.object)?;
        if let Some(end_eid) = s.end {
            let (start, mut sb) = self.atomize(s.index)?;
            let (end_atom, mut eb) = self.atomize(end_eid)?;
            b.append(&mut sb);
            b.append(&mut eb);
            let node = self.add(Hir::Slice([obj, start, end_atom]));
            Ok(self.wrap_lets(node, b))
        } else {
            let (idx, mut ib) = self.atomize(s.index)?;
            b.append(&mut ib);
            let node = self.add(Hir::Index([obj, idx]));
            Ok(self.wrap_lets(node, b))
        }
    }

    pub(super) fn collect_field_reads(
        &self,
        stmts: &[Statement],
    ) -> HashMap<(Symbol, Symbol), usize> {
        let mut counts = HashMap::new();
        for stmt in stmts {
            self.walk_stmt_for_field_reads(stmt, &mut counts, 0);
        }
        counts
    }

    fn walk_stmt_for_field_reads(
        &self,
        stmt: &Statement,
        counts: &mut HashMap<(Symbol, Symbol), usize>,
        depth: usize,
    ) {
        match stmt {
            Statement::VarDecl(d) => {
                if let Some(val) = d.value {
                    self.walk_expr_for_field_reads(val, counts, depth);
                }
            }
            Statement::Assign(a) => {
                self.walk_expr_for_field_reads(a.value, counts, depth);
                for target in &a.targets {
                    if a.op.is_some() {
                        self.walk_expr_for_field_reads(*target, counts, depth);
                    } else if let Expr::Property(p) = &self.ast.exprs[*target] {
                        self.walk_expr_for_field_reads(p.object, counts, depth);
                    } else if let Expr::Subscript(s) = &self.ast.exprs[*target] {
                        self.walk_expr_for_field_reads(s.object, counts, depth);
                        self.walk_expr_for_field_reads(s.index, counts, depth);
                    }
                }
            }
            Statement::Return(r) => {
                if let Some(val) = r.value {
                    self.walk_expr_for_field_reads(val, counts, depth);
                }
            }
            Statement::Expr(eid) => {
                self.walk_expr_for_field_reads(*eid, counts, depth);
            }
            Statement::If(i) => {
                self.walk_expr_for_field_reads(i.condition, counts, depth);
                for s in &i.then_body {
                    self.walk_stmt_for_field_reads(s, counts, depth);
                }
                for (cond, body) in &i.elif_branches {
                    self.walk_expr_for_field_reads(*cond, counts, depth);
                    for s in body {
                        self.walk_stmt_for_field_reads(s, counts, depth);
                    }
                }
                if let Some(else_body) = &i.else_body {
                    for s in else_body {
                        self.walk_stmt_for_field_reads(s, counts, depth);
                    }
                }
            }
            Statement::While(w) => {
                self.walk_expr_for_field_reads(w.condition, counts, depth);
                for s in &w.body {
                    self.walk_stmt_for_field_reads(s, counts, depth);
                }
            }
            Statement::For(f) => {
                self.walk_expr_for_field_reads(f.iterable, counts, depth);
                for s in &f.body {
                    self.walk_stmt_for_field_reads(s, counts, depth);
                }
            }
            Statement::Match(m) => {
                self.walk_expr_for_field_reads(m.expr, counts, depth);
                for arm in &m.arms {
                    for p in &arm.patterns {
                        self.walk_expr_for_field_reads(*p, counts, depth);
                    }
                    if let Some(g) = arm.guard {
                        self.walk_expr_for_field_reads(g, counts, depth);
                    }
                    for s in &arm.body {
                        self.walk_stmt_for_field_reads(s, counts, depth);
                    }
                }
            }
            Statement::TryCatch(tc) => {
                for s in &tc.try_body {
                    self.walk_stmt_for_field_reads(s, counts, depth);
                }
                for s in &tc.catch_body {
                    self.walk_stmt_for_field_reads(s, counts, depth);
                }
            }
            Statement::Throw(th) => {
                if let Some(val) = th.value {
                    self.walk_expr_for_field_reads(val, counts, depth);
                }
            }
            Statement::Cfg(_, inner) => {
                self.walk_stmt_for_field_reads(inner, counts, depth);
            }
            _ => {}
        }
    }

    fn walk_expr_for_field_reads(
        &self,
        eid: ExprId,
        counts: &mut HashMap<(Symbol, Symbol), usize>,
        depth: usize,
    ) {
        match &self.ast.exprs[eid] {
            Expr::Property(p) => {
                if let Some(obj_sym) = self.get_expr_target_var(p.object) {
                    let prop_sym = self.sym(p.property);
                    *counts.entry((obj_sym, prop_sym)).or_default() += 1;
                }
                self.walk_expr_for_field_reads(p.object, counts, depth);
            }
            Expr::Binary(b) => {
                self.walk_expr_for_field_reads(b.left, counts, depth);
                self.walk_expr_for_field_reads(b.right, counts, depth);
            }
            Expr::Unary(u) => {
                self.walk_expr_for_field_reads(u.operand, counts, depth);
            }
            Expr::Ternary(t) => {
                self.walk_expr_for_field_reads(t.cond, counts, depth);
                self.walk_expr_for_field_reads(t.then_val, counts, depth);
                self.walk_expr_for_field_reads(t.else_val, counts, depth);
            }
            Expr::Call(c) => {
                self.walk_expr_for_field_reads(c.target, counts, depth);
                for arg in &c.args {
                    self.walk_expr_for_field_reads(arg.value, counts, depth);
                }
                self.walk_inline_call_for_field_reads(c, counts, depth);
            }
            Expr::Action(a) => {
                for arg in &a.args {
                    self.walk_expr_for_field_reads(arg.value, counts, depth);
                }
            }
            Expr::Constructor(c) => {
                for arg in &c.args {
                    self.walk_expr_for_field_reads(arg.value, counts, depth);
                }
            }
            Expr::List(l) => {
                for &v in &l.values {
                    self.walk_expr_for_field_reads(v, counts, depth);
                }
            }
            Expr::Map(m) => {
                for &k in &m.keys {
                    self.walk_expr_for_field_reads(k, counts, depth);
                }
                for &v in &m.values {
                    self.walk_expr_for_field_reads(v, counts, depth);
                }
            }
            Expr::Subscript(s) => {
                self.walk_expr_for_field_reads(s.object, counts, depth);
                self.walk_expr_for_field_reads(s.index, counts, depth);
                if let Some(end) = s.end {
                    self.walk_expr_for_field_reads(end, counts, depth);
                }
            }
            Expr::Cast(c) => {
                self.walk_expr_for_field_reads(c.expr, counts, depth);
            }
            Expr::Match(m) => {
                self.walk_expr_for_field_reads(m.expr, counts, depth);
                for arm in &m.arms {
                    for p in &arm.patterns {
                        self.walk_expr_for_field_reads(*p, counts, depth);
                    }
                    if let Some(g) = arm.guard {
                        self.walk_expr_for_field_reads(g, counts, depth);
                    }
                    for s in &arm.body {
                        self.walk_stmt_for_field_reads(s, counts, depth);
                    }
                }
            }
            Expr::Lambda(l) => match &l.body {
                crate::ast::LambdaBody::Expr(e) => {
                    self.walk_expr_for_field_reads(*e, counts, depth);
                }
                crate::ast::LambdaBody::Block(stmts) => {
                    for s in stmts {
                        self.walk_stmt_for_field_reads(s, counts, depth);
                    }
                }
            },
            _ => {}
        }
    }

    fn walk_inline_call_for_field_reads(
        &self,
        c: &CallExpr,
        counts: &mut HashMap<(Symbol, Symbol), usize>,
        depth: usize,
    ) {
        if depth > 8 {
            return;
        }
        let method_sym = self.sym(c.method);
        let inline_fn = if let Expr::Ident(name, _) = self.ast.exprs[c.target] {
            let sym = self.sym(name);
            self.ir_ctx.inline_funcs.get(&sym).cloned()
        } else {
            let target_ty = self.resolve_target_ty(c.target);
            self.find_in_classes(&target_ty, |cl| {
                cl.methods.get(method_sym.as_str()).cloned()
            })
            .filter(|f| f.is_inline)
        };

        if let Some(f) = inline_fn {
            let mut aliases: HashMap<Symbol, Symbol> = HashMap::new();
            let has_self = f
                .params
                .first()
                .is_some_and(|p| crate::utils::is_self_param(self.sym(p.name).as_str()));
            if has_self {
                let p_sym = self.sym(f.params[0].name);
                if let Some(caller_var) = self.get_expr_target_var(c.target) {
                    aliases.insert(p_sym, caller_var);
                }
            }
            let args_offset = usize::from(has_self);
            for (i, param) in f.params.iter().skip(args_offset).enumerate() {
                let p_sym = self.sym(param.name);
                if let Some(arg) = c.args.get(i)
                    && let Some(caller_var) = self.get_expr_target_var(arg.value)
                {
                    aliases.insert(p_sym, caller_var);
                }
            }

            let mut callee_counts = HashMap::new();
            for s in &f.body {
                self.walk_stmt_for_field_reads(s, &mut callee_counts, depth + 1);
            }
            for ((obj, prop), count) in callee_counts {
                let mut resolved = obj;
                let mut visited = HashSet::new();
                while let Some(&aliased) = aliases.get(&resolved) {
                    if !visited.insert(resolved) || aliased == resolved {
                        break;
                    }
                    resolved = aliased;
                }
                *counts.entry((resolved, prop)).or_default() += count;
            }
        }
    }
}
