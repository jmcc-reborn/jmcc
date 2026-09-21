//! Expressions: AST expression transformation into HIR, including properties,
//! constructors, and platform actions.

use super::*;

impl OverloadExpander<'_> {
    #[instrument(skip(self), level = "trace")]
    pub(super) fn conv_ast_expr(&mut self, eid: ExprId) -> Result<Id> {
        match &self.ast.exprs[eid] {
            Expr::Number(n) => Ok(self.add(Hir::Num(n.value.into()))),
            Expr::Bool(b) => Ok(self.add(Hir::Bool(b.value))),
            Expr::Ident(n, _) => {
                let sym = self.sym(*n);
                if let Some(&id) = self
                    .inline_vars_stack
                    .last()
                    .expect("inline_vars_stack must be non-empty")
                    .get(&sym)
                {
                    return Ok(id);
                }
                let v = self.add(Hir::Var(VarName(sym)));
                Ok(self.add(Hir::Line(v)))
            }
            Expr::Variable(v) => {
                let name = self.sym_str_from_text(&v.name)?;
                if let Some(&id) = self
                    .inline_vars_stack
                    .last()
                    .expect("inline_vars_stack must be non-empty")
                    .get(&name)
                {
                    return Ok(id);
                }
                let var = self.add(Hir::Var(VarName(name)));
                Ok(self.add(Hir::Line(var)))
            }
            Expr::Text(text) => self.conv_ast_text(text),
            Expr::Binary(binary) => self.conv_ast_binary(binary),
            Expr::Unary(u) => {
                let operand = self.conv_ast_expr(u.operand)?;
                Ok(self.add(un_op_to_hir(u.op, operand)))
            }
            Expr::List(l) => {
                let mut ids = Vec::new();
                for &v in &l.values {
                    ids.push(self.conv_ast_expr(v)?);
                }
                Ok(self.add(Hir::List(ids.into_boxed_slice())))
            }
            Expr::Call(call) => self.conv_ast_call(call),
            Expr::Property(p) => self.conv_ast_property(p),
            Expr::Constructor(c) => self.conv_ast_ctor(c),
            Expr::Action(action) => self.conv_ast_action(action),
            Expr::Subscript(s) => {
                let obj = self.conv_ast_expr(s.object)?;
                if let Some(end) = s.end {
                    let start = self.conv_ast_expr(s.index)?;
                    let end_atom = self.conv_ast_expr(end)?;
                    Ok(self.add(Hir::Slice([obj, start, end_atom])))
                } else {
                    let idx = self.conv_ast_expr(s.index)?;
                    Ok(self.add(Hir::Index([obj, idx])))
                }
            }
            Expr::Cast(c) => self.conv_ast_expr(c.expr),
            Expr::Match(m) => self.conv_ast_match(&m.arms, m.expr),
            _ => Err(OverloadError::UnsupportedExpr(match &self.ast.exprs[eid] {
                Expr::Nbt(_) => "nbt",
                Expr::Map(_) => "map",
                Expr::Ternary(_) => "ternary",
                Expr::Property(_) => "property",
                Expr::Constructor(_) => "constructor",
                _ => "unknown",
            })),
        }
    }

    pub(super) fn block_or_concat(&mut self, parts: Vec<Id>) -> Id {
        match parts.as_slice() {
            [part] => *part,
            _ => self.add(Hir::Concat(parts.into_boxed_slice())),
        }
    }

    fn conv_ast_binary(&mut self, binary: &BinaryExpr) -> Result<Id> {
        let left = self.conv_ast_expr(binary.left)?;
        let right = self.conv_ast_expr(binary.right)?;
        let node = match bin_op_to_hir(binary.op, left, right) {
            BinOpHir::Node(node) => node,
            BinOpHir::Assign => Hir::Set([left, right]),
            BinOpHir::Range | BinOpHir::RangeInclusive => {
                let one = self.add(Hir::Num(1.0.into()));
                let list = self.add(Hir::List(vec![left, right, left, one].into_boxed_slice()));
                let name = self.fresh();
                let var_raw = self.add(Hir::Var(VarName(name)));
                let var = self.add(Hir::Line(var_raw));
                let class_name = if binary.op == BinOp::Range {
                    "Range"
                } else {
                    "RangeInclusive"
                };
                let ty = Type::Class(
                    self.ctx.lang_items.get(class_name).copied().unwrap_or(0),
                    vec![],
                );
                self.ctx.record_var_type(name, ty);
                return Ok(self.add(Hir::Let([var, list, var])));
            }
        };
        Ok(self.add(node))
    }

    pub(super) fn conv_ast_args(&mut self, args: &[ArgExpr]) -> Result<Id> {
        let mut ids = Vec::with_capacity(args.len());
        for arg in args {
            let value = self.conv_ast_expr(arg.value)?;
            let value = arg.name.map_or(value, |name| {
                let name = self.add(Hir::Str(StrLit(self.sym(name))));
                self.add(Hir::Named([name, value]))
            });
            ids.push(value);
        }
        Ok(self.add(Hir::List(ids.into_boxed_slice())))
    }

    fn conv_ast_ctor(&mut self, c: &ConstructorExpr) -> Result<Id> {
        let name = self.add(Hir::Str(StrLit(self.sym(c.name))));
        let args = self.conv_ast_args(&c.args)?;
        Ok(self.add(Hir::Ctor(vec![name, args].into_boxed_slice())))
    }

    fn target_ty(&self, eid: ExprId) -> Type {
        let ty = self.types.get(&eid).cloned().unwrap_or(Type::Unknown);
        if !matches!(ty, Type::Unknown | Type::InferVar(_)) {
            return ty;
        }
        if let Expr::Ident(name_id, _) = &self.ast.exprs[eid] {
            let sym = self.sym(*name_id);
            if let Some(actual_ty) = self.ctx.var_types.get(&sym).cloned() {
                return actual_ty;
            }
        }
        ty
    }

    /// Property access: enum variant, class getter, field slot, or dict.
    /// Same order as `hir::conv_property`, since overloads rewrite method bodies from the AST.
    fn conv_ast_property(&mut self, p: &PropertyExpr) -> Result<Id> {
        let prop_name = self.sym(p.property);

        if let Expr::Ident(name_id, _) = &self.ast.exprs[p.object] {
            let name = self.ast.strings.resolve(name_id).to_owned();
            if let Some(def_id) = self.ctx.enums_by_name.get(&name).copied()
                && let Some(enum_info) = self.ctx.enums_by_def.get(&def_id)
            {
                if enum_info
                    .values
                    .iter()
                    .any(|value| *value == prop_name.as_str())
                {
                    return Ok(self.add(Hir::Str(StrLit(prop_name))));
                }
                return Err(OverloadError::EnumVariantNotFound {
                    enum_name: name,
                    variant: prop_name.to_string(),
                });
            }
        }

        let target_ty = self.target_ty(p.object);

        let getter = self.find_in_classes(&target_ty, |class| {
            class.getters.get(prop_name.as_str()).cloned()
        });
        if let Some(getter) = getter {
            let target = self.conv_ast_expr(p.object)?;
            return self.expand_inline_from_hir(&getter, vec![target]);
        }

        let is_single = self.ctx.is_single_field_type(&target_ty);
        if is_single
            && self
                .find_in_classes(&target_ty, |class| {
                    class.fields.contains_key(prop_name.as_str()).then_some(())
                })
                .is_some()
        {
            return self.conv_ast_expr(p.object);
        }

        let slot = self.find_in_classes(&target_ty, |class| {
            class.fields.get(prop_name.as_str()).map(|(_, slot)| *slot)
        });
        if let Some(slot) = slot {
            let target = self.conv_ast_expr(p.object)?;
            let index = self.add(Hir::Num((slot as f64).into()));
            let args = self.add(Hir::List(vec![target, index].into_boxed_slice()));
            return Ok(self.variable_action("get_list_value", args));
        }

        if self
            .find_in_classes(&target_ty, |class| class.is_dict.then_some(()))
            .is_some()
        {
            let target = self.conv_ast_expr(p.object)?;
            let key = self.add(Hir::Str(StrLit(prop_name)));
            let args = self.add(Hir::List(vec![target, key].into_boxed_slice()));
            return Ok(self.variable_action("get_map_value", args));
        }

        Err(OverloadError::PropertyNotFound {
            property: prop_name.to_string(),
            ty: format!("{target_ty:?}"),
        })
    }

    /// Property assignment: class setter, field slot, or dict.
    /// `None` means the property is not writable; the target parses as an ordinary expression.
    pub(super) fn conv_ast_property_assign(
        &mut self,
        p: &PropertyExpr,
        val: Id,
    ) -> Result<Option<Id>> {
        let prop_name = self.sym(p.property);
        let target_ty = self.target_ty(p.object);

        let setter = self.find_in_classes(&target_ty, |class| {
            class.setters.get(prop_name.as_str()).cloned()
        });
        if let Some(setter) = setter {
            let target = self.conv_ast_expr(p.object)?;
            let result = self.expand_inline_from_hir(&setter, vec![target, val])?;
            return Ok(Some(if setter.return_type.is_some() {
                self.add(Hir::Set([target, result]))
            } else {
                result
            }));
        }

        let is_single = self.ctx.is_single_field_type(&target_ty);
        if is_single
            && self
                .find_in_classes(&target_ty, |class| {
                    class.fields.contains_key(prop_name.as_str()).then_some(())
                })
                .is_some()
        {
            let target = self.conv_ast_expr(p.object)?;
            return Ok(Some(self.add(Hir::Set([target, val]))));
        }

        let slot = self.find_in_classes(&target_ty, |class| {
            class.fields.get(prop_name.as_str()).map(|(_, slot)| *slot)
        });
        if let Some(slot) = slot {
            let target = self.conv_ast_expr(p.object)?;
            let index = self.add(Hir::Num((slot as f64).into()));
            let args = self.add(Hir::List(vec![target, index, val].into_boxed_slice()));
            let action = self.variable_action("set_list_value", args);
            return Ok(Some(self.add(Hir::Set([target, action]))));
        }

        if self
            .find_in_classes(&target_ty, |class| class.is_dict.then_some(()))
            .is_some()
        {
            let target = self.conv_ast_expr(p.object)?;
            let key = self.add(Hir::Str(StrLit(prop_name)));
            let args = self.add(Hir::List(vec![target, key, val].into_boxed_slice()));
            let action = self.variable_action("set_map_value", args);
            return Ok(Some(self.add(Hir::Set([target, action]))));
        }

        Ok(None)
    }

    /// A `variable`-object action: empty receiver and block slots, `args` in the argument slot.
    fn variable_action(&mut self, name: &str, args: Id) -> Id {
        let object = self.add(Hir::Str(StrLit(Symbol::from("variable"))));
        let name = self.add(Hir::Str(StrLit(Symbol::from(name))));
        let nop = self.add(Hir::Nop);
        self.add(Hir::Action(
            vec![object, name, nop, args, nop, nop, nop].into_boxed_slice(),
        ))
    }

    fn conv_ast_action(&mut self, action: &ActionExpr) -> Result<Id> {
        let object = self.add(Hir::Str(StrLit(self.sym(action.object))));
        let name = self.add(Hir::Str(StrLit(self.sym(action.name))));
        let selector = match action.selector {
            Some(selector) => {
                let selector = self.add(Hir::Str(StrLit(self.sym(selector))));
                self.add(Hir::Sel(selector))
            }
            None => self.add(Hir::Nop),
        };
        let args = self.conv_ast_args(&action.args)?;
        let block = action
            .operations
            .as_deref()
            .map(|operations| self.conv_ast_block(operations))
            .transpose()?
            .unwrap_or_else(|| self.add(Hir::Nop));
        let lambda = action
            .lambda
            .as_deref()
            .map(|params| self.conv_ast_exprs(params))
            .transpose()?
            .unwrap_or_else(|| self.add(Hir::Nop));
        let condition = self.add(Hir::Nop);
        Ok(self.add(Hir::Action(
            vec![object, name, selector, args, block, lambda, condition].into_boxed_slice(),
        )))
    }

    fn conv_ast_exprs(&mut self, expressions: &[ExprId]) -> Result<Id> {
        let ids = expressions
            .iter()
            .map(|expression| self.conv_ast_expr(*expression))
            .collect::<Result<Vec<_>>>()?;
        Ok(self.add(Hir::List(ids.into_boxed_slice())))
    }
}
