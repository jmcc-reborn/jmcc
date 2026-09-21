//! Вызовы: разрешение и эмиссия вызовов функций, методов и конструкторов,
//! развёртка inline-функций и подготовка аргументов.

use crate::ir::ctx::ClassInfo;

use super::*;

impl<'a> OverloadExpander<'a> {
    #[instrument(skip(self, f, hir_args), level = "debug")]
    pub(super) fn expand_inline_from_hir(
        &mut self,
        f: &FunctionDecl,
        hir_args: Vec<Id>,
    ) -> Result<Id> {
        let f_name = self.sym(f.name);
        if self.inline_stack.contains(&f_name) {
            return Err(OverloadError::RecursiveCall {
                name: f_name.to_string(),
            });
        }
        self.inline_stack.push(f_name);

        let ret_sym = self.fresh();
        let ret_var_raw = self.add(Hir::Var(VarName(ret_sym)));
        let ret_var = self.add(Hir::Line(ret_var_raw));

        self.scopes.push(HashMap::new());
        self.inline_vars_stack.push(
            self.inline_vars_stack
                .last()
                .expect("inline_vars_stack must be non-empty")
                .clone(),
        );

        if let Some(ret_ty_id) = f.return_type {
            let ret_ty_str = self.ast.strings.resolve(&ret_ty_id);
            let ret_ty = self.type_from_str(ret_ty_str);
            self.ctx.record_var_type(ret_sym, ret_ty);
        }

        let mut bindings: Vec<(Id, Id)> = Vec::new();
        let mut pos_idx = 0;
        for param in &f.params {
            let p_sym = self.sym(param.name);
            let fresh_sym = Symbol::from(format!("__inl_{}_{}", p_sym, self.temp_n));
            self.temp_n += 1;

            let p_var_raw = self.add(Hir::Var(VarName(fresh_sym)));
            let p_var = self.add(Hir::Line(p_var_raw));

            if let Some(ty_id) = &param.ty {
                let ty_str = self.ast.strings.resolve(ty_id);
                let ty = self.type_from_str(ty_str);
                self.ctx.record_var_type(fresh_sym, ty);
            }

            self.scopes
                .last_mut()
                .expect("scopes must be non-empty")
                .insert(
                    p_sym,
                    ExpanderBinding::Var {
                        name: fresh_sym,
                        scope: VarScope::Line,
                    },
                );

            self.inline_vars_stack
                .last_mut()
                .unwrap()
                .insert(p_sym, p_var);

            if param.spread == 1 {
                let list_ids: Vec<Id> = hir_args[pos_idx..].to_vec();
                pos_idx = hir_args.len();
                let list_id = self.add(Hir::List(list_ids.into_boxed_slice()));
                bindings.push((p_var, list_id));
            } else if param.spread == 2 {
                let map_id = self.add(Hir::Map(vec![].into_boxed_slice()));
                bindings.push((p_var, map_id));
            } else {
                let arg_val = if pos_idx < hir_args.len() {
                    let v = hir_args[pos_idx];
                    pos_idx += 1;
                    v
                } else if let Some(def) = param.default {
                    self.conv_default_expr(def)?
                } else {
                    self.add(Hir::Nop)
                };
                bindings.push((p_var, arg_val));
            }
        }

        let prev_ret = self.inline_return_var.take();
        self.inline_return_var = Some(ret_var);

        let mut body_ids = Vec::new();
        for stmt in &f.body {
            body_ids.push(self.conv_ast_stmt(stmt)?);
        }

        self.inline_return_var = prev_ret;
        self.scopes.pop();
        self.inline_vars_stack.pop();
        self.inline_stack.pop();

        body_ids.push(ret_var);
        let block_id = self.add(Hir::Block(body_ids.into_boxed_slice()));

        let cur = self.wrap_lets(block_id, bindings);
        trace!("inline expansion complete");
        Ok(cur)
    }

    #[instrument(skip(self), level = "trace")]
    fn conv_default_expr(&mut self, eid: ExprId) -> Result<Id> {
        self.conv_ast_expr(eid)
    }

