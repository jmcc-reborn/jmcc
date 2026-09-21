//! Declarations: functions, processes, events, classes, enums,
//! and parsing types from declaration strings.

use super::*;

macro_rules! params_list {
    ($self:expr, $params:expr, $skip_first:expr) => {{
        let ids: Vec<Id> = $params
            .iter()
            .skip(if $skip_first { 1 } else { 0 })
            .map(|p| {
                let name = match p.spread {
                    1 => format!("*{}", $self.sym(p.name)),
                    2 => format!("**{}", $self.sym(p.name)),
                    _ => $self.sym(p.name).to_string(),
                };
                // `ref` modifies argument passing semantics, and codegen
                // discovers this through the parameter name prefix (see
                // `utils::REF_PARAM_PREFIX`).
                let name = if p.is_ref {
                    format!("{}{name}", crate::utils::REF_PARAM_PREFIX)
                } else {
                    name
                };
                let name_id = $self.add(Hir::Str(StrLit(Symbol::from(name))));
                let ty_str =
                    p.ty.map(|t| $self.ast.strings.resolve(&t).to_owned())
                        .unwrap_or_default();
                let ty_id = $self.add(Hir::Str(StrLit(Symbol::from(ty_str))));
                $self.add(Hir::List(vec![name_id, ty_id].into_boxed_slice()))
            })
            .collect();
        $self.add(Hir::List(ids.into_boxed_slice()))
    }};
}

