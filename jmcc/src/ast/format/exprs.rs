//! Formatting expressions: precedence and parentheses, binary/unary/ternary
//! operators, calls, properties, subscripts, list/map literals and lambdas,
//! plus precedence and operator name tables.

use super::*;

impl Formatter<'_> {
    pub(super) fn fmt_spread(&mut self, spread: u8) {
        match spread {
            1 => self.w("*"),
            2 => self.w("**"),
            0 | 3.. => {}
        }
    }

    /// Formats an access target, enclosing in parentheses if an underlying
    /// `unary`/`binary`/`ternary` would otherwise lose expression boundaries.
    pub(super) fn fmt_parenthesized_object(&mut self, object: ExprId) {
        let need_parens = matches!(
            &self.ast.exprs[object],
            Expr::Unary(..) | Expr::Binary(..) | Expr::Ternary(..) | Expr::Cast(..)
        );
        if need_parens {
            self.output.push('(');
        }
        self.fmt_expr(object, 21);
        if need_parens {
            self.output.push(')');
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn fmt_expr(&mut self, eid: ExprId, min_prec: u8) {
        let ast = self.ast;
        let expr = &ast.exprs[eid];
        let prec = expr_precedence(expr);
        let need_parens = prec < min_prec;

        if need_parens {
            self.output.push('(');
        }

        match expr {
            Expr::Number(n) => {
                self.w(&self.source[n.span.start..n.span.end]);
            }
            Expr::Bool(b) => self.w(if b.value { "true" } else { "false" }),
            Expr::Ident(s, _) => self.w(self.r(*s)),
            Expr::Text(tv) => self.fmt_text_value(tv),
            Expr::Variable(v) => self.fmt_variable(v),
            Expr::Nbt(nbt) => self.fmt_nbt(nbt),
            Expr::List(list) => self.fmt_list(list),
            Expr::Map(map) => self.fmt_map(map),
            Expr::Ternary(t) => {
                self.fmt_expr(t.cond, 2);
                self.w(" ? ");
                self.fmt_expr(t.then_val, 0);
                self.w(" : ");
                self.fmt_expr(t.else_val, 1);
            }
            Expr::Binary(binary) => self.fmt_binary_expr(binary),
            Expr::Unary(unary) => self.fmt_unary_expr(unary),
            Expr::Property(p) => {
                self.fmt_parenthesized_object(p.object);
                self.w(".");
                self.w(self.r(p.property));
            }
            Expr::Subscript(s) => {
                self.fmt_parenthesized_object(s.object);
                self.w("[");
                self.fmt_expr(s.index, 0);
                if let Some(end) = s.end {
                    self.w(":");
                    self.fmt_expr(end, 0);
                }
                self.w("]");
            }
            Expr::Call(call) => self.fmt_call(call),
            Expr::Action(a) => self.fmt_action(a),
            Expr::Constructor(c) => {
                self.w(self.r(c.name));
                self.fmt_args(&c.args);
            }
            Expr::Cast(c) => {
                self.fmt_expr(c.expr, 22);
                self.w(" as ");
                self.w(self.r(c.ty));
            }
            Expr::Match(m) => self.fmt_match_expr(m),
            Expr::Lambda(l) => {
                self.output.push('(');
                for (i, p) in l.params.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.w(self.r(p.name));
                    if let Some(ty) = p.ty {
                        self.w(": ");
                        self.w(self.r(ty));
                    }
                }
                self.output.push(')');
                if let Some(ret) = l.return_type {
                    self.w(" -> ");
                    self.w(self.r(ret));
                }
                self.w(" => ");
                match &l.body {
                    LambdaBody::Expr(e) => self.fmt_expr(*e, 0),
                    LambdaBody::Block(stmts) => {
                        self.output.push_str("{\n");
                        self.indent += 1;
                        for stmt in stmts {
                            self.ind();
                            self.fmt_stmt(stmt);
                            self.output.push('\n');
                        }
                        self.indent -= 1;
                        self.ind();
                        self.output.push('}');
                    }
                }
            }
        }

        if need_parens {
            self.output.push(')');
        }
    }

    fn fmt_binary_expr(&mut self, binary: &BinaryExpr) {
        let prec = binop_precedence(binary.op);
        let is_right_assoc = matches!(binary.op, BinOp::Pow | BinOp::Assign);
        let left_min = if is_right_assoc { prec + 1 } else { prec };
        let right_min = if is_right_assoc { prec } else { prec + 1 };

        let is_range = matches!(binary.op, BinOp::Range | BinOp::RangeInclusive);
        self.fmt_expr(binary.left, left_min);
        if !is_range {
            self.w(" ");
        }
        self.w(binop_str(binary.op));
        if !is_range {
            self.w(" ");
        }
        self.fmt_expr(binary.right, right_min);
    }

    fn fmt_unary_expr(&mut self, unary: &UnaryExpr) {
        self.w(match unary.op {
            UnOp::Not => "not ",
            UnOp::Neg => "-",
            UnOp::Inc => "++",
            UnOp::Dec => "--",
        });
        let need_parens = matches!(unary.op, UnOp::Neg)
            && matches!(
                &self.ast.exprs[unary.operand],
                Expr::Binary(..) | Expr::Unary(..)
            );
        if need_parens {
            self.output.push('(');
        }
        self.fmt_expr(unary.operand, 23);
        if need_parens {
            self.output.push(')');
        }
    }

    fn fmt_nbt(&mut self, nbt: &NbtExpr) {
        self.w("m");
        let raw = self.r(nbt.raw);
        if !raw.contains('\n') {
            self.w(raw);
            return;
        }
        let mut inner = raw.trim();
        if let Some(stripped) = inner.strip_prefix('{') {
            inner = stripped;
        }
        if let Some(stripped) = inner.strip_suffix('}') {
            inner = stripped;
        }
        let inner = inner.trim();
        self.output.push_str("{\n");
        self.indent += 1;
        for line in inner.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.ind();
                self.output.push_str(trimmed);
                self.output.push('\n');
            }
        }
        self.indent -= 1;
        self.ind();
        self.output.push('}');
    }

    fn fmt_list(&mut self, list: &ListExpr) {
        if list.values.is_empty() {
            self.w("[]");
            return;
        }
        let mut rendered = Vec::with_capacity(list.values.len());
        self.indent += 1;
        for val in &list.values {
            let s = self.render_to_string(|fmt| fmt.fmt_expr(*val, 0));
            rendered.push(s);
        }
        self.indent -= 1;
        self.fmt_delimited_list("[", "]", &rendered);
    }

    fn fmt_map(&mut self, map: &MapExpr) {
        if map.keys.is_empty() {
            self.w("{}");
            return;
        }
        let mut rendered = Vec::with_capacity(map.keys.len());
        self.indent += 1;
        for (key, val) in map.keys.iter().zip(&map.values) {
            let s = self.render_to_string(|fmt| {
                fmt.fmt_expr(*key, 0);
                fmt.w(": ");
                fmt.fmt_expr(*val, 0);
            });
            rendered.push(s);
        }
        self.indent -= 1;
        self.fmt_delimited_list("{", "}", &rendered);
    }

    fn fmt_call(&mut self, call: &CallExpr) {
        let target = &self.ast.exprs[call.target];
        let method = self.r(call.method);
        let is_function = matches!(target, Expr::Ident(name, _) if self.r(*name) == method);
        let is_callable = method == "call" && !matches!(target, Expr::Ident(..));
        let needs_parentheses = matches!(
            target,
            Expr::Unary(..) | Expr::Binary(..) | Expr::Ternary(..) | Expr::Cast(..)
        );

        if needs_parentheses {
            self.output.push('(');
        }
        self.fmt_expr(
            call.target,
            if is_function || is_callable { 23 } else { 21 },
        );
        if needs_parentheses {
            self.output.push(')');
        }
        if !is_function && !is_callable {
            self.w(".");
            self.w(method);
        }
        self.fmt_args(&call.args);
    }

    #[instrument(skip(self, a), level = "trace")]
    fn fmt_action(&mut self, a: &ActionExpr) {
        let obj_str = self.r(a.object);
        let name_str = self.r(a.name);

        if obj_str.is_empty() && name_str.is_empty() {
            if let Some(lambda) = &a.lambda {
                self.fmt_comma_separated(lambda, |s, p| s.fmt_expr(*p, 0));
                self.w(" -> ");
            }
            if let Some(ops) = &a.operations {
                self.fmt_block(ops);
            }
            return;
        }

        if a.invert == Some(true) {
            self.w("!");
        }
        self.w(obj_str);
        self.w("::");
        self.w(name_str);
        if let Some(sel) = a.selector {
            self.w("<");
            self.w(self.r(sel));
            self.w(">");
        }
        self.fmt_args(&a.args);

        if let Some(ops) = &a.operations {
            self.w(" ");
            if let Some(lambda) = &a.lambda {
                self.w("{ ");
                self.fmt_comma_separated(lambda, |s, p| s.fmt_expr(*p, 0));
                self.w(" ->\n");
                self.indent += 1;
                for s in ops {
                    self.fmt_stmt(s);
                    self.output.push('\n');
                }
                self.indent -= 1;
                self.ind();
                self.w("}");
            } else {
                self.fmt_block(ops);
            }
        }
    }

    #[instrument(skip(self, m), level = "trace")]
    pub(super) fn fmt_match_expr(&mut self, m: &MatchExpr) {
        self.w("match ");
        self.fmt_expr(m.expr, 0);
        if m.arms.is_empty() {
            self.w(" {}");
            return;
        }
        self.w(" {\n");
        self.indent += 1;

        let open_brace = self.source[..m.arms[0].span.start]
            .rfind('{')
            .map_or(0, |i| i + 1);
        let mut last_end = open_brace;

        for arm in &m.arms {
            self.write_gap(last_end, arm.span.start, false);
            self.ind();
            for (i, p) in arm.patterns.iter().enumerate() {
                if i > 0 {
                    self.w(" | ");
                }
                self.fmt_expr(*p, 0);
            }
            if let Some(guard) = arm.guard {
                self.w(" if ");
                self.fmt_expr(guard, 0);
            }
            self.w(" => ");
            let arm_text = self.source.get(arm.span.clone()).unwrap_or("");
            let originally_block = arm_text.find("=>").is_some_and(|arrow_idx| {
                let after_arrow = arm_text[arrow_idx + 2..].trim_start();
                after_arrow.starts_with('{')
            });

            if !originally_block && arm.body.len() == 1 && matches!(arm.body[0], Statement::Expr(_))
            {
                if let Statement::Expr(eid) = arm.body[0] {
                    let expr_str = self.render_to_string(|fmt| fmt.fmt_expr(eid, 0));
                    let current_len = self.current_line_width() + expr_str.len() + 1;
                    if current_len <= Self::MAX_WIDTH && !expr_str.contains('\n') {
                        self.w(&expr_str);
                        self.w(",\n");
                    } else {
                        self.fmt_block(&arm.body);
                        self.w("\n");
                    }
                }
            } else {
                self.fmt_block(&arm.body);
                self.w("\n");
            }
            last_end = arm.span.end;
        }

        if let Some(close_offset) = self.source[last_end..m.span.end].rfind('}') {
            self.write_gap(last_end, last_end + close_offset, false);
        }

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.indent -= 1;
        self.ind();
        self.w("}");
    }
}