    /// Any non-action call; same order as `hir::conv_call`: constructor, receiver method,
    /// function, then namespace action. The parser emits constructors and plain functions as one
    /// `Call` node with a qualified `Ident` target and method `call`.
    pub(super) fn conv_ast_call(&mut self, call: &CallExpr) -> Result<Id> {
        let method = self.sym(call.method);

        if let Expr::Ident(name_id, _) = &self.ast.exprs[call.target] {
            let name = self.sym(*name_id);
            if (*name_id == call.method || method.as_str() == "call")
                && let Some(call_id) = self.conv_ast_named_call(call, name)?
            {
                return Ok(call_id);
            }
        }

        let target_ty = self
            .types
            .get(&call.target)
            .cloned()
            .unwrap_or(Type::Unknown);
        let is_direct_call = match &self.ast.exprs[call.target] {
            Expr::Ident(name_id, _) => self.sym(*name_id) == method,
            _ => method.as_str() == "call",
        };
        let receiver_method = self.find_in_classes(&target_ty, |class| {
            class
                .methods
                .get(method.as_str())
                .or_else(|| {
                    if is_direct_call || method.as_str() == "call" {
                        class.methods.get("call")
                    } else {
                        None
                    }
                })
                .cloned()
        });
        if let Some(f) = receiver_method {
            let receiver = ArgExpr {
                name: None,
                value: call.target,
                spread: 0,
                is_ref: false,
            };
            let mut args = Vec::with_capacity(call.args.len() + 1);
            args.push(receiver);
            args.extend(call.args.iter().cloned());
            return self.emit_call(&f, &args, None);
        }

        let target = self.conv_ast_expr(call.target)?;
        // As in `resolve_action_call_target`: the object is found by method name, not by
        // receiver. Action names are *not* unique in the schema, so `KNOWN_OBJECTS` order
        // decides — see `schema::action_by_method`.
        let found = schema::action_by_method(method.as_str());
        if found.is_none() && method.as_str() == "call" {
            return Err(OverloadError::CallNotFound {
                name: self.call_target_name(call),
            });
        }
        let object = found.map_or("variable", |(object, _)| object);
        let name = self.add(Hir::Str(StrLit(method)));
        let args = self.conv_ast_args(&call.args)?;

        // The receiver is the action's `origin`; otherwise the target value would be dropped.
        let args = match found {
            Some((_, def)) => {
                let existing = match &self.nodes.as_slice()[usize::from(args)] {
                    Hir::List(ids) => Some(ids.to_vec()),
                    _ => None,
                };
                existing.map_or(args, |existing| {
                    let mut new_ids = Vec::with_capacity(existing.len() + 1);
                    match def.origin {
                        Some(origin) => {
                            let origin_id = self.add(Hir::Str(StrLit(Symbol::from(origin))));
                            new_ids.push(self.add(Hir::Named([origin_id, target])));
                        }
                        None => new_ids.push(target),
                    }
                    new_ids.extend(existing);
                    self.add(Hir::List(new_ids.into_boxed_slice()))
                })
            }
            None => args,
        };

        let object = self.add(Hir::Str(StrLit(Symbol::from(object))));
        let nop = self.add(Hir::Nop);
        Ok(self.add(Hir::Action(
            vec![object, name, nop, args, nop, nop, nop].into_boxed_slice(),
        )))
    }

    /// Name the function or constructor was called by: the call target's string.
    fn call_target_name(&self, call: &CallExpr) -> String {
        match &self.ast.exprs[call.target] {
            Expr::Ident(name, _) => self.ast.strings.resolve(name).to_owned(),
            _ => self.ast.strings.resolve(&call.method).to_owned(),
        }
    }

    /// Call by qualified name: class constructor or function.
    /// `None` means the name is unknown and the call must be parsed as an action.
    fn conv_ast_named_call(&mut self, call: &CallExpr, name: Symbol) -> Result<Option<Id>> {
        if let Some(class) = self.ctx.get_class_by_name(name.as_str()).cloned() {
            let (def_id, is_dict, slots) = (class.def_id, class.is_dict, class.fields.len());
            return self
                .conv_ast_ctor_call(call, &class, def_id, slots, is_dict)
                .map(Some);
        }

        if let Some(f) = self.ctx.inline_funcs.get(&name).cloned() {
            return self.emit_call(&f, &call.args, None).map(Some);
        }

        let Some(f) = self.find_ast_function(name.as_str()) else {
            return Ok(None);
        };
        self.emit_call(f, &call.args, None).map(Some)
    }

    /// Class constructor, mirroring `hir::conv_class_constructor`: calls `__init__` (inherited
    /// too), or falls back to a bare instance of slots.
    fn conv_ast_ctor_call(
        &mut self,
        call: &CallExpr,
        class_info: &ClassInfo,
        def_id: DefId,
        slots: usize,
        is_dict: bool,
    ) -> Result<Id> {
        let ty = Type::Class(def_id, vec![]);
        let is_single = self.ctx.is_single_field_class(class_info);
        let receiver = self.fresh_instance(Some(class_info), slots, is_dict);
        let init = self.find_in_classes(&ty, |class| class.methods.get("__init__").cloned());
        if let Some(f) = init {
            let has_self = f
                .params
                .first()
                .is_some_and(|p| crate::utils::is_self_param(self.ast.strings.resolve(&p.name)));
            let receiver = if has_self {
                Some(self.fresh_instance(Some(class_info), slots, is_dict))
            } else {
                None
            };
            return self.emit_call(&f, &call.args, receiver);
        }
        let (ids, bindings) = self.conv_ast_loose_args(&call.args)?;
        let result = if is_single && !ids.is_empty() {
            ids[0]
        } else {
            receiver
        };
        Ok(self.wrap_lets(result, bindings))
    }

    /// Fresh class instance: a list of slots, or an empty dict.
    fn fresh_instance(
        &mut self,
        class_info: Option<&ClassInfo>,
        slots: usize,
        is_dict: bool,
    ) -> Id {
        if is_dict {
            return self.add(Hir::Map(vec![].into_boxed_slice()));
        }
        let zero = self.add(Hir::Num(0.0.into()));
        if class_info.is_some_and(|c| self.ctx.is_single_field_class(c)) {
            return zero;
        }
        self.add(Hir::List(vec![zero; slots].into_boxed_slice()))
    }

