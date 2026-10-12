//! Guard Factoring pass for HIR.
//!
//! When multiple consecutive `if` statements in a block share an identical
//! guard condition (e.g. `if not flagged and C1 { ... }`, `if not flagged and C2 { ... }`),
//! this pass factors the guard out into a single outer `if (guard) { ... }`.
//!
//! If any branch body modifies variables read by the guard (e.g. setting `flagged = true`),
//! subsequent branches are linked via `else` blocks so that they only execute when earlier
//! branches did not trigger. If the guard variables are unmodified, the inner `if`s are
//! executed sequentially inside the outer guard.

use super::common::{collect_action_written_vars, get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use egg::{Id, Language as _, RecExpr};
use std::collections::HashSet;

fn is_empty_or_nop(expr: &RecExpr<Hir>, id: Id) -> bool {
    match &expr[id] {
        Hir::Nop => true,
        Hir::Block(children) => children.iter().all(|&c| is_empty_or_nop(expr, c)),
        _ => false,
    }
}

fn hir_expr_equals(expr: &RecExpr<Hir>, a: Id, b: Id) -> bool {
    if a == b {
        return true;
    }
    let node_a = &expr[a];
    let node_b = &expr[b];
    if std::mem::discriminant(node_a) != std::mem::discriminant(node_b) {
        return false;
    }
    match (node_a, node_b) {
        (Hir::Str(s1), Hir::Str(s2)) => s1 == s2,
        (Hir::Num(n1), Hir::Num(n2)) => (n1.0 - n2.0).abs() < f64::EPSILON,
        (Hir::Bool(b1), Hir::Bool(b2)) => b1 == b2,
        (Hir::Var(v1), Hir::Var(v2)) => v1 == v2,
        _ => {
            let ch_a = node_a.children();
            let ch_b = node_b.children();
            if ch_a.len() != ch_b.len() {
                return false;
            }
            ch_a.iter()
                .zip(ch_b.iter())
                .all(|(&c1, &c2)| hir_expr_equals(expr, c1, c2))
        }
    }
}

fn collect_read_vars(expr: &RecExpr<Hir>, id: Id, out: &mut HashSet<String>) {
    if let Some(var_sym) = get_var_name(expr, id) {
        out.insert(var_sym.as_str().to_string());
    }
    for &child in expr[id].children() {
        collect_read_vars(expr, child, out);
    }
}

fn collect_written_vars(expr: &RecExpr<Hir>, id: Id, out: &mut HashSet<String>) {
    match &expr[id] {
        Hir::Set([target, _]) | Hir::Inc(target) | Hir::Dec(target) => {
            if let Some(target_sym) = get_target_var_name(expr, *target) {
                out.insert(target_sym.as_str().to_string());
            }
        }
        Hir::VarDecl([name, _]) => {
            if let Some(var_sym) = get_var_name(expr, *name) {
                out.insert(var_sym.as_str().to_string());
            }
        }
        Hir::Action(ids) => {
            let mut action_written = std::collections::HashSet::new();
            collect_action_written_vars(expr, ids, &mut action_written);
            for v in action_written {
                out.insert(v.as_str().to_string());
            }
        }
        _ => {}
    }
    for &child in expr[id].children() {
        collect_written_vars(expr, child, out);
    }
}

/// Extracts `(guard, sub_condition)` from a condition node if it is an `And` expression.
fn extract_guard(expr: &RecExpr<Hir>, cond: Id) -> Option<(Id, Id)> {
    if let Hir::And([left, right]) = &expr[cond] {
        Some((*left, *right))
    } else {
        None
    }
}

#[derive(Debug, Clone, Default)]
pub struct GuardFactoringPass;

impl FunctionPass<Hir> for GuardFactoringPass {
    const OPT_LEVEL: u8 = 1;

    #[expect(clippy::too_many_lines, reason = "guard factoring pass implementation")]
    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut merged_children = Vec::with_capacity(children.len());
                let mut idx = 0;

                while idx < children.len() {
                    let cur = children[idx];

                    if let Hir::If([cond_1, _then_1, else_1]) = &expr[cur]
                        && is_empty_or_nop(expr, *else_1)
                        && let Some((guard, _sub_1)) = extract_guard(expr, *cond_1)
                    {
                        // Check if consecutive statements share the same guard
                        let mut end = idx + 1;
                        while end < children.len() {
                            if let Hir::If([cond_k, _then_k, else_k]) = &expr[children[end]]
                                && is_empty_or_nop(expr, *else_k)
                                && let Some((guard_k, _)) = extract_guard(expr, *cond_k)
                                && hir_expr_equals(expr, guard, guard_k)
                            {
                                end += 1;
                            } else {
                                break;
                            }
                        }

                        if end > idx + 1 {
                            // Multiple consecutive ifs share `guard`.
                            let mut guard_reads = HashSet::new();
                            collect_read_vars(expr, guard, &mut guard_reads);

                            let mut any_modifies_guard = false;
                            for k in idx..end {
                                if let Hir::If([_, then_k, _]) = &expr[children[k]] {
                                    let mut then_writes = HashSet::new();
                                    collect_written_vars(expr, *then_k, &mut then_writes);
                                    if !guard_reads.is_disjoint(&then_writes) {
                                        any_modifies_guard = true;
                                        break;
                                    }
                                }
                            }

                            let mapped_guard = id_map[usize::from(guard)];
                            let nop = new_nodes.add(Hir::Nop);

                            let inner_body = if any_modifies_guard {
                                // Chain branches via `else`: if C_k triggers and modifies guard,
                                // subsequent checks are skipped.
                                let mut current_else = nop;
                                for k in (idx..end).rev() {
                                    let Hir::If([cond_k, then_k, _]) = &expr[children[k]] else {
                                        unreachable!()
                                    };
                                    let Some((_, sub_cond_k)) = extract_guard(expr, *cond_k) else {
                                        unreachable!()
                                    };
                                    let mapped_sub = id_map[usize::from(sub_cond_k)];
                                    let mapped_then = id_map[usize::from(*then_k)];
                                    current_else = new_nodes.add(Hir::If([
                                        mapped_sub,
                                        mapped_then,
                                        current_else,
                                    ]));
                                }
                                current_else
                            } else {
                                // Guard is unmodified: run inner checks sequentially.
                                let mut inner_stmts = Vec::with_capacity(end - idx);
                                for k in idx..end {
                                    let Hir::If([cond_k, then_k, _]) = &expr[children[k]] else {
                                        unreachable!()
                                    };
                                    let Some((_, sub_cond_k)) = extract_guard(expr, *cond_k) else {
                                        unreachable!()
                                    };
                                    let mapped_sub = id_map[usize::from(sub_cond_k)];
                                    let mapped_then = id_map[usize::from(*then_k)];
                                    inner_stmts.push(new_nodes.add(Hir::If([
                                        mapped_sub,
                                        mapped_then,
                                        nop,
                                    ])));
                                }
                                new_nodes.add(Hir::Block(inner_stmts.into_boxed_slice()))
                            };

                            let factored_if =
                                new_nodes.add(Hir::If([mapped_guard, inner_body, nop]));
                            merged_children.push(factored_if);
                            idx = end;
                            continue;
                        }
                    }

                    let mapped_child = id_map[usize::from(cur)];
                    if !matches!(&new_nodes.as_slice()[usize::from(mapped_child)], Hir::Nop) {
                        merged_children.push(mapped_child);
                    }
                    idx += 1;
                }

                if merged_children.is_empty() {
                    new_nodes.add(Hir::Nop)
                } else {
                    new_nodes.add(Hir::Block(merged_children.into_boxed_slice()))
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
