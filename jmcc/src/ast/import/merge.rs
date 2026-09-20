//! Merging imported AST into destination AST: copying expressions, strings, and declarations.

use std::collections::HashMap;

use tracing::{Level, instrument, span, trace};

use super::*;

impl ImportResolver {
    #[instrument(skip(self, dst, src), level = "debug")]
    pub(super) fn merge_ast(&self, dst: &mut Ast, src: Ast) -> Vec<Statement> {
        let _span = span!(Level::DEBUG, "merge_ast").entered();
        let expr_map = self.transfer_exprs(dst, &src);
        let existing = Self::get_existing_names(dst);
        let result: Vec<Statement> = src
            .statements
            .iter()
            .filter(|stmt| {
                if matches!(stmt, Statement::Import(_)) {
                    return false;
                }
                !matches!(stmt, Statement::Function(_) | Statement::Process(_))
                    || Self::single_name(stmt, &src.strings)
                        .is_none_or(|name| !existing.contains(&name))
            })
            .map(|stmt| self.remap_stmt(stmt, &src, dst, &expr_map))
            .collect();
        debug!(merged = result.len(), "AST merged");
        result
    }

    #[instrument(skip(self, dst, src), level = "trace")]
    pub(super) fn transfer_exprs(&self, dst: &mut Ast, src: &Ast) -> HashMap<ExprId, ExprId> {
        let _span = span!(Level::TRACE, "transfer_exprs").entered();
        let mut expr_map = HashMap::new();
        for (old_id, _) in &src.exprs {
            let dummy = Expr::Bool(BoolExpr {
                value: false,
                span: 0..0,
            });
            expr_map.insert(old_id, dst.exprs.alloc(dummy));
        }
        for (old_id, expr) in &src.exprs {
            dst.exprs[expr_map[&old_id]] = self.remap_expr(expr, src, dst, &expr_map);
        }
        for (k, v) in &src.sources {
            dst.sources.insert(k.clone(), v.clone());
        }
        for (k, v) in &src.line_indexes {
            dst.line_indexes.insert(k.clone(), v.clone());
        }
        for fo in &src.file_offsets {
            dst.file_offsets.push(fo.clone());
        }
        trace!(transferred = expr_map.len(), "Expressions transferred");
        expr_map
    }

    #[instrument(skip(dst, src), level = "trace")]
    fn map_str(dst: &mut Ast, src: &Ast, s: StrId) -> StrId {
        dst.strings.get_or_intern(src.strings.resolve(&s))
    }

    /// Re-interns a string slice into `dst` — shared pattern for generics and enum variants.
    fn map_strs(dst: &mut Ast, src: &Ast, ids: &[StrId]) -> Vec<StrId> {
        ids.iter().map(|&s| Self::map_str(dst, src, s)).collect()
    }

