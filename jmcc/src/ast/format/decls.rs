//! Formatting declarations: function/process/event/class/enum/typealias, imports,
//! parameters, and generic argument lists.

use super::text::{escape_backtick_string, escape_string, is_valid_plain_ident};
use super::*;

impl Formatter<'_> {
    #[instrument(skip(self, i), level = "trace")]
    pub(super) fn fmt_import(&mut self, i: &ImportStmt) {
        self.ind();
        self.w("import ");
        match &i.kind {
            ImportKind::SideEffect => {
                self.w("\"");
                self.w(&escape_string(self.r(i.path), '"'));
                self.w("\";");
            }
            ImportKind::Default(name) => {
                self.w(self.r(*name));
                self.w(" from \"");
                self.w(&escape_string(self.r(i.path), '"'));
                self.w("\";");
            }
            ImportKind::Namespace(name) => {
                self.w("* as ");
                self.w(self.r(*name));
                self.w(" from \"");
                self.w(&escape_string(self.r(i.path), '"'));
                self.w("\";");
            }
            ImportKind::Named(items) => {
                self.fmt_list_like("{ ", " }", items, |s, item| {
                    s.w(s.r(item.original));
                    if item.original != item.local {
                        s.w(" as ");
                        s.w(s.r(item.local));
                    }
                });
                self.w(" from \"");
                self.w(&escape_string(self.r(i.path), '"'));
                self.w("\";");
            }
        }
    }

    /// Formats the common declaration prefix: indentation, modifiers, keyword, and
    /// name.
    ///
    /// The modifier order differs by declaration kind (e.g. `inline export` for functions,
    /// `export inline` for classes), so it is specified by caller rather than hardcoded.
    pub(super) fn fmt_decl_start(
        &mut self,
        modifiers: impl IntoIterator<Item = Option<&'static str>>,
        keyword: &str,
        name: StrId,
    ) {
        self.ind();
        for modifier in modifiers.into_iter().flatten() {
            self.w(modifier);
        }
        self.w(keyword);
        let s = self.r(name);
        if is_valid_plain_ident(s) {
            self.w(s);
        } else {
            self.w("`");
            self.w(&escape_backtick_string(s));
            self.w("`");
        }
    }

    pub(super) fn fmt_aliases(&mut self, aliases: &[StrId]) {
        for a in aliases {
            self.ind();
            self.w("@alias(\"");
            self.w(&escape_string(self.r(*a), '"'));
            self.w("\")\n");
        }
    }

    #[instrument(skip(self, f), level = "trace")]
    pub(super) fn fmt_function(&mut self, f: &FunctionDecl) {
        self.fmt_aliases(&f.aliases);
        if let Some(t) = &f.test_attr {
            if t.is_test {
                self.ind();
                self.w("@test\n");
            }
            if t.should_panic {
                self.ind();
                if let Some(exp) = &t.expected_panic {
                    self.w("@should_panic(\"");
                    self.w(&escape_string(exp, '"'));
                    self.w("\")\n");
                } else {
                    self.w("@should_panic\n");
                }
            }
            if t.is_ignore {
                self.ind();
                if let Some(reason) = &t.ignore_reason {
                    self.w("@ignore(\"");
                    self.w(&escape_string(reason, '"'));
                    self.w("\")\n");
                } else {
                    self.w("@ignore\n");
                }
            }
        }
        if f.is_getter {
            self.ind();
            self.w("@getter\n");
        }
        if f.is_setter {
            self.ind();
            self.w("@setter\n");
        }
        if f.is_overload {
            self.ind();
            self.w("@overload\n");
        }
        self.fmt_decl_start(
            [
                f.is_exported.then_some("export "),
                f.is_inline.then_some("inline "),
            ],
            "function ",
            f.name,
        );
        if !f.generics.is_empty() {
            self.fmt_list_like("<", ">", &f.generics, |s, g| s.w(s.r(*g)));
        }
        let extra_len = f.return_type.map_or(0, |rt| 4 + self.r(rt).len()) + 2;
        self.fmt_params_with_extra(&f.params, extra_len);
        if let Some(rt) = f.return_type {
            self.w(" -> ");
            self.w(self.r(rt));
        }
        let is_proto = f.body.is_empty()
            && self
                .source
                .get(f.span.clone())
                .is_some_and(|s| s.trim_end().ends_with(';'));
        if is_proto {
            self.w(";");
        } else {
            self.w(" ");
            if f.body.is_empty() {
                // An empty body may still carry comments such as the
                // `/* compiler built-in */` marker, so reproduce them instead
                // of collapsing the block to `{}`.
                let sig_end = self.signature_end(f.span.start);
                let inner = self.body_inner_range(sig_end);
                self.fmt_empty_block(inner);
            } else {
                self.fmt_block(&f.body);
            }
        }
    }

    #[instrument(skip(self, p), level = "trace")]
    pub(super) fn fmt_process(&mut self, p: &ProcessDecl) {
        self.fmt_aliases(&p.aliases);
        self.fmt_decl_start([p.is_exported.then_some("export ")], "process ", p.name);
        self.fmt_params(&p.params);
        self.w(" ");
        self.fmt_block(&p.body);
    }

    #[instrument(skip(self, e), level = "trace")]
    pub(super) fn fmt_event(&mut self, e: &EventDecl) {
        self.ind();
        self.w("event<");
        self.w(self.r(e.event_name));
        self.w("> ");
        self.fmt_block(&e.body);
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn fmt_class(&mut self, c: &ClassDecl) {
        self.fmt_aliases(&c.aliases);
        if c.lang_item {
            self.ind();
            self.w("@lang_item\n");
        }
        if c.is_dict {
            self.ind();
            self.w("@dict\n");
        }
        self.fmt_decl_start(
            [
                c.is_exported.then_some("export "),
                c.is_inline.then_some("inline "),
            ],
            "class ",
            c.name,
        );
        if !c.generics.is_empty() {
            self.fmt_list_like("<", ">", &c.generics, |s, g| s.w(s.r(*g)));
        }
        if let Some(parent) = c.parent {
            self.w("(");
            self.w(self.r(parent));
            self.w(")");
        }
        if !c.implements.is_empty() {
            self.w(" implements ");
            self.fmt_comma_separated(&c.implements, |s, iface| s.w(s.r(*iface)));
        }
        self.w(" ");
        self.fmt_block(&c.body);
    }

    #[instrument(skip(self, i), level = "trace")]
    pub(super) fn fmt_interface(&mut self, i: &InterfaceDecl) {
        self.fmt_aliases(&i.aliases);
        self.fmt_decl_start([i.is_exported.then_some("export ")], "interface ", i.name);
        if !i.generics.is_empty() {
            self.fmt_list_like("<", ">", &i.generics, |s, g| s.w(s.r(*g)));
        }
        if !i.parents.is_empty() {
            self.w(" extends ");
            self.fmt_comma_separated(&i.parents, |s, p| s.w(s.r(*p)));
        }
        self.w(" ");
        self.fmt_block(&i.body);
    }

    #[instrument(skip(self, e), level = "trace")]
    pub(super) fn fmt_enum(&mut self, e: &EnumDecl) {
        self.fmt_aliases(&e.aliases);
        self.ind();
        if e.is_exported {
            self.w("export ");
        }
        self.w("enum ");
        self.w(self.r(e.name));
        if e.values.is_empty() {
            self.w(" {}");
            return;
        }
        self.w(" {\n");
        self.indent += 1;
        for (i, v) in e.values.iter().enumerate() {
            self.ind();
            self.w(self.r(*v));
            if i + 1 < e.values.len() {
                self.w(",");
            }
            self.output.push('\n');
        }
        self.indent -= 1;
        self.ind();
        self.w("}");
    }

    /// Formats a single parameter: `ref`, spread flag, name, type, and default
    /// value.
    pub(super) fn fmt_param(&mut self, p: &Param) {
        if p.is_ref {
            self.w("ref ");
        }
        self.fmt_spread(p.spread);
        let s = self.r(p.name);
        if is_valid_plain_ident(s) {
            self.w(s);
        } else {
            self.w("`");
            self.w(&escape_backtick_string(s));
            self.w("`");
        }
        if let Some(ty) = p.ty {
            self.w(": ");
            self.w(self.r(ty));
        }
        if let Some(def) = p.default {
            self.w(" = ");
            self.fmt_expr(def, 0);
        }
    }

    #[instrument(skip(self, params), level = "trace")]
    pub(super) fn fmt_params(&mut self, params: &[Param]) {
        self.fmt_params_with_extra(params, 0);
    }

    pub(super) fn fmt_params_with_extra(&mut self, params: &[Param], extra_len: usize) {
        if params.is_empty() {
            self.w("()");
            return;
        }
        let mut rendered = Vec::with_capacity(params.len());
        self.indent += 1;
        for param in params {
            let s = self.render_to_string(|fmt| fmt.fmt_param(param));
            rendered.push(s);
        }
        self.indent -= 1;
        self.fmt_delimited_list_with_extra("(", ")", &rendered, extra_len);
    }

    /// Formats a single argument: `ref`, spread flag, optional named argument
    /// name, and value expression.
    pub(super) fn fmt_arg(&mut self, arg: &ArgExpr) {
        if arg.is_ref {
            self.w("ref ");
        }
        self.fmt_spread(arg.spread);
        if let Some(name) = arg.name {
            self.w(self.r(name));
            self.w(" = ");
        }
        self.fmt_expr(arg.value, 0);
    }

    #[instrument(skip(self, args), level = "trace")]
    pub(super) fn fmt_args(&mut self, args: &[ArgExpr]) {
        if args.is_empty() {
            self.w("()");
            return;
        }
        let mut rendered = Vec::with_capacity(args.len());
        self.indent += 1;
        for arg in args {
            let s = self.render_to_string(|fmt| fmt.fmt_arg(arg));
            rendered.push(s);
        }
        self.indent -= 1;
        self.fmt_delimited_list("(", ")", &rendered);
    }
}
