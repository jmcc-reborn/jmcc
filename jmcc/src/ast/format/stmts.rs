//! Formatting statements: blocks, `if`/`elif`/`else`, var-decl, assign,
//! `return`/`break`; branching logic is in `fmt_stmt`.

use super::*;

impl Formatter<'_> {
    #[instrument(skip(self, stmt), level = "trace")]
    pub(super) fn fmt_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Import(i) => self.fmt_import(i),
            Statement::Function(f) => self.fmt_function(f),
            Statement::Process(p) => self.fmt_process(p),
            Statement::Event(e) => self.fmt_event(e),
            Statement::Class(c) => self.fmt_class(c),
            Statement::Enum(e) => self.fmt_enum(e),
            Statement::TypeAlias(ta) => {
                self.fmt_aliases(&ta.aliases);
                self.ind();
                if ta.is_exported {
                    self.w("export ");
                }
                self.w("typealias ");
                self.w(self.r(ta.name));
                if !ta.generics.is_empty() {
                    self.fmt_list_like("<", ">", &ta.generics, |s, g| s.w(s.r(*g)));
                }
                self.w(" = ");
                self.w(self.r(ta.target_ty));
                self.w(";");
            }
            Statement::If(i) => self.fmt_if(i),
            Statement::While(w) => {
                self.ind();
                if let Some(lbl) = w.label {
                    self.w(&format!("'{}: ", self.r(lbl)));
                }
                self.w("while ");
                if w.is_not {
                    self.w("not ");
                }
                self.fmt_expr(w.condition, 2);
                self.w(" ");
                self.fmt_block(&w.body);
            }
            Statement::For(f) => {
                self.ind();
                if let Some(lbl) = f.label {
                    self.w(&format!("'{}: ", self.r(lbl)));
                }
                self.w("for ");
                for (idx, var_name) in f.vars.iter().enumerate() {
                    if idx > 0 {
                        self.w(", ");
                    }
                    if let Some(scope) = f.scopes.get(idx).and_then(|s| *s) {
                        self.fmt_var_scope(scope);
                    }
                    self.fmt_var_name(var_name);
                }
                self.w(" in ");
                self.fmt_expr(f.iterable, 0);
                self.w(" ");
                self.fmt_block(&f.body);
            }
            Statement::Break(b) => {
                self.ind();
                if let Some(lbl) = b.label {
                    self.w(&format!("break '{};", self.r(lbl)));
                } else {
                    self.w("break;");
                }
            }
            Statement::VarDecl(v) => {
                self.fmt_var_decl(v);
                self.w(";");
            }
            Statement::Assign(a) => {
                self.fmt_assign(a);
                self.w(";");
            }
            Statement::Return(r) => {
                self.fmt_return(r);
                self.w(";");
            }
            Statement::Expr(eid) => {
                self.ind();
                self.fmt_expr(*eid, 0);
                let expr = &self.ast.exprs[*eid];
                let needs_semicolon = !matches!(expr, Expr::Action(a) if a.operations.is_some());
                if needs_semicolon {
                    self.w(";");
                }
            }
            Statement::Match(m) => self.fmt_match(m),
            Statement::TryCatch(tc) => self.fmt_try_catch(tc),
            Statement::Throw(th) => self.fmt_throw(th),
            Statement::Interface(i) => self.fmt_interface(i),
        }
    }

    #[instrument(skip(self, ifst), level = "trace")]
    pub(super) fn fmt_if(&mut self, ifst: &IfStmt) {
        self.ind();
        self.w("if ");
        if ifst.is_not {
            self.w("not ");
        }
        self.fmt_expr(ifst.condition, 2);
        self.w(" ");
        self.fmt_block(&ifst.then_body);

        for (cond, body) in &ifst.elif_branches {
            self.output.push_str(" elif ");
            self.fmt_expr(*cond, 2);
            self.output.push(' ');
            self.fmt_block(body);
        }

        if let Some(body) = &ifst.else_body {
            self.output.push_str(" else ");
            self.fmt_block(body);
        }
    }

    pub(super) fn fmt_var_scope(&mut self, scope: VarScope) {
        self.w(match scope {
            VarScope::Inline => "inline ",
            VarScope::Local => "local ",
            VarScope::Game => "game ",
            VarScope::Save => "save ",
            VarScope::Line => "line ",
            VarScope::Jmcc => "jmcc ",
        });
    }

    #[instrument(skip(self, v), level = "trace")]
    pub(super) fn fmt_var_decl(&mut self, v: &VarDecl) {
        self.ind();
        if v.is_exported {
            self.w("export ");
        }

        let mut i = 0;
        while i < v.names.len() {
            let scope = v.scopes.get(i).copied().unwrap_or(None);
            if let Some(s) = scope {
                self.fmt_var_scope(s);
            }
            self.w("var ");

            let mut first = true;
            while i < v.names.len() {
                if v.scopes.get(i).copied().unwrap_or(None) != scope {
                    break;
                }
                if !first {
                    self.w(", ");
                }
                first = false;
                self.fmt_var_name(&v.names[i]);
                if let Some(Some(ty)) = v.tys.get(i) {
                    self.w(": ");
                    self.w(self.r(*ty));
                }
                i += 1;
            }
        }
        if let Some(val) = v.value {
            self.w(" = ");
            self.fmt_expr(val, 0);
        }
    }

    #[instrument(skip(self, a), level = "trace")]
    pub(super) fn fmt_assign(&mut self, a: &AssignStmt) {
        self.ind();
        self.fmt_comma_separated(&a.targets, |s, target| s.fmt_expr(*target, 1));
        self.w(" ");
        if let Some(op) = a.op {
            self.w(self.r(op));
        } else {
            self.w("=");
        }
        self.w(" ");
        self.fmt_expr(a.value, 0);
    }

    #[instrument(skip(self, r), level = "trace")]
    pub(super) fn fmt_return(&mut self, r: &ReturnStmt) {
        self.ind();
        self.w("return");
        if let Some(v) = r.value {
            self.w(" ");
            self.fmt_expr(v, 0);
        }
    }

    /// Writes an empty block, preserving any comments written inside the braces.
    ///
    /// An "empty" block may still contain comments, e.g. the `/* compiler built-in */`
    /// marker used by `@lang_item` constructors. Dropping them would silently delete
    /// source text, so the character range between the braces is copied through
    /// [`Formatter::write_gap`] when it holds anything other than whitespace.
    pub(super) fn fmt_empty_block(&mut self, range: Span) {
        if !self.range_has_content(range.clone()) {
            self.w("{}");
            return;
        }
        self.w("{\n");
        self.indent += 1;
        self.write_gap(range.start, range.end, false);
        while self.output.ends_with('\n') {
            self.output.pop();
        }
        self.output.push('\n');
        self.indent -= 1;
        self.ind();
        self.w("}");
    }

    /// Reports whether `range` holds anything other than whitespace in the source.
    fn range_has_content(&self, range: Span) -> bool {
        self.source
            .get(range)
            .is_some_and(|text| text.chars().any(|c| !c.is_whitespace()))
    }

    /// Returns the source range strictly between a declaration body's braces,
    /// given the byte offset where its signature ends.
    ///
    /// The scan starts *after* the parameter list and return type, because a
    /// parameter default may itself contain braces (`m{}`), which would
    /// otherwise be mistaken for the body's opening brace.
    pub(super) fn body_inner_range(&self, signature_end: usize) -> Span {
        let bytes = self.source.as_bytes();
        let open = (signature_end..bytes.len()).find(|&i| bytes[i] == b'{');
        let Some(open) = open else {
            return signature_end..signature_end;
        };
        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return open + 1..i;
                    }
                }
                _ => {}
            }
        }
        open + 1..bytes.len()
    }

    /// Returns the byte offset just past the closing `>`/`)` of the parameter
    /// list that ends the given declaration's signature, starting the search at
    /// `from`. Used to skip parameter defaults when locating a body.
    pub(super) fn signature_end(&self, from: usize) -> usize {
        let bytes = self.source.as_bytes();
        let Some(open) = (from..bytes.len()).find(|&i| bytes[i] == b'(') else {
            return from;
        };
        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        bytes.len()
    }

    #[instrument(skip(self, stmts), level = "trace")]
    pub(super) fn fmt_block(&mut self, stmts: &[Statement]) {
        if stmts.is_empty() {
            self.w("{}");
            return;
        }
        self.w("{\n");
        self.indent += 1;

        let first_span = self.stmt_span(&stmts[0]);
        let open_brace = self.source[..first_span.start]
            .rfind('{')
            .map_or(0, |i| i + 1);
        let mut last_end = open_brace;

        for s in stmts {
            let span = self.stmt_span(s);
            self.write_gap(last_end, span.start, false);
            self.fmt_stmt(s);
            last_end = span.end;
        }

        if let Some(close_offset) = self.source[last_end..].find('}') {
            self.write_gap(last_end, last_end + close_offset, false);
        }

        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.indent -= 1;
        self.ind();
        self.w("}");
    }

    #[instrument(skip(self, m), level = "trace")]
    pub(super) fn fmt_match(&mut self, m: &MatchStmt) {
        self.ind();
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

    #[instrument(skip(self, tc), level = "trace")]
    pub(super) fn fmt_try_catch(&mut self, tc: &TryCatchStmt) {
        self.ind();
        self.w("try ");
        self.fmt_block(&tc.try_body);
        self.w(" catch");
        if tc.catch_var.is_some() || tc.catch_type.is_some() {
            self.w(" (");
            if let Some(var) = &tc.catch_var {
                self.fmt_text_value(var);
            }
            if let Some(ty) = tc.catch_type {
                if tc.catch_var.is_some() {
                    self.w(": ");
                }
                self.w(self.r(ty));
            }
            self.w(")");
        }
        self.w(" ");
        self.fmt_block(&tc.catch_body);
    }

    #[instrument(skip(self, th), level = "trace")]
    pub(super) fn fmt_throw(&mut self, th: &ThrowStmt) {
        self.ind();
        self.w("throw");
        if let Some(ty) = th.exception_type {
            self.w(" ");
            self.w(self.r(ty));
        }
        if let Some(v) = th.value {
            self.w(" ");
            self.fmt_expr(v, 0);
        }
        self.w(";");
    }
}
