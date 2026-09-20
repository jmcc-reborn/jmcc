//! Scopes and name bindings: `scopes` stack and `inline_vars`,
//! `declare` / `lookup`, `wrap_scope`, and fresh symbols.

use super::*;

impl HirBuilder<'_> {
    pub(super) fn sym(&self, id: StrId) -> Symbol {
        Symbol::from(self.ast.strings.resolve(&id))
    }

    pub(super) fn fresh(&mut self) -> Symbol {
        fresh_temp(&mut self.temp_n)
    }

    pub(super) fn inline_vars(&self) -> &HashMap<Symbol, Id> {
        self.inline_vars_stack.last().unwrap()
    }

    pub(super) fn inline_vars_mut(&mut self) -> &mut HashMap<Symbol, Id> {
        self.inline_vars_stack.last_mut().unwrap()
    }

    pub(super) fn lookup(&self, name: Symbol) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|s| s.get(&name))
    }

    pub(super) fn push(&mut self) {
        self.scopes.push(HashMap::new());
        self.inline_vars_stack.push(self.inline_vars().clone());
    }

    pub(super) fn pop(&mut self) {
        self.scopes.pop();
        self.inline_vars_stack.pop();
    }

    pub(super) fn declare(&mut self, name: Symbol, b: Binding) {
        let idx = match &b {
            Binding::Var { scope, .. } if *scope == VarScope::Game || *scope == VarScope::Save => 0,
            _ => self.scopes.len() - 1,
        };
        self.scopes[idx].insert(name, b);
    }

    pub(super) fn lang_item(&self, name: &str) -> DefId {
        self.ir_ctx.lang_items.get(name).copied().unwrap_or(0)
    }

    pub(super) fn get_class_name(&self, ty: &Type) -> Option<String> {
        self.ir_ctx.get_class_name(ty)
    }

    pub(super) fn wrap_scope(&mut self, var: Id, scope: VarScope) -> Id {
        match scope {
            VarScope::Game => self.add(Hir::Game(var)),
            VarScope::Save => self.add(Hir::Save(var)),
            VarScope::Line => self.add(Hir::Line(var)),
            VarScope::Local => self.add(Hir::Local(var)),
            _ => var,
        }
    }

    pub(super) fn binding_to_string(&self, sym: Symbol) -> String {
        match self.lookup(sym) {
            Some(&Binding::Var { scope, .. }) => get_var_placeholder(&sym, scope),
            Some(&Binding::Func { name } | &Binding::Proc { name }) => name.as_str().to_owned(),
            None => {
                trace!(name = %sym, scope = ?self.default_scope, "Undeclared variable in string interpolation, assuming default scope");
                get_var_placeholder(&sym, self.default_scope)
            }
        }
    }
}
