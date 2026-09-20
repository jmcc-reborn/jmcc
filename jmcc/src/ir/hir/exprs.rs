//! Expressions: literals, lists and dicts, ternary, binary and unary
//! operators, constructors, and `atomize`.

use super::*;

impl HirBuilder<'_> {
    #[instrument(skip(self, eid), level = "trace")]
    pub(super) fn atomize(&mut self, eid: ExprId) -> Result<(Id, Vec<(Id, Id)>), IrError> {
        let val = self.conv_expr(eid)?;
        let mut current = val;
        let mut bindings = Vec::new();
        while let Hir::Let([var, v, body]) = self.get(current) {
            bindings.push((*var, *v));
            current = *body;
        }
        if self.is_atomic(current) {
            return Ok((current, bindings));
        }
        let name = self.fresh();
        let var_raw = self.add(Hir::Var(VarName(name)));
        let var = self.wrap_scope(var_raw, self.default_scope);
        if let Some(ty) = self.types.get(&eid) {
            self.ir_ctx.record_var_type(name, ty.clone());
        }
        bindings.push((var, current));
        Ok((var, bindings))
    }

    #[instrument(skip(self, eid), level = "trace")]
    pub(super) fn conv_condition(&mut self, eid: ExprId) -> Result<(Id, Vec<(Id, Id)>), IrError> {
        let val = self.conv_expr(eid)?;
        let mut current = val;
        let mut bindings = Vec::new();
        while let Hir::Let([var, v, body]) = self.get(current) {
            bindings.push((*var, *v));
            current = *body;
        }
        Ok((current, bindings))
    }

    #[instrument(skip(self, eid), level = "trace")]
    pub(super) fn conv_expr(&mut self, eid: ExprId) -> Result<Id, IrError> {
        match &self.ast.exprs[eid] {
            Expr::Number(n) => Ok(self.add(Hir::Num(n.value.into()))),
            Expr::Bool(b) => Ok(self.add(Hir::Bool(b.value))),
            Expr::Ident(n, _) => Ok(self.resolve_ident(*n)),
            Expr::Variable(v) => self.resolve_variable(v),
            Expr::Text(tv) => self.conv_text(tv),
            Expr::Nbt(n) => {
                let s = self.str_lit(self.sym(n.raw));
                Ok(self.add(Hir::Nbt(s)))
            }
            Expr::List(l) => self.conv_list(l),
            Expr::Map(m) => self.conv_map(m),
            Expr::Ternary(t) => self.conv_ternary(t),
            Expr::Binary(b) => self.conv_binary(b),
            Expr::Unary(u) => self.conv_unary(u),
            Expr::Property(p) => self.conv_property(p),
            Expr::Subscript(s) => self.conv_subscript(s),
            Expr::Call(c) => self.conv_call(c),
            Expr::Action(a) => self.conv_action(a),
            Expr::Constructor(c) => self.conv_ctor(c),
            Expr::Cast(c) => self.conv_expr(c.expr),
            Expr::Match(m) => self.conv_match_expr(m),
            Expr::Lambda(_) => unreachable!("Lambdas must be lifted before HIR lowering"),
        }
    }

    fn resolve_ident(&mut self, name: StrId) -> Id {
        let sym = self.sym(name);
        if let Some(&id) = self.inline_vars().get(&sym) {
            return id;
        }
        match self.lookup(sym) {
            Some(&Binding::Var { name, scope }) => {
                let var = self.add(Hir::Var(VarName(name)));
                self.wrap_scope(var, scope)
            }
            Some(&Binding::Func { name } | &Binding::Proc { name }) => self.str_lit(name),
            None => {
                trace!(name = %sym, scope = ?self.default_scope, "Undeclared variable, assuming default scope");
                let var = self.add(Hir::Var(VarName(sym)));
                self.wrap_scope(var, self.default_scope)
            }
        }
    }

    fn resolve_variable(&mut self, v: &VariableExpr) -> Result<Id, IrError> {
        let sym = self.eval_var_name(&v.name)?;
        if let Some(&id) = self.inline_vars().get(&sym) {
            return Ok(id);
        }
        let var = self.add(Hir::Var(VarName(sym)));
        let scope = match self.lookup(sym) {
            Some(&Binding::Var { scope, .. }) => scope,
            _ => v.scope,
        };
        Ok(self.wrap_scope(var, scope))
    }

    #[instrument(skip(self, l), level = "trace")]
    fn conv_list(&mut self, l: &ListExpr) -> Result<Id, IrError> {
        let mut bindings = Vec::new();
        let mut ids = Vec::new();
        for &v in &l.values {
            let (a, b) = self.atomize(v)?;
            ids.push(a);
            bindings.extend(b);
        }
        let node = self.add(Hir::List(ids.into_boxed_slice()));
        Ok(self.wrap_lets(node, bindings))
    }

    #[instrument(skip(self, m), level = "trace")]
    fn conv_map(&mut self, m: &MapExpr) -> Result<Id, IrError> {
        let mut bindings = Vec::new();
        let mut ids = Vec::new();
        for (&k, &v) in m.keys.iter().zip(&m.values) {
            let (ka, kb) = self.atomize(k)?;
            let (va, vb) = self.atomize(v)?;
            ids.push(ka);
            ids.push(va);
            bindings.extend(kb);
            bindings.extend(vb);
        }
        let node = self.add(Hir::Map(ids.into_boxed_slice()));
        Ok(self.wrap_lets(node, bindings))
    }

    #[instrument(skip(self, t), level = "trace")]
    fn conv_ternary(&mut self, t: &TernaryExpr) -> Result<Id, IrError> {
        let (c, mut b) = self.conv_condition(t.cond)?;
        let (th, mut tb) = self.atomize(t.then_val)?;
        let (el, mut eb) = self.atomize(t.else_val)?;
        b.append(&mut tb);
        b.append(&mut eb);
        let node = self.add(Hir::If([c, th, el]));
        Ok(self.wrap_lets(node, b))
    }

    #[instrument(skip(self, bin), level = "trace")]
    fn conv_binary(&mut self, bin: &BinaryExpr) -> Result<Id, IrError> {
        let (l, mut b) = self.atomize(bin.left)?;
        let (r, mut rb) = self.atomize(bin.right)?;
        b.append(&mut rb);

        let node = match bin_op_to_hir(bin.op, l, r) {
            BinOpHir::Node(node) => node,
            BinOpHir::In => return Err(IrError::UnsupportedIn),
            BinOpHir::Range | BinOpHir::RangeInclusive => {
                let one = self.add(Hir::Num(OrderedFloat(1.0)));
                let list = self.add(Hir::List(vec![l, r, l, one].into_boxed_slice()));
                return Ok(self.wrap_lets(list, b));
            }
            BinOpHir::Assign => {
                let set = self.add(Hir::Set([l, r]));
                let block = self.add(Hir::Block(vec![set, r].into_boxed_slice()));
                return Ok(self.wrap_lets(block, b));
            }
        };
        let node_id = self.add(node);
        Ok(self.wrap_lets(node_id, b))
    }

    #[instrument(skip(self, un), level = "trace")]
    fn conv_unary(&mut self, un: &UnaryExpr) -> Result<Id, IrError> {
        let (operand, b) = self.atomize(un.operand)?;
        // Divergence from `hir_expand::conv_ast_expr` is intentional: here
        // `UnOp::Neg` over a numeric literal is folded in place, whereas there it is not.
        // Verified on `- 5`, `-(5)`, and `2 - - 5`: emitted HIR/MIR/JSON dumps match.
        if un.op == UnOp::Neg
            && let Hir::Num(n) = self.get(operand)
        {
            let id = self.add(Hir::Num(OrderedFloat(-n.0)));
            return Ok(self.wrap_lets(id, b));
        }
        let node = self.add(un_op_to_hir(un.op, operand));
        Ok(self.wrap_lets(node, b))
    }

    #[instrument(skip(self, c), level = "trace")]
    fn conv_ctor(&mut self, c: &ConstructorExpr) -> Result<Id, IrError> {
        let nid = self.str_lit(self.sym(c.name));
        let (args, b) = self.conv_args(&c.args, None, false, false)?;
        let node = self.add(Hir::Ctor(vec![nid, args].into_boxed_slice()));
        Ok(self.wrap_lets(node, b))
    }
}
