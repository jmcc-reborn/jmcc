//! Loop Unswitching pass for HIR.
//!
//! Transforms loops containing loop-invariant conditionals into a conditional
//! containing two loops:
//!
//! `while cond { if inv { A } else { B } }`
//!   ==>
//! `if inv { while cond { A } } else { while cond { B } }`

use super::common::{collect_action_written_vars, get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::HashSet;

/// Checks whether an expression in HIR is pure (has no side effects or control flow).
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

fn collect_written_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |node, _| {
        match node {
            Hir::VarDecl([target, _])
            | Hir::Set([target, _])
            | Hir::Inc(target)
            | Hir::Dec(target) => {
                if let Some(name) = get_target_var_name(expr, *target) {
                    out.insert(name);
                }
            }
            Hir::Action(ids) => {
                collect_action_written_vars(expr, ids, out);
            }
            _ => {}
        }
        true
    });
}

#[derive(Debug, Clone, Default)]
pub struct LoopUnswitchPass;

impl FunctionPass<Hir> for LoopUnswitchPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len() * 2);
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::While([cond_id, body_id]) = node {
                let mapped_cond = id_map[usize::from(*cond_id)];
                let mapped_body = id_map[usize::from(*body_id)];

                // Check if the loop body contains a single top-level invariant If statement
                let single_if = match &expr[*body_id] {
                    Hir::If([if_cond, then_b, else_b]) => Some((*if_cond, *then_b, *else_b)),
                    Hir::Block(children) => {
                        let non_nops: Vec<Id> = children
                            .iter()
                            .copied()
                            .filter(|&c| !matches!(expr[c], Hir::Nop))
                            .collect();
                        if non_nops.len() == 1
                            && let Hir::If([if_cond, then_b, else_b]) = &expr[non_nops[0]]
                        {
                            Some((*if_cond, *then_b, *else_b))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                if let Some((if_cond, then_b, else_b)) = single_if
                    && is_pure_hir(expr, if_cond)
                    && !has_placeholders(expr, if_cond)
                {
                    let mut cond_reads = HashSet::new();
                    collect_read_vars(expr, if_cond, &mut cond_reads);

                    let mut loop_written = HashSet::new();
                    collect_written_vars(expr, *body_id, &mut loop_written);
                    collect_written_vars(expr, *cond_id, &mut loop_written);

                    // If condition is invariant if no variable it reads is modified in the loop
                    let is_invariant = !cond_reads.is_empty()
                        && cond_reads.iter().all(|v| !loop_written.contains(v));

                    if is_invariant {
                        let mapped_if_cond = id_map[usize::from(if_cond)];
                        let mapped_then = id_map[usize::from(then_b)];
                        let mapped_else = id_map[usize::from(else_b)];

                        let loop_then = new_nodes.add(Hir::While([mapped_cond, mapped_then]));
                        let loop_else = new_nodes.add(Hir::While([mapped_cond, mapped_else]));

                        new_nodes.add(Hir::If([mapped_if_cond, loop_then, loop_else]))
                    } else {
                        new_nodes.add(Hir::While([mapped_cond, mapped_body]))
                    }
                } else {
                    new_nodes.add(Hir::While([mapped_cond, mapped_body]))
                }
            } else {
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                new_nodes.add(mapped)
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}
