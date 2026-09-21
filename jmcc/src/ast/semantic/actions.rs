//! Action analysis: matching arguments against `JustMC` schema, checking enum/selectors,
//! determining return types, and variable inference in action blocks and lambdas.

use super::*;

impl Analyzer<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "Dispatches action resolution across schema, class methods, and processes"
    )]
    #[instrument(skip(self, a), level = "trace")]
    pub(super) fn analyze_action(&mut self, a: &ActionExpr) -> Type {
        let object = self.str(a.object);
        let name = self.str(a.name);
        trace!(%object, %name, "Analyzing action");

        let is_statement = self.statement_context;
        self.statement_context = false;

        if let Some(sel) = a.selector {
            let sel_str = self.str(sel);
            if !sel_str.is_empty() {
                trace!(selector = %sel_str, "Action selector present");
            }
        }

        // `value::name` is a platform game value, not an action: `value::main_hand_item` is an
        // `item`, `value::location` a coordinate. Left unresolved, the first action with a plural
        // slot binds the variable to that slot's type through `require_type`.
        if object == "value" {
            for arg in &a.args {
                self.analyze_expr(arg.value);
            }
            let ty = jmcdata::generated::get_game_value_type(&name)
                .map(|game_value_type| self.type_from_action_arg(game_value_type, false))
                .unwrap_or(Type::Unknown);
            if matches!(ty, Type::Unknown) {
                self.error(
                    SemanticErrorKind::UnknownGameValue { name: name.clone() },
                    a.span.clone(),
                );
            }
            self.analyze_action_body(a, &name, None, &[]);
            return ty;
        }

        if KNOWN_OBJECTS.contains(&object.as_str()) {
            let def = jmcdata::generated::get_action_def(&object, &name);
            let arg_types = self.analyze_known_action_args(a, def);
            self.lint_raw_action(&object, &name, &a.span);

            if (object == "world" || object == "game")
                && name == "cancel_event"
                && let Some(current_event) = &self.current_event
                && !jmcdata::generated::is_event_cancellable(current_event)
            {
                self.warning(
                    SemanticErrorKind::EventNotCancellable {
                        event: current_event.clone(),
                    },
                    a.span.clone(),
                );
            }

            let ret_ty = self.check_action_args(
                &object,
                &name,
                &a.args,
                &arg_types,
                &a.span,
                ActionCallKind {
                    is_method: false,
                    is_statement,
                },
            );
            self.analyze_action_body(a, &name, def, &arg_types);
            return ret_ty;
        }

        if let Some(class) = self.get_class(&object).cloned() {
            if let Some(method) = class.methods.get(&name).cloned() {
                let subst =
                    self.infer_class_method_subst(a.args.first().map(|arg| arg.value), &class);
                let params = self.method_params(&class, &method.params, &subst);
                let arg_types = a
                    .args
                    .iter()
                    .map(|arg| self.analyze_expr(arg.value))
                    .collect::<Vec<_>>();
                self.check_func_args(&params, &a.args, &arg_types, &a.span, &object);
                self.analyze_action_body(a, &name, None, &arg_types);
                if method.return_type.is_none() && !is_statement {
                    self.error(
                        SemanticErrorKind::VoidReturnValueUsed {
                            name: format!("{object}::{name}"),
                        },
                        a.span.clone(),
                    );
                }
                return self.callable_return_type(method.return_type, &class, &subst, &a.span);
            }
            if let Some(process) = class.processes.get(&name).cloned() {
                let arg_types = a
                    .args
                    .iter()
                    .map(|arg| self.analyze_expr(arg.value))
                    .collect::<Vec<_>>();
                let params = self.method_params(&class, &process.params, &HashMap::new());
                self.check_func_args(&params, &a.args, &arg_types, &a.span, &object);
                self.analyze_action_body(a, &name, None, &arg_types);
                if !is_statement {
                    self.error(
                        SemanticErrorKind::ProcessReturnValueUsed {
                            name: format!("{object}::{name}"),
                        },
                        a.span.clone(),
                    );
                    return Type::Unknown;
                }
                return Type::Never;
            }
        }

        let suggestion = crate::utils::suggest_action(&object, &name)
            .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
        self.error(
            SemanticErrorKind::UnknownAction {
                object: object.clone(),
                name: name.clone(),
                suggestion,
            },
            a.span.clone(),
        );
        for arg in &a.args {
            self.analyze_expr(arg.value);
        }
        self.analyze_action_body(a, &name, None, &[]);
        Type::Unknown
    }

    fn analyze_known_action_args(
        &mut self,
        action: &ActionExpr,
        definition: Option<&jmcdata::generated::ActionDef>,
    ) -> Vec<Type> {
        let mut used = HashSet::new();
        let mut position = 0;
        action
            .args
            .iter()
            .map(|arg| {
                let name = arg.name.map(|name| self.str(name));
                let definition =
                    resolve_action_arg_def(name.as_deref(), definition, &mut used, &mut position);
                let undeclared_variable = definition.is_some_and(|arg| arg.arg_type == "variable")
                    && matches!(
                        &self.ast.exprs[arg.value],
                        Expr::Ident(name, _) if self.lookup(&self.str(*name)).is_none()
                    );
                if undeclared_variable {
                    self.lang_type("variable", vec![])
                } else {
                    self.analyze_expr(arg.value)
                }
            })
            .collect()
    }

    fn is_action_arg_compatible(
        &mut self,
        actual: &Type,
        expected_arg_type: &str,
        expected_ty: &Type,
    ) -> bool {
        // An unannotated variable holds an unresolved `InferVar`, so without resolving it through
        // the unifier the comparison against the slot's class does not fire. `find` only
        // compresses paths, it binds nothing.
        let actual = self.unifier.find(actual);
        let expected_ty = self.unifier.find(expected_ty);
        let (actual, expected_ty) = (&actual, &expected_ty);

        let boolean_id = self.ir_ctx.lang_items.get("boolean").copied().unwrap_or(0);
        let text_id = self.ir_ctx.lang_items.get("text").copied().unwrap_or(0);
        let array_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
        let value_id = self.ir_ctx.lang_items.get("value").copied().unwrap_or(0);

        let is_bool = |ty: &Type| matches!(ty, Type::Class(id, _) if *id == boolean_id);
        let is_text = |ty: &Type| matches!(ty, Type::Class(id, _) if *id == text_id);
        let is_value = |ty: &Type| matches!(ty, Type::Class(id, _) if *id == value_id);
        // `item` is not a lang item but an ordinary class from `std/primitives/world/item.jc`.
        let is_item = |ty: &Type| matches!(ty, Type::Class(id, _) if self.class_short_name(*id) == Some("item"));

        // An `any` slot takes anything and yields no type to infer from: binding a variable to
        // the `any` class makes every later call on it fail with MethodCallOnAny.
        if expected_arg_type == "any" {
            return true;
        }

        // Number and text are interchangeable: `player::set_movement_speed("0", "FLY")`.
        if self.is_scalar_compatible(actual, expected_ty) {
            return true;
        }

        if let (Type::Class(a_id, a_args), Type::Class(e_id, e_args)) = (actual, expected_ty)
            && *a_id == array_id
            && *e_id == array_id
            && let Some(a_elem) = a_args.first()
        {
            // Same per element: `variable::add([list, "1"])`.
            if e_args
                .first()
                .is_some_and(|e_elem| self.is_scalar_compatible(a_elem, e_elem))
            {
                return true;
            }
            if (expected_arg_type == "text" || expected_arg_type == "enum") && is_bool(a_elem) {
                return true;
            }
            if (expected_arg_type == "item" || expected_arg_type == "block") && is_text(a_elem) {
                return true;
            }
            if (expected_arg_type == "text"
                || expected_arg_type == "enum"
                || expected_arg_type == "item"
                || expected_arg_type == "block")
                && is_value(a_elem)
            {
                return true;
            }
        }

        if (expected_arg_type == "text" || expected_arg_type == "enum") && is_bool(actual) {
            return true;
        }
        // Text and item in `item`/`block` slots: `block = "air"`, and an item naming the block's
        // material in `player::disguise_as_block(item("minecraft:barrier"))`.
        if (expected_arg_type == "item" || expected_arg_type == "block")
            && (is_text(actual) || is_item(actual))
        {
            return true;
        }
        if matches!(
            expected_arg_type,
            "text" | "string" | "enum" | "item" | "block" | "any" | "variable"
        ) && is_value(actual)
        {
            return true;
        }

        // An item in a text slot: `player::message(value::main_hand_item)`. A plural `text` slot
        // (`player::message`) expects `array<text>`, so the value is checked here, not per element.
        if matches!(expected_arg_type, "text" | "string" | "enum") && is_item(actual) {
            return true;
        }

        // The rest goes by the general rules, the same ones an assignment uses.
        self.is_assignable_to(actual, expected_ty)
    }

    /// Number and text are compatible in both directions, see `is_scalar_class`.
    fn is_scalar_compatible(&self, actual: &Type, expected: &Type) -> bool {
        let is_scalar = |ty: &Type| matches!(ty, Type::Class(id, _) if self.is_scalar_class(*id));
        is_scalar(actual) && is_scalar(expected)
    }

    #[instrument(skip(self, args, arg_types, kind), level = "trace")]
    pub(super) fn check_action_args(
        &mut self,
        object: &str,
        name: &str,
        args: &[ArgExpr],
        arg_types: &[Type],
        span: &Span,
        kind: ActionCallKind,
    ) -> Type {
        let Some(def) = jmcdata::generated::get_action_def(object, name) else {
            trace!(object, name, "Action def not found");
            let suggestion = crate::utils::suggest_action(object, name)
                .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
            self.error(
                SemanticErrorKind::UnknownAction {
                    object: object.to_owned(),
                    name: name.to_owned(),
                    suggestion,
                },
                span.clone(),
            );
            return Type::Unknown;
        };

        if !def.args.is_empty() {
            self.validate_action_args(def, name, args, arg_types, span, kind);
        }
        self.action_return_type(def, object, name)
    }

    fn validate_action_args(
        &mut self,
        def: &jmcdata::generated::ActionDef,
        name: &str,
        args: &[ArgExpr],
        arg_types: &[Type],
        span: &Span,
        kind: ActionCallKind,
    ) {
        let mut used = self.initial_action_params(def, args, kind);
        let (start, mut position) =
            self.action_method_offset(def, name, args, arg_types, span, kind);
        for (arg, arg_type) in args[start..].iter().zip(&arg_types[start..]) {
            let arg_name = arg.name.map(|name| self.str(name));
            let target =
                resolve_action_arg_def(arg_name.as_deref(), Some(def), &mut used, &mut position);
            if let Some(target) = target {
                self.validate_action_arg(arg, arg_type, target, name, span);
            } else {
                self.error(
                    SemanticErrorKind::TooManyArgs {
                        name: name.to_owned(),
                        expected: def.args.len(),
                        actual: args.len(),
                    },
                    span.clone(),
                );
            }
        }
    }

    fn initial_action_params(
        &self,
        def: &jmcdata::generated::ActionDef,
        args: &[ArgExpr],
        kind: ActionCallKind,
    ) -> HashSet<String> {
        let mut used = HashSet::new();
        if kind.is_method
            && let Some(origin) = def.origin
        {
            used.insert(origin.to_owned());
        }
        if !kind.is_statement
            && let Some(assigns) = def.assign
        {
            let assign_supplied = args.iter().filter_map(|arg| arg.name).any(|name| {
                let name = self.str(name);
                assigns.iter().any(|assign| assign.id == name)
            });
            if !assign_supplied {
                used.extend(assigns.iter().map(|assign| assign.id.to_owned()));
            }
        }
        used
    }

    fn action_method_offset(
        &mut self,
        def: &jmcdata::generated::ActionDef,
        name: &str,
        args: &[ArgExpr],
        arg_types: &[Type],
        span: &Span,
        kind: ActionCallKind,
    ) -> (usize, usize) {
        let Some(position) = kind
            .is_method
            .then_some(def.origin)
            .flatten()
            .and_then(|origin| def.args.iter().position(|arg| arg.id == origin))
        else {
            return (0, 0);
        };
        let origin = &def.args[position];
        let expected = self.type_from_action_arg(origin.arg_type, origin.array.is_some());
        if !self.is_action_arg_compatible(&arg_types[0], origin.arg_type, &expected) {
            self.require_action_arg_type(&arg_types[0], &expected, name, origin.id, span);
        }
        // The caller may repeat the receiver as the first argument
        // (`map.set_map_value(map, key, value)`); the `origin` slot is already taken by the
        // receiver, so such an argument is skipped.
        let start = if args.get(1).is_some_and(|arg| {
            arg.name.is_none() && structurally_eq(self.ast, arg.value, args[0].value)
        }) {
            2
        } else {
            1
        };
        // Positional arguments fill the free slots in declaration order, skipping the taken ones
        // (`origin`, `assign`) via `used_params`. Starting from `position` would leave the slots
        // before a non-leading `origin` empty (`start` and `stop` of `lerp_number`). Codegen
        // (`conv_args`) counts the same way.
        (start, 0)
    }

    fn validate_action_arg(
        &mut self,
        arg: &ArgExpr,
        actual: &Type,
        target: &jmcdata::generated::ActionArg,
        action_name: &str,
        span: &Span,
    ) {
        let expected = self.type_from_action_arg(target.arg_type, target.array.is_some());
        let compatible = self.is_action_arg_compatible(actual, target.arg_type, &expected);
        let enum_expected =
            matches!(expected, Type::Class(id, _) if self.ir_ctx.enums_by_def.contains_key(&id));
        let valid_literal = enum_expected && self.check_enum_arg(arg.value, target.values, span);
        if !compatible && !valid_literal {
            self.require_action_arg_type(actual, &expected, action_name, target.id, span);
        }
    }

    fn require_action_arg_type(
        &mut self,
        actual: &Type,
        expected: &Type,
        action: &str,
        argument: &str,
        span: &Span,
    ) {
        self.require_type(actual, expected, span.clone(), |expected, actual| {
            SemanticErrorKind::ActionArgTypeMismatch {
                name: action.to_owned(),
                arg: argument.to_owned(),
                expected,
                actual,
            }
        });
    }

    fn action_return_type(
        &self,
        def: &jmcdata::generated::ActionDef,
        object: &str,
        name: &str,
    ) -> Type {
        if def.boolean {
            trace!(object, name, "Action returns Boolean");
            self.lang_type("boolean", vec![])
        } else if def.action_type == "container" || def.action_type.contains("conditional") {
            trace!(object, name, "Action returns Never");
            Type::Never
        } else {
            def.assign
                .map(|assigns| {
                    if assigns.is_empty() {
                        Type::Unknown
                    } else if assigns.len() == 1 {
                        self.type_from_action_arg(assigns[0].arg_type, false)
                    } else {
                        let first_ty = self.type_from_action_arg(assigns[0].arg_type, false);
                        let all_same = assigns
                            .iter()
                            .all(|a| self.type_from_action_arg(a.arg_type, false) == first_ty);
                        self.lang_type(
                            "array",
                            vec![if all_same { first_ty } else { Type::Unknown }],
                        )
                    }
                })
                .unwrap_or(Type::Unknown)
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn type_from_action_arg(&self, s: &str, is_array: bool) -> Type {
        let get_id = |name: &str| self.ir_ctx.lang_items.get(name).copied().unwrap_or(0);
        let base = match s {
            "number" => Type::Class(get_id("number"), vec![]),
            "text" | "string" | "enum" => Type::Class(get_id("text"), vec![]),
            "boolean" | "bool" => Type::Class(get_id("boolean"), vec![]),
            "location" => self.ir_ctx.type_from_str("location"),
            "vector" => self.ir_ctx.type_from_str("vector"),
            "item" => self.ir_ctx.type_from_str("item"),
            "block" => self.ir_ctx.type_from_str("block"),
            "sound" => self.ir_ctx.type_from_str("sound"),
            "particle" => self.ir_ctx.type_from_str("particle"),
            "potion" => self.ir_ctx.type_from_str("potion"),
            "variable" => self.ir_ctx.type_from_str("variable"),
            "any" => self.ir_ctx.type_from_str("any"),
            "list" | "array" => Type::Class(get_id("array"), vec![Type::Unknown]),
            "map" => Type::Class(get_id("map"), vec![Type::Unknown, Type::Unknown]),
            _ => Type::Unknown,
        };
        if is_array {
            Type::Class(get_id("array"), vec![base])
        } else {
            base
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn check_enum_arg(&mut self, expr_id: ExprId, values: Option<&[&str]>, span: &Span) -> bool {
        let Some(vals) = values else {
            return false;
        };
        let expr = &self.ast.exprs[expr_id];
        trace!(?expr, ?vals, "Checking enum arg");

        match expr {
            Expr::Text(tv) => {
                let s: String = tv
                    .parts
                    .iter()
                    .filter_map(|p| {
                        if let TextPart::Literal(l) = p {
                            Some(self.ast.strings.resolve(l))
                        } else {
                            None
                        }
                    })
                    .collect();
                self.check_enum_literal(&s, vals, span);
                true
            }
            Expr::Ident(name, _) => {
                let s = self.str(*name);
                if self.lookup(&s).is_some() {
                    return false;
                }
                self.check_enum_literal(&s, vals, span);
                true
            }
            Expr::Number(n) => {
                if n.value.fract() == 0.0 && n.value >= 0.0 {
                    let idx = n.value as usize;
                    if idx >= vals.len() {
                        self.error(
                            SemanticErrorKind::EnumIndexOutOfBounds {
                                idx,
                                max: vals.len().saturating_sub(1),
                            },
                            span.clone(),
                        );
                    }
                }
                true
            }
            Expr::Bool(b) => {
                let s = if b.value { "TRUE" } else { "FALSE" };
                if !vals.iter().any(|v| v.eq_ignore_ascii_case(s)) {
                    self.error(
                        SemanticErrorKind::InvalidBoolEnum {
                            expected: vals.join(", "),
                        },
                        span.clone(),
                    );
                }
                true
            }
            _ => false,
        }
    }

    /// Records `InvalidEnumValue` error if the value is not in the allowed list.
    fn check_enum_literal(&mut self, value: &str, vals: &[&str], span: &Span) {
        if !vals.iter().any(|v| v.eq_ignore_ascii_case(value)) {
            self.error(
                SemanticErrorKind::InvalidEnumValue {
                    value: value.to_owned(),
                    expected: vals.join(", "),
                },
                span.clone(),
            );
        }
    }

    #[instrument(skip(self, a), level = "trace")]
    fn analyze_action_body(
        &mut self,
        a: &ActionExpr,
        name: &str,
        def: Option<&jmcdata::generated::ActionDef>,
        arg_types: &[Type],
    ) {
        let Some(operations) = &a.operations else {
            return;
        };
        self.push_scope();
        let previous_loop_depth = self.loop_depth;
        if is_loop_action(name) {
            self.loop_depth += 1;
        }
        let object = self.str(a.object);
        self.declare_lambda_params_with_inference(
            &a.lambda, &object, name, &a.args, arg_types, def,
        );
        if let Some(def) = def {
            self.declare_action_variables(a, &object, name, def, arg_types);
        }
        self.predeclare_in_block(operations);
        for statement in operations {
            self.analyze_stmt(statement);
        }
        self.loop_depth = previous_loop_depth;
        self.pop_scope();
    }

    fn declare_action_variables(
        &mut self,
        action: &ActionExpr,
        object: &str,
        action_name: &str,
        def: &jmcdata::generated::ActionDef,
        arg_types: &[Type],
    ) {
        let mut used = HashSet::new();
        let mut position = 0;
        for arg in &action.args {
            let name = arg.name.map(|name| self.str(name));
            let target =
                resolve_action_arg_def(name.as_deref(), Some(def), &mut used, &mut position);
            let Some(target) = target.filter(|target| target.arg_type == "variable") else {
                continue;
            };
            let assign_type = def.assign.and_then(|assigns| {
                assigns
                    .iter()
                    .find(|assign| assign.id == target.id)
                    .map(|assign| self.type_from_action_arg(assign.arg_type, false))
            });
            self.update_action_arg_type(arg.value, assign_type.as_ref());
            let Some(variable_name) = self.action_variable_name(arg.value) else {
                continue;
            };
            if let Some(Symbol::Var { ty, .. }) = self.lookup(&variable_name) {
                if let Some(assign_type) = &assign_type {
                    let _unify_result: std::result::Result<Type, String> =
                        self.unifier.unify(&ty, assign_type);
                }
                continue;
            }
            let inferred = (object == "repeat")
                .then(|| self.infer_repeat_variable(action_name, target.id, action, arg_types, def))
                .flatten();
            let variable_type = inferred
                .or(assign_type)
                .or_else(|| self.lambda_variable_type(def, target.id))
                .unwrap_or_else(|| self.unifier.new_var());
            self.declare(
                variable_name,
                Symbol::Var {
                    ty: variable_type,
                    scope: self.default_scope,
                    declared: false,
                },
            );
        }
    }

    fn update_action_arg_type(&mut self, expression: ExprId, assign_type: Option<&Type>) {
        let Some(assign_type) = assign_type else {
            return;
        };
        let Some(arg_type) = self.expr_types.get_mut(&expression) else {
            return;
        };
        let _unify_result: std::result::Result<Type, String> =
            self.unifier.unify(arg_type, assign_type);
        *arg_type = self.unifier.find(arg_type);
    }

    pub(super) fn action_variable_name(&self, expression: ExprId) -> Option<String> {
        match &self.ast.exprs[expression] {
            Expr::Ident(name, _) => Some(self.str(*name)),
            Expr::Variable(variable) => Some(text_value_to_string(self.ast, &variable.name)),
            _ => None,
        }
    }

    fn infer_repeat_variable(
        &self,
        action_name: &str,
        target: &str,
        action: &ActionExpr,
        arg_types: &[Type],
        def: &jmcdata::generated::ActionDef,
    ) -> Option<Type> {
        let any = || self.lang_type("any", vec![]);
        match (action_name, target) {
            ("for_each_in_list", "index_variable") => Some(self.lang_type("number", vec![])),
            ("for_each_in_list", "value_variable") => {
                let ty =
                    self.get_action_arg_type_by_name("list", &action.args, arg_types, Some(def))?;
                let Type::Class(id, args) = ty else {
                    return None;
                };
                (id == self.ir_ctx.lang_items.get("array").copied().unwrap_or(0))
                    .then(|| args.first().cloned().unwrap_or_else(any))
            }
            ("for_each_map_entry", "key_variable" | "value_variable") => {
                let ty =
                    self.get_action_arg_type_by_name("map", &action.args, arg_types, Some(def))?;
                let Type::Class(id, args) = ty else {
                    return None;
                };
                if id != self.ir_ctx.lang_items.get("map").copied().unwrap_or(0) {
                    return None;
                }
                let index = usize::from(target == "value_variable");
                Some(args.get(index).cloned().unwrap_or_else(any))
            }
            _ => None,
        }
    }

    /// Loop variable type declared in the `lambda` of a container action.
    ///
    /// `repeat::on_circle` declares `variable: location`, `repeat::multi_times` `number`. An
    /// `any` lambda yields nothing; the type is then inferred from the collection element by
    /// `infer_repeat_variable`.
    fn lambda_variable_type(
        &self,
        def: &jmcdata::generated::ActionDef,
        target: &str,
    ) -> Option<Type> {
        let lambda = def.lambda?.iter().find(|arg| arg.id == target)?;
        if matches!(lambda.arg_type, "any" | "variable") {
            return None;
        }
        Some(self.type_from_action_arg(lambda.arg_type, lambda.array.is_some()))
    }

    /// Resolves action argument type by name: checks named arguments first, then positional
    /// index in declaration. `def` = `None` indicates schema-less action (resolved by name only).
    #[instrument(skip(self, args, arg_types), level = "trace")]
    fn get_action_arg_type_by_name(
        &self,
        arg_name: &str,
        args: &[ArgExpr],
        arg_types: &[Type],
        def: Option<&jmcdata::generated::ActionDef>,
    ) -> Option<Type> {
        for (i, arg) in args.iter().enumerate() {
            if arg.name.map(|n| self.str(n)) == Some(arg_name.to_owned()) {
                return Some(arg_types[i].clone());
            }
        }
        if let Some(def) = def
            && let Some(pos) = def.args.iter().position(|a| a.id == arg_name)
            && pos < args.len()
            && args[pos].name.is_none()
        {
            return Some(arg_types[pos].clone());
        }
        None
    }

    #[instrument(skip(self, lambda, args, arg_types), level = "trace")]
    fn declare_lambda_params_with_inference(
        &mut self,
        lambda: &Option<Vec<ExprId>>,
        object: &str,
        name: &str,
        args: &[ArgExpr],
        arg_types: &[Type],
        def: Option<&jmcdata::generated::ActionDef>,
    ) {
        if let Some(lambda) = lambda {
            for (i, &p) in lambda.iter().enumerate() {
                if let Expr::Ident(name_str_id, _) = &self.ast.exprs[p] {
                    let name_str = self.str(*name_str_id);
                    let ty = self.resolve_lambda_param_ty(object, name, i, args, arg_types, def);
                    self.declare(name_str, Symbol::Param { ty });
                }
            }
        }
    }

    #[instrument(skip(self, args, arg_types), level = "trace")]
    fn resolve_lambda_param_ty(
        &mut self,
        object: &str,
        name: &str,
        index: usize,
        args: &[ArgExpr],
        arg_types: &[Type],
        def: Option<&jmcdata::generated::ActionDef>,
    ) -> Type {
        let number_id = self.ir_ctx.lang_items.get("number").copied().unwrap_or(0);
        let location_id = self.ir_ctx.lang_items.get("location").copied().unwrap_or(0);
        let array_def_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
        let map_def_id = self.ir_ctx.lang_items.get("map").copied().unwrap_or(0);
        let any = || self.lang_type("any", vec![]);

        match (object, name) {
            ("repeat", "multi_times" | "on_range") => Type::Class(number_id, vec![]),
            ("repeat", "on_circle" | "on_grid" | "on_path" | "on_sphere" | "adjacently") => {
                Type::Class(location_id, vec![])
            }
            ("repeat", "for_each_in_list") => {
                if index == 0 {
                    Type::Class(number_id, vec![])
                } else {
                    if let Some(list_ty) =
                        self.get_action_arg_type_by_name("list", args, arg_types, def)
                        && let Type::Class(id, list_args) = list_ty
                        && id == array_def_id
                    {
                        return list_args.first().cloned().unwrap_or_else(any);
                    }
                    any()
                }
            }
            ("repeat", "for_each_map_entry") => {
                if let Some(map_ty) = self.get_action_arg_type_by_name("map", args, arg_types, def)
                    && let Type::Class(id, map_args) = map_ty
                    && id == map_def_id
                {
                    if index == 0 {
                        return map_args.first().cloned().unwrap_or_else(any);
                    }
                    return map_args.get(1).cloned().unwrap_or_else(any);
                }
                any()
            }
            _ => self.unifier.new_var(),
        }
    }
}

fn is_loop_action(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("repeat")
        || lower.contains("loop")
        || lower.contains("while")
        || lower.contains("for")
}
