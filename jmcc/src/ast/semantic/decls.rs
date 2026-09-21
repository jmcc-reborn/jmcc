//! Объявления: анализ функций, процессов, событий и классов, сбор параметров,
//! вывод типа возврата.

use super::*;

impl Analyzer<'_> {
    #[instrument(skip(self, params), level = "trace")]
    pub(super) fn collect_params(
        &mut self,
        params: &[Param],
        subst: Option<&HashMap<StrId, Type>>,
    ) -> Vec<ParamInfo> {
        params
            .iter()
            .map(|p| {
                let ty = match p.ty {
                    Some(t) => {
                        let raw_ty = self.parse_decl_type(&self.str(t), p.span.clone());
                        if let Some(s) = subst {
                            self.substitute(&raw_ty, s)
                        } else {
                            raw_ty
                        }
                    }
                    None => self.unifier.new_var(),
                };
                ParamInfo {
                    name: self.str(p.name),
                    ty,
                    spread: p.spread,
                    has_default: p.default.is_some(),
                }
            })
            .collect()
    }

    /// Параметры вызываемого с учётом generic-подстановки класса; пустая подстановка — без неё.
    pub(super) fn method_params(
        &mut self,
        class: &ClassInfo,
        params: &[Param],
        subst: &HashMap<StrId, Type>,
    ) -> Vec<ParamInfo> {
        let subst_ref = (!subst.is_empty()).then_some(subst);
        self.with_generic_scope(&class.generics, |this| {
            this.collect_params(params, subst_ref)
        })
    }

    #[instrument(skip(self, body, params), level = "trace")]
    fn analyze_callable(
        &mut self,
        body: &[Statement],
        params: &[Param],
        return_type: Option<Type>,
        is_function: bool,
    ) {
        self.push_scope();
        self.predeclare_in_block(body);
        let prev_in_function = self.in_function;
        let prev_in_process = self.in_process;
        let prev_ret = self.function_return_type.take();
        let prev_event = self.current_event.take();
        self.in_function = is_function;
        self.in_process = !is_function;
        self.function_return_type = if is_function { return_type } else { None };
        for p in params {
            let ty = match p.ty {
                Some(t) => self.parse_decl_type(&self.str(t), p.span.clone()),
                None => self.unifier.new_var(),
            };
            // Recorded for `open_param_vars`: a parameter that never gets inferred is not an error.
            if let Type::InferVar(id) = ty {
                self.open_param_vars.insert(id);
            }
            self.declare(self.str(p.name), Symbol::Param { ty });
        }
        for s in body {
            self.analyze_stmt(s);
        }
        self.in_function = prev_in_function;
        self.in_process = prev_in_process;
        self.function_return_type = prev_ret;
        self.current_event = prev_event;
        self.pop_scope();
    }

    /// Return type for a predeclared function: the annotation, else inferred from `return <value>`.
    ///
    /// `JustMC` projects rarely annotate it and still use the call in an expression
    /// (`s = formatItemList(items)`). The inferred type is a fresh var that `analyze_return` binds.
    pub(super) fn inferred_return_type(&mut self, f: &FunctionDecl) -> Option<Type> {
        if let Some(t) = f.return_type {
            return Some(self.parse_decl_type(&self.str(t), f.span.clone()));
        }
        has_value_return(&f.body).then(|| self.unifier.new_var())
    }

    #[instrument(skip(self, f), level = "trace")]
    pub(super) fn analyze_function(&mut self, f: &FunctionDecl) {
        self.with_generic_scope(&f.generics, |this| {
            let full_name = this.str(f.name);
            // Take the type from the symbol predeclared earlier: the `return` inference must land in
            // the same variable. Matching the declaration span tells this function from a namesake
            // declared in another scope.
            let return_type = match this.lookup(&full_name) {
                Some(Symbol::Func {
                    return_type, span, ..
                }) if span == f.span => return_type,
                _ => this.inferred_return_type(f),
            };
            let short_name = full_name
                .rsplit("::")
                .next()
                .unwrap_or(&full_name)
                .to_owned();

            let is_property = f.is_getter || f.is_setter;
            if is_property {
                this.getter_setter_stack.push(short_name);
            }

            this.analyze_callable(&f.body, &f.params, return_type, true);

            if is_property {
                this.getter_setter_stack.pop();
            }
        });
    }

    #[instrument(skip(self, p), level = "trace")]
    pub(super) fn analyze_process(&mut self, p: &ProcessDecl) {
        self.analyze_callable(&p.body, &p.params, None, false);
    }

    #[instrument(skip(self, e), level = "trace")]
    pub(super) fn analyze_event(&mut self, e: &EventDecl) {
        let ev_name = self.str(e.event_name);
        let prev_event = self.current_event.replace(ev_name);
        self.analyze_block(&e.body);
        self.current_event = prev_event;
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn analyze_class(&mut self, c: &ClassDecl) {
        let class_name = self.str(c.name);

        if let Some(parent_str_id) = c.parent {
            let parent_name = self.str(parent_str_id);
            if let Some(parent_info) = self.get_class(&parent_name).cloned() {
                let mut current_def = parent_info.def_id;
                let mut visited = HashSet::new();
                visited.insert(current_def);

                while let Some(current_class) = self.ir_ctx.classes_by_def.get(&current_def) {
                    let parent = current_class.parent;
                    // Своё имя в цепочке родителей или повтор уже пройденного узла — цикл.
                    if current_class.name == class_name
                        || parent.is_some_and(|p| !visited.insert(p))
                    {
                        self.error(
                            SemanticErrorKind::CyclicInheritance { class: class_name },
                            c.span.clone(),
                        );
                        break;
                    }
                    let Some(parent) = parent else { break };
                    current_def = parent;
                }
            }
        }

        self.with_generic_scope(&c.generics, |this| {
            this.analyze_block(&c.body);
        });
    }

    #[instrument(skip(self, i), level = "trace")]
    pub(super) fn analyze_interface(&mut self, i: &InterfaceDecl) {
        self.with_generic_scope(&i.generics, |this| {
            this.analyze_block(&i.body);
        });
    }
}

/// Whether the body contains a `return <value>`, i.e. the function has a result.
///
/// Nested declarations are not entered: their `return` belongs to them, not to the outer function.
pub(super) fn has_value_return(stmts: &[Statement]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Statement::Return(r) => r.value.is_some(),
        Statement::If(i) => {
            has_value_return(&i.then_body)
                || i.elif_branches.iter().any(|elif| has_value_return(&elif.1))
                || i.else_body
                    .as_ref()
                    .is_some_and(|body| has_value_return(body))
        }
        Statement::Match(m) => m.arms.iter().any(|arm| has_value_return(&arm.body)),
        Statement::TryCatch(tc) => {
            has_value_return(&tc.try_body) || has_value_return(&tc.catch_body)
        }
        Statement::While(w) => has_value_return(&w.body),
        Statement::For(f) => has_value_return(&f.body),
        Statement::Import(_)
        | Statement::Function(_)
        | Statement::Process(_)
        | Statement::Event(_)
        | Statement::Class(_)
        | Statement::Interface(_)
        | Statement::Enum(_)
        | Statement::TypeAlias(_)
        | Statement::Break(_)
        | Statement::Continue(_)
        | Statement::VarDecl(_)
        | Statement::Assign(_)
        | Statement::Expr(_)
        | Statement::Throw(_) => false,
    })
}
