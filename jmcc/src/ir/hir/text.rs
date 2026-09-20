//! Text literals: `Text` nodes, `$`-substitutions, and converting names and
//! expressions to strings.

use super::*;

impl HirBuilder<'_> {
    #[instrument(skip(self, tv), level = "trace")]
    pub(super) fn conv_text(&mut self, tv: &TextValue) -> Result<Id, IrError> {
        let tp = match tv.parsing {
            TextParsing::Plain => "plain",
            TextParsing::Legacy => "legacy",
            TextParsing::MiniMessage => "minimessage",
            TextParsing::Json => "json",
        };
        let tid = self.str_lit(tp);
        let mut bindings = Vec::new();
        let mut parts = Vec::new();
        for p in &tv.parts {
            match p {
                TextPart::Literal(s) => {
                    let expanded = self.expand_dollar_vars_in_literal(&self.sym(*s).to_string())?;
                    parts.push(self.str_lit(expanded));
                }
                TextPart::Interp(eid) => {
                    let (a, b) = self.atomize(*eid)?;
                    parts.push(a);
                    bindings.extend(b);
                }
            }
        }
        let content = if parts.len() == 1 {
            parts[0]
        } else {
            self.add(Hir::Concat(parts.into_boxed_slice()))
        };
        let node = self.add(Hir::Text([tid, content]));
        Ok(self.wrap_lets(node, bindings))
    }

    #[instrument(skip(self, s), level = "trace")]
    fn expand_dollar_vars_in_literal(&self, s: &str) -> Result<String, IrError> {
        Ok(expand_dollar_vars(s, |var_name, is_braced, out| {
            if let Some(expanded) = self.expand_special_dollar_var(var_name, is_braced) {
                out.push_str(&expanded);
                return;
            }

            let var_sym = Symbol::from(var_name);

            if let Some(&inline_id) = self.inline_vars().get(&var_sym)
                && let Some(resolved) = self.eval_inline_var_to_string(inline_id)
            {
                out.push_str(&resolved);
                return;
            }

            match self.lookup(var_sym) {
                Some(&Binding::Var { name, scope }) => {
                    out.push_str(&get_var_placeholder(&name, scope));
                }
                Some(&Binding::Func { name } | &Binding::Proc { name }) => {
                    out.push_str(name.as_str());
                }
                None => {
                    trace!(name = %var_sym, scope = ?self.default_scope, "Undeclared variable in literal, assuming default scope");
                    out.push_str(&get_var_placeholder(&var_sym, self.default_scope));
                }
            }
        }))
    }

    fn expand_special_dollar_var(&self, name: &str, is_braced: bool) -> Option<String> {
        let (kind, value) = name.split_once("::")?;
        match kind.trim() {
            "value" => {
                let value = value.trim();
                Some(value.find('<').map_or_else(
                    || format!("%value {value}"),
                    |bracket| {
                        format!(
                            "%value {}<{}>",
                            value[..bracket].trim(),
                            value[bracket + 1..].trim_end_matches('>').trim()
                        )
                    },
                ))
            }
            "variable" => {
                let symbol = Symbol::from(value.trim());
                let Some(&Binding::Var { scope, .. }) = self.lookup(symbol) else {
                    unreachable!()
                };
                Some(get_var_placeholder(&symbol, scope))
            }
            _ if is_braced => Some(format!("${{{name}}}")),
            _ => Some(format!("${name}")),
        }
    }

    #[instrument(skip(self, tv), level = "trace")]
    pub(super) fn eval_var_name(&self, tv: &TextValue) -> Result<Symbol, IrError> {
        let mut name = String::new();
        for part in &tv.parts {
            match part {
                TextPart::Literal(s) => name.push_str(self.ast.strings.resolve(s)),
                TextPart::Interp(eid) => {
                    name.push_str(&self.eval_expr_to_string(&self.ast.exprs[*eid])?);
                }
            }
        }
        Ok(Symbol::from(name))
    }

    #[instrument(skip(self, inline_id), level = "trace")]
    fn eval_inline_var_to_string(&self, inline_id: Id) -> Option<String> {
        match self.get(inline_id) {
            Hir::Str(s) => Some(s.0.to_string()),
            Hir::Num(n) => Some(n.0.to_string()),
            Hir::Bool(b) => Some(b.to_string()),
            Hir::Var(VarName(name)) => {
                let sym = *name;
                match self.lookup(sym) {
                    Some(&Binding::Var { scope, .. }) => Some(get_var_placeholder(&sym, scope)),
                    Some(&Binding::Func { name } | &Binding::Proc { name }) => {
                        Some(name.as_str().to_owned())
                    }
                    None => None,
                }
            }
            Hir::Text([_, c]) => match self.get(*c) {
                Hir::Str(s) => Some(s.0.to_string()),
                _ => None,
            },
            Hir::Game(inner) | Hir::Save(inner) | Hir::Line(inner) | Hir::Local(inner) => {
                if let Hir::Var(VarName(name)) = self.get(*inner) {
                    let scope = match self.get(inline_id) {
                        Hir::Game(_) => VarScope::Game,
                        Hir::Save(_) => VarScope::Save,
                        Hir::Line(_) => VarScope::Line,
                        Hir::Local(_) => VarScope::Local,
                        _ => return None,
                    };
                    Some(get_var_placeholder(name, scope))
                } else {
                    None
                }
            }
            Hir::Concat(ids) => {
                let mut out = String::new();
                for id in ids {
                    out.push_str(&self.eval_inline_var_to_string(*id)?);
                }
                Some(out)
            }
            _ => None,
        }
    }

    #[instrument(skip(self, expr), level = "trace")]
    fn eval_expr_to_string(&self, expr: &Expr) -> Result<String, IrError> {
        match expr {
            Expr::Ident(n, _) => {
                let sym = self.sym(*n);
                if let Some(&inline_id) = self.inline_vars().get(&sym)
                    && let Some(s) = self.eval_inline_var_to_string(inline_id)
                {
                    return Ok(s);
                }
                Ok(self.binding_to_string(sym))
            }
            Expr::Text(tv) => {
                let mut s = String::new();
                for part in &tv.parts {
                    match part {
                        TextPart::Literal(l) => s.push_str(self.ast.strings.resolve(l)),
                        TextPart::Interp(eid) => {
                            s.push_str(&self.eval_expr_to_string(&self.ast.exprs[*eid])?);
                        }
                    }
                }
                Ok(s)
            }
            Expr::Variable(v) => {
                let sym = self.eval_var_name(&v.name)?;
                Ok(self.binding_to_string(sym))
            }
            Expr::Number(n) => Ok(n.value.to_string()),
            Expr::Bool(b) => Ok(b.value.to_string()),
            _ => Err(IrError::InvalidVarNameExpr),
        }
    }
}
