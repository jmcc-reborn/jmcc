//! Invocations: class methods and processes, argument substitution matching
//! declarations, and inlining functions.

use super::*;

impl HirBuilder<'_> {
    /// Method arguments with a repeated receiver dropped: `map.set_map_value(map, key, value)`
    /// occurs in exported projects, and the extra arg would shift the `origin` slot.
    pub(super) fn call_args_without_duplicate_receiver<'c>(
        &self,
        call: &'c CallExpr,
        def: Option<&jmcdata::generated::ActionDef>,
        is_method: bool,
    ) -> &'c [ArgExpr] {
        let duplicated = is_method
            && def.is_some_and(|d| d.origin.is_some())
            && call.args.first().is_some_and(|arg| {
                arg.name.is_none() && crate::ast::structurally_eq(self.ast, arg.value, call.target)
            });
        if duplicated {
            &call.args[1..]
        } else {
            &call.args
        }
    }

    #[instrument(skip(self, args, def), level = "trace")]
    pub(super) fn conv_args(
        &mut self,
        args: &[ArgExpr],
        def: Option<&jmcdata::generated::ActionDef>,
        is_method: bool,
        is_statement: bool,
    ) -> Result<(Id, Vec<(Id, Id)>), IrError> {
        let mut bindings = Vec::new();
        let mut ids = Vec::new();
        let will_extract = self.should_extract_assign(args, def, is_method);
        let mut used_params = Self::initial_used_params(def, is_method, will_extract, is_statement);

        let mut positional_idx = 0;
        let mut extracted_assign_var: Option<Id> = None;

        for arg in args {
            let mut val = Default::default();
            let mut bindings_to_extend = Vec::new();
            let mut did_extract = false;

            if will_extract && extracted_assign_var.is_none() {
                let arg_name_str = arg.name.map(|n| self.sym(n).to_string());
                if arg_name_str.as_deref() == Some("value")
                    && let Expr::List(l) = &self.ast.exprs[arg.value]
                    && !l.values.is_empty()
                {
                    let (v, b) = self.atomize(l.values[0])?;
                    bindings_to_extend.extend(b);
                    extracted_assign_var = Some(v);

                    let mut new_ids = Vec::new();
                    for &v in l.values.iter().skip(1) {
                        let (a, b) = self.atomize(v)?;
                        new_ids.push(a);
                        bindings_to_extend.extend(b);
                    }
                    val = self.add(Hir::List(new_ids.into_boxed_slice()));
                    did_extract = true;
                }
            }

            if !did_extract {
                let (v, b) = self.atomize(arg.value)?;
                bindings_to_extend.extend(b);
                val = v;
            }
            bindings.extend(bindings_to_extend);

            if let Some(def) = def {
                let arg_name_str = arg.name.map(|n| self.sym(n).to_string());
                let target_arg = crate::utils::resolve_action_arg_def(
                    arg_name_str.as_deref(),
                    Some(def),
                    &mut used_params,
                    &mut positional_idx,
                )
                .ok_or_else(|| IrError::TooManyArguments {
                    object: def.object.to_owned(),
                    name: def.name.to_owned(),
                })?;

                if target_arg.arg_type == "enum" {
                    match *self.get(val) {
                        Hir::Num(n) => {
                            if let Some(values) = target_arg.values {
                                let idx = n.0 as usize;
                                if idx < values.len() {
                                    let s = self.str_lit(values[idx]);
                                    val = self.add(Hir::Enum(s));
                                } else {
                                    return Err(IrError::EnumIndexOutOfBounds {
                                        index: idx,
                                        max: values.len(),
                                    });
                                }
                            }
                        }
                        Hir::Text([_, c]) => {
                            val = self.add(Hir::Enum(c));
                        }
                        _ => {}
                    }
                }
            }

            let val = arg.name.map_or(val, |name| {
                let nid = self.str_lit(self.sym(name));
                self.add(Hir::Named([nid, val]))
            });
            ids.push(val);
        }

        if let Some(var_id) = extracted_assign_var
            && let Some(def) = def
            && let Some(assign_args) = &def.assign
            && assign_args.len() == 1
        {
            let assign_arg_id = assign_args[0].id;
            let nid = self.str_lit(assign_arg_id);
            let named = self.add(Hir::Named([nid, var_id]));
            ids.insert(0, named);
        }

        let list = self.add(Hir::List(ids.into_boxed_slice()));
        Ok((list, bindings))
    }

    fn initial_used_params(
        def: Option<&jmcdata::generated::ActionDef>,
        is_method: bool,
        will_extract: bool,
        is_statement: bool,
    ) -> HashSet<String> {
        let mut used = HashSet::new();
        let Some(def) = def else {
            return used;
        };
        if is_method && let Some(origin) = def.origin {
            used.insert(origin.to_owned());
        }
        // In expression position the surrounding `Set` supplies the `assign`
        // target, so those slots stay reserved. In statement position a *method*
        // call's receiver supplies them (`map.set_map_value(k, v)` writes back
        // into `map`, and `self.set_map_value(i, v)` inside `map.__subscript__`
        // must write into `self`), so those slots are reserved here too. An
        // unreserved `assign` slot would swallow the first positional argument
        // and shift every later one, losing the last.
        let receiver_assigns = Self::receiver_assign_slots(Some(def), is_method, is_statement);
        if (!is_statement || !receiver_assigns.is_empty())
            && let Some(assign_args) = def.assign.as_ref()
        {
            used.extend(assign_args.iter().map(|arg| arg.id.to_owned()));
        }
        if will_extract
            && let Some(assign_args) = def.assign.as_ref()
            && assign_args.len() == 1
        {
            used.insert(assign_args[0].id.to_owned());
        }
        used
    }

    /// The `assign` slots a method call synthesises from its receiver.
    ///
    /// `.jc` calls an action on the variable it reads from: `objects.set_map_value(k, v)`.
    /// When the action writes its container back — `assign` names a slot of the
    /// same type as the `origin` argument — that variable is the target as well.
    /// Only such slots are taken: output slots of another type (`removed_value`,
    /// the components of a location) are left for the caller's arguments.
    pub(super) fn receiver_assign_slots(
        def: Option<&jmcdata::generated::ActionDef>,
        is_method: bool,
        is_statement: bool,
    ) -> Vec<&'static str> {
        if !is_method || !is_statement {
            return Vec::new();
        }
        let Some(def) = def else {
            return Vec::new();
        };
        let Some(origin) = def.origin else {
            return Vec::new();
        };
        let Some(origin_type) = def
            .args
            .iter()
            .find(|arg| arg.id == origin)
            .map(|arg| arg.arg_type)
        else {
            return Vec::new();
        };
        def.assign
            .into_iter()
            .flatten()
            .filter(|arg| arg.arg_type == origin_type)
            .map(|arg| arg.id)
            .collect()
    }

    fn should_extract_assign(
        &self,
        args: &[ArgExpr],
        def: Option<&jmcdata::generated::ActionDef>,
        is_method: bool,
    ) -> bool {
        if is_method {
            return false;
        }
        let Some(assign_args) = def.and_then(|definition| definition.assign.as_ref()) else {
            return false;
        };
        if assign_args.len() != 1 {
            return false;
        }
        let assign_id = assign_args[0].id;
        if args.iter().any(|arg| {
            arg.name
                .is_some_and(|name| self.sym(name).as_str() == assign_id)
        }) {
            return false;
        }
        args.iter().any(|arg| {
            arg.name
                .is_some_and(|name| self.sym(name).as_str() == "value")
                && matches!(&self.ast.exprs[arg.value], Expr::List(list) if !list.values.is_empty())
        })
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "inline function call expansion binds arguments, default values, variadics, and records types"
    )]
    #[instrument(
        skip(self, f, args, bindings, predefined_args, class_info),
        level = "trace"
    )]
    pub(super) fn expand_inline_func_call(
        &mut self,
        f: &Rc<FunctionDecl>,
        args: &[ArgExpr],
        mut bindings: Vec<(Id, Id)>,
        mut predefined_args: Vec<(String, Id)>,
        class_info: Option<&crate::ir::ctx::ClassInfo>,
    ) -> Result<Id, IrError> {
        let fn_name = self.sym(f.name);
        trace!(?fn_name, "Expanding inline function call");

        let ret_sym = self.fresh();
        let ret_var_raw = self.add(Hir::Var(VarName(ret_sym)));
        let ret_var = self.wrap_scope(ret_var_raw, self.default_scope);

        self.push();

        if let Some(ret_ty_id) = f.return_type {
            let ret_ty_str = self.ast.strings.resolve(&ret_ty_id);
            if let Ok(ret_ty) = self.parse_decl_type_str(ret_ty_str) {
                self.ir_ctx.record_var_type(ret_sym, ret_ty);
            }
        }

        let (evaluated_pos, mut evaluated_named) =
            self.evaluate_inline_args(args, &mut bindings)?;

        let mut is_init = false;
        if let Some(_info) = class_info {
            let short_name = fn_name.as_str().rsplit("::").next().unwrap_or("");
            if crate::ir::dunder::is_init_dunder(short_name) {
                is_init = true;
            }
        }

        let mut pos_idx = 0;
        let mut first_param_var = None;
        for (i, param) in f.params.iter().enumerate() {
            let p_sym = self.sym(param.name);
            let fresh_sym = Symbol::from(format!("__inl_{}_{}", p_sym, self.temp_n));
            self.temp_n += 1;
            let p_var_raw = self.add(Hir::Var(VarName(fresh_sym)));
            let p_var = self.wrap_scope(p_var_raw, self.default_scope);
            if i == 0 {
                first_param_var = Some(p_var);
            }
            self.declare(
                p_sym,
                Binding::Var {
                    name: fresh_sym,
                    scope: self.default_scope,
                },
            );

            let actual_arg_ty = if let Some(arg) = args.get(i) {
                self.types.get(&arg.value).cloned()
            } else {
                None
            };

            if let Some(arg_ty) = actual_arg_ty {
                self.ir_ctx.record_var_type(fresh_sym, arg_ty);
            } else if let Some(ty_id) = param.ty {
                let ty_str = self.ast.strings.resolve(&ty_id);
                if let Ok(ty) = self.parse_decl_type_str(ty_str) {
                    self.ir_ctx.record_var_type(fresh_sym, ty);
                }
            }

            if param.spread == 1 {
                let mut list_ids = Vec::new();
                while pos_idx < evaluated_pos.len() {
                    list_ids.push(evaluated_pos[pos_idx]);
                    pos_idx += 1;
                }
                let list = self.add(Hir::List(list_ids.into_boxed_slice()));
                bindings.push((p_var, list));
            } else if param.spread == 2 {
                let mut map_ids = Vec::new();
                for (name, val_id) in &evaluated_named {
                    let nid = self.str_lit(name.clone());
                    map_ids.push(nid);
                    map_ids.push(*val_id);
                }
                let map = self.add(Hir::Map(map_ids.into_boxed_slice()));
                bindings.push((p_var, map));
            } else {
                let p_name = p_sym.to_string();
                if let Some(pos) = predefined_args.iter().position(|(n, _)| *n == p_name) {
                    let (_, val_id) = predefined_args.remove(pos);
                    bindings.push((p_var, val_id));
                    continue;
                }

                let is_self_name = crate::utils::is_self_param(p_sym.as_str());
                let is_self_param = is_init && i == 0 && is_self_name;
                if is_self_param {
                    let is_single =
                        class_info.is_some_and(|c| self.ir_ctx.is_single_field_class(c));
                    let slots_len = class_info.map(|c| c.fields.len()).unwrap_or(0);
                    let zero = self.add(Hir::Num(0.0.into()));
                    let list = if is_single {
                        zero
                    } else {
                        self.add(Hir::List(vec![zero; slots_len].into_boxed_slice()))
                    };
                    bindings.push((p_var, list));
                    continue;
                }

                let arg_val = if let Some(val) = evaluated_named.remove(&p_name) {
                    val
                } else if pos_idx < evaluated_pos.len() {
                    let v = evaluated_pos[pos_idx];
                    pos_idx += 1;
                    v
                } else if let Some(def) = param.default {
                    self.conv_expr(def)?
                } else {
                    self.nop()
                };
                bindings.push((p_var, arg_val));
            }
        }

        let prev_ret = self.inline_return_var.take();
        self.inline_return_var = Some(ret_var);

        let mut body_ids = Vec::new();
        for stmt in &f.body {
            body_ids.push(self.conv_stmt(stmt)?);
        }

        if f.is_setter
            && f.return_type.is_none()
            && let Some(first_p) = first_param_var
        {
            body_ids.push(self.add(Hir::Set([ret_var, first_p])));
        }

        self.inline_return_var = prev_ret;
        self.pop();

        body_ids.push(ret_var);
        let block_id = self.add(Hir::Block(body_ids.into_boxed_slice()));
        Ok(self.wrap_lets(block_id, bindings))
    }

    fn evaluate_inline_args(
        &mut self,
        args: &[ArgExpr],
        bindings: &mut Vec<(Id, Id)>,
    ) -> Result<(Vec<Id>, HashMap<String, Id>), IrError> {
        let mut positional_exprs = Vec::new();
        let mut named_exprs = HashMap::new();
        for argument in args {
            if let Some(name) = argument.name {
                named_exprs.insert(self.sym(name).to_string(), argument.value);
            } else {
                positional_exprs.push(argument.value);
            }
        }
        let mut positional = Vec::with_capacity(positional_exprs.len());
        for expression in positional_exprs {
            let (value, new_bindings) = self.atomize(expression)?;
            positional.push(value);
            bindings.extend(new_bindings);
        }
        let mut named = HashMap::new();
        for (name, expression) in named_exprs {
            let (value, new_bindings) = self.atomize(expression)?;
            named.insert(name, value);
            bindings.extend(new_bindings);
        }
        Ok((positional, named))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "Dispatches call expression lowering across constructors, static methods, instance methods, and actions"
    )]
    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn conv_call(&mut self, c: &CallExpr) -> Result<Id, IrError> {
        let target_expr = &self.ast.exprs[c.target];
        let method_sym = self.sym(c.method);

        if let Expr::Ident(name, _) = target_expr {
            let sym = self.sym(*name);
            let name_str = sym.to_string();
            if let Some(class) = self.ir_ctx.get_class_by_name(&name_str).cloned() {
                if *name == c.method || method_sym.as_str() == "call" {
                    return self.conv_class_constructor(c, &name_str);
                }
                let class_ty = Type::Class(class.def_id, Vec::new());
                if let Some(f) = self
                    .find_in_classes(&class_ty, |cl| cl.methods.get(method_sym.as_str()).cloned())
                {
                    if f.is_inline {
                        return self.expand_inline_func_call(
                            &f,
                            &c.args,
                            Vec::new(),
                            Vec::new(),
                            Some(&class),
                        );
                    }
                    let target_id = self.str_lit(self.sym(f.name));
                    let (args_list, b) = self.build_args_list(&f, &c.args, false)?;
                    let node = self.add(Hir::FuncCall([target_id, args_list]));
                    return Ok(self.wrap_lets(node, b));
                }
                if let Some(p) = self.find_in_classes(&class_ty, |cl| {
                    cl.processes.get(method_sym.as_str()).cloned()
                }) {
                    let named_args = c
                        .args
                        .iter()
                        .enumerate()
                        .map(|(i, arg)| {
                            p.params.get(i).map_or_else(
                                || arg.clone(),
                                |param| ArgExpr {
                                    name: Some(param.name),
                                    value: arg.value,
                                    spread: arg.spread,
                                    is_ref: arg.is_ref,
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    let (args_list, b) = self.conv_args(&named_args, None, false, false)?;
                    let target_id = self.str_lit(self.sym(p.name));
                    let node = self.add(Hir::ProcCall([target_id, args_list]));
                    return Ok(self.wrap_lets(node, b));
                }
            }
        }

        let target_ty = if let Expr::Ident(name, _) = target_expr {
            let sym = self.sym(*name);
            if let Some(&Binding::Var {
                name: bound_sym, ..
            }) = self.lookup(sym)
                && let Some(actual_ty) = self.ir_ctx.var_types.get(&bound_sym).cloned()
            {
                actual_ty
            } else {
                self.types.get(&c.target).cloned().unwrap_or(Type::Unknown)
            }
        } else {
            self.types.get(&c.target).cloned().unwrap_or(Type::Unknown)
        };

        let is_direct_call = match target_expr {
            Expr::Ident(name, _) => self.sym(*name) == method_sym,
            _ => method_sym.as_str() == "call",
        };

        let method = self.find_in_classes(&target_ty, |c| {
            c.methods
                .get(method_sym.as_str())
                .or_else(|| {
                    if is_direct_call || method_sym.as_str() == "call" {
                        c.methods.get("call")
                    } else {
                        None
                    }
                })
                .cloned()
                .map(ClassMember::Method)
        });
        if let Some(ClassMember::Method(f)) = method {
            let has_self = f
                .params
                .first()
                .is_some_and(|p| crate::utils::is_self_param(self.sym(p.name).as_str()));
            let all_args = if has_self {
                let mut a = vec![positional(c.target)];
                a.extend(c.args.iter().cloned());
                a
            } else {
                c.args.clone()
            };
            if f.is_inline {
                return self.expand_inline_func_call(&f, &all_args, Vec::new(), Vec::new(), None);
            }
            let target_id = self.str_lit(self.sym(f.name));
            let (args_list, b) = self.build_args_list(&f, &all_args, false)?;
            let node = self.add(Hir::FuncCall([target_id, args_list]));
            return Ok(self.wrap_lets(node, b));
        }

        let process = self.find_in_classes(&target_ty, |c| {
            c.processes
                .get(method_sym.as_str())
                .cloned()
                .map(ClassMember::Process)
        });
        if let Some(ClassMember::Process(p)) = process {
            let has_self = p
                .params
                .first()
                .is_some_and(|param| crate::utils::is_self_param(self.sym(param.name).as_str()));
            let all_args = if has_self {
                let mut a = vec![positional(c.target)];
                a.extend(c.args.iter().cloned());
                a
            } else {
                c.args.clone()
            };
            let named_args = all_args
                .iter()
                .enumerate()
                .map(|(i, arg)| {
                    p.params.get(i).map_or_else(
                        || arg.clone(),
                        |param| ArgExpr {
                            name: Some(param.name),
                            value: arg.value,
                            spread: arg.spread,
                            is_ref: arg.is_ref,
                        },
                    )
                })
                .collect::<Vec<_>>();
            let (args_list, b) = self.conv_args(&named_args, None, false, false)?;
            let target_id = self.str_lit(self.sym(p.name));
            let node = self.add(Hir::ProcCall([target_id, args_list]));
            return Ok(self.wrap_lets(node, b));
        }

        self.conv_action_call(c)
    }

    #[instrument(skip(self, c, class_name), level = "trace")]
    fn conv_class_constructor(&mut self, c: &CallExpr, class_name: &str) -> Result<Id, IrError> {
        let ty = self.ir_ctx.type_from_str(class_name);
        let init_method = self.find_in_classes(&ty, |c| {
            c.methods.get("__init__").cloned().map(ClassMember::Method)
        });

        let class_info = self.ir_ctx.get_class_by_name(class_name).cloned();

        if let Some(ClassMember::Method(f)) = init_method {
            if f.is_inline {
                return self.expand_inline_func_call(
                    &f,
                    &c.args,
                    Vec::new(),
                    Vec::new(),
                    class_info.as_ref(),
                );
            }

            let target_id = self.str_lit(self.sym(f.name));
            let (args_list, b) = self.build_args_list(&f, &c.args, true)?;
            let node = self.add(Hir::FuncCall([target_id, args_list]));
            return Ok(self.wrap_lets(node, b));
        }

        let is_dict = class_info.as_ref().is_some_and(|c| c.is_dict);
        if is_dict {
            let mut bindings = Vec::new();
            let mut map_ids = Vec::new();
            for arg in &c.args {
                let key_name = if let Some(name_id) = arg.name {
                    self.sym(name_id).to_string()
                } else {
                    continue;
                };
                let key_id = self.str_lit(key_name);
                let (val_id, b) = self.atomize(arg.value)?;
                map_ids.push(key_id);
                map_ids.push(val_id);
                bindings.extend(b);
            }
            let map_node = self.add(Hir::Map(map_ids.into_boxed_slice()));
            return Ok(self.wrap_lets(map_node, bindings));
        }

        let is_single = class_info
            .as_ref()
            .is_some_and(|c| self.ir_ctx.is_single_field_class(c));
        let slots_len = class_info.as_ref().map_or(0, |c| c.fields.len());
        let zero = self.add(Hir::Num(0.0.into()));
        let mut slots = vec![zero; slots_len];
        let mut bindings = Vec::new();

        let mut pos_idx = 0;
        for arg in &c.args {
            let target_slot = arg.name.map_or_else(
                || {
                    let idx = pos_idx;
                    pos_idx += 1;
                    Some(idx)
                },
                |name_id| {
                    let name = self.sym(name_id).to_string();
                    let info = class_info.as_ref()?;
                    info.fields.get(&name).map(|(_, idx)| *idx)
                },
            );

            let (val_id, b) = self.atomize(arg.value)?;
            bindings.extend(b);
            if let Some(slot) = target_slot
                && slot < slots_len
            {
                slots[slot] = val_id;
            }
        }

        let default_instance = if is_single {
            slots.first().copied().unwrap_or(zero)
        } else {
            self.add(Hir::List(slots.into_boxed_slice()))
        };
        Ok(self.wrap_lets(default_instance, bindings))
    }

    pub(super) fn build_args_list(
        &mut self,
        f: &Rc<FunctionDecl>,
        args: &[ArgExpr],
        skip_first: bool,
    ) -> Result<(Id, Vec<(Id, Id)>), IrError> {
        let mut bindings = Vec::new();
        let mut ids = Vec::new();

        let mut positional_args = Vec::new();
        let mut named_args = HashMap::new();
        for arg in args {
            if let Some(name) = arg.name {
                named_args.insert(self.sym(name).to_string(), arg.value);
            } else {
                positional_args.push(arg.value);
            }
        }

        let mut pos_idx = 0;
        for (i, param) in f.params.iter().enumerate() {
            if skip_first && i == 0 {
                continue;
            }

            let p_name = self.sym(param.name).to_string();
            let arg_val = if let Some(val) = named_args.remove(&p_name) {
                let (v, b) = self.atomize(val)?;
                bindings.extend(b);
                v
            } else if pos_idx < positional_args.len() {
                let (v, b) = self.atomize(positional_args[pos_idx])?;
                bindings.extend(b);
                pos_idx += 1;
                v
            } else if let Some(def) = param.default {
                self.conv_expr(def)?
            } else {
                self.nop()
            };
            ids.push(arg_val);
        }

        let list = self.add(Hir::List(ids.into_boxed_slice()));
        Ok((list, bindings))
    }
}
