//! Branch Merging pass for MIR.
//!
//! Merges consecutive `if` actions with identical conditions when the first
//! branch body does not modify any variables in the condition.

use super::common::{get_variable_name, is_else_action};
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use egg::{Id, Language as _, RecExpr};
use std::collections::HashSet;

fn has_following_else(expr: &RecExpr<Mir>, children: &[Id], i: usize) -> bool {
    i + 1 < children.len() && is_else_action(expr, children[i + 1])
}

fn is_condition_action(expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Action(ids) = &expr[id]
        && ids.len() >= 2
        && let (Mir::Str(obj), Mir::Str(name)) = (&expr[ids[0]], &expr[ids[1]])
    {
        let obj_str = obj.0.as_str();
        let name_str = name.0.as_str();
        if name_str.starts_with("if_") || obj_str == "if" {
            return true;
        }
        if let Some(action_id) = jmcdata::generated::get_action_id(obj_str, name_str) {
            return action_id.as_str().starts_with("if_");
        }
        if let Some(def) = jmcdata::generated::get_action_def(obj_str, name_str) {
            return def.id.starts_with("if_")
                || def.action_type == "conditional"
                || def.action_type.ends_with("_with_conditional");
        }
    }
    false
}

fn expr_equals(expr: &RecExpr<Mir>, a: Id, b: Id) -> bool {
    if a == b {
        return true;
    }
    let node_a = &expr[a];
    let node_b = &expr[b];
    if std::mem::discriminant(node_a) != std::mem::discriminant(node_b) {
        return false;
    }
    match (node_a, node_b) {
        (Mir::Str(s1), Mir::Str(s2)) => s1 == s2,
        (Mir::Num(n1), Mir::Num(n2)) => (n1.0 - n2.0).abs() < f64::EPSILON,
        (Mir::Bool(b1), Mir::Bool(b2)) => b1 == b2,
        (Mir::Var(v1), Mir::Var(v2)) => v1 == v2,
        (Mir::Sel(s1), Mir::Sel(s2)) => s1 == s2,
        _ => {
            let ch_a = node_a.children();
            let ch_b = node_b.children();
            if ch_a.len() != ch_b.len() {
                return false;
            }
            ch_a.iter()
                .zip(ch_b.iter())
                .all(|(&c1, &c2)| expr_equals(expr, c1, c2))
        }
    }
}

fn collect_read_vars(expr: &RecExpr<Mir>, id: Id, out: &mut HashSet<String>) {
    if let Some(var_name) = get_variable_name(expr, id) {
        out.insert(var_name.to_string());
    }
    for &child in expr[id].children() {
        collect_read_vars(expr, child, out);
    }
}

fn collect_written_vars(expr: &RecExpr<Mir>, id: Id, out: &mut HashSet<String>) {
    if let Mir::Action(ids) = &expr[id]
        && ids.len() >= 4
        && let (Mir::Str(obj), Mir::Str(name)) = (&expr[ids[0]], &expr[ids[1]])
    {
        let obj_str = obj.0.as_str();
        let name_str = name.0.as_str();
        if (obj_str == "variable" || name_str.starts_with("set_"))
            && let Mir::List(args) = &expr[ids[3]]
        {
            for &a in args {
                if let Mir::Named([n, v]) = &expr[a]
                    && let Mir::Str(arg_name) = &expr[*n]
                {
                    let a_name = arg_name.0.as_str();
                    if a_name == "variable" {
                        if let Some(var_name) = get_variable_name(expr, *v) {
                            out.insert(var_name.to_string());
                        }
                    } else if a_name == "variables" {
                        if let Mir::List(items) = &expr[*v] {
                            for &item in items {
                                if let Some(var_name) = get_variable_name(expr, item) {
                                    out.insert(var_name.to_string());
                                }
                            }
                        } else if let Some(var_name) = get_variable_name(expr, *v) {
                            out.insert(var_name.to_string());
                        }
                    }
                }
            }
        }
    }
    for &child in expr[id].children() {
        collect_written_vars(expr, child, out);
    }
}

fn has_placeholders(expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Str(s) = &expr[id]
        && s.0.as_str().contains('%')
    {
        return true;
    }
    expr[id]
        .children()
        .iter()
        .any(|&c| has_placeholders(expr, c))
}

#[derive(Debug, Clone, Default)]
pub struct BranchMergingPass;

impl FunctionPass<Mir> for BranchMergingPass {
    const OPT_LEVEL: u8 = 2;

