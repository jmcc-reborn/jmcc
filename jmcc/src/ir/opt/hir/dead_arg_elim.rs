//! Dead Argument Elimination pass for HIR.
//!
//! Eliminates unused parameters from non-exported, internal functions and
//! updates their call sites accordingly.

use super::common::{get_var_name, string_lit};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::ModulePass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::{HashMap, HashSet};

fn has_placeholders(expr: &RecExpr<Hir>, root: Id) -> bool {
    let mut found = false;
    walk_subtree(expr, root, |node, _| match node {
        Hir::Str(s) if s.0.as_str().contains('%') => {
            found = true;
            false
        }
        Hir::Var(v) if v.0.as_str().contains('%') => {
            found = true;
            false
        }
        _ => true,
    });
    found
}

fn collect_read_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |_node, id| {
        if let Some(name) = get_var_name(expr, id) {
            out.insert(name);
        }
        true
    });
}

fn is_pure_hir(expr: &RecExpr<Hir>, id: Id) -> bool {
    let mut pure = true;
    walk_subtree(expr, id, |node, _| {
        if matches!(
            node,
            Hir::Action(_)
                | Hir::FuncCall(_)
                | Hir::ProcCall(_)
                | Hir::While(_)
                | Hir::If(_)
                | Hir::Break
                | Hir::Continue
                | Hir::Return(_)
                | Hir::FuncDecl(_)
                | Hir::ProcDecl(_)
                | Hir::EventDecl(_)
        ) {
            pure = false;
            false
        } else {
            true
        }
    });
    pure
}

fn find_dead_params(expr: &RecExpr<Hir>) -> HashMap<Symbol, Vec<usize>> {
    let mut dead_params: HashMap<Symbol, Vec<usize>> = HashMap::new();

    for node in expr.as_ref() {
        if let Hir::FuncDecl([name_id, params_id, body_id]) = node
            && let Some(name) = string_lit(expr, *name_id)
        {
            let name_str = name.as_str();
            if name_str.starts_with("__")
                || name_str.contains("::")
                || name_str.starts_with('@')
                || has_placeholders(expr, *body_id)
            {
                continue;
            }

            if let Hir::List(params) = &expr[*params_id] {
                let mut body_reads = HashSet::new();
                collect_read_vars(expr, *body_id, &mut body_reads);

                let mut to_remove = Vec::new();
                for (idx, &param_id) in params.iter().enumerate() {
                    if let Hir::List(p_items) = &expr[param_id]
                        && let Some(&p_name_id) = p_items.first()
                        && let Some(p_name) = string_lit(expr, p_name_id)
                    {
                        let p_str = p_name.as_str();
                        if p_str.starts_with('_')
                            || p_str.starts_with('*')
                            || p_str.starts_with(crate::utils::REF_PARAM_PREFIX)
                            || p_str.contains('%')
                        {
                            continue;
                        }

                        if !body_reads.contains(&p_name) {
                            to_remove.push(idx);
                        }
                    }
                }

                if !to_remove.is_empty() {
                    dead_params.insert(name, to_remove);
                }
            }
        }
    }

    // Verify that at every call site, the argument being removed is pure.
    // If a call site passes an expression with side effects, the argument cannot be dropped.
    for node in expr.as_ref() {
        if let Hir::FuncCall([callee_id, args_id]) = node {
            let callee_name = match &expr[*callee_id] {
                Hir::Str(s) => Some(s.0),
                Hir::Var(v) => Some(v.0),
                _ => None,
            };
            if let Some(name) = callee_name
                && let Some(indices) = dead_params.get_mut(&name)
                && let Hir::List(args) = &expr[*args_id]
            {
                indices.retain(|&idx| {
                    if let Some(&arg_id) = args.get(idx) {
                        is_pure_hir(expr, arg_id)
                    } else {
                        true
                    }
                });
            }
        }
    }

    dead_params.retain(|_, indices| !indices.is_empty());
    dead_params
}

#[derive(Debug, Clone, Default)]
pub struct DeadArgumentEliminationPass;

impl ModulePass<Hir> for DeadArgumentEliminationPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let dead_params = find_dead_params(expr);
        if dead_params.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = match node {
                Hir::FuncDecl([name_id, params_id, body_id]) => {
                    let mapped_name = id_map[usize::from(*name_id)];
                    let mapped_body = id_map[usize::from(*body_id)];

                    if let Some(name) = string_lit(expr, *name_id)
                        && let Some(indices_to_remove) = dead_params.get(&name)
                        && let Hir::List(params) = &expr[*params_id]
                    {
                        let mut new_param_ids = Vec::with_capacity(params.len());
                        for (idx, &p) in params.iter().enumerate() {
                            if !indices_to_remove.contains(&idx) {
                                new_param_ids.push(id_map[usize::from(p)]);
                            }
                        }
                        let new_params = new_nodes.add(Hir::List(new_param_ids.into_boxed_slice()));
                        new_nodes.add(Hir::FuncDecl([mapped_name, new_params, mapped_body]))
                    } else {
                        let mapped_params = id_map[usize::from(*params_id)];
                        new_nodes.add(Hir::FuncDecl([mapped_name, mapped_params, mapped_body]))
                    }
                }
                Hir::FuncCall([callee_id, args_id]) => {
                    let mapped_callee = id_map[usize::from(*callee_id)];

                    let callee_name = match &expr[*callee_id] {
                        Hir::Str(s) => Some(s.0),
                        Hir::Var(v) => Some(v.0),
                        _ => None,
                    };

                    if let Some(name) = callee_name
                        && let Some(indices_to_remove) = dead_params.get(&name)
                        && let Hir::List(args) = &expr[*args_id]
                    {
                        let mut new_arg_ids = Vec::with_capacity(args.len());
                        for (idx, &a) in args.iter().enumerate() {
                            if !indices_to_remove.contains(&idx) {
                                new_arg_ids.push(id_map[usize::from(a)]);
                            }
                        }
                        let new_args = new_nodes.add(Hir::List(new_arg_ids.into_boxed_slice()));
                        new_nodes.add(Hir::FuncCall([mapped_callee, new_args]))
                    } else {
                        let mapped_args = id_map[usize::from(*args_id)];
                        new_nodes.add(Hir::FuncCall([mapped_callee, mapped_args]))
                    }
                }
                _ => {
                    let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                    new_nodes.add(mapped)
                }
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}
