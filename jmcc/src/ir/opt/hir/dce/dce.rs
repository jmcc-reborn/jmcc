//! Dead Code Elimination (Variables): removes assignments to unused temporary variables.

use super::super::common::{get_var_name, is_temp_var};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::{walk_subtree, walk_subtree_with};
use egg::*;
use std::collections::HashSet;

const fn is_pure(enode: &Hir) -> bool {
    !matches!(
        enode,
        Hir::Action(_)
            | Hir::FuncCall(_)
            | Hir::ProcCall(_)
            | Hir::Set(_)
            | Hir::Return(_)
            | Hir::Break
            | Hir::While(_)
            | Hir::Inc(_)
            | Hir::Dec(_)
    )
}

fn is_pure_subtree(expr: &RecExpr<Hir>, root: Id) -> bool {
    walk_subtree(expr, root, |node, _| is_pure(node))
}

fn extract_vars_from_str(s: &str, vars: &mut HashSet<Symbol>) {
    let mut remainder = s;
    while let Some(pos) = remainder.find("%var") {
        remainder = &remainder[pos + 4..];
        let after_prefix = if let Some(stripped) = remainder.strip_prefix("_line(") {
            stripped
        } else if let Some(stripped) = remainder.strip_prefix("_local(") {
            stripped
        } else if let Some(stripped) = remainder.strip_prefix("_save(") {
            stripped
        } else if let Some(stripped) = remainder.strip_prefix('(') {
            stripped
        } else {
            continue;
        };
        if let Some(end) = after_prefix.find(')') {
            let var_name = &after_prefix[..end];
            vars.insert(Symbol::from(var_name));
            remainder = &after_prefix[end + 1..];
        }
    }
}

fn collect_read_vars(expr: &RecExpr<Hir>) -> HashSet<Symbol> {
    if expr.is_empty() {
        return HashSet::new();
    }

    let mut read_vars = HashSet::new();

    walk_subtree_with(expr, expr.root(), |node, id, stack| {
        if let Some(name) = get_var_name(expr, id) {
            read_vars.insert(name);
        }
        if let Hir::Str(s) = node {
            extract_vars_from_str(s.0.as_str(), &mut read_vars);
        }

        match node {
            // Binding name is not a variable read, so do not descend into it;
            // otherwise a dropped definition would appear as read.
            Hir::Let([_, val, body]) => {
                stack.push(*val);
                stack.push(*body);
            }
            Hir::VarDecl([_, val]) => stack.push(*val),
            // Variable assignment does not read the write target — only the value.
            // Targets of other kinds (e.g. fields) are read as ordinary expressions.
            Hir::Set([target, val]) => {
                if !matches!(
                    &expr[*target],
                    Hir::Var(_) | Hir::Local(_) | Hir::Game(_) | Hir::Save(_) | Hir::Line(_)
                ) {
                    stack.push(*target);
                }
                stack.push(*val);
            }
            _ => stack.extend(node.children().iter().copied()),
        }
    });

    read_vars
}

/// A definition is dead if its target is an unread temporary and its value is a pure subtree.
fn is_dead_assignment(
    expr: &RecExpr<Hir>,
    target: Id,
    val: Id,
    read_vars: &HashSet<Symbol>,
) -> bool {
    get_var_name(expr, target).is_some_and(|n| is_temp_var(n.as_str()) && !read_vars.contains(&n))
        && is_pure_subtree(expr, val)
}

fn is_dead_definition(expr: &RecExpr<Hir>, id: Id, read_vars: &HashSet<Symbol>) -> bool {
    match &expr[id] {
        Hir::VarDecl([target, val]) | Hir::Set([target, val]) => {
            is_dead_assignment(expr, *target, *val, read_vars)
        }
        _ => false,
    }
}

#[must_use]
pub fn eliminate_dead_code(expr: &RecExpr<Hir>) -> RecExpr<Hir> {
    if expr.is_empty() {
        return expr.clone();
    }

    let mut current = expr.clone();

    loop {
        let read_vars = collect_read_vars(&current);
        let mut new_nodes = NodeArena::with_capacity(current.len());
        let mut id_map: Vec<Id> = vec![Id::from(0); current.len()];
        let mut changed = false;

        for (i, node) in current.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = match node {
                Hir::Block(children) => {
                    let mut new_children = Vec::new();
                    for &child in children {
                        if is_dead_definition(&current, child, &read_vars) {
                            changed = true;
                            continue;
                        }
                        new_children.push(id_map[usize::from(child)]);
                    }

                    match new_children.len() {
                        0 => {
                            changed = true;
                            new_nodes.add(Hir::Nop)
                        }
                        1 => {
                            changed = true;
                            new_children[0]
                        }
                        _ => new_nodes.add(Hir::Block(new_children.into_boxed_slice())),
                    }
                }
                Hir::Let([var, val, body]) => {
                    let mapped_val = id_map[usize::from(*val)];
                    let mapped_body = id_map[usize::from(*body)];

                    if is_dead_assignment(&current, *var, *val, &read_vars) {
                        changed = true;
                        mapped_body
                    } else {
                        let mapped_var = id_map[usize::from(*var)];
                        new_nodes.add(Hir::Let([mapped_var, mapped_val, mapped_body]))
                    }
                }
                Hir::VarDecl([name, val]) => {
                    let mapped_val = id_map[usize::from(*val)];
                    if is_dead_assignment(&current, *name, *val, &read_vars) {
                        changed = true;
                        new_nodes.add(Hir::Nop)
                    } else {
                        let mapped_name = id_map[usize::from(*name)];
                        new_nodes.add(Hir::VarDecl([mapped_name, mapped_val]))
                    }
                }
                Hir::Set([target, val]) => {
                    let mapped_val = id_map[usize::from(*val)];
                    if is_dead_assignment(&current, *target, *val, &read_vars) {
                        changed = true;
                        new_nodes.add(Hir::Nop)
                    } else {
                        let mapped_target = id_map[usize::from(*target)];
                        new_nodes.add(Hir::Set([mapped_target, mapped_val]))
                    }
                }
                _ => {
                    let mapped = node.clone().map_children(|c| id_map[usize::from(c)]);
                    new_nodes.add(mapped)
                }
            };

            id_map[usize::from(old_id)] = new_id;
        }

        let root_old_id = Id::from(current.as_ref().len() - 1);
        let root_new_id = id_map[usize::from(root_old_id)];
        if !new_nodes.is_empty() && usize::from(root_new_id) != new_nodes.len() - 1 {
            let root_node = new_nodes.as_slice()[usize::from(root_new_id)].clone();
            new_nodes.add(root_node);
        }

        current = new_nodes.into_recexpr();

        if !changed {
            break;
        }
    }

    current
}

#[derive(Debug, Clone, Default)]
pub struct DeadCodeEliminationPass;

impl FunctionPass<Hir> for DeadCodeEliminationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        eliminate_dead_code(expr)
    }
}
