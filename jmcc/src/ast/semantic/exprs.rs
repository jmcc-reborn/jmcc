//! Expressions: identifiers, unary and binary operations, ternary, casts, properties,
//! indexing, and resolving operators to `__dunder__` methods.

use super::*;

impl Analyzer<'_> {
    #[instrument(skip(self), level = "trace")]
    pub(super) fn analyze_expr(&mut self, eid: ExprId) -> Type {
        let expr = &self.ast.exprs[eid];
        let ty = match expr {
            Expr::Number(_) => self.lang_type("number", vec![]),
            Expr::Bool(_) => self.lang_type("boolean", vec![]),
            Expr::Ident(name, span) => self.analyze_ident(*name, span),
            Expr::Variable(variable) => self.analyze_variable(variable),
            Expr::Text(text) => {
                for part in &text.parts {
                    if let TextPart::Interp(eid) = part {
                        self.analyze_expr(*eid);
                    }
                }
                self.lang_type("text", vec![])
            }
            Expr::Nbt(_) => Type::Unknown,
            Expr::List(list) => {
                let types = list
                    .values
                    .iter()
                    .map(|&value| self.analyze_expr(value))
                    .collect::<Vec<_>>();
                let element = self.unify_element_types(&types);
                self.lang_type("array", vec![element])
            }
            Expr::Map(map) => {
                let key = self.common_element_type(&map.keys);
                let value = self.common_element_type(&map.values);
                self.lang_type("map", vec![key, value])
            }
            Expr::Ternary(ternary) => self.analyze_ternary(ternary),
            Expr::Binary(b) => self.analyze_binary(b),
            Expr::Unary(unary) => self.analyze_unary(unary),
            Expr::Property(p) => self.analyze_property(p),
            Expr::Cast(cast) => self.analyze_cast(cast),
            Expr::Subscript(s) => self.analyze_subscript(s),
            Expr::Call(c) => self.analyze_call(c),
            Expr::Action(a) => self.analyze_action(a),
            Expr::Constructor(c) => self.analyze_constructor(c),
            Expr::Match(m) => self.analyze_match_expr(m),
            Expr::Lambda(_) => unreachable!("Lambdas must be lifted before semantic analysis"),
        };
        trace!(expr_id = ?eid, ty = %ty, "Analyzed expression");
        self.expr_types.insert(eid, ty.clone());
        ty
    }

    fn analyze_ident(&mut self, name: StrId, span: &Span) -> Type {
        let name = self.str(name);
        if name == "_" {
            return Type::Unknown;
        }
        match self.lookup(&name) {
            Some(Symbol::Var { ty, .. } | Symbol::Param { ty }) => ty,
            Some(Symbol::Func { .. }) => Type::Unknown,
            Some(Symbol::Proc { .. }) => Type::Never,
            None => {
                if let Some(class) = self.get_class(&name) {
                    return Type::Class(class.def_id, vec![]);
                }
                self.analyze_unresolved_name(name, span)
            }
        }
    }

    fn analyze_variable(&mut self, variable: &VariableExpr) -> Type {
        for part in &variable.name.parts {
            if let TextPart::Interp(eid) = part {
                self.analyze_expr(*eid);
            }
        }
        let name = text_value_to_string(self.ast, &variable.name);
        match self.lookup(&name) {
            Some(Symbol::Var { ty, .. } | Symbol::Param { ty }) => ty,
            Some(Symbol::Func { .. }) => Type::Unknown,
            Some(Symbol::Proc { .. }) => Type::Never,
            None => self.analyze_unresolved_name(name, &variable.span),
        }
    }

    fn analyze_unresolved_name(&mut self, name: String, span: &Span) -> Type {
        if let Some(info) = self.get_enum(&name) {
            return Type::Enum(info.def_id);
        }
        if self.in_lambda {
            return self.unifier.new_var();
        }
        if self.edition >= 2026 {
            let suggestion = crate::utils::did_you_mean(
                &name,
                self.scopes
                    .iter()
                    .flat_map(|s| s.keys().map(String::as_str)),
            )
            .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"));
            self.error(
                SemanticErrorKind::UndeclaredVariable { name, suggestion },
                span.clone(),
            );
        } else {
            warn!(%name, ?span, "Use of undeclared variable, inferring type");
        }
        self.unifier.new_var()
    }

    fn analyze_ternary(&mut self, ternary: &TernaryExpr) -> Type {
        let condition = self.analyze_expr(ternary.cond);
        if !self.is_truthy(&condition) {
            self.error(
                SemanticErrorKind::InvalidTernaryCondition {
                    actual: self.format_type(&condition),
                },
                ternary.span.clone(),
            );
        }
        let then = self.analyze_expr(ternary.then_val);
        let otherwise = self.analyze_expr(ternary.else_val);
        match self.unifier.unify(&then, &otherwise) {
            Ok(unified) => unified,
            Err(_) if self.is_assignable_to(&then, &otherwise) => otherwise,
            Err(_) if self.is_assignable_to(&otherwise, &then) => then,
            Err(_) => {
                if !then.is_unknown() && !otherwise.is_unknown() {
                    self.error(
                        SemanticErrorKind::TernaryBranchTypeMismatch {
                            then_ty: self.format_type(&then),
                            else_ty: self.format_type(&otherwise),
                        },
                        ternary.span.clone(),
                    );
                }
                Type::Unknown
            }
        }
    }

    fn analyze_match_expr(&mut self, m: &MatchExpr) -> Type {
        let _scrutinee = self.analyze_expr(m.expr);
        self.analyze_match_arms(&m.arms);
        let mut result = Type::Unknown;
        for arm in &m.arms {
            if let [Statement::Expr(eid)] = arm.body.as_slice() {
                let ty = self.analyze_expr(*eid);
                if result.is_unknown() {
                    result = ty;
                } else if !ty.is_unknown() {
                    match self.unifier.unify(&result, &ty) {
                        Ok(unified) => result = unified,
                        Err(_) if self.is_assignable_to(&result, &ty) => result = ty,
                        Err(_) if self.is_assignable_to(&ty, &result) => {}
                        Err(_) => {
                            self.error(
                                SemanticErrorKind::MatchArmTypeMismatch {
                                    expected: self.format_type(&result),
                                    actual: self.format_type(&ty),
                                },
                                arm.span.clone(),
                            );
                        }
                    }
                }
            }
        }
        result
    }

    fn analyze_unary(&mut self, unary: &UnaryExpr) -> Type {
        let operand = self.analyze_expr(unary.operand);
        match unary.op {
            UnOp::Not => {
                if !self.is_truthy(&operand) {
                    self.error(
                        SemanticErrorKind::InvalidNotOperand {
                            actual: self.format_type(&operand),
                        },
                        unary.span.clone(),
                    );
                }
                self.lang_type("boolean", vec![])
            }
            UnOp::Neg | UnOp::Inc | UnOp::Dec => {
                if !self.is_numeric(&operand) {
                    self.error(
                        SemanticErrorKind::InvalidNumericOperand {
                            op: format!("{:?}", unary.op),
                            actual: self.format_type(&operand),
                        },
                        unary.span.clone(),
                    );
                }
                self.lang_type("number", vec![])
            }
        }
    }

    fn analyze_cast(&mut self, cast: &CastExpr) -> Type {
        let original = self.analyze_expr(cast.expr);
        let target = self.parse_decl_type(&self.str(cast.ty), cast.span.clone());
        let original = self.unifier.find(&original);
        let resolved_target = self.unifier.find(&target);
        if !matches!(original, Type::Unknown | Type::InferVar(_))
            && !matches!(resolved_target, Type::Unknown | Type::InferVar(_))
            && original == resolved_target
        {
            self.error(
                SemanticErrorKind::RedundantCast {
                    ty: self.format_type(&resolved_target),
                },
                cast.span.clone(),
            );
        }
        target
    }

    #[instrument(skip(self, b), level = "trace")]
    fn analyze_binary(&mut self, b: &BinaryExpr) -> Type {
        let lty = self.analyze_expr(b.left);
        let rty = self.analyze_expr(b.right);
        let op_str = format!("{:?}", b.op);
        trace!(op = %op_str, %lty, %rty, "Binary op");

        let lty_res = self.unifier.find(&lty);
        let rty_res = self.unifier.find(&rty);

        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        let is_any = |t: &Type| matches!(t, Type::Class(id, _) if *id == any_def_id);
        if is_any(&lty_res) || is_any(&rty_res) {
            self.error(
                SemanticErrorKind::OperationOnAny { op: op_str },
                b.span.clone(),
            );
            return Type::Unknown;
        }

        if let Some(result) = self.analyze_dunder_binary(b, &lty, &rty, &lty_res) {
            return result;
        }

        use BinOp::{
            Add, And, Assign, BitAnd, BitOr, BitXor, Div, Eq, Ge, Gt, In, Le, Lt, Mod, Mul, Ne, Or,
            Pow, Range, RangeInclusive, Shl, Shr, Sub,
        };
        match b.op {
            Range | RangeInclusive => {
                if !self.is_numeric(&lty) || !self.is_numeric(&rty) {
                    self.error(
                        SemanticErrorKind::InvalidArithmetic {
                            op: op_str,
                            lty: self.format_type(&lty),
                            rty: self.format_type(&rty),
                        },
                        b.span.clone(),
                    );
                }
                let class_name = if b.op == Range {
                    "Range"
                } else {
                    "RangeInclusive"
                };
                self.lang_type(class_name, vec![])
            }
            Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr => {
                if !self.is_numeric(&lty) || !self.is_numeric(&rty) {
                    let kind = if matches!(b.op, Add | Sub | Mul | Div | Mod | Pow) {
                        SemanticErrorKind::InvalidArithmetic {
                            op: op_str,
                            lty: self.format_type(&lty),
                            rty: self.format_type(&rty),
                        }
                    } else {
                        SemanticErrorKind::InvalidBitwise {
                            op: op_str,
                            lty: self.format_type(&lty),
                            rty: self.format_type(&rty),
                        }
                    };
                    self.error(kind, b.span.clone());
                }
                self.lang_type("number", vec![])
            }
            Eq | Ne | Lt | Le | Gt | Ge | In => self.lang_type("boolean", vec![]),
            And | Or => {
                if !self.is_truthy(&lty) || !self.is_truthy(&rty) {
                    self.error(
                        SemanticErrorKind::InvalidLogical {
                            op: op_str,
                            lty: self.format_type(&lty),
                            rty: self.format_type(&rty),
                        },
                        b.span.clone(),
                    );
                }
                self.lang_type("boolean", vec![])
            }
            Assign => rty,
        }
    }

    pub(super) fn has_binary_dunder(&mut self, dunder: &str, ty: &Type) -> bool {
        let resolved = self.unifier.find(ty);
        let Type::Class(initial_def, _) = resolved else {
            return false;
        };
        let mut current_def = initial_def;
        let empty_subst = HashMap::new();
        while let Some(class) = self.ir_ctx.classes_by_def.get(&current_def) {
            if let Some(method) = class.methods.get(dunder)
                && method.params.len() == 2
            {
                return true;
            }
            let Some((def, _)) = self.parent_step(class, current_def, &empty_subst) else {
                break;
            };
            current_def = def;
        }
        false
    }

    pub(super) fn lookup_binary_dunder(
        &mut self,
        dunder: &str,
        left: &Type,
        right: &Type,
        span: &Span,
    ) -> Option<Type> {
        let resolved_left = self.unifier.find(left);
        let Type::Class(initial_def, initial_args) = resolved_left else {
            return None;
        };
        let mut current_def = initial_def;
        let mut current_args = initial_args;
        while let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() {
            if let Some(method) = class.methods.get(dunder).cloned()
                && method.params.len() == 2
            {
                let subst = Self::build_generic_subst(&class.generics, &current_args);
                let params = self.method_params(&class, &method.params, &subst);
                self.require_type(left, &params[0].ty, span.clone(), |expected, actual| {
                    SemanticErrorKind::FuncArgTypeMismatch {
                        name: dunder.to_owned(),
                        arg: "lhs".to_owned(),
                        expected,
                        actual,
                    }
                });
                self.require_type(right, &params[1].ty, span.clone(), |expected, actual| {
                    SemanticErrorKind::FuncArgTypeMismatch {
                        name: dunder.to_owned(),
                        arg: "rhs".to_owned(),
                        expected,
                        actual,
                    }
                });
                return Some(self.callable_return_type(method.return_type, &class, &subst, span));
            }
            let subst = Self::build_generic_subst(&class.generics, &current_args);
            let Some((def, args)) = self.parent_step(&class, current_def, &subst) else {
                break;
            };
            current_def = def;
            current_args = args;
        }
        None
    }

    fn analyze_dunder_binary(
        &mut self,
        binary: &BinaryExpr,
        left: &Type,
        right: &Type,
        _resolved_left: &Type,
    ) -> Option<Type> {
        let dunder = op_to_dunder(&binary.op)?;
        self.lookup_binary_dunder(dunder, left, right, &binary.span)
    }

    #[instrument(skip(self, p), level = "trace")]
    fn analyze_property(&mut self, p: &PropertyExpr) -> Type {
        let obj_ty = self.analyze_expr(p.object);
        let prop_name = self.str(p.property);

        if let Some(current_prop) = self.getter_setter_stack.last()
            && *current_prop == prop_name
            && let Expr::Ident(name, _) = &self.ast.exprs[p.object]
        {
            let name_str = self.str(*name);
            if let Some(Symbol::Param { .. }) = self.lookup(&name_str) {
                self.error(
                    SemanticErrorKind::InfinitePropertyRecursion {
                        property: prop_name,
                    },
                    p.span.clone(),
                );
                return Type::Unknown;
            }
        }

        if let Expr::Ident(name, _) = &self.ast.exprs[p.object] {
            let name_str = self.str(*name);
            if let Some(enum_info) = self.get_enum(&name_str).cloned()
                && enum_info.values.iter().any(|v| v == &prop_name)
            {
                return Type::Enum(enum_info.def_id);
            }
        }

        let resolved_ty = self.unifier.find(&obj_ty);

        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        if matches!(resolved_ty, Type::Class(id, _) if id == any_def_id) {
            self.error(
                SemanticErrorKind::PropertyAccessOnAny {
                    property: prop_name,
                },
                p.span.clone(),
            );
            return Type::Unknown;
        }

        if let Type::Class(initial_def, initial_args) = &resolved_ty {
            let mut current_def = *initial_def;
            let mut current_args = initial_args.clone();

            while let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() {
                let subst = Self::build_generic_subst(&class.generics, &current_args);

                if let Some((field_ty, _)) = class.fields.get(&prop_name) {
                    if !subst.is_empty() {
                        return self.substitute(field_ty, &subst);
                    }
                    return field_ty.clone();
                }

                if let Some(getter) = class.getters.get(&prop_name) {
                    return self.callable_return_type(getter.return_type, &class, &subst, &p.span);
                }

                if class.setters.contains_key(&prop_name) {
                    return Type::Unknown;
                }

                if class.is_dict {
                    return Type::Unknown;
                }

                let Some((def, args)) = self.parent_step(&class, current_def, &subst) else {
                    break;
                };
                current_def = def;
                current_args = args;
            }
        }

        let suggestion = if resolved_ty.is_unknown() {
            " (cannot access property on expression of unknown type; try adding an explicit type annotation)".to_owned()
        } else if let Type::Class(def_id, _) = &resolved_ty
            && let Some(info) = self.ir_ctx.classes_by_def.get(def_id)
        {
            let mut candidates: Vec<&str> = info.fields.keys().map(String::as_str).collect();
            candidates.extend(info.getters.keys().map(String::as_str));
            candidates.extend(info.setters.keys().map(String::as_str));
            crate::utils::did_you_mean(&prop_name, candidates)
                .map_or_else(String::new, |s| format!(" (did you mean '{s}'?)"))
        } else {
            String::new()
        };

        self.error(
            SemanticErrorKind::UnknownProperty {
                ty: self.format_type(&resolved_ty),
                property: prop_name,
                suggestion,
            },
            p.span.clone(),
        );
        Type::Unknown
    }

    fn analyze_subscript(&mut self, s: &SubscriptExpr) -> Type {
        let obj_ty = self.analyze_expr(s.object);
        self.analyze_expr(s.index);
        let resolved_ty = self.unifier.find(&obj_ty);

        if let Some(end) = s.end {
            self.analyze_expr(end);
            if let Type::Class(initial_def, initial_args) = &resolved_ty {
                let mut current_def = *initial_def;
                let mut current_args = initial_args.clone();
                while let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() {
                    let subst = Self::build_generic_subst(&class.generics, &current_args);
                    if let Some(getter) = class.getters.get("__slice__")
                        && let Some(ret_ty) = getter.return_type
                    {
                        return self.callable_return_type(Some(ret_ty), &class, &subst, &s.span);
                    }
                    let Some((def, args)) = self.parent_step(&class, current_def, &subst) else {
                        break;
                    };
                    current_def = def;
                    current_args = args;
                }
            }
            if matches!(resolved_ty, Type::Class(_, _)) {
                return resolved_ty;
            }
            self.error(
                SemanticErrorKind::InvalidSlice {
                    ty: self.format_type(&resolved_ty),
                },
                s.span.clone(),
            );
            return Type::Unknown;
        }

        if let Type::Class(initial_def, initial_args) = &resolved_ty {
            let mut current_def = *initial_def;
            let mut current_args = initial_args.clone();
            while let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() {
                let subst = Self::build_generic_subst(&class.generics, &current_args);

                if let Some(getter) = class.getters.get("__subscript__")
                    && let Some(ret_ty) = getter.return_type
                {
                    return self.callable_return_type(Some(ret_ty), &class, &subst, &s.span);
                }

                let array_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
                let map_id = self.ir_ctx.lang_items.get("map").copied().unwrap_or(0);
                let text_id = self.ir_ctx.lang_items.get("text").copied().unwrap_or(0);

                if class.def_id == array_id {
                    return current_args.first().cloned().unwrap_or(Type::Unknown);
                }
                if class.def_id == map_id {
                    return current_args.get(1).cloned().unwrap_or(Type::Unknown);
                }
                if class.def_id == text_id {
                    return Type::Class(text_id, vec![]);
                }

                let Some((def, args)) = self.parent_step(&class, current_def, &subst) else {
                    break;
                };
                current_def = def;
                current_args = args;
            }
        }

        self.error(
            SemanticErrorKind::InvalidSubscript {
                ty: self.format_type(&resolved_ty),
            },
            s.span.clone(),
        );
        Type::Unknown
    }
}

const fn op_to_dunder(op: &BinOp) -> Option<&'static str> {
    match op {
        BinOp::Add => Some("__add__"),
        BinOp::Sub => Some("__subtract__"),
        BinOp::Mul => Some("__multiply__"),
        BinOp::Div => Some("__divide__"),
        BinOp::Mod => Some("__remainder__"),
        BinOp::Pow => Some("__pow__"),
        BinOp::Eq => Some("__equals__"),
        BinOp::Ne => Some("__not_equals__"),
        BinOp::Gt => Some("__greater__"),
        BinOp::Lt => Some("__less__"),
        BinOp::Ge => Some("__greater_or_equals__"),
        BinOp::Le => Some("__less_or_equals__"),
        BinOp::BitAnd => Some("__bit_and__"),
        BinOp::BitOr => Some("__bit_or__"),
        BinOp::BitXor => Some("__bit_xor__"),
        BinOp::Shl => Some("__shl__"),
        BinOp::Shr => Some("__shr__"),
        BinOp::In => Some("__contains__"),
        _ => None,
    }
}
