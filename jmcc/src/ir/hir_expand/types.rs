//! Types: HIR node type inference, operator overload lookup, and class member resolution.

use super::*;

impl OverloadExpander<'_> {
    fn lang_item(&self, name: &str) -> DefId {
        self.ctx.lang_items.get(name).copied().unwrap_or(0)
    }

    #[instrument(skip(self), level = "trace")]
    fn type_from_action_arg(&self, s: &str) -> Type {
        match s {
            "number" => Type::Class(self.lang_item("number"), vec![]),
            "text" | "string" | "enum" => Type::Class(self.lang_item("text"), vec![]),
            "boolean" | "bool" => Type::Class(self.lang_item("boolean"), vec![]),
            "any" => Type::Class(self.lang_item("any"), vec![]),
            _ => Type::Unknown,
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn infer_type(&self, id: Id) -> Type {
        let node = &self.nodes.as_slice()[usize::from(id)];
        match node {
            Hir::Num(_) => Type::Class(self.lang_item("number"), vec![]),
            Hir::Bool(_)
            | Hir::Eq([_, _])
            | Hir::Ne([_, _])
            | Hir::Lt([_, _])
            | Hir::Le([_, _])
            | Hir::Gt([_, _])
            | Hir::Ge([_, _])
            | Hir::In([_, _]) => Type::Class(self.lang_item("boolean"), vec![]),
            Hir::Str(_) | Hir::Text(_) => Type::Class(self.lang_item("text"), vec![]),
            Hir::Var(v) => self
                .ctx
                .get_var_type(&v.0)
                .cloned()
                .unwrap_or(Type::Unknown),
            Hir::Line(inner) | Hir::Game(inner) | Hir::Save(inner) | Hir::Local(inner) => {
                let inner_node = &self.nodes.as_slice()[usize::from(*inner)];
                if let Hir::Var(v) = inner_node {
                    self.ctx
                        .get_var_type(&v.0)
                        .cloned()
                        .unwrap_or(Type::Unknown)
                } else {
                    self.infer_type(*inner)
                }
            }
            Hir::Add([a, _])
            | Hir::Sub([a, _])
            | Hir::Mul([a, _])
            | Hir::Div([a, _])
            | Hir::Mod([a, _])
            | Hir::Pow([a, _]) => {
                let lty = self.infer_type(*a);
                let method = match node {
                    Hir::Add(_) => "__add__",
                    Hir::Sub(_) => "__subtract__",
                    Hir::Mul(_) => "__multiply__",
                    Hir::Div(_) => "__divide__",
                    Hir::Mod(_) => "__remainder__",
                    Hir::Pow(_) => "__pow__",
                    _ => unreachable!(),
                };
                if let Some((f, _)) = self.ctx.find_overload_method(&lty, method)
                    && let Some(ret_id) = f.return_type
                {
                    return self.type_from_str(self.ast.strings.resolve(&ret_id));
                }
                Type::Class(self.lang_item("number"), vec![])
            }
            Hir::And([_, _]) | Hir::Or([_, _]) | Hir::Not(_) => {
                Type::Class(self.lang_item("boolean"), vec![])
            }
            Hir::Neg(_) | Hir::Inc(_) | Hir::Dec(_) => {
                Type::Class(self.lang_item("number"), vec![])
            }
            Hir::List(_) => Type::Class(self.lang_item("array"), vec![Type::Unknown]),
            Hir::Map(_) => Type::Class(self.lang_item("map"), vec![Type::Unknown, Type::Unknown]),
            Hir::If([_, t, _]) => self.infer_type(*t),
            Hir::Action(ids) => {
                let obj_node = &self.nodes.as_slice()[usize::from(ids[0])];
                let name_node = &self.nodes.as_slice()[usize::from(ids[1])];
                if let (Hir::Str(obj), Hir::Str(name)) = (obj_node, name_node) {
                    let obj_str = obj.0.as_str();
                    let name_str = name.0.as_str();
                    if let Some(def) = schema::action_def(obj_str, name_str) {
                        if def.boolean {
                            return Type::Class(self.lang_item("boolean"), vec![]);
                        }
                        if let Some(assigns) = def.assign
                            && assigns.len() == 1
                        {
                            return self.type_from_action_arg(assigns[0].arg_type);
                        }
                    }
                }
                Type::Unknown
            }
            Hir::Block(ids) => ids.last().map_or(Type::Unknown, |i| self.infer_type(*i)),
            Hir::Let([_, _, body]) => self.infer_type(*body),
            _ => Type::Unknown,
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn type_from_str(&self, s: &str) -> Type {
        self.ctx.type_from_str(s)
    }

    pub(super) fn needs_overload(&self, id: Id, method: &str) -> Option<Rc<FunctionDecl>> {
        let ty = self.infer_type(id);
        let (f, _class_name) = self.ctx.find_overload_method(&ty, method)?;
        Some(f)
    }

    /// Looks up a class member in the class and its ancestors, like `hir::find_in_classes`.
    pub(super) fn find_in_classes<F, R>(&self, ty: &Type, f: F) -> Option<R>
    where
        F: Fn(&crate::ir::ctx::ClassInfo) -> Option<R>,
    {
        find_in_class_chain(ty, &self.ctx.classes_by_def, f)
    }
}
