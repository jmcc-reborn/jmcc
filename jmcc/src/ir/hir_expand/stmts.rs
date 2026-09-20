//! Statements: AST statement and block transformation into HIR, variable
//! declarations, and computing extra invocation arguments.

use super::*;

impl OverloadExpander<'_> {
    #[instrument(skip(self, stmt), level = "trace")]
    pub(super) fn conv_ast_stmt(&mut self, stmt: &Statement) -> Result<Id> {
        match stmt {
            Statement::Return(r) => {
                if let Some(ret_var) = self.inline_return_var
                    && let Some(v) = r.value
                {
                    let val = self.conv_ast_expr(v)?;
                    return Ok(self.add(Hir::Set([ret_var, val])));
                }
                Ok(self.add(Hir::Nop))
            }
            Statement::Expr(eid) => self.conv_ast_expr(*eid),
            Statement::Assign(a) => {
                let val = self.conv_ast_expr(a.value)?;
                let mut stmts = Vec::new();
                for &target in &a.targets {
                    // `self.x = x` in an inline: write via setter/slot, not `Set` over a read.
                    if let Expr::Property(p) = &self.ast.exprs[target]
                        && let Some(stmt) = self.conv_ast_property_assign(p, val)?
                    {
                        stmts.push(stmt);
                        continue;
                    }
                    let tgt = self.conv_ast_expr(target)?;
                    stmts.push(self.add(Hir::Set([tgt, val])));
                }
                Ok(self.block_or_single(stmts))
            }
            Statement::VarDecl(decl) => self.conv_ast_var_decl(decl),
            Statement::If(i) => {
                let cond = self.conv_ast_expr(i.condition)?;
                let then_body = self.conv_ast_block(&i.then_body)?;
                let else_body = match &i.else_body {
                    Some(eb) => self.conv_ast_block(eb)?,
                    None => self.add(Hir::Nop),
                };
                Ok(self.add(Hir::If([cond, then_body, else_body])))
            }
            Statement::Match(m) => self.conv_ast_match(&m.arms, m.expr),
            Statement::TryCatch(tc) => self.conv_ast_try_catch(tc),
            Statement::Throw(th) => self.conv_ast_throw(th),
            _ => Err(OverloadError::UnsupportedStatement),
        }
    }

    fn conv_ast_var_decl(&mut self, decl: &VarDecl) -> Result<Id> {
        let value = decl
            .value
            .map(|value| self.conv_ast_expr(value))
            .transpose()?;
        if decl.names.len() == 1 {
            let name = self.sym_str_from_text(&decl.names[0])?;
            let scope = decl_scope(&decl.scopes, 0, VarScope::Line);
            let variable = self.scoped_var(name, scope)?;
            let value = value.unwrap_or_else(|| self.add(Hir::Nop));
            return Ok(self.add(Hir::VarDecl([variable, value])));
        }

        let mut statements = Vec::new();
        if let Some(value) = value {
            // `var a, b = action()`: bind the action's output slots, not array elements.
            if let Some((temps, stmt)) = destructure_action_value(self, value, decl.names.len()) {
                statements.push(stmt);
                for (index, name) in decl.names.iter().enumerate() {
                    let variable = self.declared_var(decl, index, name)?;
                    statements.push(self.add(Hir::VarDecl([variable, temps[index]])));
                }
                return Ok(self.block_or_single(statements));
            }

            let temp_name = self.fresh();
            let temp_raw = self.add(Hir::Var(VarName(temp_name)));
            let temp = self.add(Hir::Line(temp_raw));
            statements.push(self.add(Hir::VarDecl([temp, value])));
            for (index, name) in decl.names.iter().enumerate() {
                let index_id = self.add(Hir::Num((index as f64).into()));
                let args = self.add(Hir::List(vec![temp, index_id].into_boxed_slice()));
                let object = self.add(Hir::Str(StrLit(Symbol::from("variable"))));
                let name_id = self.add(Hir::Str(StrLit(Symbol::from("get_list_value"))));
                let nop = self.add(Hir::Nop);
                let action = self.add(Hir::Action(
                    vec![object, name_id, nop, args, nop, nop, nop].into_boxed_slice(),
                ));
                let variable = self.declared_var(decl, index, name)?;
                statements.push(self.add(Hir::VarDecl([variable, action])));
            }
        } else {
            for (index, name) in decl.names.iter().enumerate() {
                let variable = self.declared_var(decl, index, name)?;
                let nop = self.add(Hir::Nop);
                statements.push(self.add(Hir::VarDecl([variable, nop])));
            }
        }
        Ok(self.block_or_single(statements))
    }

    #[instrument(skip(self, stmts), level = "trace")]
    pub(super) fn conv_ast_block(&mut self, stmts: &[Statement]) -> Result<Id> {
        let mut ids = Vec::new();
        for s in stmts {
            ids.push(self.conv_ast_stmt(s)?);
        }
        match ids.len() {
            0 => Ok(self.add(Hir::Nop)),
            1 => Ok(ids[0]),
            _ => Ok(self.add(Hir::Block(ids.into_boxed_slice()))),
        }
    }

    /// Call arguments not matched to parameters: evaluated only for side effects.
    pub(super) fn conv_ast_loose_args(&mut self, args: &[ArgExpr]) -> Result<AtomizedArgs> {
        let mut ids = Vec::with_capacity(args.len());
        for arg in args {
            ids.push(self.conv_ast_expr(arg.value)?);
        }
        Ok(self.atomize_args(ids))
    }

    pub(super) fn conv_ast_match(&mut self, arms: &[MatchArm], expr: ExprId) -> Result<Id> {
        let scrutinee = self.conv_ast_expr(expr)?;
        let mut else_body = self.add(Hir::Nop);
        for arm in arms.iter().rev() {
            let body = self.conv_ast_block(&arm.body)?;
            let mut cond = None;
            for &pattern in &arm.patterns {
                let pat_cond = self.conv_ast_pattern(scrutinee, pattern)?;
                cond = Some(cond.map_or(pat_cond, |prev| self.add(Hir::Or([prev, pat_cond]))));
            }
            let mut cond = cond.unwrap_or_else(|| self.add(Hir::Bool(true)));
            if let Some(guard) = arm.guard {
                let g = self.conv_ast_expr(guard)?;
                cond = self.add(Hir::And([cond, g]));
            }
            else_body = self.add(Hir::If([cond, body, else_body]));
        }
        Ok(else_body)
    }

    fn conv_ast_pattern(&mut self, scrutinee: Id, pattern: ExprId) -> Result<Id> {
        if let Expr::Ident(name, _) = &self.ast.exprs[pattern]
            && self.ast.strings.resolve(name) == "_"
        {
            return Ok(self.add(Hir::Bool(true)));
        }
        let pat = self.conv_ast_expr(pattern)?;
        Ok(self.add(Hir::Eq([scrutinee, pat])))
    }

    fn conv_ast_try_catch(&mut self, tc: &TryCatchStmt) -> Result<Id> {
        let try_body = self.conv_ast_block(&tc.try_body)?;
        let catch_var = if let Some(var) = &tc.catch_var {
            let name = self.sym_str_from_text(var)?;
            self.scoped_var(name, VarScope::Line)?
        } else {
            let name = self.fresh();
            self.scoped_var(name, VarScope::Line)?
        };
        let kind = match &tc.catch_type {
            Some(ty) => self.ast.strings.resolve(ty).to_ascii_uppercase(),
            None => "ALL".to_owned(),
        };
        let kind = match kind.as_str() {
            "WARNING" | "ERROR" | "ALL" => kind,
            _ => "ALL".to_owned(),
        };
        let var_name = self.add(Hir::Str(StrLit(Symbol::from("variable"))));
        let named_var = self.add(Hir::Named([var_name, catch_var]));
        let type_name = self.add(Hir::Str(StrLit(Symbol::from("exception_type"))));
        let kind_lit = self.add(Hir::Str(StrLit(Symbol::from(kind))));
        let type_val = self.add(Hir::Enum(kind_lit));
        let named_type = self.add(Hir::Named([type_name, type_val]));
        let args = self.add(Hir::List(vec![named_var, named_type].into_boxed_slice()));
        let obj = self.add(Hir::Str(StrLit(Symbol::from("controller"))));
        let name = self.add(Hir::Str(StrLit(Symbol::from("catch_exception"))));
        let nop = self.add(Hir::Nop);
        let action = self.add(Hir::Action(
            vec![obj, name, nop, args, try_body, nop, nop].into_boxed_slice(),
        ));
        let catch_body = self.conv_ast_block(&tc.catch_body)?;
        let exists_obj = self.add(Hir::Str(StrLit(Symbol::from("variable"))));
        let exists_name = self.add(Hir::Str(StrLit(Symbol::from("exists"))));
        let exists_args = self.add(Hir::List(vec![catch_var].into_boxed_slice()));
        let exists = self.add(Hir::Action(
            vec![exists_obj, exists_name, nop, exists_args, nop, nop, nop].into_boxed_slice(),
        ));
        let guarded = self.add(Hir::If([exists, catch_body, nop]));
        Ok(self.block_or_single(vec![action, guarded]))
    }

    fn conv_ast_throw(&mut self, th: &ThrowStmt) -> Result<Id> {
        let mut named = Vec::new();
        if let Some(v) = th.value {
            let msg = self.conv_ast_expr(v)?;
            let key = self.add(Hir::Str(StrLit(Symbol::from("message"))));
            named.push(self.add(Hir::Named([key, msg])));
        }
        let kind = match &th.exception_type {
            Some(ty) => self.ast.strings.resolve(ty).to_ascii_uppercase(),
            None => "ERROR".to_owned(),
        };
        let kind = match kind.as_str() {
            "WARNING" | "ERROR" | "FATAL" => kind,
            _ => "ERROR".to_owned(),
        };
        let type_key = self.add(Hir::Str(StrLit(Symbol::from("type"))));
        let kind_lit = self.add(Hir::Str(StrLit(Symbol::from(kind))));
        let type_val = self.add(Hir::Enum(kind_lit));
        named.push(self.add(Hir::Named([type_key, type_val])));
        let args = self.add(Hir::List(named.into_boxed_slice()));
        let obj = self.add(Hir::Str(StrLit(Symbol::from("code"))));
        let name = self.add(Hir::Str(StrLit(Symbol::from("call_exception"))));
        let nop = self.add(Hir::Nop);
        Ok(self.add(Hir::Action(
            vec![obj, name, nop, args, nop, nop, nop].into_boxed_slice(),
        )))
    }
}