impl HirBuilder<'_> {
    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_decl_type_str(&self, s: &str) -> Result<Type, IrError> {
        let s = s.trim();
        let lower = s.to_lowercase();
        Ok(match lower.as_str() {
            "number" | "num" | "int" | "float" | "double" => {
                Type::Class(self.lang_item("number"), vec![])
            }
            "text" | "string" | "str" => Type::Class(self.lang_item("text"), vec![]),
            "boolean" | "bool" => Type::Class(self.lang_item("boolean"), vec![]),
            "list" | "array" => Type::Class(self.lang_item("array"), vec![Type::Unknown]),
            "map" | "dict" => {
                Type::Class(self.lang_item("map"), vec![Type::Unknown, Type::Unknown])
            }
            "value" => Type::Class(self.lang_item("value"), vec![]),
            "any" => Type::Class(self.lang_item("any"), vec![]),
            _ => {
                let ty = self.ir_ctx.type_from_str(s);
                if matches!(ty, Type::Unknown) {
                    return Err(IrError::UnknownType(s.to_owned()));
                }
                ty
            }
        })
    }

    /// Declares a top-level function or process; ignores other statement kinds.
    pub(super) fn declare_callable(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Function(f) if !f.is_inline => {
                let sym = self.sym(f.name);
                self.declare(sym, Binding::Func { name: sym });
                for a in &f.aliases {
                    let a_sym = self.sym(*a);
                    self.declare(a_sym, Binding::Func { name: sym });
                }
            }
            Statement::Process(p) => {
                let sym = self.sym(p.name);
                self.declare(sym, Binding::Proc { name: sym });
                for a in &p.aliases {
                    let a_sym = self.sym(*a);
                    self.declare(a_sym, Binding::Proc { name: sym });
                }
            }
            _ => {}
        }
    }

    #[instrument(skip(self, f), level = "trace")]
    pub(super) fn conv_function(&mut self, f: &FunctionDecl) -> Result<Id, IrError> {
        if f.is_inline || f.is_overload || f.body.is_empty() {
            return Ok(self.nop());
        }
        let fn_sym = self.sym(f.name);
        self.declare(fn_sym, Binding::Func { name: fn_sym });
        for a in &f.aliases {
            let a_sym = self.sym(*a);
            self.declare(a_sym, Binding::Func { name: fn_sym });
        }
        self.push();
        let mut body_ids = Vec::new();

        let short_name = fn_sym.as_str().rsplit("::").next().unwrap_or("");
        let class_info = if crate::ir::dunder::is_init_dunder(short_name) {
            let class_name = fn_sym.as_str().rsplit("::").nth(1).unwrap_or("");
            self.ir_ctx.get_class_by_name(class_name).cloned()
        } else {
            None
        };

        for (i, p) in f.params.iter().enumerate() {
            let sym = self.sym(p.name);
            let scope = self.default_scope;
            self.declare(sym, Binding::Var { name: sym, scope });

            if let Some(ty_id) = p.ty {
                let ty_str = self.ast.strings.resolve(&ty_id);
                if let Ok(ty) = self.parse_decl_type_str(ty_str) {
                    self.ir_ctx.record_var_type(sym, ty);
                }
            }

            if let Some(info) = &class_info
                && i == 0
            {
                let is_single = self.ir_ctx.is_single_field_class(info);
                let slots_len = info.fields.len();
                let zero = self.add(Hir::Num(0.0.into()));
                let list = if is_single {
                    zero
                } else {
                    self.add(Hir::List(vec![zero; slots_len].into_boxed_slice()))
                };

                let lhs_raw = self.add(Hir::Var(VarName(sym)));
                let lhs = self.wrap_scope(lhs_raw, scope);
                body_ids.push(self.add(Hir::VarDecl([lhs, list])));
                continue;
            }

            if scope != VarScope::Line {
                let line_var_raw = self.add(Hir::Var(VarName(sym)));
                let line_var = self.add(Hir::Line(line_var_raw));
                let lhs_raw = self.add(Hir::Var(VarName(sym)));
                let lhs = self.wrap_scope(lhs_raw, scope);
                body_ids.push(self.add(Hir::VarDecl([lhs, line_var])));
            }
        }
        for stmt in &f.body {
            body_ids.push(self.conv_stmt(stmt)?);
        }

        let body = if body_ids.is_empty() {
            self.nop()
        } else {
            self.block_or_single(body_ids)
        };
        self.pop();
        let nid = self.str_lit(self.sym(f.name));
        let pid = params_list!(self, &f.params, class_info.is_some());
        Ok(self.add(Hir::FuncDecl([nid, pid, body])))
    }

    #[instrument(skip(self, p), level = "trace")]
    pub(super) fn conv_process(&mut self, p: &ProcessDecl) -> Result<Id, IrError> {
        let proc_sym = self.sym(p.name);
        self.declare(proc_sym, Binding::Proc { name: proc_sym });
        for a in &p.aliases {
            let a_sym = self.sym(*a);
            self.declare(a_sym, Binding::Proc { name: proc_sym });
        }
        self.push();
        for param in &p.params {
            let sym = self.sym(param.name);
            self.declare(
                sym,
                Binding::Var {
                    name: sym,
                    scope: self.default_scope,
                },
            );
            if let Some(ty_id) = param.ty {
                let ty_str = self.ast.strings.resolve(&ty_id);
                if let Ok(ty) = self.parse_decl_type_str(ty_str) {
                    self.ir_ctx.record_var_type(sym, ty);
                }
            }
        }
        let nid = self.str_lit(self.sym(p.name));
        let pid = params_list!(self, &p.params, false);
        let body = self.convert_block_inner(&p.body)?;
        self.pop();
        Ok(self.add(Hir::ProcDecl([nid, pid, body])))
    }

    #[instrument(skip(self, e), level = "trace")]
    pub(super) fn conv_event(&mut self, e: &EventDecl) -> Result<Id, IrError> {
        let nid = self.str_lit(self.sym(e.event_name));
        let body = self.convert_block_inner(&e.body)?;
        Ok(self.add(Hir::EventDecl([nid, body])))
    }

    #[instrument(skip(self, c), level = "trace")]
    pub(super) fn conv_class(&mut self, c: &ClassDecl) -> Result<Id, IrError> {
        let nid = self.str_lit(self.sym(c.name));
        let parent = match c.parent {
            Some(p) => self.str_lit(self.sym(p)),
            None => self.nop(),
        };
        let body = self.convert_block_inner(&c.body)?;
        Ok(self.add(Hir::ClassDecl([nid, parent, body])))
    }

    #[instrument(skip(self, e), level = "trace")]
    pub(super) fn conv_enum(&mut self, e: &EnumDecl) -> Id {
        let nid = self.str_lit(self.sym(e.name));
        let vals: Vec<_> = e
            .values
            .iter()
            .map(|v| self.str_lit(self.sym(*v)))
            .collect();
        let vid = self.add(Hir::List(vals.into_boxed_slice()));
        self.add(Hir::EnumDecl([nid, vid]))
    }
}
