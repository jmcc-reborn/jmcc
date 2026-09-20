use crate::ast::*;
use std::fmt::{self, Formatter};
use tracing::{Level, instrument, span};

macro_rules! w_ind {
    ($f:expr, $indent:expr) => {{
        for _ in 0..$indent {
            $f.write_str("  ")?;
        }
        Ok::<(), fmt::Error>(())
    }};
    ($f:expr, $indent:expr, $($arg:tt)*) => {{
        for _ in 0..$indent {
            $f.write_str("  ")?;
        }
        write!($f, $($arg)*)
    }};
}

impl fmt::Display for Ast {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let _span = span!(Level::TRACE, "ast_display").entered();
        let pp = PrettyPrinter {
            exprs: &self.exprs,
            strings: &self.strings,
        };
        for stmt in &self.statements {
            pp.fmt_stmt(stmt, 0, f)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

struct PrettyPrinter<'a> {
    exprs: &'a id_arena::Arena<Expr>,
    strings: &'a lasso::Rodeo,
}

impl<'a> PrettyPrinter<'a> {
    #[inline]
    fn r(&self, s: StrId) -> &'a str {
        self.strings.resolve(&s)
    }

    #[instrument(skip(self), level = "trace")]
    fn text_str(&self, tv: &TextValue) -> String {
        tv.parts
            .iter()
            .map(|p| match p {
                TextPart::Literal(id) => self.r(*id).to_owned(),
                TextPart::Interp(e) => format!("${{Expr#{e:?}}}"),
            })
            .collect()
    }

    #[inline]
    fn fmt_node(f: &mut Formatter<'_>, name: &str, span: &Span) -> fmt::Result {
        writeln!(f, "{} @{}..{}", name, span.start, span.end)
    }

    #[inline]
    fn fmt_inline(f: &mut Formatter<'_>, name: &str, span: &Span) -> fmt::Result {
        write!(f, "{} @{}..{}", name, span.start, span.end)
    }

    #[inline]
    #[instrument(skip(self, f), level = "trace")]
    fn fmt_child(
        &self,
        f: &mut Formatter<'_>,
        indent: usize,
        name: &str,
        id: ExprId,
    ) -> fmt::Result {
        w_ind!(f, indent + 1)?;
        writeln!(f, "{name}:")?;
        self.fmt_expr(id, indent + 2, f)?;
        writeln!(f)
    }

    #[inline]
    #[instrument(skip(self, f), level = "trace")]
    fn fmt_stmts_block(
        &self,
        f: &mut Formatter<'_>,
        indent: usize,
        name: &str,
        stmts: &[Statement],
    ) -> fmt::Result {
        w_ind!(f, indent + 1)?;
        writeln!(f, "{name}:")?;
        for s in stmts {
            self.fmt_stmt(s, indent + 2, f)?;
            writeln!(f)?;
        }
        Ok(())
    }

