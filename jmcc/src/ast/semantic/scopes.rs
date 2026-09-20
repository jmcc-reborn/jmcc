//! Scopes and name declarations: scope stacks, declare/lookup, forward declarations
//! of functions and processes, and symbol construction.

use super::*;

impl Analyzer<'_> {
    pub(super) fn str(&self, id: Spur) -> String {
        self.ast.strings.resolve(&id).to_owned()
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn push_scope(&mut self) {
        trace!("Pushing scope");
        self.scopes.push(HashMap::new());
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn pop_scope(&mut self) {
        trace!("Popping scope");
        self.scopes.pop();
    }

    #[instrument(skip(self, symbol), level = "trace")]
    pub(super) fn declare(&mut self, name: String, symbol: Symbol) {
        let scope_idx = match &symbol {
            Symbol::Var { scope, .. } if *scope == VarScope::Game || *scope == VarScope::Save => 0,
            _ => self.scopes.len() - 1,
        };
        trace!(name = %name, scope_idx, "Declaring symbol");
        self.scopes[scope_idx].insert(name, symbol);
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn lookup(&self, name: &str) -> Option<Symbol> {
        let sym = self.scopes.iter().rev().find_map(|s| s.get(name)).cloned();
        trace!(name = %name, found = sym.is_some(), "Looking up symbol");
        sym
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn predeclare_top_level(&mut self) {
        let ast = self.ast;
        for stmt in &ast.statements {
            match stmt {
                Statement::Function(f) => {
                    let name = self.str(f.name);
                    let symbol = self.function_symbol(f);
                    if self.declared_top_level.insert(name.clone()) {
                        self.declare(name, symbol.clone());
                    } else if let Some(Symbol::Func {
                        span: prev_span, ..
                    }) = self.lookup(&name)
                    {
                        self.error(
                            SemanticErrorKind::DuplicateFunction {
                                name,
                                prev_span: self.format_span(&prev_span),
                            },
                            f.span.clone(),
                        );
                    }
                    for a in &f.aliases {
                        let a_name = self.str(*a);
                        if self.declared_top_level.insert(a_name.clone()) {
                            self.declare(a_name, symbol.clone());
                        }
                    }
                }
                Statement::Process(p) => {
                    let name = self.str(p.name);
                    let symbol = self.process_symbol(p);
                    if self.declared_top_level.insert(name.clone()) {
                        self.declare(name, symbol.clone());
                    } else if let Some(Symbol::Proc {
                        span: prev_span, ..
                    }) = self.lookup(&name)
                    {
                        self.error(
                            SemanticErrorKind::DuplicateProcess {
                                name,
                                prev_span: self.format_span(&prev_span),
                            },
                            p.span.clone(),
                        );
                    }
                    for a in &p.aliases {
                        let a_name = self.str(*a);
                        if self.declared_top_level.insert(a_name.clone()) {
                            self.declare(a_name, symbol.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[instrument(skip(self, stmts), level = "trace")]
    pub(super) fn predeclare_in_block(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            match stmt {
                Statement::Function(f) => {
                    if f.is_getter || f.is_setter {
                        continue;
                    }
                    let name = self.str(f.name);
                    let symbol = self.function_symbol(f);
                    if self.lookup(&name).is_none() {
                        self.declare(name, symbol.clone());
                    }
                    for a in &f.aliases {
                        let a_name = self.str(*a);
                        if self.lookup(&a_name).is_none() {
                            self.declare(a_name, symbol.clone());
                        }
                    }
                }
                Statement::Process(p) => {
                    let name = self.str(p.name);
                    let symbol = self.process_symbol(p);
                    if self.lookup(&name).is_none() {
                        self.declare(name, symbol.clone());
                    }
                    for a in &p.aliases {
                        let a_name = self.str(*a);
                        if self.lookup(&a_name).is_none() {
                            self.declare(a_name, symbol.clone());
                        }
                    }
                }
                Statement::If(i) => {
                    self.predeclare_in_block(&i.then_body);
                    for (_, body) in &i.elif_branches {
                        self.predeclare_in_block(body);
                    }
                    if let Some(body) = &i.else_body {
                        self.predeclare_in_block(body);
                    }
                }
                Statement::Match(m) => {
                    for arm in &m.arms {
                        self.predeclare_in_block(&arm.body);
                    }
                }
                Statement::TryCatch(tc) => {
                    self.predeclare_in_block(&tc.try_body);
                    self.predeclare_in_block(&tc.catch_body);
                }
                _ => {}
            }
        }
    }

    /// Function symbol: parameters and return type inferred from `return` when unannotated.
    fn function_symbol(&mut self, f: &FunctionDecl) -> Symbol {
        let params = self.collect_params(&f.params, None);
        let return_type = self.inferred_return_type(f);
        Symbol::Func {
            params,
            return_type,
            span: f.span.clone(),
        }
    }

    /// Process symbol: parameters only, processes do not have return values.
    fn process_symbol(&mut self, p: &ProcessDecl) -> Symbol {
        let params = self.collect_params(&p.params, None);
        Symbol::Proc {
            params,
            span: p.span.clone(),
        }
    }
}