    /// Function call: inline ones expand in place, others stay `FuncCall`. Functions with no HIR
    /// declaration also expand here, since `code::call_function` requires the decl and the
    /// optimizer drops declarations inlined everywhere. `receiver` is a constructor's `self`
    /// (the decl takes no first parameter).
    fn emit_call(
        &mut self,
        f: &FunctionDecl,
        args: &[ArgExpr],
        receiver: Option<Id>,
    ) -> Result<Id> {
        let name = self.sym(f.name);
        let skip_self = receiver.is_some();
        if f.is_inline || !self.src_has_func_decl(name.as_str()) {
            let mut hir_args = receiver.into_iter().collect::<Vec<_>>();
            hir_args.extend(self.conv_ast_call_args(f, args, skip_self)?);
            return self.expand_inline_from_hir(f, hir_args);
        }
        let target = self.add(Hir::Str(StrLit(name)));
        let ids = self.conv_ast_call_args(f, args, skip_self)?;
        let (ids, bindings) = self.atomize_args(ids);
        let list = self.add(Hir::List(ids.into_boxed_slice()));
        let node = self.add(Hir::FuncCall([target, list]));
        Ok(self.wrap_lets(node, bindings))
    }

    /// Call arguments in parameter order, mirroring `hir::build_args_list`: named args take their
    /// parameter's slot, the rest positionally, missing ones use the default. `skip_self` drops
    /// the first parameter, unused in constructor calls.
    fn conv_ast_call_args(
        &mut self,
        f: &FunctionDecl,
        args: &[ArgExpr],
        skip_self: bool,
    ) -> Result<Vec<Id>> {
        let mut positional = Vec::new();
        let mut named = Vec::new();
        for arg in args {
            match arg.name {
                Some(name) => named.push((self.sym(name), arg.value)),
                None => positional.push(arg.value),
            }
        }

        let mut ids = Vec::new();
        let mut pos = 0;
        for (i, param) in f.params.iter().enumerate() {
            if skip_self && i == 0 {
                continue;
            }
            let param_name = self.sym(param.name);
            if param.spread != 0 {
                // Variadic parameters are unpacked by the callee: the whole remainder lands here.
                while pos < positional.len() {
                    ids.push(self.conv_ast_expr(positional[pos])?);
                    pos += 1;
                }
                continue;
            }
            if let Some(at) = named.iter().position(|(name, _)| *name == param_name) {
                let (_, value) = named.remove(at);
                ids.push(self.conv_ast_expr(value)?);
                continue;
            }
            if pos < positional.len() {
                ids.push(self.conv_ast_expr(positional[pos])?);
                pos += 1;
                continue;
            }
            if let Some(default) = param.default {
                ids.push(self.conv_ast_expr(default)?);
                continue;
            }
            ids.push(self.add(Hir::Nop));
        }
        Ok(ids)
    }

    /// Hoists non-atomic values into temporaries: an HIR argument list holds values, not
    /// computations (MIR cannot lower a nested `+` or `if`).
    pub(super) fn atomize_args(&mut self, ids: Vec<Id>) -> AtomizedArgs {
        let mut bindings = Vec::new();
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            if self.is_atomic(id) {
                result.push(id);
                continue;
            }
            let name = self.fresh();
            let raw = self.add(Hir::Var(VarName(name)));
            let var = self.add(Hir::Line(raw));
            bindings.push((var, id));
            result.push(var);
        }
        (result, bindings)
    }

    /// Function or class-method declaration by qualified name.
    fn find_ast_function(&self, name: &str) -> Option<&'a FunctionDecl> {
        let ast: &'a Ast = self.ast;
        ast.statements
            .iter()
            .find_map(|stmt| self.find_function_in(stmt, name))
    }

    fn find_function_in(&self, stmt: &'a Statement, name: &str) -> Option<&'a FunctionDecl> {
        match stmt {
            Statement::Function(f) => self.name_matches(&f.name, name).then_some(f),
            Statement::Class(class) => class
                .body
                .iter()
                .find_map(|stmt| self.find_function_in(stmt, name)),
            _ => None,
        }
    }

    /// Tree names are qualified (`tests::foo::bar`), while a reference may be short.
    fn name_matches(&self, id: &StrId, name: &str) -> bool {
        let resolved = self.ast.strings.resolve(id);
        resolved == name
            || resolved.ends_with(&format!("::{name}"))
            || name.ends_with(&format!("::{resolved}"))
    }

    /// Whether the source HIR declares the function; `code::call_function` needs it in place.
    fn src_has_func_decl(&self, name: &str) -> bool {
        self.src.as_ref().iter().any(|node| {
            matches!(node, Hir::FuncDecl([n, _, _]) if matches!(&self.src[*n], Hir::Str(s) if s.0.as_str() == name))
        })
    }
}
