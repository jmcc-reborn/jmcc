//! Types and compatibility: subtypes and upcasts, generic substitutions, parsing declared types,
//! assignable/truthy/numeric rules, class and enum lookups, and type formatting.

use super::*;

impl Analyzer<'_> {
    pub(super) fn format_type(&self, ty: &Type) -> String {
        match ty {
            Type::Class(id, args) => {
                let name = self
                    .ir_ctx
                    .classes_by_def
                    .get(id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| format!("Class#{id}"));
                let id_str = format!("Class#{id}");
                if args.is_empty() {
                    format!("{name} ({id_str})")
                } else {
                    let args_str = args
                        .iter()
                        .map(|a| self.format_type(a))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}<{args_str}> ({id_str})")
                }
            }
            Type::Enum(id) => {
                let name = self
                    .ir_ctx
                    .enums_by_def
                    .get(id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| format!("Enum#{id}"));
                format!("{name} (Enum#{id})")
            }
            Type::InferVar(id) => format!("?{id}"),
            Type::Param(id) => self.str(*id),
            Type::Never => "Never".to_owned(),
            Type::Unknown => "Error".to_owned(),
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn is_subclass_of(&self, child: DefId, parent: DefId) -> bool {
        if child == parent {
            return true;
        }
        let mut queue = std::collections::VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back(child);
        visited.insert(child);

        while let Some(current) = queue.pop_front() {
            if let Some(class) = self.ir_ctx.classes_by_def.get(&current) {
                if let Some(p) = class.parent {
                    if p == parent {
                        return true;
                    }
                    if visited.insert(p) {
                        queue.push_back(p);
                    }
                }
                for (iface_id, _) in &class.implements {
                    if *iface_id == parent {
                        return true;
                    }
                    if visited.insert(*iface_id) {
                        queue.push_back(*iface_id);
                    }
                }
            }
        }
        false
    }

    /// Upcasts a class while substituting generic arguments at each parent boundary.
    fn upcast_to(&self, ty: &Type, target_def: DefId) -> Option<Type> {
        if let Type::Class(def_id, args) = ty {
            let mut queue = std::collections::VecDeque::new();
            let mut visited = HashSet::new();
            queue.push_back((*def_id, args.clone()));
            visited.insert(*def_id);

            while let Some((current_def, current_args)) = queue.pop_front() {
                if current_def == target_def {
                    return Some(Type::Class(current_def, current_args));
                }
                let Some(class) = self.ir_ctx.classes_by_def.get(&current_def).cloned() else {
                    continue;
                };
                let subst = Self::build_generic_subst(&class.generics, &current_args);
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
        }
        None
    }

    /// One step up the class inheritance chain: substitutes generic arguments across
    /// the inheritance boundary. Returns `None` if the chain terminates or loops.
    pub(super) fn parent_step(
        &self,
        class: &ClassInfo,
        current_def: DefId,
        subst: &HashMap<StrId, Type>,
    ) -> Option<(DefId, Vec<Type>)> {
        let parent = class.parent.filter(|parent| *parent != current_def)?;
        let args = class
            .parent_args
            .iter()
            .map(|ty| self.substitute(ty, subst))
            .collect();
        Some((parent, args))
    }

    /// Whether the class is `number` or `text`.
    ///
    /// Values are stored as written and converted by the platform on use, so the two are
    /// interchangeable: `"0.0"` in a numeric slot, `10` in a text one.
    pub(super) fn is_scalar_class(&self, id: DefId) -> bool {
        ["number", "text"].iter().any(|name| {
            self.ir_ctx
                .lang_items
                .get(*name)
                .is_some_and(|&def_id| def_id == id)
        })
    }

    /// Short class name (`number`, `block`, …), for the rules that match on names rather than ids.
    pub(super) fn class_short_name(&self, id: DefId) -> Option<&str> {
        let name = self.ir_ctx.classes_by_def.get(&id)?.name.as_str();
        Some(name.rsplit("::").next().unwrap_or(name))
    }

    /// Pairs that the platform treats as the same slot but that are not related by inheritance.
    ///
    /// `boolean` is accepted where a number is expected (`canDeposit = a.not_equals(b)`), and
    /// `array` and `map` are one untyped variable slot under two spellings.
    fn is_interchangeable_platform_class(&self, actual: &Type, target: &Type) -> bool {
        let (Type::Class(actual_id, _), Type::Class(target_id, _)) = (actual, target) else {
            return false;
        };
        let (Some(actual), Some(target)) = (
            self.class_short_name(*actual_id),
            self.class_short_name(*target_id),
        ) else {
            return false;
        };
        matches!((actual, target), ("boolean", "number"))
            || (matches!(actual, "array" | "map") && matches!(target, "array" | "map"))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn is_assignable_to(&self, actual: &Type, target: &Type) -> bool {
        let variable_def_id = self.ir_ctx.lang_items.get("variable").copied().unwrap_or(0);
        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        let array_def_id = self.ir_ctx.lang_items.get("array").copied().unwrap_or(0);
        // `value` is "any value" just like `any`: it holds a UUID text or an item.
        let value_def_id = self.ir_ctx.lang_items.get("value").copied().unwrap_or(0);

        match (actual, target) {
            (Type::Unknown | Type::Never, _) | (_, Type::Unknown) => true,

            (Type::Class(id1, _), Type::Class(id2, _))
                if *id1 == variable_def_id
                    || *id2 == variable_def_id
                    || *id1 == any_def_id
                    || *id2 == any_def_id
                    || *id1 == value_def_id
                    || *id2 == value_def_id =>
            {
                true
            }

            // Number and text, see `is_scalar_class`.
            (Type::Class(id1, _), Type::Class(id2, _))
                if self.is_scalar_class(*id1) && self.is_scalar_class(*id2) =>
            {
                true
            }

            // `is_interchangeable_platform_class`.
            (Type::Class(..), Type::Class(..))
                if self.is_interchangeable_platform_class(actual, target) =>
            {
                true
            }

            (actual, Type::Class(id, args))
                if *id == array_def_id
                    && args.len() == 1
                    && !matches!(actual, Type::Class(a_id, _) if *a_id == array_def_id) =>
            {
                // Scalar-to-array coercion is unsafe when the element type is unknown.
                if matches!(&args[0], Type::Unknown) {
                    return false;
                }
                self.is_assignable_to(actual, &args[0])
            }

            (Type::Class(id1, args1), Type::Class(id2, args2)) => {
                if !self.is_subclass_of(*id1, *id2) {
                    return false;
                }

                if *id1 == *id2 {
                    if args1.is_empty() || args2.is_empty() {
                        return true;
                    }
                    if args1.len() != args2.len() {
                        return false;
                    }
                    return args1
                        .iter()
                        .zip(args2.iter())
                        .all(|(a1, a2)| self.is_assignable_to(a1, a2));
                }

                if let Some(Type::Class(_, upcasted_args1)) =
                    self.upcast_to(&Type::Class(*id1, args1.clone()), *id2)
                {
                    if args2.is_empty() || upcasted_args1.is_empty() {
                        return true;
                    }
                    if upcasted_args1.len() != args2.len() {
                        return false;
                    }
                    return upcasted_args1
                        .iter()
                        .zip(args2.iter())
                        .all(|(a1, a2)| self.is_assignable_to(a1, a2));
                }
                false
            }
            (Type::Enum(id1), Type::Enum(id2)) => id1 == id2,
            (Type::InferVar(_) | Type::Param(_), _) | (_, Type::InferVar(_) | Type::Param(_)) => {
                true
            }
            _ => false,
        }
    }

    #[instrument(skip(self, make_err), level = "trace")]
    pub(super) fn require_type(
        &mut self,
        actual: &Type,
        expected: &Type,
        span: Span,
        make_err: impl FnOnce(String, String) -> SemanticErrorKind,
    ) {
        let actual_res = self.unifier.find(actual);
        let expected_res = self.unifier.find(expected);
        if let (Type::InferVar(_), _) | (_, Type::InferVar(_)) = (&actual_res, &expected_res)
            && self.unifier.unify(&actual_res, &expected_res).is_ok()
        {
            return;
        }
        if !self.is_assignable_to(&actual_res, &expected_res)
            && actual_res != Type::Unknown
            && expected_res != Type::Unknown
        {
            self.error(
                make_err(
                    self.format_type(&expected_res),
                    self.format_type(&actual_res),
                ),
                span,
            );
        }
    }

    pub(super) fn is_truthy(&mut self, ty: &Type) -> bool {
        let ty = self.unifier.find(ty);
        let boolean_id = self.ir_ctx.lang_items.get("boolean").copied().unwrap_or(0);
        let number_id = self.ir_ctx.lang_items.get("number").copied().unwrap_or(0);
        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        match &ty {
            Type::InferVar(id) => {
                let _unify_result: std::result::Result<Type, String> = self
                    .unifier
                    .unify(&Type::InferVar(*id), &Type::Class(boolean_id, vec![]));
                true
            }
            Type::Class(def_id, _) => {
                *def_id == boolean_id || *def_id == number_id || *def_id == any_def_id
            }
            _ => false,
        }
    }

    pub(super) fn is_numeric(&mut self, ty: &Type) -> bool {
        let ty = self.unifier.find(ty);
        let number_id = self.ir_ctx.lang_items.get("number").copied().unwrap_or(0);
        let any_def_id = self.ir_ctx.lang_items.get("any").copied().unwrap_or(0);
        match &ty {
            Type::InferVar(id) => {
                let _unify_result: std::result::Result<Type, String> = self
                    .unifier
                    .unify(&Type::InferVar(*id), &Type::Class(number_id, vec![]));
                true
            }
            Type::Class(def_id, _) => *def_id == number_id || *def_id == any_def_id,
            _ => false,
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn get_class(&self, name: &str) -> Option<&crate::ir::ctx::ClassInfo> {
        self.ir_ctx.get_class_by_name(name)
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn get_enum(&self, name: &str) -> Option<&crate::ir::ctx::EnumInfo> {
        let id = self.ir_ctx.enums_by_name.get(name)?;
        self.ir_ctx.enums_by_def.get(id)
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_decl_type(&mut self, s: &str, span: Span) -> Type {
        let s = s.trim();

        if let Some(spur) = self.ast.strings.get(s) {
            for scope in self.generic_scopes.iter().rev() {
                if let Some(ty) = scope.get(&spur) {
                    return ty.clone();
                }
            }
        }

        if let Some(lt_pos) = crate::ir::ctx::find_top_level_lt(s)
            && s.ends_with('>')
        {
            let base_name = s[..lt_pos].trim();
            let args_str = &s[lt_pos + 1..s.len() - 1];

            if let Some(spur) = self.ast.strings.get(base_name) {
                for scope in self.generic_scopes.iter().rev() {
                    if scope.contains_key(&spur) {
                        self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
                        return Type::Unknown;
                    }
                }
            }

            if let Some(alias) = self.ir_ctx.get_type_alias(base_name).cloned() {
                let args: Vec<Type> = crate::ir::ctx::split_type_args(args_str)
                    .iter()
                    .map(|arg| self.parse_decl_type(arg, span.clone()))
                    .collect();

                if args.len() != alias.generics.len() {
                    self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
                    return Type::Unknown;
                }

                let subst = Self::build_generic_subst(&alias.generics, &args);

                self.generic_scopes.push(subst.clone());
                let target_ty = self.parse_decl_type(&alias.target_ty, span);
                self.generic_scopes.pop();

                return self.substitute(&target_ty, &subst);
            }

            if let Some(class_info) = self.get_class(base_name).cloned() {
                let args: Vec<Type> = crate::ir::ctx::split_type_args(args_str)
                    .iter()
                    .map(|arg| self.parse_decl_type(arg, span.clone()))
                    .collect();

                if !class_info.generics.is_empty()
                    && args.len() != class_info.generics.len()
                    && !args.is_empty()
                {
                    self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
                    return Type::Unknown;
                }
                return Type::Class(class_info.def_id, args);
            }

            self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
            return Type::Unknown;
        }

        if let Some(alias) = self.ir_ctx.get_type_alias(s).cloned() {
            if !alias.generics.is_empty() {
                self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
                return Type::Unknown;
            }
            return self.parse_decl_type(&alias.target_ty, span);
        }

        let ty = self.ir_ctx.type_from_str(s);
        if matches!(ty, Type::Unknown) {
            self.error(SemanticErrorKind::UnknownType(s.to_owned()), span);
        }
        ty
    }

    #[instrument(skip(self, expected, actual, subst), level = "trace")]
    pub(super) fn match_generics(
        &self,
        expected: &Type,
        actual: &Type,
        subst: &mut HashMap<StrId, Type>,
    ) {
        match (expected, actual) {
            (Type::Param(id), actual_ty) => {
                if !matches!(actual_ty, Type::Unknown | Type::InferVar(_)) {
                    subst.insert(*id, actual_ty.clone());
                }
            }
            (Type::Class(id1, args1), Type::Class(id2, args2))
                if id1 == id2 && args1.len() == args2.len() =>
            {
                for (e, a) in args1.iter().zip(args2.iter()) {
                    self.match_generics(e, a, subst);
                }
            }
            _ => {}
        }
    }

    #[instrument(skip(self, ty, subst), level = "trace")]
    pub(super) fn substitute(&self, ty: &Type, subst: &HashMap<StrId, Type>) -> Type {
        match ty {
            Type::Param(id) => subst.get(id).cloned().unwrap_or(Type::Unknown),
            Type::Class(id, args) => Type::Class(
                *id,
                args.iter().map(|a| self.substitute(a, subst)).collect(),
            ),
            _ => ty.clone(),
        }
    }

    /// Builds generic parameter substitution from concrete arguments. `Unknown` and unconstrained
    /// variables are skipped to avoid invalid parameter bindings.
    pub(super) fn build_generic_subst(generics: &[StrId], args: &[Type]) -> HashMap<StrId, Type> {
        let mut subst = HashMap::new();
        if !generics.is_empty() && args.len() == generics.len() {
            for (g, arg) in generics.iter().zip(args.iter()) {
                if !matches!(arg, Type::Unknown | Type::InferVar(_)) {
                    subst.insert(*g, arg.clone());
                }
            }
        }
        subst
    }

    pub(super) fn lang_type(&self, name: &str, args: Vec<Type>) -> Type {
        Type::Class(self.ir_ctx.lang_items.get(name).copied().unwrap_or(0), args)
    }

    pub(super) fn common_element_type(&mut self, expressions: &[ExprId]) -> Type {
        let Some((&first, rest)) = expressions.split_first() else {
            return self.unifier.new_var();
        };
        let mut common = self.analyze_expr(first);
        for &expression in rest {
            let ty = self.analyze_expr(expression);
            if let Ok(unified) = self.unifier.unify(&common, &ty) {
                common = unified;
            }
        }
        common
    }

    pub(super) fn unify_element_types(&mut self, types: &[Type]) -> Type {
        let Some((first, rest)) = types.split_first() else {
            return self.unifier.new_var();
        };
        let mut common = first.clone();
        for ty in rest {
            if let Ok(unified) = self.unifier.unify(&common, ty) {
                common = unified;
            }
        }
        common
    }
}
