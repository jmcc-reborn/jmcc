//! Invocations: plain, named, and instance method calls, argument checking,
//! constructors, and call diagnostics.

use super::*;

impl Analyzer<'_> {
    /// The action an expression lowers into, if it lowers into one at all.
    ///
    /// Mirrors the dispatch order of `analyze_call`: direct function call, then object
    /// namespace, then class method, then the `variable::` fallback.
    pub(super) fn action_def_of_expr(
        &self,
        eid: ExprId,
    ) -> Option<&'static jmcdata::generated::ActionDef> {
        let (object, name) = match &self.ast.exprs[eid] {
            Expr::Action(a) => (self.str(a.object), self.str(a.name)),
            Expr::Call(c) => {
                let method = self.str(c.method);
                match &self.ast.exprs[c.target] {
                    Expr::Ident(target, _) => {
                        let target = self.str(*target);
                        // Direct call: the parser sets `method` to the target name.
                        if method == target {
                            return None;
                        }
                        if KNOWN_OBJECTS.contains(&target.as_str()) {
                            return jmcdata::generated::get_action_def(&target, &method);
                        }
                        if let Some(class) = self.get_class(&target)
                            && self.class_has_method(class.def_id, &method)
                        {
                            return None;
                        }
                    }
                    _ => {
                        if let Some(Type::Class(id, _)) = self.expr_types.get(&c.target)
                            && self.class_has_method(*id, &method)
                        {
                            return None;
                        }
                    }
                }
                ("variable".to_owned(), method)
            }
            _ => return None,
        };
        jmcdata::generated::get_action_def(&object, &name)
    }

    fn class_has_method(&self, def_id: DefId, method: &str) -> bool {
        let mut queue = std::collections::VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back(def_id);
        visited.insert(def_id);

        while let Some(id) = queue.pop_front() {
            let Some(class) = self.ir_ctx.classes_by_def.get(&id) else {
                continue;
            };
            if class.methods.contains_key(method) || class.processes.contains_key(method) {
                return true;
            }
            if let Some(p) = class.parent.filter(|parent| *parent != id)
                && visited.insert(p)
            {
                queue.push_back(p);
            }
            for (iface_id, _) in &class.implements {
                if visited.insert(*iface_id) {
                    queue.push_back(*iface_id);
                }
            }
        }
        false
    }

    /// Warns about a raw action that has a syntactic form, see [`RAW_ACTION_FORMS`].
    pub(super) fn lint_raw_action(&self, object: &str, method: &str, span: &Span) {
        if self.is_std_span(span) {
            return;
        }
        let Some((_, _, form, note)) =
            RAW_ACTION_FORMS
                .iter()
                .find(|(object_name, method_name, _, _)| {
                    *object_name == object && *method_name == method
                })
        else {
            return;
        };
        let at = self.format_span(span);
        warn!(at = %at, "Raw action '{object}::{method}': write '{form}' instead — {note}");
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn analyze_call(&mut self, c: &CallExpr) -> Type {
        let target_expr = &self.ast.exprs[c.target];
        let method_str = self.str(c.method);
        trace!(method = %method_str, "Analyzing call");

        let is_statement = self.statement_context;
        self.statement_context = false;

        let target_ty = self.analyze_expr(c.target);
        let arg_types: Vec<Type> = c.args.iter().map(|a| self.analyze_expr(a.value)).collect();

        if let Expr::Ident(name, _) = target_expr
            && let Some(result) = self.analyze_named_call(
                c,
                &self.str(*name),
                &method_str,
                &target_ty,
                &arg_types,
                is_statement,
            )
        {
            return result;
        }

        let resolved_target_ty = self.unifier.find(&target_ty);

        if let Some(result) = self.analyze_instance_call(
            c,
            &method_str,
            &target_ty,
            &arg_types,
            &resolved_target_ty,
            is_statement,
        ) {
            return result;
        }

        if let Some(result) = self.analyze_variable_method_fallback(
            c,
            &method_str,
            &target_ty,
            &arg_types,
            is_statement,
        ) {
            return result;
        }

        // The `variable::` fallback above already ran, so a call left here lowers into nothing.
        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        if matches!(resolved_target_ty, Type::Class(id, _) if id == any_def_id) {
            return self.method_error(None, &method_str, &c.span);
        }

        if let Expr::Ident(name, _) = target_expr {
            let name_str = self.str(*name);
            if self.get_class(&name_str).is_some() {
                return self.method_error(Some(name_str), &method_str, &c.span);
            }
        }

        if let Type::Class(def_id, _) = &resolved_target_ty
            && let Some(class_info) = self.ir_ctx.classes_by_def.get(def_id)
        {
            return self.method_error(Some(class_info.name.clone()), &method_str, &c.span);
        }

        if !resolved_target_ty.is_unknown() || self.edition >= 2026 {
            let type_name = self.format_type(&resolved_target_ty);
            return self.method_error(Some(type_name), &method_str, &c.span);
        }

        warn!(method = %method_str, "Unknown method call");
        Type::Unknown
    }

    /// Emits an unknown method error and returns `Type::Unknown`.
    /// `class` = `None` indicates a method call on an `any` value.
    fn method_error(&mut self, class: Option<String>, method: &str, span: &Span) -> Type {
        let kind = class.map_or_else(
            || SemanticErrorKind::MethodCallOnAny {
                method: method.to_owned(),
            },
            |class| {
                let suggestion = self
                    .get_class(&class)
                    .and_then(|info| {
                        crate::utils::did_you_mean(method, info.methods.keys().map(String::as_str))
                    })
                    .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
                SemanticErrorKind::UnknownMethod {
                    class,
                    method: method.to_owned(),
                    suggestion,
                }
            },
        );
        self.error(kind, span.clone());
        Type::Unknown
    }

    /// The receiver as the leading positional argument of a method-style action call, with types.
    fn with_receiver(
        call: &CallExpr,
        target_ty: &Type,
        arg_types: &[Type],
    ) -> (Vec<ArgExpr>, Vec<Type>) {
        let mut args = Vec::with_capacity(call.args.len() + 1);
        args.push(ArgExpr {
            name: None,
            value: call.target,
            spread: 0,
            is_ref: false,
        });
        args.extend(call.args.iter().cloned());

        let mut types = Vec::with_capacity(arg_types.len() + 1);
        types.push(target_ty.clone());
        types.extend(arg_types.iter().cloned());
        (args, types)
    }

    /// Lowers `value.method(args)` into `variable::method(value, args)`, `None` if there is no
    /// such action and the call has to be handled further.
    fn analyze_variable_method_fallback(
        &mut self,
        call: &CallExpr,
        method: &str,
        target_ty: &Type,
        arg_types: &[Type],
        is_statement: bool,
    ) -> Option<Type> {
        jmcdata::generated::get_action_def("variable", method)?;
        trace!(method = %method, "Falling back to variable::method");
        let (args, types) = Self::with_receiver(call, target_ty, arg_types);
        Some(self.check_action_args(
            "variable",
            method,
            &args,
            &types,
            &call.span,
            ActionCallKind {
                is_method: true,
                is_statement,
            },
        ))
    }

    fn analyze_named_call(
        &mut self,
        call: &CallExpr,
        target: &str,
        method: &str,
        target_ty: &Type,
        arg_types: &[Type],
        is_statement: bool,
    ) -> Option<Type> {
        // A direct call is marked by the parser with the target's own name (`Foo(x)` →
        // `method == "Foo"`), so `target.method(...)` is not a call of `target`: functions and
        // variables live in separate namespaces here.
        //
        // Module mangling rewrites the target to a full name and sets the method to `"call"`,
        // which is still a direct call.
        let is_direct_call = method == target || method == "call";
        match self.lookup(target) {
            Some(Symbol::Func {
                params,
                return_type,
                ..
            }) if is_direct_call => {
                self.check_func_args(&params, &call.args, arg_types, &call.span, target);
                if return_type.is_none() && !is_statement {
                    self.error(
                        SemanticErrorKind::VoidReturnValueUsed {
                            name: target.to_owned(),
                        },
                        call.span.clone(),
                    );
                }
                return Some(return_type.unwrap_or(Type::Unknown));
            }
            Some(Symbol::Proc { params, .. }) if is_direct_call => {
                self.check_func_args(&params, &call.args, arg_types, &call.span, target);
                if !is_statement {
                    self.error(
                        SemanticErrorKind::ProcessReturnValueUsed {
                            name: target.to_owned(),
                        },
                        call.span.clone(),
                    );
                    return Some(Type::Unknown);
                }
                return Some(Type::Never);
            }
            _ => {}
        }
        if is_direct_call && let Some(class) = self.get_class(target).cloned() {
            self.check_constructor_call(call, arg_types, target, &class);
            return Some(Type::Class(class.def_id, vec![]));
        }
        if KNOWN_OBJECTS.contains(&target) {
            if jmcdata::generated::get_action_def(target, method).is_some() {
                self.lint_raw_action(target, method, &call.span);
                return Some(self.check_action_args(
                    target,
                    method,
                    &call.args,
                    arg_types,
                    &call.span,
                    ActionCallKind {
                        is_method: false,
                        is_statement,
                    },
                ));
            }
            // No exact `object::method` in the schema: the object may be a variable and the
            // method an action of another object. HIR lowering (`resolve_action_call_target`)
            // searches the same way, passing the receiver as the first argument:
            // `item.item_has_tag(...)` is `variable::item_has_tag(item, ...)`.
            if let Some(object) = KNOWN_OBJECTS
                .iter()
                .find(|&&object| jmcdata::generated::get_action_id(object, method).is_some())
            {
                let (args, types) = Self::with_receiver(call, target_ty, arg_types);
                return Some(self.check_action_args(
                    object,
                    method,
                    &args,
                    &types,
                    &call.span,
                    ActionCallKind {
                        is_method: true,
                        is_statement,
                    },
                ));
            }
            let suggestion = crate::utils::suggest_action(target, method)
                .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
            self.error(
                SemanticErrorKind::UnknownAction {
                    object: target.to_owned(),
                    name: method.to_owned(),
                    suggestion,
                },
                call.span.clone(),
            );
            return Some(Type::Unknown);
        }
        let class = self.get_class(target).cloned()?;
        if method == "call" || method == target {
            self.check_constructor_call(call, arg_types, target, &class);
            return Some(Type::Class(class.def_id, vec![]));
        }
        let function = class.methods.get(method)?.clone();
        let subst = self.infer_class_method_subst(call.args.first().map(|arg| arg.value), &class);
        let params = self.method_params(&class, &function.params, &subst);
        self.check_func_args(&params, &call.args, arg_types, &call.span, target);
        Some(self.callable_return_type(function.return_type, &class, &subst, &call.span))
    }

    fn check_constructor_call(
        &mut self,
        call: &CallExpr,
        arg_types: &[Type],
        name: &str,
        class: &ClassInfo,
    ) {
        let Some(init) = class.methods.get("__init__").cloned() else {
            return;
        };
        let params = self.method_params(class, &init.params, &HashMap::new());
        let has_self = init
            .params
            .first()
            .is_some_and(|p| crate::utils::is_self_param(self.str(p.name).as_str()));
        let params_to_check = if has_self { &params[1..] } else { &params[..] };
        self.check_func_args(params_to_check, &call.args, arg_types, &call.span, name);
    }

    /// Class method generic substitution based on receiver type (`self_arg` is the first argument).
    pub(super) fn infer_class_method_subst(
        &mut self,
        self_arg: Option<ExprId>,
        class: &ClassInfo,
    ) -> HashMap<StrId, Type> {
        let mut subst = HashMap::new();
        let Some(self_arg) = self_arg.filter(|_| !class.generics.is_empty()) else {
            return subst;
        };
        let actual = self.analyze_expr(self_arg);
        let expected = Type::Class(
            class.def_id,
            class
                .generics
                .iter()
                .map(|&generic| Type::Param(generic))
                .collect(),
        );
        self.match_generics(&expected, &actual, &mut subst);
        subst
    }

    pub(super) fn callable_return_type(
        &mut self,
        return_type: Option<StrId>,
        class: &ClassInfo,
        subst: &HashMap<StrId, Type>,
        span: &Span,
    ) -> Type {
        return_type.map_or(Type::Unknown, |return_type| {
            let raw = self.with_generic_scope(&class.generics, |this| {
                this.parse_decl_type(&this.str(return_type), span.clone())
            });
            if subst.is_empty() {
                raw
            } else {
                self.substitute(&raw, subst)
            }
        })
    }

    #[expect(
        clippy::too_many_lines,
        reason = "comprehensive method resolution across class inheritance and synthetic lambdas"
    )]
    fn analyze_instance_call(
        &mut self,
        call: &CallExpr,
        method: &str,
        target_type: &Type,
        arg_types: &[Type],
        resolved_target: &Type,
        is_statement: bool,
    ) -> Option<Type> {
        let Type::Class(initial_def, initial_args) = resolved_target else {
            return None;
        };
        let mut queue = std::collections::VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back((*initial_def, initial_args.clone()));
        visited.insert(*initial_def);

        while let Some((current_def, current_args)) = queue.pop_front() {
            let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() else {
                continue;
            };
            let subst = Self::build_generic_subst(&class.generics, &current_args);
            let is_direct_call = match &self.ast.exprs[call.target] {
                Expr::Ident(name, _) => self.str(*name) == method,
                _ => method == "call",
            };
            let function_opt = class
                .methods
                .get(method)
                .or_else(|| {
                    if is_direct_call || method == "call" {
                        class.methods.get("call")
                    } else {
                        None
                    }
                })
                .cloned();
            if let Some(function) = function_opt {
                self.check_instance_callable(
                    call,
                    arg_types,
                    target_type,
                    &class,
                    &function.params,
                    &subst,
                );
                let lambda_ret = if class.name.starts_with("__LambdaClass_") {
                    let fn_name = format!("__lambda_{}", &class.name["__LambdaClass_".len()..]);
                    match self.lookup(&fn_name) {
                        Some(Symbol::Func { return_type, .. }) => return_type,
                        _ => None,
                    }
                } else {
                    None
                };
                let has_ret = function.return_type.is_some()
                    || lambda_ret.is_some()
                    || decls::has_value_return(&function.body);
                if !has_ret && !is_statement {
                    self.error(
                        SemanticErrorKind::VoidReturnValueUsed {
                            name: format!("{}::{method}", class.name),
                        },
                        call.span.clone(),
                    );
                    return Some(Type::Unknown);
                }
                let ret_ty = if function.return_type.is_some() {
                    self.callable_return_type(function.return_type, &class, &subst, &call.span)
                } else if let Some(lr) = lambda_ret {
                    self.unifier.find(&lr)
                } else {
                    Type::Unknown
                };
                return Some(ret_ty);
            }
            if let Some(process) = class.processes.get(method).cloned() {
                self.check_instance_callable(
                    call,
                    arg_types,
                    target_type,
                    &class,
                    &process.params,
                    &HashMap::new(),
                );
                if !is_statement {
                    self.error(
                        SemanticErrorKind::ProcessReturnValueUsed {
                            name: format!("{}::{method}", class.name),
                        },
                        call.span.clone(),
                    );
                    return Some(Type::Unknown);
                }
                return Some(Type::Never);
            }
            if let Some((def, args)) = self.parent_step(&class, current_def, &subst)
                && visited.insert(def)
            {
                queue.push_back((def, args));
            }
            for (iface_def, iface_args) in &class.implements {
                let substituted_args = iface_args
                    .iter()
                    .map(|ty| self.substitute(ty, &subst))
                    .collect();
                if visited.insert(*iface_def) {
                    queue.push_back((*iface_def, substituted_args));
                }
            }
        }
        None
    }

    fn check_instance_callable(
        &mut self,
        call: &CallExpr,
        arg_types: &[Type],
        target_type: &Type,
        class: &ClassInfo,
        raw_params: &[Param],
        subst: &HashMap<StrId, Type>,
    ) {
        let params = self.method_params(class, raw_params, subst);
        let mut args = vec![ArgExpr {
            name: None,
            value: call.target,
            spread: 0,
            is_ref: raw_params.first().is_some_and(|param| param.is_ref),
        }];
        args.extend(call.args.iter().cloned());
        let mut types = vec![target_type.clone()];
        types.extend(arg_types.iter().cloned());
        self.check_func_args(&params, &args, &types, &call.span, &class.name);
    }

    #[instrument(skip(self, params, args, arg_types), level = "trace")]
    pub(super) fn check_func_args(
        &mut self,
        params: &[ParamInfo],
        args: &[ArgExpr],
        arg_types: &[Type],
        span: &Span,
        name: &str,
    ) {
        let has_args_spread = params.iter().any(|p| p.spread == 1);
        let has_kwargs_spread = params.iter().any(|p| p.spread == 2);
        let normal_params: Vec<&ParamInfo> = params.iter().filter(|p| p.spread == 0).collect();
        let mut positional_idx = 0;
        let mut provided_names = HashSet::new();

        for (arg_idx, (arg, arg_ty)) in args.iter().zip(arg_types.iter()).enumerate() {
            if let Some(arg_name_id) = arg.name {
                let arg_name_str = self.str(arg_name_id);
                provided_names.insert(arg_name_str.clone());
                if let Some(p) = params.iter().find(|p| p.name == arg_name_str) {
                    self.require_type(arg_ty, &p.ty, span.clone(), |expected, actual| {
                        SemanticErrorKind::FuncArgTypeMismatch {
                            name: name.to_owned(),
                            arg: arg_name_str.clone(),
                            expected,
                            actual,
                        }
                    });
                } else if !has_kwargs_spread {
                    let suggestion = crate::utils::did_you_mean(
                        &arg_name_str,
                        params.iter().map(|p| p.name.as_str()),
                    )
                    .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
                    self.error(
                        SemanticErrorKind::UnknownParam {
                            name: name.to_owned(),
                            arg: arg_name_str,
                            suggestion,
                        },
                        span.clone(),
                    );
                }
            } else if positional_idx < normal_params.len() {
                let p = normal_params[positional_idx];
                provided_names.insert(p.name.clone());
                let idx = arg_idx + 1;
                self.require_type(arg_ty, &p.ty, span.clone(), |expected, actual| {
                    SemanticErrorKind::FuncPositionalArgTypeMismatch {
                        name: name.to_owned(),
                        idx,
                        expected,
                        actual,
                    }
                });
                positional_idx += 1;
            } else if !has_args_spread {
                self.error(
                    SemanticErrorKind::TooManyArgs {
                        name: name.to_owned(),
                        expected: normal_params.len(),
                        actual: args.len(),
                    },
                    span.clone(),
                );
                return;
            }
        }

        for p in params {
            if p.spread != 0 {
                continue;
            }
            if !p.has_default && !provided_names.contains(p.name.as_str()) {
                let was_positional = normal_params
                    .iter()
                    .take(positional_idx)
                    .any(|np| np.name == p.name);
                if !was_positional {
                    self.error(
                        SemanticErrorKind::MissingArgument {
                            name: name.to_owned(),
                            arg: p.name.clone(),
                            suggestion: String::new(),
                        },
                        span.clone(),
                    );
                }
            }
        }
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn analyze_constructor(&mut self, c: &ConstructorExpr) -> Type {
        let name = self.str(c.name);
        trace!(constructor = %name, "Analyzing constructor");

        if let Some(class_info) = self.get_class(&name).cloned() {
            let mut arg_types = Vec::new();
            for arg in &c.args {
                arg_types.push(self.analyze_expr(arg.value));
            }
            if let Some(init_rc) = class_info.methods.get("__init__").cloned() {
                let params = self.method_params(&class_info, &init_rc.params, &HashMap::new());
                self.check_func_args(&params, &c.args, &arg_types, &c.span, &name);
            }
            return Type::Class(class_info.def_id, vec![]);
        }

        self.error(
            SemanticErrorKind::UnknownConstructor { name },
            c.span.clone(),
        );
        Type::Unknown
    }
}