pub(super) const fn expr_precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::Binary(b) => binop_precedence(b.op),
        Expr::Unary(_) => 25,
        Expr::Cast(_) => 23,
        Expr::Ternary(_) => 1,
        Expr::Lambda(..) => 0,
        Expr::Number(_)
        | Expr::Bool(_)
        | Expr::Ident(..)
        | Expr::Text(_)
        | Expr::Variable(_)
        | Expr::Nbt(_)
        | Expr::List(_)
        | Expr::Map(_)
        | Expr::Property(_)
        | Expr::Subscript(_)
        | Expr::Call(_)
        | Expr::Action(_)
        | Expr::Constructor(_)
        | Expr::Match(_) => 255,
    }
}

const fn binop_precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Assign => 0,
        BinOp::Or => 1,
        BinOp::And => 3,
        BinOp::Eq | BinOp::Ne => 5,
        BinOp::Le | BinOp::Ge | BinOp::Lt | BinOp::Gt | BinOp::In => 7,
        BinOp::Range | BinOp::RangeInclusive => 9,
        BinOp::Add | BinOp::Sub => 11,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 13,
        BinOp::BitAnd => 15,
        BinOp::BitOr => 17,
        BinOp::BitXor => 19,
        BinOp::Shl | BinOp::Shr => 21,
        BinOp::Pow => 23,
    }
}

pub(super) const fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "or",
        BinOp::And => "and",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::In => "in",
        BinOp::Range => "..",
        BinOp::RangeInclusive => "..=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor | BinOp::Pow => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Assign => "=",
    }
}
