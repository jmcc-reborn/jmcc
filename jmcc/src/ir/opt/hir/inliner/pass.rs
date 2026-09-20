use crate::ir::VarName;
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::ModulePass;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::HashMap;

use super::advisor::{FuncInfo, InlineAdvisor};
use super::cost::inline_constants;
use super::utils::{extract_args, extract_name, extract_var_name};

#[derive(Debug, Clone, Default)]
pub struct InlinePass {
    pub params: super::cost::InlineParams,
}

impl ModulePass<Hir> for InlinePass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        log::info!("Running HIR InlinePass...");
        let mut current = expr.clone();

        for iter in 0..inline_constants::MAX_INLINE_DEPTH + 1 {
            log::trace!("InlinePass iteration {iter}");

            let advisor = InlineAdvisor::new(&current, &self.params);
            if advisor.func_table.is_empty() {
                log::trace!("No functions to inline, stopping.");
                break;
            }

            let (new_expr, inlined_count) = Inliner::run(&current, &advisor);

            if inlined_count == 0 {
                log::info!(
                    "InlinePass finished after {iter} iterations (no more inlinable calls)."
                );
                break;
            }

            log::trace!("Inlined {inlined_count} calls in iteration {iter}");
            current = new_expr;
        }

        current
    }
}

struct Inliner<'a> {
    expr: &'a RecExpr<Hir>,
    new_nodes: NodeArena<Hir>,
    copy_memo: HashMap<Id, Id>,
    advisor: &'a InlineAdvisor<'a>,
    fresh_counter: u64,
}

impl<'a> Inliner<'a> {
    fn run(expr: &'a RecExpr<Hir>, advisor: &'a InlineAdvisor<'a>) -> (RecExpr<Hir>, usize) {
        if expr.is_empty() {
            return (expr.clone(), 0);
        }
        let mut inliner = Self {
            expr,
            new_nodes: NodeArena::with_capacity(expr.as_ref().len()),
            copy_memo: HashMap::new(),
            advisor,
            fresh_counter: 0,
        };

        let root_id = Id::from(expr.as_ref().len() - 1);
        log::trace!("Inliner::run: starting DAG copy from root={root_id:?}");
        let inlined_count = inliner.copy_node(root_id, 0);

        (inliner.new_nodes.into_recexpr(), inlined_count)
    }

    fn emit(&mut self, node: Hir) -> Id {
        let id = self.new_nodes.add(node);
        log::trace!(
            "  emit: id={id:?}, node={:?}",
            self.new_nodes.as_slice()[usize::from(id)]
        );
        id
    }

    fn make_fresh_name(&mut self, original: &str) -> String {
        self.fresh_counter += 1;
        let name = format!("__inl_{}_{}", original, self.fresh_counter);
        log::trace!("  make_fresh_name: {original} -> {name}");
        name
    }

    fn rename_var_node(&mut self, old_var_id: Id, fresh_name: &str) -> Id {
        let node = &self.expr[old_var_id];
        match node {
            Hir::Var(_) => self.emit(Hir::Var(VarName(Symbol::from(fresh_name)))),
            Hir::Line(i) => {
                let new_inner = self.rename_var_node(*i, fresh_name);
                self.emit(Hir::Line(new_inner))
            }
            Hir::Game(i) => {
                let new_inner = self.rename_var_node(*i, fresh_name);
                self.emit(Hir::Game(new_inner))
            }
            Hir::Save(i) => {
                let new_inner = self.rename_var_node(*i, fresh_name);
                self.emit(Hir::Save(new_inner))
            }
            Hir::Local(i) => {
                let new_inner = self.rename_var_node(*i, fresh_name);
                self.emit(Hir::Local(new_inner))
            }
            _ => self.emit(node.clone()),
        }
    }

    fn copy_node(&mut self, id: Id, depth: u32) -> usize {
        if let Some(&new_id) = self.copy_memo.get(&id) {
            log::trace!("copy_node: id={id:?} already copied to {new_id:?}");
            return 0;
        }

        let node = &self.expr[id];
        log::trace!("copy_node: visiting id={id:?}, node={node:?}");
        let mut inlined_count = 0;

        if let Hir::FuncCall([target, args]) = node
            && let Some(callee_name) = extract_name(self.expr, *target)
            && let Some(func_info) = self.advisor.func_table.get(&callee_name)
        {
            if !self.advisor.recursive.contains(&callee_name) {
                let arg_ids = extract_args(self.expr, *args);
                let advice = self.advisor.get_advice(&callee_name, &arg_ids, depth);

                if advice.should_inline() {
                    log::trace!("  Inlining call to '{callee_name}' at depth {depth}");

                    let mut local_inlined = 0;
                    let mut new_arg_ids = Vec::with_capacity(arg_ids.len());
                    for &a in &arg_ids {
                        local_inlined += self.copy_node(a, depth);
                        new_arg_ids.push(self.copy_memo[&a]);
                    }

                    let new_id = self.inline_call(func_info, &new_arg_ids, depth + 1);
                    self.copy_memo.insert(id, new_id);
                    inlined_count += local_inlined + 1;
                    return inlined_count;
                } else {
                    log::trace!("  Skipping inline for '{callee_name}' (cost too high)");
                }
            } else {
                log::trace!("  Skipping inline for '{callee_name}' (recursive)");
            }
        }

        let children: Vec<Id> = node.children().to_vec();
        let mut new_children = Vec::with_capacity(children.len());
        let mut local_inlined = 0;
        for &c in &children {
            local_inlined += self.copy_node(c, depth);
            new_children.push(self.copy_memo[&c]);
        }

        let mut idx = 0;
        let new_node = node.clone().map_children(|_old| {
            let new_c = new_children[idx];
            idx += 1;
            new_c
        });

        let new_id = self.emit(new_node);
        self.copy_memo.insert(id, new_id);
        inlined_count + local_inlined
    }