    #[instrument(skip(self, f, id), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        reason = "pretty-print dispatch for every expression kind"
    )]
    fn fmt_expr(&self, id: ExprId, indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        let expr = &self.exprs[id];
        w_ind!(f, indent, "Expr#{id:?} ")?;

        match expr {
            Expr::Number(n) => Self::fmt_inline(f, &format!("Number({})", n.value), &n.span),
            Expr::Bool(b) => Self::fmt_inline(f, &format!("Bool({})", b.value), &b.span),
            Expr::Ident(s, span) => {
                Self::fmt_inline(f, &format!("Ident(\"{}\")", self.r(*s)), span)
            }
            Expr::Text(t) => {
                Self::fmt_node(f, "Text", &t.span)?;
                w_ind!(f, indent + 1)?;
                write!(f, "value: \"{}\" [{:?}]", self.text_str(t), t.parsing)
            }
            Expr::Variable(v) => {
                let mut s = format!("Variable \"{}\" [{:?}]", self.text_str(&v.name), v.scope);
                if let Some(ty) = v.value_type {
                    s.push_str(&format!(" : {}", self.r(ty)));
                }
                Self::fmt_inline(f, &s, &v.span)
            }
            Expr::Nbt(n) => Self::fmt_inline(f, &format!("Nbt(\"{}\")", self.r(n.raw)), &n.span),
            Expr::List(l) => {
                Self::fmt_node(f, &format!("List[{}]", l.values.len()), &l.span)?;
                for (i, v) in l.values.iter().enumerate() {
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "[{i}]:")?;
                    self.fmt_expr(*v, indent + 2, f)?;
                    writeln!(f)?;
                }
                Ok(())
            }
            Expr::Map(m) => {
                Self::fmt_node(f, &format!("Map{{{}}}", m.keys.len()), &m.span)?;
                for (i, (k, v)) in m.keys.iter().zip(m.values.iter()).enumerate() {
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "entry {i}:")?;
                    w_ind!(f, indent + 2)?;
                    writeln!(f, "key:")?;
                    self.fmt_expr(*k, indent + 3, f)?;
                    writeln!(f)?;
                    w_ind!(f, indent + 2)?;
                    writeln!(f, "value:")?;
                    self.fmt_expr(*v, indent + 3, f)?;
                    writeln!(f)?;
                }
                Ok(())
            }
            Expr::Ternary(t) => {
                Self::fmt_node(f, "Ternary", &t.span)?;
                self.fmt_child(f, indent, "cond", t.cond)?;
                self.fmt_child(f, indent, "then", t.then_val)?;
                self.fmt_child(f, indent, "else", t.else_val)
            }
            Expr::Binary(b) => {
                Self::fmt_node(f, &format!("Binary({:?})", b.op), &b.span)?;
                self.fmt_child(f, indent, "left", b.left)?;
                self.fmt_child(f, indent, "right", b.right)
            }
            Expr::Unary(u) => {
                Self::fmt_node(f, &format!("Unary({:?})", u.op), &u.span)?;
                self.fmt_child(f, indent, "operand", u.operand)
            }
            Expr::Property(p) => {
                Self::fmt_node(f, &format!("Property(\"{}\")", self.r(p.property)), &p.span)?;
                self.fmt_child(f, indent, "object", p.object)
            }
            Expr::Subscript(s) => {
                Self::fmt_node(f, "Subscript", &s.span)?;
                self.fmt_child(f, indent, "object", s.object)?;
                self.fmt_child(f, indent, "index", s.index)?;
                if let Some(end) = s.end {
                    self.fmt_child(f, indent, "end", end)?;
                }
                Ok(())
            }
            Expr::Call(c) => {
                Self::fmt_node(f, &format!("Call method=\"{}\"", self.r(c.method)), &c.span)?;
                self.fmt_child(f, indent, "target", c.target)?;
                self.fmt_args(&c.args, indent, f)
            }
            Expr::Action(a) => self.fmt_action_expr(a, indent, f),
            Expr::Constructor(c) => {
                Self::fmt_node(f, &format!("Constructor \"{}\"", self.r(c.name)), &c.span)?;
                self.fmt_args(&c.args, indent, f)
            }
            Expr::Cast(c) => {
                Self::fmt_node(f, &format!("Cast as {}", self.r(c.ty)), &c.span)?;
                self.fmt_child(f, indent, "expr", c.expr)
            }
            Expr::Match(m) => {
                Self::fmt_node(f, "MatchExpr", &m.span)?;
                self.fmt_child(f, indent, "expr", m.expr)?;
                for (i, arm) in m.arms.iter().enumerate() {
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "arm[{i}]:")?;
                    for p in &arm.patterns {
                        self.fmt_child(f, indent + 2, "pattern", *p)?;
                    }
                    if let Some(g) = arm.guard {
                        self.fmt_child(f, indent + 2, "guard", g)?;
                    }
                    self.fmt_body(&arm.body, indent + 2, f)?;
                }
                Ok(())
            }
            Expr::Lambda(l) => {
                Self::fmt_node(f, "Lambda", &l.span)?;
                w_ind!(f, indent + 1)?;
                write!(f, "params: [")?;
                for (i, p) in l.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", self.r(p.name))?;
                    if let Some(ty) = p.ty {
                        write!(f, ": {}", self.r(ty))?;
                    }
                }
                writeln!(f, "]")?;
                if let Some(ret) = l.return_type {
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "return_type: {}", self.r(ret))?;
                }
                match &l.body {
                    LambdaBody::Expr(e) => self.fmt_child(f, indent + 1, "body", *e),
                    LambdaBody::Block(stmts) => self.fmt_body(stmts, indent + 1, f),
                }
            }
        }
    }

    fn fmt_action_expr(
        &self,
        action: &ActionExpr,
        indent: usize,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        Self::fmt_node(
            f,
            &format!("Action {}.{}", self.r(action.object), self.r(action.name)),
            &action.span,
        )?;
        if let Some(selector) = action.selector {
            w_ind!(f, indent + 1)?;
            writeln!(f, "selector: {}", self.r(selector))?;
        }
        if let Some(invert) = action.invert {
            w_ind!(f, indent + 1)?;
            writeln!(f, "invert: {invert}")?;
        }
        self.fmt_args(&action.args, indent, f)?;
        if let Some(operations) = &action.operations {
            w_ind!(f, indent + 1)?;
            writeln!(f, "operations:")?;
            for operation in operations {
                self.fmt_stmt(operation, indent + 2, f)?;
                writeln!(f)?;
            }
        }
        if let Some(lambda) = &action.lambda {
            w_ind!(f, indent + 1)?;
            writeln!(f, "lambda:")?;
            for expression in lambda {
                self.fmt_expr(*expression, indent + 2, f)?;
                writeln!(f)?;
            }
        }
        Ok(())
    }

    #[instrument(skip(self, f), level = "trace")]
    fn fmt_args(&self, args: &[ArgExpr], indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        for (i, arg) in args.iter().enumerate() {
            w_ind!(f, indent + 1)?;
            write!(f, "arg {i}")?;
            if let Some(name) = arg.name {
                write!(f, " ({})", self.r(name))?;
            }
            if arg.is_ref {
                write!(f, " ref")?;
            }
            if arg.spread > 0 {
                write!(f, " spread={}", arg.spread)?;
            }
            writeln!(f, ":")?;
            self.fmt_expr(arg.value, indent + 2, f)?;
            writeln!(f)?;
        }
        Ok(())
    }

    #[instrument(skip(self, f), level = "trace")]
    fn fmt_params(&self, params: &[Param], indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        if params.is_empty() {
            return Ok(());
        }
        w_ind!(f, indent)?;
        writeln!(f, "params:")?;
        for (i, p) in params.iter().enumerate() {
            w_ind!(f, indent + 1)?;
            write!(f, "[{}] {}", i, self.r(p.name))?;
            if p.is_ref {
                write!(f, " ref")?;
            }
            if let Some(ty) = p.ty {
                write!(f, " : {}", self.r(ty))?;
            }
            if let Some(def) = p.default {
                writeln!(f, " =")?;
                self.fmt_expr(def, indent + 2, f)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }

    #[instrument(skip(self, f), level = "trace")]
    fn fmt_body(&self, body: &[Statement], indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        w_ind!(f, indent)?;
        writeln!(f, "body:")?;
        for s in body {
            self.fmt_stmt(s, indent + 1, f)?;
            writeln!(f)?;
        }
        Ok(())
    }

    #[instrument(skip(self, f), level = "trace")]
    #[expect(
        clippy::too_many_lines,
        reason = "pretty-print dispatch for every statement kind"
    )]
    #[expect(
        clippy::cognitive_complexity,
        reason = "pretty-print dispatch for every statement kind"
    )]
    fn fmt_stmt(&self, stmt: &Statement, indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        w_ind!(f, indent)?;
        match stmt {
            Statement::Import(i) => Self::fmt_inline(
                f,
                &format!("Import \"{}\" {:?}", self.r(i.path), i.kind),
                &i.span,
            ),
            Statement::Function(fd) => {
                let mut s = format!("Function \"{}\"", self.r(fd.name));
                if fd.is_inline {
                    s.push_str(" [inline]");
                }
                if fd.is_exported {
                    s.push_str(" [exported]");
                }
                Self::fmt_node(f, &s, &fd.span)?;
                self.fmt_params(&fd.params, indent, f)?;
                if let Some(rt) = fd.return_type {
                    w_ind!(f, indent)?;
                    writeln!(f, "return_type: {}", self.r(rt))?;
                }
                self.fmt_body(&fd.body, indent, f)
            }
            Statement::Process(pd) => {
                let mut s = format!("Process \"{}\"", self.r(pd.name));
                if pd.is_exported {
                    s.push_str(" [exported]");
                }
                Self::fmt_node(f, &s, &pd.span)?;
                self.fmt_params(&pd.params, indent, f)?;
                self.fmt_body(&pd.body, indent, f)
            }
            Statement::Event(ed) => {
                Self::fmt_node(f, &format!("Event \"{}\"", self.r(ed.event_name)), &ed.span)?;
                self.fmt_body(&ed.body, indent, f)
            }
            Statement::Class(cd) => {
                let mut s = format!("Class \"{}\"", self.r(cd.name));
                if cd.is_inline {
                    s.push_str(" [inline]");
                }
                if let Some(parent) = cd.parent {
                    s.push_str(&format!(" : {}", self.r(parent)));
                }
                if !cd.implements.is_empty() {
                    let ifaces: Vec<String> = cd
                        .implements
                        .iter()
                        .map(|i| self.r(*i).to_owned())
                        .collect();
                    s.push_str(&format!(" implements {}", ifaces.join(", ")));
                }
                Self::fmt_node(f, &s, &cd.span)?;
                self.fmt_body(&cd.body, indent, f)
            }
            Statement::Interface(id) => {
                let mut s = format!("Interface \"{}\"", self.r(id.name));
                if !id.generics.is_empty() {
                    let gens: Vec<String> =
                        id.generics.iter().map(|g| self.r(*g).to_owned()).collect();
                    s.push_str(&format!("<{}>", gens.join(", ")));
                }
                if !id.parents.is_empty() {
                    let parents: Vec<String> =
                        id.parents.iter().map(|p| self.r(*p).to_owned()).collect();
                    s.push_str(&format!(" : {}", parents.join(", ")));
                }
                Self::fmt_node(f, &s, &id.span)?;
                self.fmt_body(&id.body, indent, f)
            }
            Statement::Enum(ed) => {
                Self::fmt_inline(f, &format!("Enum \"{}\"", self.r(ed.name)), &ed.span)?;
                writeln!(f)?;
                w_ind!(f, indent + 1)?;
                write!(f, "values: [")?;
                let vals: Vec<String> = ed
                    .values
                    .iter()
                    .map(|v| format!("\"{}\"", self.r(*v)))
                    .collect();
                write!(f, "{}]", vals.join(", "))
            }
            Statement::TypeAlias(ta) => {
                let mut s = format!("TypeAlias \"{}\"", self.r(ta.name));
                if !ta.generics.is_empty() {
                    s.push_str(&format!(
                        "<{}>",
                        ta.generics
                            .iter()
                            .map(|g| self.r(*g))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                Self::fmt_node(f, &s, &ta.span)?;
                w_ind!(f, indent + 1)?;
                writeln!(f, "target: {}", self.r(ta.target_ty))
            }
            Statement::If(ifst) => self.fmt_if_stmt(ifst, indent, f),
            Statement::While(w) => self.fmt_while_stmt(w, indent, f),
            Statement::For(for_stmt) => self.fmt_for_stmt(for_stmt, indent, f),
            Statement::Break(b) => {
                if let Some(lbl) = b.label {
                    Self::fmt_inline(f, &format!("Break '{}", self.r(lbl)), &b.span)
                } else {
                    Self::fmt_inline(f, "Break", &b.span)
                }
            }
            Statement::VarDecl(declaration) => self.fmt_var_decl_stmt(declaration, indent, f),
            Statement::Assign(assignment) => self.fmt_assign_stmt(assignment, indent, f),
            Statement::Return(r) => {
                Self::fmt_inline(f, "Return", &r.span)?;
                if let Some(v) = r.value {
                    writeln!(f)?;
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "value:")?;
                    self.fmt_expr(v, indent + 2, f)?;
                }
                Ok(())
            }
            Statement::Expr(e) => {
                writeln!(f, "Expr:")?;
                self.fmt_expr(*e, indent + 1, f)
            }
            Statement::Match(m) => {
                Self::fmt_node(f, "MatchStmt", &m.span)?;
                self.fmt_child(f, indent, "expr", m.expr)?;
                for (i, arm) in m.arms.iter().enumerate() {
                    w_ind!(f, indent + 1)?;
                    writeln!(f, "arm[{i}]:")?;
                    for p in &arm.patterns {
                        self.fmt_child(f, indent + 2, "pattern", *p)?;
                    }
                    if let Some(g) = arm.guard {
                        self.fmt_child(f, indent + 2, "guard", g)?;
                    }
                    self.fmt_body(&arm.body, indent + 2, f)?;
                }
                Ok(())
            }
            Statement::TryCatch(tc) => {
                Self::fmt_node(f, "TryCatch", &tc.span)?;
                self.fmt_stmts_block(f, indent, "try", &tc.try_body)?;
                let mut catch_title = "catch".to_owned();
                if let Some(var) = &tc.catch_var {
                    catch_title.push_str(&format!(" ({})", self.text_str(var)));
                }
                if let Some(ty) = tc.catch_type {
                    catch_title.push_str(&format!(": {}", self.r(ty)));
                }
                self.fmt_stmts_block(f, indent, &catch_title, &tc.catch_body)
            }
            Statement::Throw(th) => {
                let mut s = "Throw".to_owned();
                if let Some(ty) = th.exception_type {
                    s.push_str(&format!(" [{}]", self.r(ty)));
                }
                Self::fmt_inline(f, &s, &th.span)?;
                if let Some(v) = th.value {
                    writeln!(f)?;
                    self.fmt_child(f, indent, "value", v)?;
                }
                Ok(())
            }
        }
    }

    fn fmt_if_stmt(&self, if_stmt: &IfStmt, indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        Self::fmt_node(
            f,
            &format!("If{}", if if_stmt.is_not { "!" } else { "" }),
            &if_stmt.span,
        )?;
        self.fmt_stmts_block(f, indent, "then", &if_stmt.then_body)?;
        for (condition, body) in &if_stmt.elif_branches {
            w_ind!(f, indent + 1)?;
            writeln!(f, "elif:")?;
            w_ind!(f, indent + 2)?;
            writeln!(f, "cond:")?;
            self.fmt_expr(*condition, indent + 3, f)?;
            writeln!(f)?;
            self.fmt_stmts_block(f, indent + 1, "body", body)?;
        }
        if let Some(body) = &if_stmt.else_body {
            w_ind!(f, indent + 1)?;
            writeln!(f, "else:")?;
            for statement in body {
                self.fmt_stmt(statement, indent + 2, f)?;
                writeln!(f)?;
            }
        }
        Ok(())
    }

    fn fmt_while_stmt(&self, w: &WhileStmt, indent: usize, f: &mut Formatter<'_>) -> fmt::Result {
        let label_str = w
            .label
            .map(|l| format!(" '{}:", self.r(l)))
            .unwrap_or_default();
        Self::fmt_node(
            f,
            &format!("While{}{}", label_str, if w.is_not { "!" } else { "" }),
            &w.span,
        )?;
        w_ind!(f, indent + 1)?;
        writeln!(f, "cond:")?;
        self.fmt_expr(w.condition, indent + 2, f)?;
        writeln!(f)?;
        self.fmt_stmts_block(f, indent, "body", &w.body)
    }

    fn fmt_for_stmt(
        &self,
        for_stmt: &ForStmt,
        indent: usize,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        let label_str = for_stmt
            .label
            .map(|l| format!(" '{}:", self.r(l)))
            .unwrap_or_default();
        Self::fmt_node(f, &format!("For{label_str}"), &for_stmt.span)?;
        w_ind!(f, indent + 1)?;
        writeln!(f, "iterable:")?;
        self.fmt_expr(for_stmt.iterable, indent + 2, f)?;
        writeln!(f)?;
        self.fmt_stmts_block(f, indent, "body", &for_stmt.body)
    }

    fn fmt_var_decl_stmt(
        &self,
        declaration: &VarDecl,
        indent: usize,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        let mut description = String::from("VarDecl");
        if declaration.is_exported {
            description.push_str(" [exported]");
        }
        description.push(' ');
        for (index, name) in declaration.names.iter().enumerate() {
            if index > 0 {
                description.push_str(", ");
            }
            if let Some(scope) = declaration.scopes.get(index).and_then(|scope| *scope) {
                description.push_str(&format!("[{scope:?}] "));
            }
            description.push_str(&self.text_str(name));
            if let Some(Some(ty)) = declaration.tys.get(index) {
                description.push_str(&format!(" : {}", self.r(*ty)));
            }
        }
        Self::fmt_inline(f, &description, &declaration.span)?;
        if let Some(value) = declaration.value {
            writeln!(f)?;
            self.fmt_child(f, indent, "value", value)?;
        }
        Ok(())
    }

    fn fmt_assign_stmt(
        &self,
        assignment: &AssignStmt,
        indent: usize,
        f: &mut Formatter<'_>,
    ) -> fmt::Result {
        Self::fmt_inline(f, "Assign", &assignment.span)?;
        if let Some(operator) = assignment.op {
            writeln!(f)?;
            w_ind!(f, indent + 1)?;
            write!(f, "op: \"{}\"", self.r(operator))?;
        }
        writeln!(f)?;
        w_ind!(f, indent + 1)?;
        writeln!(f, "targets:")?;
        for target in &assignment.targets {
            self.fmt_expr(*target, indent + 2, f)?;
            writeln!(f)?;
        }
        w_ind!(f, indent + 1)?;
        writeln!(f, "value:")?;
        self.fmt_expr(assignment.value, indent + 2, f)
    }
}
