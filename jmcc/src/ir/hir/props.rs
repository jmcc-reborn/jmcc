//! Properties and indexing: reading and writing class fields,
//! `__subscript__` / `__slice__`, and member resolution along inheritance chains.

use super::*;

impl HirBuilder<'_> {
    pub(super) fn find_in_classes<F, R>(&self, ty: &Type, f: F) -> Option<R>
    where
        F: Fn(&crate::ir::ctx::ClassInfo) -> Option<R>,
    {
        find_in_class_chain(ty, &self.ir_ctx.classes_by_def, f)
    }

    fn resolve_target_ty(&self, eid: ExprId) -> Type {
        let ty = self.types.get(&eid).cloned().unwrap_or(Type::Unknown);
        if !matches!(ty, Type::Unknown | Type::InferVar(_)) {
            return ty;
        }
        if let Expr::Ident(name_id, _) = &self.ast.exprs[eid] {
            let sym = self.sym(*name_id);
            if let Some(&Binding::Var {
                name: bound_sym, ..
            }) = self.lookup(sym)
                && let Some(actual_ty) = self.ir_ctx.var_types.get(&bound_sym).cloned()
            {
                return actual_ty;
            }
        }
        ty
    }

    /// Class field: dictionary-backed, direct single-field, or numeric slot, resolved along inheritance chain.
    fn find_field_access(&self, ty: &Type, prop_name: &Symbol) -> Option<FieldAccess> {
        let is_single_field = self.ir_ctx.is_single_field_type(ty);
        self.find_in_classes(ty, |c| {
            if c.is_dict {
                Some(FieldAccess::Dict)
            } else if is_single_field && c.fields.contains_key(prop_name.as_str()) {
                Some(FieldAccess::Direct)
            } else {
                c.fields
                    .get(prop_name.as_str())
                    .map(|(_, i)| FieldAccess::Slot(*i))
            }
        })
    }

    pub(super) fn try_property_assign(
        &mut self,
        p: &PropertyExpr,
        val: Id,
        stmts: &mut Vec<Id>,
    ) -> Result<bool, IrError> {
        let target_ty = self.resolve_target_ty(p.object);
        let prop_name = self.sym(p.property);

        let member = self.find_in_classes(&target_ty, |c| {
            c.setters
                .get(prop_name.as_str())
                .cloned()
                .map(ClassMember::Setter)
        });

        if let Some(ClassMember::Setter(f)) = member {
            let all_args = vec![positional(p.object)];
            let last_param_name = f
                .params
                .last()
                .map(|p| self.sym(p.name).to_string())
                .unwrap_or_else(|| "value".to_string());
            let predefined = vec![(last_param_name, val)];
            let result =
                self.expand_inline_func_call(&f, &all_args, Vec::new(), predefined, None)?;
            let tgt = self.conv_expr(p.object)?;
            stmts.push(self.add(Hir::Set([tgt, result])));
            return Ok(true);
        }

        let field_info = self.find_field_access(&target_ty, &prop_name);

        if let Some(access) = field_info {
            let target_id = self.conv_expr(p.object)?;
            match access {
                FieldAccess::Direct => {
                    stmts.push(self.add(Hir::Set([target_id, val])));
                }
                FieldAccess::Slot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    let args =
                        self.add(Hir::List(vec![target_id, index_id, val].into_boxed_slice()));
                    let act = self.action("variable", Symbol::from("set_list_value"), args);
                    stmts.push(self.add(Hir::Set([target_id, act])));
                }
                FieldAccess::Dict => {
                    let key_id = self.str_lit(prop_name);
                    let args = self.add(Hir::List(vec![target_id, key_id, val].into_boxed_slice()));
                    let act = self.action("variable", Symbol::from("set_map_value"), args);
                    stmts.push(self.add(Hir::Set([target_id, act])));
                }
            }
            return Ok(true);
        }

        Ok(false)
    }

    pub(super) fn try_subscript_assign(
        &mut self,
        s: &SubscriptExpr,
        val: Id,
        stmts: &mut Vec<Id>,
    ) -> Result<bool, IrError> {
        let target_ty = self.resolve_target_ty(s.object);
        let method_name = if s.end.is_some() {
            "__slice__"
        } else {
            "__subscript__"
        };

        let setter = self.find_in_classes(&target_ty, |c| {
            c.setters.get(method_name).cloned().map(ClassMember::Setter)
        });

        if let Some(ClassMember::Setter(f)) = setter {
            let mut all_args = vec![positional(s.object), positional(s.index)];
            if let Some(end) = s.end {
                all_args.push(positional(end));
            }
            let last_param_name = f
                .params
                .last()
                .map(|p| self.sym(p.name).to_string())
                .unwrap_or_else(|| "value".to_string());
            let predefined = vec![(last_param_name, val)];
            let result =
                self.expand_inline_func_call(&f, &all_args, Vec::new(), predefined, None)?;
            let tgt = self.conv_expr(s.object)?;
            stmts.push(self.add(Hir::Set([tgt, result])));
            return Ok(true);
        }

        if s.end.is_some() {
            return Err(IrError::SliceAssignNotSupported(format!("{target_ty:?}")));
        }

        let array_id = self.lang_item("array");
        let map_id = self.lang_item("map");

        if !matches!(target_ty, Type::Class(id, _) if id == array_id || id == map_id) {
            return Err(IrError::SubscriptAssignNotSupported(format!(
                "{target_ty:?}"
            )));
        }
        Ok(false)
    }

    #[instrument(skip(self, p), level = "trace")]
    pub(super) fn conv_property(&mut self, p: &PropertyExpr) -> Result<Id, IrError> {
        let prop_name = self.sym(p.property);

        if let Expr::Ident(name_id, _) = &self.ast.exprs[p.object] {
            let name = self.ast.strings.resolve(name_id);
            if let Some(def_id) = self.ir_ctx.enums_by_name.get(name)
                && let Some(enum_info) = self.ir_ctx.enums_by_def.get(def_id)
            {
                if enum_info.values.iter().any(|v| v == prop_name.as_str()) {
                    return Ok(self.str_lit(prop_name));
                }
                return Err(IrError::EnumVariantNotFound {
                    enum_name: name.to_owned(),
                    variant: prop_name.to_string(),
                });
            }
        }

        let target_ty = self.resolve_target_ty(p.object);

        let member = self.find_in_classes(&target_ty, |c| {
            c.getters
                .get(prop_name.as_str())
                .cloned()
                .map(ClassMember::Getter)
        });

        if let Some(ClassMember::Getter(f)) = member {
            return self.expand_inline_func_call(
                &f,
                &[positional(p.object)],
                Vec::new(),
                Vec::new(),
                None,
            );
        }

        let field_info = self.find_field_access(&target_ty, &prop_name);

        if let Some(access) = field_info {
            let target_id = self.conv_expr(p.object)?;
            return Ok(match access {
                FieldAccess::Direct => target_id,
                FieldAccess::Slot(idx) => {
                    let index_id = self.add(Hir::Num(OrderedFloat(idx as f64)));
                    let args = self.add(Hir::List(vec![target_id, index_id].into_boxed_slice()));
                    self.action("variable", Symbol::from("get_list_value"), args)
                }
                FieldAccess::Dict => {
                    let key_id = self.str_lit(prop_name);
                    let args = self.add(Hir::List(vec![target_id, key_id].into_boxed_slice()));
                    self.action("variable", Symbol::from("get_map_value"), args)
                }
            });
        }

        let ty_str = self
            .get_class_name(&target_ty)
            .unwrap_or_else(|| format!("{target_ty:?}"));
        Err(IrError::PropertyNotFound {
            property: prop_name.to_string(),
            ty: ty_str,
        })
    }

    #[instrument(skip(self, s), level = "trace")]
    pub(super) fn conv_subscript(&mut self, s: &SubscriptExpr) -> Result<Id, IrError> {
        let target_ty = self.resolve_target_ty(s.object);
        let method_name = if s.end.is_some() {
            "__slice__"
        } else {
            "__subscript__"
        };

        let getter = self.find_in_classes(&target_ty, |c| {
            c.getters.get(method_name).cloned().map(ClassMember::Getter)
        });

        if let Some(ClassMember::Getter(f)) = getter {
            let mut all_args = vec![positional(s.object), positional(s.index)];
            if let Some(end) = s.end {
                all_args.push(positional(end));
            }
            return self.expand_inline_func_call(&f, &all_args, Vec::new(), Vec::new(), None);
        }

        let (obj, mut b) = self.atomize(s.object)?;
        if let Some(end_eid) = s.end {
            let (start, mut sb) = self.atomize(s.index)?;
            let (end_atom, mut eb) = self.atomize(end_eid)?;
            b.append(&mut sb);
            b.append(&mut eb);
            let node = self.add(Hir::Slice([obj, start, end_atom]));
            Ok(self.wrap_lets(node, b))
        } else {
            let (idx, mut ib) = self.atomize(s.index)?;
            b.append(&mut ib);
            let node = self.add(Hir::Index([obj, idx]));
            Ok(self.wrap_lets(node, b))
        }
    }
}
