//! Loop-Invariant Code Motion (LICM) pass for HIR.
//!
//! Hoists pure assignments whose operands do not change inside the loop body
//! out of `while` loops, executing them once before entering the loop.

use super::common::{collect_action_written_vars, get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::{HashMap, HashSet};

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

/// Collects all variable names read within a HIR subtree.
fn collect_read_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |_node, id| {
        if let Some(name) = get_var_name(expr, id) {
            out.insert(name);
        }
        true
    });
}

/// Collects all variable names written (assigned) within a HIR subtree.
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

/// Counts how many times each variable is assigned within a HIR subtree.
fn count_variable_writes(expr: &RecExpr<Hir>, root: Id, out: &mut HashMap<Symbol, usize>) {
    walk_subtree(expr, root, |node, _| {
        match node {
            Hir::VarDecl([target, _])
            | Hir::Set([target, _])
            | Hir::Inc(target)
            | Hir::Dec(target) => {
                if let Some(name) = get_target_var_name(expr, *target) {
                    *out.entry(name).or_insert(0) += 1;
                }
            }
            Hir::Action(ids) => {
                let mut action_written = HashSet::new();
                collect_action_written_vars(expr, ids, &mut action_written);
                for name in action_written {
                    *out.entry(name).or_insert(0) += 1;
                }
            }
            _ => {}
        }
        true
    });
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
pub struct LoopInvariantCodeMotionPass;

impl FunctionPass<Hir> for LoopInvariantCodeMotionPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let global_read_counts = count_variable_reads(expr);

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut new_children = Vec::with_capacity(children.len() * 2);

                for &child in children {
                    let mapped_child = id_map[usize::from(child)];

                    // Check if this child is a While loop with loop invariants to hoist
                    if let Hir::While([cond_id, body_id]) = &expr[child]
                        && let Hir::Block(body_children) = &expr[*body_id]
                        && !has_placeholders(expr, child)
                    {
                        let mut loop_written = HashSet::new();
                        collect_written_vars(expr, *body_id, &mut loop_written);
                        collect_written_vars(expr, *cond_id, &mut loop_written);

                        let mut write_counts = HashMap::new();
                        count_variable_writes(expr, *body_id, &mut write_counts);

                        let mut hoisted_ids = Vec::new();
                        let mut hoisted_vars = HashSet::new();
                        let mut is_hoisted = vec![false; body_children.len()];

                        for (idx, &stmt_id) in body_children.iter().enumerate() {
                            if let Hir::VarDecl([target, val]) | Hir::Set([target, val]) =
                                &expr[stmt_id]
                                && is_line_scoped_target(expr, *target)
                                && let Some(target_name) = get_target_var_name(expr, *target)
                                && write_counts.get(&target_name).copied().unwrap_or(0) == 1
                                && !target_name.as_str().contains('%')
                                && is_pure_hir(expr, *val)
                            {
                                let total_reads =
                                    global_read_counts.get(&target_name).copied().unwrap_or(0);
                                let body_reads =
                                    count_reads_in_subtree(expr, *body_id, target_name);

                                // Target must not be read outside this loop body, so hoisting does
                                // not leak a modified value if the loop executes 0 times.
                                if total_reads == body_reads {
                                    let mut val_reads = HashSet::new();
                                    collect_read_vars(expr, *val, &mut val_reads);

                                    // Invariant condition: no operand is written in the loop (unless hoisted earlier)
                                    let is_inv = !val_reads.contains(&target_name)
                                        && val_reads.iter().all(|v| {
                                            !loop_written.contains(v) || hoisted_vars.contains(v)
                                        });

                                    if is_inv {
                                        is_hoisted[idx] = true;
                                        hoisted_vars.insert(target_name);
                                        // Hoist this mapped statement before the loop
                                        let mapped_stmt = id_map[usize::from(stmt_id)];
                                        hoisted_ids.push(mapped_stmt);
                                    }
                                }
                            }
                        }

                        if !hoisted_ids.is_empty() {
                            // Add hoisted statements before the loop
                            new_children.extend(hoisted_ids);

                            // Rebuild loop body replacing hoisted statements with Nop
                            let mut new_body_children = Vec::with_capacity(body_children.len());
                            for (idx, &b_child) in body_children.iter().enumerate() {
                                if is_hoisted[idx] {
                                    let nop = new_nodes.add(Hir::Nop);
                                    new_body_children.push(nop);
                                } else {
                                    new_body_children.push(id_map[usize::from(b_child)]);
                                }
                            }
                            let new_body =
                                new_nodes.add(Hir::Block(new_body_children.into_boxed_slice()));
                            let new_while = new_nodes
                                .add(Hir::While([id_map[usize::from(*cond_id)], new_body]));
                            new_children.push(new_while);
                            continue;
                        }
                    }

                    new_children.push(mapped_child);
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
