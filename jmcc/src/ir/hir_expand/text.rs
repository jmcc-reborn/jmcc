//! Text literals and `$`-substitutions: `Text`/`Concat` assembly and variable
//! interpolation inside string literals.

use super::*;

impl OverloadExpander<'_> {
    pub(super) fn conv_ast_text(&mut self, text: &TextValue) -> Result<Id> {
        let mut parts = Vec::new();
        for part in &text.parts {
            let part = match part {
                TextPart::Literal(value) => {
                    let expanded =
                        self.expand_dollar_vars_in_literal(&self.sym(*value).to_string())?;
                    self.add(Hir::Str(StrLit(Symbol::from(expanded))))
                }
                TextPart::Interp(expr) => self.conv_ast_expr(*expr)?,
            };
            parts.push(part);
        }
        let content = self.block_or_concat(parts);
        let text_type = self.add(Hir::Str(StrLit(Symbol::from("plain"))));
        Ok(self.add(Hir::Text([text_type, content])))
    }

    #[instrument(skip(self, s), level = "trace")]
    fn expand_dollar_vars_in_literal(&self, s: &str) -> Result<String> {
        Ok(expand_dollar_vars(s, |var_name, is_braced, result| {
            if let Some((kind, rest)) = var_name.split_once("::") {
                match kind.trim() {
                    "value" => {
                        let rest = rest.trim();
                        if let Some(bp) = rest.find('<') {
                            result.push_str(&format!(
                                "%value {}<{}>",
                                rest[..bp].trim(),
                                rest[bp + 1..].trim_end_matches('>').trim()
                            ));
                        } else {
                            result.push_str(&format!("%value {rest}"));
                        }
                    }
                    "variable" => {
                        result.push_str(&self.var_placeholder(Symbol::from(rest.trim())));
                    }
                    _ => {
                        let fmt_str = if is_braced {
                            format!("${{{var_name}}}")
                        } else {
                            format!("${var_name}")
                        };
                        result.push_str(&fmt_str);
                    }
                }
                return;
            }

            let var_sym = Symbol::from(var_name);
            if self.lookup_binding(var_sym).is_none() {
                trace!(name = %var_sym, "Undeclared variable in literal, assuming line scope");
            }
            result.push_str(&self.var_placeholder(var_sym));
        }))
    }
}
