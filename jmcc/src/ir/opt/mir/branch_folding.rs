//! Redundant Condition & Branch Pruning pass for MIR.
//!
//! Eliminates empty conditional actions, folds redundant checks where an outer
//! conditional already proves the condition true or false, and prunes dead branches.

use std::collections::HashSet;

use super::common::is_else_action;
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use egg::{Id, Language as _, RecExpr};

fn is_empty_block(expr: &RecExpr<Mir>, id: Id) -> bool {
    match &expr[id] {
        Mir::Nop => true,
        Mir::Block(children) => children.iter().all(|&c| is_empty_block(expr, c)),
        _ => false,
    }
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
                || def.action_type == "container"
                || def.action_type.ends_with("_with_conditional");
        }
    }
    false
}

#[derive(Debug, Clone, Default)]
pub struct BranchFoldingPass;

impl FunctionPass<Mir> for BranchFoldingPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        let mut used_as_value = HashSet::new();
        let mut has_following_else = HashSet::new();
        for node in expr.as_ref() {
            match node {
                Mir::Block(children) => {
                    for i in 0..children.len().saturating_sub(1) {
                        if is_else_action(expr, children[i + 1]) {
                            has_following_else.insert(children[i]);
                        }
                    }
                }
                Mir::Named([_, val])
                | Mir::Let([_, val, _])
                | Mir::Set([_, val])
                | Mir::VarDecl([_, val])
                | Mir::ReturnFunc(val)
                | Mir::Not(val) => {
                    used_as_value.insert(*val);
                }
                Mir::List(ids) | Mir::Map(ids) | Mir::Concat(ids) | Mir::Ctor(ids) => {
                    for &id in ids {
                        used_as_value.insert(id);
                    }
                }
                Mir::Action(ids) | Mir::InvertedAction(ids) if ids.len() >= 7 => {
                    used_as_value.insert(ids[6]);
                    if let Mir::Not(inner) = &expr[ids[6]] {
                        used_as_value.insert(*inner);
                    }
                }
                _ => {}
            }
        }

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = match node {
                Mir::Action(ids)
                    if is_condition_action(expr, old_id)
                        && !used_as_value.contains(&old_id)
                        && !has_following_else.contains(&old_id) =>
                {
                    // Check if then block is empty (and condition has no following else)
                    let then_empty = ids.len() >= 5 && is_empty_block(expr, ids[4]);

                    if then_empty {
                        // Empty condition with no else: prune completely!
                        new_nodes.add(Mir::Nop)
                    } else {
                        let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                        new_nodes.add(mapped)
                    }
                }
                Mir::Block(children) => {
                    // Filter out trailing and intermediate Nops if possible
                    let mut new_children = Vec::with_capacity(children.len());
                    for &child in children {
                        let mapped_child = id_map[usize::from(child)];
                        if !matches!(&new_nodes.as_slice()[usize::from(mapped_child)], Mir::Nop) {
                            new_children.push(mapped_child);
                        }
                    }
                    if new_children.is_empty() {
                        new_nodes.add(Mir::Nop)
                    } else {
                        new_nodes.add(Mir::Block(new_children.into_boxed_slice()))
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
