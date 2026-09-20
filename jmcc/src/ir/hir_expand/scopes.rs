//! Scopes and name bindings: variable declarations, binding lookups,
//! literal placeholders, and variable locality checking.

use super::*;

impl OverloadExpander<'_> {
    pub(super) fn declared_var(
        &mut self,
        decl: &VarDecl,
        index: usize,
        name: &TextValue,
    ) -> Result<Id> {
        let name = self.sym_str_from_text(name)?;
        let scope = decl_scope(&decl.scopes, index, VarScope::Line);
        self.scoped_var(name, scope)
    }

    pub(super) fn scoped_var(&mut self, name: Symbol, scope: VarScope) -> Result<Id> {
        let raw = self.add(Hir::Var(VarName(name)));
        match scope {
            VarScope::Game => Ok(self.add(Hir::Game(raw))),
            VarScope::Save => Ok(self.add(Hir::Save(raw))),
            VarScope::Line => Ok(self.add(Hir::Line(raw))),
            VarScope::Local => Ok(self.add(Hir::Local(raw))),
            _ => Err(OverloadError::UnsupportedVarScope(scope)),
        }
    }

    /// Variable placeholder for literals: based on binding, defaults to `line`.
    ///
    /// The `trace!` about undeclared names is left to the caller, as `$variable::…`
    /// does not emit one.
    pub(super) fn var_placeholder(&self, sym: Symbol) -> String {
        match self.lookup_binding(sym) {
            Some(ExpanderBinding::Var { name, scope }) => get_var_placeholder(name, *scope),
            None => get_var_placeholder(&sym, VarScope::Line),
        }
    }

    pub(super) fn lookup_binding(&self, name: Symbol) -> Option<&ExpanderBinding> {
        self.scopes.iter().rev().find_map(|s| s.get(&name))
    }
}