    #[expect(clippy::too_many_lines, reason = "branch merging pass implementation")]
    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Mir::Block(children) = node {
                let mut merged_children = Vec::with_capacity(children.len());
                let mut idx = 0;

                while idx < children.len() {
                    let cur = children[idx];

                    if idx + 1 < children.len()
                        && is_condition_action(expr, cur)
                        && is_condition_action(expr, children[idx + 1])
                    {
                        let next = children[idx + 1];
                        if let (Mir::Action(ids1), Mir::Action(ids2)) = (&expr[cur], &expr[next]) {
                            let cur_no_else = !has_following_else(expr, children, idx);
                            let next_no_else = !has_following_else(expr, children, idx + 1);

                            let same_cond = ids1.len() >= 5
                                && ids2.len() >= 5
                                && expr_equals(expr, ids1[0], ids2[0])
                                && expr_equals(expr, ids1[1], ids2[1])
                                && expr_equals(expr, ids1[2], ids2[2])
                                && expr_equals(expr, ids1[3], ids2[3])
                                && !has_placeholders(expr, ids1[3]);

                            if cur_no_else && next_no_else && same_cond {
                                // Scan all consecutive actions with the same condition and no else
                                let mut end = idx + 2;
                                while end < children.len()
                                    && is_condition_action(expr, children[end])
                                {
                                    if let Mir::Action(ids_k) = &expr[children[end]] {
                                        let no_else = !has_following_else(expr, children, end);
                                        let matches = ids_k.len() >= 5
                                            && expr_equals(expr, ids1[0], ids_k[0])
                                            && expr_equals(expr, ids1[1], ids_k[1])
                                            && expr_equals(expr, ids1[2], ids_k[2])
                                            && expr_equals(expr, ids1[3], ids_k[3])
                                            && !has_placeholders(expr, ids1[3]);
                                        if no_else && matches {
                                            end += 1;
                                            continue;
                                        }
                                    }
                                    break;
                                }

                                let mut cond_reads = HashSet::new();
                                collect_read_vars(expr, ids1[3], &mut cond_reads);

                                // Fold children[idx..end] from right to left
                                let mut acc_action_id = id_map[usize::from(children[end - 1])];

                                for k in (idx..end - 1).rev() {
                                    let cur_child = children[k];
                                    let Mir::Action(ids_k) = &expr[cur_child] else {
                                        unreachable!()
                                    };

                                    let mut then_writes = HashSet::new();
                                    collect_written_vars(expr, ids_k[4], &mut then_writes);

                                    let mut combined_then = Vec::new();
                                    if let Mir::Block(b) = &expr[ids_k[4]] {
                                        for &c in b {
                                            combined_then.push(id_map[usize::from(c)]);
                                        }
                                    } else {
                                        combined_then.push(id_map[usize::from(ids_k[4])]);
                                    }

                                    if cond_reads.is_disjoint(&then_writes) {
                                        // Condition is unchanged by then_k: unpack acc's then body directly!
                                        if let Mir::Action(ids_acc) =
                                            &new_nodes.as_slice()[usize::from(acc_action_id)]
                                        {
                                            let then_acc = ids_acc[4];
                                            if let Mir::Block(b) =
                                                &new_nodes.as_slice()[usize::from(then_acc)]
                                            {
                                                combined_then.extend_from_slice(b);
                                            } else {
                                                combined_then.push(then_acc);
                                            }
                                        } else {
                                            combined_then.push(acc_action_id);
                                        }
                                    } else {
                                        // Condition may have been modified by then_k:
                                        // nest acc as a conditional check inside combined_then!
                                        combined_then.push(acc_action_id);
                                    }

                                    let merged_then_id =
                                        new_nodes.add(Mir::Block(combined_then.into_boxed_slice()));

                                    let mut new_action_ids = ids_k.clone();
                                    new_action_ids[4] = merged_then_id;
                                    let mapped_action =
                                        Mir::Action(new_action_ids).map_children(|c| {
                                            if c == merged_then_id {
                                                c
                                            } else {
                                                id_map[usize::from(c)]
                                            }
                                        });

                                    acc_action_id = new_nodes.add(mapped_action);
                                }

                                merged_children.push(acc_action_id);
                                idx = end;
                                continue;
                            }
                        }
                    }

                    let mapped_child = id_map[usize::from(cur)];
                    if !matches!(&new_nodes.as_slice()[usize::from(mapped_child)], Mir::Nop) {
                        merged_children.push(mapped_child);
                    }
                    idx += 1;
                }

                if merged_children.is_empty() {
                    new_nodes.add(Mir::Nop)
                } else {
                    new_nodes.add(Mir::Block(merged_children.into_boxed_slice()))
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
