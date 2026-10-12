//! Code Sinking pass for HIR.
//!
//! Moves pure variable declarations and assignments into the specific branch of
//! an immediately following `if` statement where the variable is actually used,
//! avoiding unnecessary evaluation when the other branch is taken.

use super::common::{get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::HashMap;

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

fn is_line_scoped_target(expr: &RecExpr<Hir>, id: Id) -> bool {
    match &expr[id] {
        Hir::Line(_) => true,
        Hir::Index([obj, _]) | Hir::Slice([obj, _, _]) => is_line_scoped_target(expr, *obj),
        Hir::Var(crate::ir::VarName(name)) => super::common::is_temp_var(name.as_str()),
        _ => false,
    }
}

fn count_variable_reads(expr: &RecExpr<Hir>) -> HashMap<Symbol, usize> {
    let mut counts = HashMap::new();
    for node in expr.as_ref() {
        if let Hir::Var(crate::ir::VarName(name)) = node {
            *counts.entry(*name).or_insert(0) += 1;
        }
    }
    counts
}

fn count_variable_writes(expr: &RecExpr<Hir>) -> HashMap<Symbol, usize> {
    let mut counts = HashMap::new();
    for node in expr.as_ref() {
        if let Hir::VarDecl([target, _]) | Hir::Set([target, _]) = node
            && let Some(name) = get_target_var_name(expr, *target)
        {
            *counts.entry(name).or_insert(0) += 1;
        }
    }
    counts
}

fn count_reads_in_subtree(expr: &RecExpr<Hir>, root: Id, var: Symbol) -> usize {
    let mut count = 0;
    walk_subtree(expr, root, |_node, id| {
        if let Some(name) = get_var_name(expr, id)
            && name == var
        {
            count += 1;
        }
        true
    });
    count
}

#[derive(Debug, Clone, Default)]
pub struct CodeSinkingPass;

impl FunctionPass<Hir> for CodeSinkingPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let global_read_counts = count_variable_reads(expr);
        let global_write_counts = count_variable_writes(expr);

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut new_children: Vec<Id> =
                    children.iter().map(|&c| id_map[usize::from(c)]).collect();

                // Look for patterns:
                // children[k] = VarDecl([target, val]) or Set([target, val])
                // children[k+1] = If([cond, then_b, else_b])
                let mut k = 0;
                while k + 1 < children.len() {
                    let stmt_id = children[k];
                    let if_id = children[k + 1];

                    if let Hir::VarDecl([target, val]) | Hir::Set([target, val]) = &expr[stmt_id]
                        && is_line_scoped_target(expr, *target)
                        && let Some(target_name) = get_target_var_name(expr, *target)
                        && global_write_counts.get(&target_name).copied().unwrap_or(0) == 1
                        && !target_name.as_str().contains('%')
                        && is_pure_hir(expr, *val)
                        && !has_placeholders(expr, *val)
                        && let Hir::If([cond_id, then_id, else_id]) = &expr[if_id]
                        && is_pure_hir(expr, *cond_id)
                        && !has_placeholders(expr, if_id)
                    {
                        let total_reads =
                            global_read_counts.get(&target_name).copied().unwrap_or(0);
                        let then_reads = count_reads_in_subtree(expr, *then_id, target_name);
                        let else_reads = count_reads_in_subtree(expr, *else_id, target_name);

                        if total_reads > 0 && total_reads == then_reads {
                            // Sink into then branch
                            let mapped_stmt = id_map[usize::from(stmt_id)];
                            let mapped_then = id_map[usize::from(*then_id)];
                            let mapped_cond = id_map[usize::from(*cond_id)];
                            let mapped_else = id_map[usize::from(*else_id)];

                            let new_then_block = match &expr[*then_id] {
                                Hir::Block(b_children) => {
                                    let mut items = Vec::with_capacity(b_children.len() + 1);
                                    items.push(mapped_stmt);
                                    items.extend_from_slice(
                                        &b_children
                                            .iter()
                                            .map(|&bc| id_map[usize::from(bc)])
                                            .collect::<Vec<_>>(),
                                    );
                                    new_nodes.add(Hir::Block(items.into_boxed_slice()))
                                }
                                _ => new_nodes.add(Hir::Block(
                                    vec![mapped_stmt, mapped_then].into_boxed_slice(),
                                )),
                            };

                            let new_if =
                                new_nodes.add(Hir::If([mapped_cond, new_then_block, mapped_else]));
                            let nop = new_nodes.add(Hir::Nop);

                            new_children[k] = nop;
                            new_children[k + 1] = new_if;
                            k += 2;
                            continue;
                        } else if total_reads > 0 && total_reads == else_reads {
                            // Sink into else branch
                            let mapped_stmt = id_map[usize::from(stmt_id)];
                            let mapped_else = id_map[usize::from(*else_id)];
                            let mapped_cond = id_map[usize::from(*cond_id)];
                            let mapped_then = id_map[usize::from(*then_id)];

                            let new_else_block = match &expr[*else_id] {
                                Hir::Block(b_children) => {
                                    let mut items = Vec::with_capacity(b_children.len() + 1);
                                    items.push(mapped_stmt);
                                    items.extend_from_slice(
                                        &b_children
                                            .iter()
                                            .map(|&bc| id_map[usize::from(bc)])
                                            .collect::<Vec<_>>(),
                                    );
                                    new_nodes.add(Hir::Block(items.into_boxed_slice()))
                                }
                                _ => new_nodes.add(Hir::Block(
                                    vec![mapped_stmt, mapped_else].into_boxed_slice(),
                                )),
                            };

                            let new_if =
                                new_nodes.add(Hir::If([mapped_cond, mapped_then, new_else_block]));
                            let nop = new_nodes.add(Hir::Nop);

                            new_children[k] = nop;
                            new_children[k + 1] = new_if;
                            k += 2;
                            continue;
                        }
                    }
                    k += 1;
                }

                new_nodes.add(Hir::Block(new_children.into_boxed_slice()))
            } else {
                let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                new_nodes.add(mapped)
            };

            id_map[usize::from(old_id)] = new_id;
        }

        new_nodes.into_recexpr()
    }
}
