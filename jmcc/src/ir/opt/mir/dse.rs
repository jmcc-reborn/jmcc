//! Dead Store Elimination (DSE) pass for MIR:
//! Removes dead assignments (`variable::set_value`) to local/line variables that are never read
//! before being overwritten or before the frame ends.

use super::common::{
    VarScope, extract_var_value_args, get_scoped_var_name, get_variable_name, is_set_value_action,
};
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::{Id, Language as _, RecExpr, Symbol};
use std::collections::HashSet;

fn has_any_placeholders(expr: &RecExpr<Mir>) -> bool {
    expr.as_ref().iter().any(|node| match node {
        Mir::Str(s) => s.0.as_str().contains('%'),
        Mir::Var(v) => v.0.as_str().contains('%'),
        _ => false,
    })
}

/// Checks if a MIR subtree is pure (has no side effects and does not invoke handlers/loops).
fn is_pure_mir(expr: &RecExpr<Mir>, id: Id) -> bool {
    walk_subtree(expr, id, |node, _| {
        !matches!(
            node,
            Mir::Action(_)
                | Mir::InvertedAction(_)
                | Mir::ReturnFunc(_)
                | Mir::Break
                | Mir::Continue
                | Mir::FuncDecl(_)
                | Mir::ProcDecl(_)
                | Mir::EventDecl(_)
        )
    })
}

/// Collects all variable names read within a MIR subtree.
fn collect_read_vars(expr: &RecExpr<Mir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |_node, id| {
        if let Some((_, name)) = get_scoped_var_name(expr, id) {
            out.insert(name);
        } else if let Some(name) = get_variable_name(expr, id) {
            out.insert(name);
        }
        true
    });
}

/// Checks if a node is a container action (e.g. repeat, if, call) that may have complex control flow.
fn is_control_flow_or_call(expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Action(ids) = &expr[id] {
        if ids.len() >= 2
            && let (Mir::Str(obj), Mir::Str(name)) = (&expr[ids[0]], &expr[ids[1]])
        {
            let obj_str = obj.0.as_str();
            let name_str = name.0.as_str();
            if obj_str == "code" || obj_str == "control" || name_str.starts_with("repeat") {
                return true;
            }
        }
        // If action has children blocks (then/else)
        if ids.len() >= 5 && !matches!(&expr[ids[4]], Mir::Nop) {
            return true;
        }
        if ids.len() >= 7 && !matches!(&expr[ids[6]], Mir::Nop) {
            return true;
        }
    }
    matches!(
        &expr[id],
        Mir::ReturnFunc(_)
            | Mir::Break
            | Mir::Continue
            | Mir::FuncDecl(_)
            | Mir::ProcDecl(_)
            | Mir::EventDecl(_)
    )
}

/// Eliminates dead stores in a block: if a variable is assigned and later unconditionally
/// overwritten before any read or control-flow branch, the first assignment is dead.
fn eliminate_dead_stores_in_block(
    expr: &RecExpr<Mir>,
    children: &[Id],
    id_map: &[Id],
    new_nodes: &mut NodeArena<Mir>,
) -> Vec<Id> {
    let mut is_dead = vec![false; children.len()];

    for i in 0..children.len() {
        if is_set_value_action(expr, children[i])
            && let Some((var_id, val_id)) = extract_var_value_args(expr, children[i])
            && let Some((scope, name)) = get_scoped_var_name(expr, var_id)
        {
            let is_line = matches!(scope, VarScope::Line | VarScope::Bare);
            if !is_line || name.as_str().contains('%') || !is_pure_mir(expr, val_id) {
                continue;
            }

            // Scan forward in the same block.
            for &child in children.iter().skip(i + 1) {
                // If control flow or branch occurs, stop.
                if is_control_flow_or_call(expr, child) {
                    break;
                }

                // If this statement is a set_value to the exact same variable:
                if is_set_value_action(expr, child)
                    && let Some((next_var_id, next_val_id)) = extract_var_value_args(expr, child)
                    && let Some((next_scope, next_name)) = get_scoped_var_name(expr, next_var_id)
                    && next_scope == scope
                    && next_name == name
                {
                    let mut next_reads = HashSet::new();
                    collect_read_vars(expr, next_val_id, &mut next_reads);
                    if !next_reads.contains(&name) {
                        // Unconditionally overwritten before any read!
                        log::trace!("  [DSE] Overwritten dead store to '{name}' at index {i}");
                        is_dead[i] = true;
                    }
                    break;
                }

                // Check if child reads the variable.
                let mut reads = HashSet::new();
                collect_read_vars(expr, child, &mut reads);
                if reads.contains(&name) {
                    break;
                }
            }
        }
    }

    let mut result = Vec::with_capacity(children.len());
    for (i, &child) in children.iter().enumerate() {
        if is_dead[i] {
            let nop = new_nodes.add(Mir::Nop);
            result.push(nop);
        } else {
            let mapped = id_map[usize::from(child)];
            result.push(mapped);
        }
    }
    result
}

/// Dead Store Elimination pass for MIR.
#[must_use]
pub fn eliminate_dead_stores(expr: &RecExpr<Mir>) -> RecExpr<Mir> {
    if expr.is_empty() || has_any_placeholders(expr) {
        return expr.clone();
    }

    let mut new_nodes = NodeArena::with_capacity(expr.len());
    let mut id_map = vec![Id::from(0); expr.len()];

    for (i, node) in expr.as_ref().iter().enumerate() {
        let old_id = Id::from(i);

        let new_id = if let Mir::Block(children) = node {
            let processed = eliminate_dead_stores_in_block(expr, children, &id_map, &mut new_nodes);
            new_nodes.add(Mir::Block(processed.into_boxed_slice()))
        } else {
            let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
            new_nodes.add(mapped)
        };

        id_map[usize::from(old_id)] = new_id;
    }

    new_nodes.into_recexpr()
}

#[derive(Debug, Clone, Default)]
pub struct DeadStoreEliminationPass;

impl FunctionPass<Mir> for DeadStoreEliminationPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        eliminate_dead_stores(expr)
    }
}