    #[instrument(skip(params, src, dst, m), level = "trace")]
    fn remap_params(
        params: &[Param],
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> Vec<Param> {
        params
            .iter()
            .map(|p| Param {
                name: Self::map_str(dst, src, p.name),
                ty: p.ty.map(|t| Self::map_str(dst, src, t)),
                default: p.default.map(|d| m[&d]),
                is_ref: p.is_ref,
                spread: p.spread,
                span: p.span.clone(),
            })
            .collect()
    }

    #[instrument(skip(self, body, src, dst, m), level = "trace")]
    fn remap_body(
        &self,
        body: &[Statement],
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> Vec<Statement> {
        body.iter()
            .map(|s| self.remap_stmt(s, src, dst, m))
            .collect()
    }

    #[instrument(skip(args, src, dst, m), level = "trace")]
    fn remap_args(
        args: &[ArgExpr],
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> Vec<ArgExpr> {
        args.iter()
            .map(|a| ArgExpr {
                name: a.name.map(|n| Self::map_str(dst, src, n)),
                value: m[&a.value],
                spread: a.spread,
                is_ref: a.is_ref,
            })
            .collect()
    }

    #[instrument(skip(tv, src, dst, m), level = "trace")]
    fn remap_text_value(
        tv: &TextValue,
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> TextValue {
        TextValue {
            parts: tv
                .parts
                .iter()
                .map(|p| match p {
                    TextPart::Literal(s) => TextPart::Literal(Self::map_str(dst, src, *s)),
                    TextPart::Interp(e) => TextPart::Interp(m[e]),
                })
                .collect(),
            parsing: tv.parsing.clone(),
            span: tv.span.clone(),
        }
    }

    fn remap_import(import: &ImportStmt, src: &Ast, dst: &mut Ast) -> Statement {
        Statement::Import(ImportStmt {
            path: Self::map_str(dst, src, import.path),
            kind: match &import.kind {
                ImportKind::SideEffect => ImportKind::SideEffect,
                ImportKind::Default(name) => ImportKind::Default(Self::map_str(dst, src, *name)),
                ImportKind::Namespace(name) => {
                    ImportKind::Namespace(Self::map_str(dst, src, *name))
                }
                ImportKind::Named(items) => ImportKind::Named(
                    items
                        .iter()
                        .map(|item| ImportItem {
                            original: Self::map_str(dst, src, item.original),
                            local: Self::map_str(dst, src, item.local),
                        })
                        .collect(),
                ),
            },
            span: import.span.clone(),
        })
    }

    fn remap_type_alias(alias: &TypeAliasDecl, src: &Ast, dst: &mut Ast) -> Statement {
        Statement::TypeAlias(TypeAliasDecl {
            name: Self::map_str(dst, src, alias.name),
            generics: Self::map_strs(dst, src, &alias.generics),
            target_ty: Self::map_str(dst, src, alias.target_ty),
            is_exported: alias.is_exported,
            aliases: Self::map_strs(dst, src, &alias.aliases),
            span: alias.span.clone(),
        })
    }

    #[instrument(skip(self, expr, src, dst, m), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        reason = "AST expression remapping across file merge"
    )]
    fn remap_expr(
        &self,
        expr: &Expr,
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> Expr {
        match expr {
            Expr::Number(n) => Expr::Number(n.clone()),
            Expr::Bool(b) => Expr::Bool(b.clone()),
            Expr::Ident(s, sp) => Expr::Ident(Self::map_str(dst, src, *s), sp.clone()),
            Expr::Text(tv) => Expr::Text(Self::remap_text_value(tv, src, dst, m)),
            Expr::Variable(v) => Expr::Variable(VariableExpr {
                name: Self::remap_text_value(&v.name, src, dst, m),
                scope: v.scope,
                value_type: v.value_type.map(|s| Self::map_str(dst, src, s)),
                span: v.span.clone(),
            }),
            Expr::Nbt(n) => Expr::Nbt(NbtExpr {
                raw: Self::map_str(dst, src, n.raw),
                span: n.span.clone(),
            }),
            Expr::List(l) => Expr::List(ListExpr {
                values: l.values.iter().map(|&v| m[&v]).collect(),
                span: l.span.clone(),
            }),
            Expr::Map(mp) => Expr::Map(MapExpr {
                keys: mp.keys.iter().map(|&k| m[&k]).collect(),
                values: mp.values.iter().map(|&v| m[&v]).collect(),
                span: mp.span.clone(),
            }),
            Expr::Ternary(t) => Expr::Ternary(TernaryExpr {
                cond: m[&t.cond],
                then_val: m[&t.then_val],
                else_val: m[&t.else_val],
                span: t.span.clone(),
            }),
            Expr::Binary(b) => Expr::Binary(BinaryExpr {
                op: b.op,
                left: m[&b.left],
                right: m[&b.right],
                span: b.span.clone(),
            }),
            Expr::Unary(u) => Expr::Unary(UnaryExpr {
                op: u.op,
                operand: m[&u.operand],
                span: u.span.clone(),
            }),
            Expr::Property(p) => Expr::Property(PropertyExpr {
                object: m[&p.object],
                property: Self::map_str(dst, src, p.property),
                span: p.span.clone(),
            }),
            Expr::Subscript(s) => Expr::Subscript(SubscriptExpr {
                object: m[&s.object],
                index: m[&s.index],
                end: s.end.map(|e| m[&e]),
                span: s.span.clone(),
            }),
            Expr::Call(c) => Expr::Call(CallExpr {
                target: m[&c.target],
                method: Self::map_str(dst, src, c.method),
                args: Self::remap_args(&c.args, src, dst, m),
                span: c.span.clone(),
            }),
            Expr::Action(a) => Expr::Action(ActionExpr {
                object: Self::map_str(dst, src, a.object),
                name: Self::map_str(dst, src, a.name),
                args: Self::remap_args(&a.args, src, dst, m),
                operations: a
                    .operations
                    .as_ref()
                    .map(|ops| self.remap_body(ops, src, dst, m)),
                lambda: a
                    .lambda
                    .as_ref()
                    .map(|l| l.iter().map(|&e| m[&e]).collect()),
                selector: a.selector.map(|s| Self::map_str(dst, src, s)),
                invert: a.invert,
                span: a.span.clone(),
            }),
            Expr::Constructor(c) => Expr::Constructor(ConstructorExpr {
                name: Self::map_str(dst, src, c.name),
                args: Self::remap_args(&c.args, src, dst, m),
                span: c.span.clone(),
            }),
            Expr::Cast(c) => Expr::Cast(CastExpr {
                expr: m[&c.expr],
                ty: Self::map_str(dst, src, c.ty),
                span: c.span.clone(),
            }),
            Expr::Match(match_expr) => Expr::Match(MatchExpr {
                expr: m[&match_expr.expr],
                arms: match_expr
                    .arms
                    .iter()
                    .map(|arm| MatchArm {
                        patterns: arm.patterns.iter().map(|&p| m[&p]).collect(),
                        guard: arm.guard.map(|g| m[&g]),
                        body: self.remap_body(&arm.body, src, dst, m),
                        span: arm.span.clone(),
                    })
                    .collect(),
                span: match_expr.span.clone(),
            }),
            Expr::Lambda(l) => Expr::Lambda(LambdaExpr {
                params: l
                    .params
                    .iter()
                    .map(|p| Param {
                        name: Self::map_str(dst, src, p.name),
                        ty: p.ty.map(|t| Self::map_str(dst, src, t)),
                        default: p.default.map(|d| m[&d]),
                        is_ref: p.is_ref,
                        spread: p.spread,
                        span: p.span.clone(),
                    })
                    .collect(),
                return_type: l.return_type.map(|t| Self::map_str(dst, src, t)),
                body: match &l.body {
                    LambdaBody::Expr(e) => LambdaBody::Expr(m[e]),
                    LambdaBody::Block(stmts) => {
                        LambdaBody::Block(self.remap_body(stmts, src, dst, m))
                    }
                },
                span: l.span.clone(),
            }),
        }
    }

    #[instrument(skip(self, stmt, src, dst, m), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        reason = "remap every statement variant after import"
    )]
    pub(super) fn remap_stmt(
        &self,
        stmt: &Statement,
        src: &Ast,
        dst: &mut Ast,
        m: &HashMap<ExprId, ExprId>,
    ) -> Statement {
        match stmt {
            Statement::Import(import) => Self::remap_import(import, src, dst),
            Statement::TypeAlias(alias) => Self::remap_type_alias(alias, src, dst),
            Statement::Function(f) => Statement::Function(FunctionDecl {
                name: Self::map_str(dst, src, f.name),
                params: Self::remap_params(&f.params, src, dst, m),
                return_type: f.return_type.map(|t| Self::map_str(dst, src, t)),
                body: self.remap_body(&f.body, src, dst, m),
                is_inline: f.is_inline,
                is_exported: f.is_exported,
                is_getter: f.is_getter,
                is_setter: f.is_setter,
                aliases: Self::map_strs(dst, src, &f.aliases),
                test_attr: f.test_attr.clone(),
                span: f.span.clone(),
            }),
            Statement::Process(p) => Statement::Process(ProcessDecl {
                name: Self::map_str(dst, src, p.name),
                params: Self::remap_params(&p.params, src, dst, m),
                body: self.remap_body(&p.body, src, dst, m),
                is_exported: p.is_exported,
                aliases: Self::map_strs(dst, src, &p.aliases),
                span: p.span.clone(),
            }),
            Statement::Event(e) => Statement::Event(EventDecl {
                event_name: Self::map_str(dst, src, e.event_name),
                body: self.remap_body(&e.body, src, dst, m),
                span: e.span.clone(),
            }),
            Statement::Class(c) => Statement::Class(ClassDecl {
                name: Self::map_str(dst, src, c.name),
                generics: Self::map_strs(dst, src, &c.generics),
                parent: c.parent.map(|p| Self::map_str(dst, src, p)),
                implements: Self::map_strs(dst, src, &c.implements),
                body: self.remap_body(&c.body, src, dst, m),
                is_inline: c.is_inline,
                lang_item: c.lang_item,
                is_dict: c.is_dict,
                is_exported: c.is_exported,
                aliases: Self::map_strs(dst, src, &c.aliases),
                span: c.span.clone(),
            }),
            Statement::Interface(i) => Statement::Interface(InterfaceDecl {
                name: Self::map_str(dst, src, i.name),
                generics: Self::map_strs(dst, src, &i.generics),
                parents: Self::map_strs(dst, src, &i.parents),
                body: self.remap_body(&i.body, src, dst, m),
                is_exported: i.is_exported,
                aliases: Self::map_strs(dst, src, &i.aliases),
                span: i.span.clone(),
            }),
            Statement::Enum(e) => Statement::Enum(EnumDecl {
                name: Self::map_str(dst, src, e.name),
                values: Self::map_strs(dst, src, &e.values),
                is_exported: e.is_exported,
                aliases: Self::map_strs(dst, src, &e.aliases),
                span: e.span.clone(),
            }),
            Statement::If(i) => Statement::If(IfStmt {
                condition: m[&i.condition],
                then_body: self.remap_body(&i.then_body, src, dst, m),
                elif_branches: i
                    .elif_branches
                    .iter()
                    .map(|(c, b)| (m[c], self.remap_body(b, src, dst, m)))
                    .collect(),
                elif_spans: i.elif_spans.clone(),
                else_body: i
                    .else_body
                    .as_ref()
                    .map(|b| self.remap_body(b, src, dst, m)),
                is_not: i.is_not,
                span: i.span.clone(),
            }),
            Statement::While(w) => Statement::While(WhileStmt {
                label: w.label.map(|l| Self::map_str(dst, src, l)),
                condition: m[&w.condition],
                body: self.remap_body(&w.body, src, dst, m),
                is_not: w.is_not,
                span: w.span.clone(),
            }),
            Statement::For(f) => Statement::For(ForStmt {
                label: f.label.map(|l| Self::map_str(dst, src, l)),
                vars: f
                    .vars
                    .iter()
                    .map(|n| Self::remap_text_value(n, src, dst, m))
                    .collect(),
                scopes: f.scopes.clone(),
                iterable: m[&f.iterable],
                body: self.remap_body(&f.body, src, dst, m),
                span: f.span.clone(),
            }),
            Statement::Break(b) => Statement::Break(BreakStmt {
                label: b.label.map(|l| Self::map_str(dst, src, l)),
                span: b.span.clone(),
            }),
            Statement::VarDecl(d) => Statement::VarDecl(VarDecl {
                scopes: d.scopes.clone(),
                names: d
                    .names
                    .iter()
                    .map(|n| Self::remap_text_value(n, src, dst, m))
                    .collect(),
                tys: d
                    .tys
                    .iter()
                    .map(|t| t.map(|x| Self::map_str(dst, src, x)))
                    .collect(),
                value: d.value.map(|v| m[&v]),
                is_exported: d.is_exported,
                span: d.span.clone(),
            }),
            Statement::Assign(a) => Statement::Assign(AssignStmt {
                targets: a.targets.iter().map(|&t| m[&t]).collect(),
                op: a.op.map(|o| Self::map_str(dst, src, o)),
                value: m[&a.value],
                span: a.span.clone(),
            }),
            Statement::Return(r) => Statement::Return(ReturnStmt {
                value: r.value.map(|v| m[&v]),
                span: r.span.clone(),
            }),
            Statement::Expr(e) => Statement::Expr(m[e]),
            Statement::Match(match_stmt) => Statement::Match(MatchStmt {
                expr: m[&match_stmt.expr],
                arms: match_stmt
                    .arms
                    .iter()
                    .map(|arm| MatchArm {
                        patterns: arm.patterns.iter().map(|&p| m[&p]).collect(),
                        guard: arm.guard.map(|g| m[&g]),
                        body: self.remap_body(&arm.body, src, dst, m),
                        span: arm.span.clone(),
                    })
                    .collect(),
                span: match_stmt.span.clone(),
            }),
            Statement::TryCatch(tc) => Statement::TryCatch(TryCatchStmt {
                try_body: self.remap_body(&tc.try_body, src, dst, m),
                catch_var: tc
                    .catch_var
                    .as_ref()
                    .map(|v| Self::remap_text_value(v, src, dst, m)),
                catch_type: tc.catch_type.map(|t| Self::map_str(dst, src, t)),
                catch_body: self.remap_body(&tc.catch_body, src, dst, m),
                span: tc.span.clone(),
            }),
            Statement::Throw(th) => Statement::Throw(ThrowStmt {
                value: th.value.map(|v| m[&v]),
                exception_type: th.exception_type.map(|t| Self::map_str(dst, src, t)),
                span: th.span.clone(),
            }),
        }
    }
}