    fn inline_call(&mut self, func_info: &FuncInfo, arg_ids: &[Id], depth: u32) -> Id {
        log::trace!(
            "inline_call: preparing to inline function '{}' with {} args",
            func_info.name,
            arg_ids.len()
        );
        let mut var_map: HashMap<String, Id> = HashMap::new();
        let mut inline_memo: HashMap<Id, Id> = HashMap::new();

        for (i, param_name) in func_info.params.iter().enumerate() {
            if let Some(&arg_id) = arg_ids.get(i) {
                log::trace!("  Binding param '{param_name}' to arg_id {arg_id:?}");
                var_map.insert(param_name.clone(), arg_id);
            }
        }

        log::trace!("  Walking callee body {:?}", func_info.body_id);
        self.walk_inline(func_info.body_id, &mut var_map, &mut inline_memo, depth)
    }

    fn walk_inline(
        &mut self,
        id: Id,
        var_map: &mut HashMap<String, Id>,
        memo: &mut HashMap<Id, Id>,
        _depth: u32,
    ) -> Id {
        if let Some(&new_id) = memo.get(&id) {
            log::trace!("  walk_inline: id={id:?} already processed -> {new_id:?}");
            return new_id;
        }

        let node = &self.expr[id];
        log::trace!("  walk_inline: visiting id={id:?}, node={node:?}");

        let new_id = match node {
            Hir::Var(name) => {
                let name_str = name.0.to_string();
                if let Some(&mapped_id) = var_map.get(&name_str) {
                    mapped_id
                } else {
                    self.emit(node.clone())
                }
            }

            Hir::Line(inner) | Hir::Game(inner) | Hir::Save(inner) | Hir::Local(inner) => {
                let name = extract_var_name(self.expr, id).unwrap_or_else(|| Symbol::from(""));
                if let Some(&mapped_id) = var_map.get(name.as_str()) {
                    mapped_id
                } else {
                    let new_inner = self.walk_inline(*inner, var_map, memo, _depth);
                    match node {
                        Hir::Line(_) => self.emit(Hir::Line(new_inner)),
                        Hir::Game(_) => self.emit(Hir::Game(new_inner)),
                        Hir::Save(_) => self.emit(Hir::Save(new_inner)),
                        Hir::Local(_) => self.emit(Hir::Local(new_inner)),
                        _ => unreachable!(),
                    }
                }
            }

            Hir::Let([var_id, val_id, body_id]) => {
                let Some(old_name) = extract_var_name(self.expr, *var_id) else {
                    let new_var_id = self.walk_inline(*var_id, var_map, memo, _depth);
                    let new_val_id = self.walk_inline(*val_id, var_map, memo, _depth);
                    let new_body_id = self.walk_inline(*body_id, var_map, memo, _depth);
                    return self.emit(Hir::Let([new_var_id, new_val_id, new_body_id]));
                };

                let fresh_name = self.make_fresh_name(old_name.as_str());
                let fresh_var_id = self.rename_var_node(*var_id, &fresh_name);

                let new_val_id = self.walk_inline(*val_id, var_map, memo, _depth);

                let old_name = old_name.as_str().to_owned();
                let prev_binding = var_map.insert(old_name.clone(), fresh_var_id);
                let new_body_id = self.walk_inline(*body_id, var_map, memo, _depth);

                if let Some(prev) = prev_binding {
                    var_map.insert(old_name, prev);
                } else {
                    var_map.remove(old_name.as_str());
                }

                self.emit(Hir::Let([fresh_var_id, new_val_id, new_body_id]))
            }

            Hir::VarDecl([name_id, val_id]) => {
                let Some(old_name) = extract_var_name(self.expr, *name_id) else {
                    let new_name_id = self.walk_inline(*name_id, var_map, memo, _depth);
                    let new_val_id = self.walk_inline(*val_id, var_map, memo, _depth);
                    return self.emit(Hir::VarDecl([new_name_id, new_val_id]));
                };

                let fresh_name = self.make_fresh_name(old_name.as_str());
                let fresh_var_id = self.rename_var_node(*name_id, &fresh_name);

                let new_val_id = self.walk_inline(*val_id, var_map, memo, _depth);

                var_map.insert(old_name.as_str().to_owned(), fresh_var_id);

                self.emit(Hir::VarDecl([fresh_var_id, new_val_id]))
            }

            Hir::Return(val) => self.walk_inline(*val, var_map, memo, _depth),

            _ => {
                let children: Vec<Id> = node.children().to_vec();
                let mut new_children = Vec::with_capacity(children.len());
                for &c in &children {
                    new_children.push(self.walk_inline(c, var_map, memo, _depth));
                }

                let mut idx = 0;
                let new_node = node.clone().map_children(|_old| {
                    let new_c = new_children[idx];
                    idx += 1;
                    new_c
                });
                self.emit(new_node)
            }
        };

        memo.insert(id, new_id);
        new_id
    }
}
