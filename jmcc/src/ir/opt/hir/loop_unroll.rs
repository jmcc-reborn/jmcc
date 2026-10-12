//! Loop Unrolling pass for HIR.
//!
//! Unrolls `while` loops with small, statically known iteration counts (<= 4),
//! eliminating loop overhead and enabling subsequent constant folding and CSE.

use super::common::{get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr};
use ordered_float::OrderedFloat;

fn contains_break_or_continue(expr: &RecExpr<Hir>, root: Id) -> bool {
    let mut found = false;
    walk_subtree(expr, root, |node, _| {
        if matches!(node, Hir::Break | Hir::Continue) {
            found = true;
            false
        } else {
            true
        }
    });
    found
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

#[derive(Debug, Clone, Default)]
pub struct LoopUnrollPass;

impl FunctionPass<Hir> for LoopUnrollPass {
    const OPT_LEVEL: u8 = 2;

    #[expect(clippy::too_many_lines, reason = "Loop unrolling pass for HIR")]
    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len() * 3);
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut new_children = Vec::with_capacity(children.len() * 4);
                let mut idx = 0;

                while idx < children.len() {
                    let child = children[idx];

                    // Check for pattern: VarDecl([i, start]) followed by While([i < end, body])
                    if idx + 1 < children.len()
                        && let Hir::VarDecl([target, init_val]) = &expr[child]
                        && let Some(var_name) = get_target_var_name(expr, *target)
                        && let Hir::Num(start_num) = &expr[*init_val]
                        && let start_val = start_num.0 as i64
                        && let Hir::While([cond_id, body_id]) = &expr[children[idx + 1]]
                        && let Hir::Block(body_children) = &expr[*body_id]
                        && !contains_break_or_continue(expr, *body_id)
                        && !has_placeholders(expr, children[idx + 1])
                    {
                        // Inspect condition: i < end or i <= end
                        let cond_info = match &expr[*cond_id] {
                            Hir::Lt([lhs, rhs]) => {
                                if get_var_name(expr, *lhs) == Some(var_name)
                                    && let Hir::Num(end_num) = &expr[*rhs]
                                {
                                    Some((end_num.0 as i64, false))
                                } else {
                                    None
                                }
                            }
                            Hir::Le([lhs, rhs]) => {
                                if get_var_name(expr, *lhs) == Some(var_name)
                                    && let Hir::Num(end_num) = &expr[*rhs]
                                {
                                    Some((end_num.0 as i64, true))
                                } else {
                                    None
                                }
                            }
                            _ => None,
                        };

                        if let Some((end_val, inclusive)) = cond_info {
                            let trip_count = if inclusive {
                                end_val - start_val + 1
                            } else {
                                end_val - start_val
                            };

                            if (1..=4).contains(&trip_count) {
                                // Check if body ends with i += 1 / i = i + 1
                                let mut has_step = false;
                                let mut non_step_stmts = Vec::new();

                                for &b_stmt in body_children {
                                    if let Hir::Set([tgt, val]) = &expr[b_stmt]
                                        && get_target_var_name(expr, *tgt) == Some(var_name)
                                    {
                                        // Check if it's i + 1
                                        if let Hir::Add([lhs, rhs]) = &expr[*val]
                                            && get_var_name(expr, *lhs) == Some(var_name)
                                            && let Hir::Num(one) = &expr[*rhs]
                                            && (one.0 - 1.0).abs() < f64::EPSILON
                                        {
                                            has_step = true;
                                            continue;
                                        }
                                    }
                                    non_step_stmts.push(b_stmt);
                                }

                                if has_step {
                                    // Unroll!
                                    // Emit initial assignment
                                    new_children.push(id_map[usize::from(child)]);

                                    let mapped_target = id_map[usize::from(*target)];

                                    for iter in 0..trip_count {
                                        let current_val = start_val + iter;
                                        let num_node = new_nodes
                                            .add(Hir::Num(OrderedFloat(current_val as f64)));
                                        let set_i =
                                            new_nodes.add(Hir::Set([mapped_target, num_node]));
                                        new_children.push(set_i);

                                        // Copy non-step statements of the body
                                        for &stmt in &non_step_stmts {
                                            new_children.push(id_map[usize::from(stmt)]);
                                        }
                                    }

                                    // Final value of i
                                    let final_val = start_val + trip_count;
                                    let final_num =
                                        new_nodes.add(Hir::Num(OrderedFloat(final_val as f64)));
                                    let set_final =
                                        new_nodes.add(Hir::Set([mapped_target, final_num]));
                                    new_children.push(set_final);

                                    idx += 2;
                                    continue;
                                }
                            }
                        }
                    }

                    new_children.push(id_map[usize::from(child)]);
                    idx += 1;
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
