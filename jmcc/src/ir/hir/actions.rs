//! `JustMC` actions: `Action` nodes, selectors, and `*_with_conditional` conditions.

use super::*;

impl HirBuilder<'_> {
    pub(super) fn action(&mut self, obj: &str, name: Symbol, args: Id) -> Id {
        let oid = self.str_lit(obj);
        let nid = self.str_lit(name);
        let nop = self.nop();
        self.add(Hir::Action(
            vec![oid, nid, nop, args, nop, nop, nop].into_boxed_slice(),
        ))
    }

    pub(super) fn action_with(&mut self, obj: Symbol, name: Symbol, parts: [Id; 5]) -> Id {
        let [sel, args, block, lambda, cond] = parts;
        let oid = self.str_lit(obj);
        let nid = self.str_lit(name);
        self.add(Hir::Action(
            vec![oid, nid, sel, args, block, lambda, cond].into_boxed_slice(),
        ))
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn conv_action_call(&mut self, c: &CallExpr) -> Result<Id, IrError> {
        let target_expr = &self.ast.exprs[c.target];
        let method_sym = self.sym(c.method);

        let is_statement = self.is_statement;
        self.is_statement = false;

        let (target, mut b) = if matches!(target_expr, Expr::List(_)) {
            (self.conv_expr(c.target)?, Vec::new())
        } else {
            self.atomize(c.target)?
        };

        if let Expr::Ident(name, _) = target_expr {
            let sym = self.sym(*name);
            if let Some(f) = self.ir_ctx.inline_funcs.get(&sym).cloned() {
                return self.expand_inline_func_call(&f, &c.args, b, Vec::new(), None);
            }
        }

        let (obj_str, def, is_method) = self.resolve_action_call_target(target_expr, method_sym);

        let call_args = self.call_args_without_duplicate_receiver(c, def, is_method);
        let (args_list_id, mut ab) = self.conv_args(call_args, def, is_method, is_statement)?;
        b.append(&mut ab);

        if let Expr::Ident(name, _) = target_expr {
            let sym = self.sym(*name);
            match self.lookup(sym) {
                Some(Binding::Proc { .. }) => {
                    let proc_params = self.ast.statements.iter().find_map(|s| {
                        if let Statement::Process(p) = s
                            && self.sym(p.name) == sym
                        {
                            Some(&p.params)
                        } else {
                            None
                        }
                    });
                    let mut named_args = Vec::new();
                    for (i, arg) in c.args.iter().enumerate() {
                        if let Some(params) = proc_params
                            && let Some(param) = params.get(i)
                        {
                            named_args.push(ArgExpr {
                                name: Some(param.name),
                                value: arg.value,
                                spread: arg.spread,
                                is_ref: arg.is_ref,
                            });
                            continue;
                        }
                        named_args.push(arg.clone());
                    }
                    let (args_list_id, b) = self.conv_args(&named_args, None, false, false)?;
                    let node = self.add(Hir::ProcCall([target, args_list_id]));
                    return Ok(self.wrap_lets(node, b));
                }
                Some(Binding::Func { .. }) => {
                    let node = self.add(Hir::FuncCall([target, args_list_id]));
                    return Ok(self.wrap_lets(node, b));
                }
                _ => {}
            }
        }

        if is_method && def.is_some_and(|d| d.object == "variable") {
            let right_id = match self.get(args_list_id) {
                Hir::List(ids) if ids.len() == 1 => ids[0],
                _ => args_list_id,
            };
            let binop = match method_sym.as_str() {
                "equals" => Some(Hir::Eq([target, right_id])),
                "not_equals" => Some(Hir::Ne([target, right_id])),
                "less" => Some(Hir::Lt([target, right_id])),
                "less_or_equals" => Some(Hir::Le([target, right_id])),
                "greater" => Some(Hir::Gt([target, right_id])),
                "greater_or_equals" => Some(Hir::Ge([target, right_id])),
                _ => None,
            };
            if let Some(op) = binop {
                let node = self.add(op);
                return Ok(self.wrap_lets(node, b));
            }
        }

        let new_args = self.receiver_args(args_list_id, target, def, is_method, is_statement);

        let act = self.action(obj_str, method_sym, new_args);
        Ok(self.wrap_lets(act, b))
    }

    /// The argument list of a method call, with the receiver synthesised into it.
    ///
    /// The receiver fills every slot it belongs to: the action's `origin` value,
    /// and — in statement position — the `assign` slots of the same type, so that
    /// `map.set_map_value(k, v)` reads and writes `map` itself.
    fn receiver_args(
        &mut self,
        args_list_id: Id,
        target: Id,
        def: Option<&'static jmcdata::generated::ActionDef>,
        is_method: bool,
        is_statement: bool,
    ) -> Id {
        if !is_method {
            return args_list_id;
        }
        let Hir::List(ids) = self.get(args_list_id) else {
            return args_list_id;
        };
        let ids_vec = ids.to_vec();
        let mut slots: Vec<&'static str> = Vec::new();
        if let Some(origin) = def.and_then(|d| d.origin) {
            slots.push(origin);
        }
        slots.extend(Self::receiver_assign_slots(def, is_method, is_statement));
        let mut new_ids = Vec::with_capacity(ids_vec.len() + slots.len());
        if slots.is_empty() {
            new_ids.push(target);
        } else {
            for slot in slots {
                let slot_id = self.str_lit(slot);
                new_ids.push(self.add(Hir::Named([slot_id, target])));
            }
        }
        new_ids.extend(ids_vec);
        self.add(Hir::List(new_ids.into_boxed_slice()))
    }

    fn resolve_action_call_target(
        &self,
        target: &Expr,
        method: Symbol,
    ) -> (
        &'static str,
        Option<&'static jmcdata::generated::ActionDef>,
        bool,
    ) {
        let method_name = method.as_str();
        if let Expr::Ident(name, _) = target {
            let symbol = self.sym(*name);
            if KNOWN_OBJECTS.contains(&symbol.as_str()) {
                let object = symbol.as_str();
                if let Some(definition) = schema::action_def(object, method_name) {
                    return (object, Some(definition), false);
                }
            } else if matches!(
                self.lookup(symbol),
                Some(Binding::Proc { .. } | Binding::Func { .. })
            ) {
                return ("variable", None, false);
            }
        }
        if let Some((object, definition)) = schema::action_by_method(method_name) {
            return (object, Some(definition), true);
        }
        ("variable", None, false)
    }

    #[instrument(skip(self, a), level = "trace")]
    pub(super) fn conv_action(&mut self, a: &ActionExpr) -> Result<Id, IrError> {
        let is_statement = self.is_statement;
        self.is_statement = false;

        let object_sym = self.sym(a.object);
        let name_sym = self.sym(a.name);
        let object_str = object_sym.as_str();
        let name_str = name_sym.as_str();

        let sel_id = self.convert_selector(object_str, a.selector)?;

        let def = schema::action_def(object_str, name_str);
        let (cond_id, args_to_process) = self.convert_action_condition(a, def)?;

        let (args, b) = self.conv_args(&args_to_process, def, false, is_statement)?;

        let block_id = match &a.operations {
            Some(ops) => {
                self.push();
                if let Some(lambda) = &a.lambda {
                    for &p in lambda {
                        if let Expr::Ident(name, _) = &self.ast.exprs[p] {
                            let sym = self.sym(*name);
                            self.declare(
                                sym,
                                Binding::Var {
                                    name: sym,
                                    scope: self.default_scope,
                                },
                            );
                        }
                    }
                }
                let ids: Vec<_> = ops
                    .iter()
                    .map(|s| self.conv_stmt(s))
                    .collect::<Result<_, _>>()?;
                self.pop();
                if ids.is_empty() {
                    self.nop()
                } else {
                    self.block_or_single(ids)
                }
            }
            None => self.nop(),
        };

        let lambda_id = if let Some(lambda) = &a.lambda {
            let params: Vec<_> = lambda
                .iter()
                .map(|&p| self.atomize(p).map(|(a, _)| a))
                .collect::<Result<_, _>>()?;
            self.add(Hir::List(params.into_boxed_slice()))
        } else {
            self.nop()
        };

        let action_id = self.action_with(
            object_sym,
            name_sym,
            [sel_id, args, block_id, lambda_id, cond_id],
        );
        let final_id = if a.invert == Some(true) {
            self.add(Hir::Not(action_id))
        } else {
            action_id
        };
        Ok(self.wrap_lets(final_id, b))
    }

    fn convert_action_condition(
        &mut self,
        action: &ActionExpr,
        definition: Option<&jmcdata::generated::ActionDef>,
    ) -> Result<(Id, Vec<ArgExpr>), IrError> {
        let Some(first_arg) = action.args.first().filter(|_| {
            definition.is_some_and(|def| def.action_type.ends_with("_with_conditional"))
        }) else {
            return Ok((self.nop(), action.args.clone()));
        };
        let (expression, inverted) = match &self.ast.exprs[first_arg.value] {
            Expr::Unary(unary) if unary.op == UnOp::Not => (unary.operand, true),
            _ => (first_arg.value, false),
        };

        let condition = match &self.ast.exprs[expression] {
            Expr::Action(inner_action) => {
                let mut inner_action = inner_action.clone();
                inner_action.invert = Some(inverted ^ inner_action.invert.unwrap_or(false));
                Some(self.conv_action(&inner_action)?)
            }
            Expr::Call(call) => self.convert_call_condition(call, inverted)?,
            _ => None,
        };
        condition.map_or_else(
            || Ok((self.nop(), action.args.clone())),
            |condition| Ok((condition, action.args[1..].to_vec())),
        )
    }

    fn convert_call_condition(
        &mut self,
        call: &CallExpr,
        inverted: bool,
    ) -> Result<Option<Id>, IrError> {
        let method = self.sym(call.method);
        let Some((object, definition)) = schema::action_by_method(method.as_str())
            .map(|(object, definition)| (Symbol::from(object), definition))
        else {
            return Ok(None);
        };
        if definition.action_type != "container"
            && !definition.action_type.ends_with("_with_conditional")
        {
            return Ok(None);
        }

        let (target, mut bindings) = self.atomize(call.target)?;
        let call_args = self.call_args_without_duplicate_receiver(call, Some(definition), true);
        let (args, args_bindings) = self.conv_args(call_args, Some(definition), true, false)?;
        bindings.extend(args_bindings);
        let mut all_args = Vec::new();
        if let Some(origin) = definition.origin {
            let origin = self.str_lit(origin);
            all_args.push(self.add(Hir::Named([origin, target])));
        } else {
            all_args.push(target);
        }
        if let Hir::List(ids) = self.get(args) {
            all_args.extend(ids.iter().copied());
        }
        let args = self.add(Hir::List(all_args.into_boxed_slice()));
        let nop = self.nop();
        let action = self.action_with(object, method, [nop, args, nop, nop, nop]);
        let condition = if inverted {
            self.add(Hir::Not(action))
        } else {
            action
        };
        Ok(Some(self.wrap_lets(condition, bindings)))
    }

    fn convert_selector(&mut self, object: &str, selector: Option<StrId>) -> Result<Id, IrError> {
        let Some(selector) = selector else {
            return Ok(self.nop());
        };
        let selector = self.sym(selector);
        let resolved = match object {
            "player" => jmcdata::generated::SELECTOR_PLAYER_MAP
                .get(selector.as_str())
                .copied(),
            "entity" => jmcdata::generated::SELECTOR_ENTITY_MAP
                .get(selector.as_str())
                .copied(),
            "value" => jmcdata::generated::SELECTOR_GAME_VALUE_MAP
                .get(selector.as_str())
                .copied(),
            _ => {
                return Err(IrError::UnknownSelector {
                    object: object.to_owned(),
                    selector: String::new(),
                });
            }
        }
        .ok_or_else(|| IrError::UnknownSelector {
            object: object.to_owned(),
            selector: selector.to_string(),
        })?;
        let selector = self.str_lit(resolved);
        Ok(self.add(Hir::Sel(selector)))
    }
}
