//! Shared helpers for MIR optimization passes.

use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::walk::walk_subtree;
use egg::{Id, RecExpr, Symbol};
use std::collections::HashSet;

/// Returns true if the node is a string literal equal to `expected`.
#[must_use]
pub fn node_str_eq(node: &Mir, expected: &str) -> bool {
    matches!(node, Mir::Str(s) if s.0.as_str() == expected)
}

/// Extracts variable name from a variable wrapper node (`var` / `local` / `game` / `save` / `line`).
#[must_use]
pub fn get_variable_name(expr: &RecExpr<Mir>, id: Id) -> Option<Symbol> {
    match &expr[id] {
        Mir::Var(v) => Some(v.0),
        Mir::Local(i) | Mir::Game(i) | Mir::Save(i) | Mir::Line(i) => get_variable_name(expr, *i),
        _ => None,
    }
}

/// Variable scope representation in MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarScope {
    /// Bare variable without scope wrapper.
    Bare,
    /// Line variable scope.
    Line,
    /// Local variable scope.
    Local,
    /// Game variable scope.
    Game,
    /// Save variable scope.
    Save,
}

/// Returns the scoped variable name and its scope.
#[must_use]
pub fn get_scoped_var_name(expr: &RecExpr<Mir>, id: Id) -> Option<(VarScope, Symbol)> {
    match &expr[id] {
        Mir::Var(v) => Some((VarScope::Bare, v.0)),
        Mir::Line(i) => Some((VarScope::Line, get_variable_name(expr, *i)?)),
        Mir::Local(i) => Some((VarScope::Local, get_variable_name(expr, *i)?)),
        Mir::Game(i) => Some((VarScope::Game, get_variable_name(expr, *i)?)),
        Mir::Save(i) => Some((VarScope::Save, get_variable_name(expr, *i)?)),
        _ => None,
    }
}

/// Checks if two nodes reference the same variable, including scope.
#[must_use]
pub fn same_variable(expr: &RecExpr<Mir>, first: Id, second: Id) -> bool {
    match (
        get_scoped_var_name(expr, first),
        get_scoped_var_name(expr, second),
    ) {
        (Some(first), Some(second)) => first == second,
        _ => false,
    }
}

/// Checks if a variable name is a compiler temporary by prefix.
#[must_use]
pub fn is_temp_var(name: &str, extra_prefixes: &[&str]) -> bool {
    name.starts_with("__ht")
        || name.starts_with("__t")
        || name.starts_with("__inl_")
        || extra_prefixes.iter().any(|p| name.starts_with(p))
}

/// Strips inlining prefixes: `__inl_foo` -> `foo`.
#[must_use]
pub fn strip_inline_prefixes(name: &str) -> &str {
    let mut rest = name;
    while let Some(stripped) = rest.strip_prefix("__inl_") {
        rest = stripped;
    }
    rest
}

/// Checks if a variable is a compiler temporary rather than an inlined user variable.
#[must_use]
pub fn is_plain_temp(name: &str, extra_prefixes: &[&str]) -> bool {
    let rest = strip_inline_prefixes(name);
    rest.starts_with("__ht")
        || rest.starts_with("__t")
        || extra_prefixes.iter().any(|p| rest.starts_with(p))
}

/// Checks if a node is a `variable::set_value` action.
#[must_use]
pub fn is_set_value_action(expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Action(ids) = &expr[id]
        && ids.len() >= 4
    {
        return node_str_eq(&expr[ids[0]], "variable") && node_str_eq(&expr[ids[1]], "set_value");
    }
    false
}

/// Returns the arguments of an action.
#[must_use]
pub fn action_args(expr: &RecExpr<Mir>, id: Id) -> Option<&[Id]> {
    if let Mir::Action(ids) = &expr[id]
        && ids.len() >= 4
        && let Mir::List(args) = &expr[ids[3]]
    {
        return Some(args);
    }
    None
}

/// Extracts named arguments `variable` and `value` from an action.
#[must_use]
pub fn extract_var_value_args(expr: &RecExpr<Mir>, id: Id) -> Option<(Id, Id)> {
    let args = action_args(expr, id)?;
    let mut var_id = None;
    let mut val_id = None;
    for &arg_id in args {
        if let Mir::Named([name_id, value_id]) = &expr[arg_id] {
            if node_str_eq(&expr[*name_id], "variable") {
                var_id = Some(*value_id);
            } else if node_str_eq(&expr[*name_id], "value") {
                val_id = Some(*value_id);
            }
        }
    }
    if let (Some(v), Some(val)) = (var_id, val_id) {
        return Some((v, val));
    }
    None
}

/// Checks if a subtree references any variable in `vars`.
#[must_use]
pub fn references_any_variable(expr: &RecExpr<Mir>, root: Id, vars: &HashSet<String>) -> bool {
    !walk_subtree(expr, root, |_, id| {
        get_variable_name(expr, id).is_none_or(|name| !vars.contains(name.as_str()))
    })
}

/// Re-emits a variable wrapper chain in `new_nodes` mapping children through `id_map`.
pub fn clone_var_node(
    expr: &RecExpr<Mir>,
    id: Id,
    new_nodes: &mut NodeArena<Mir>,
    id_map: &[Id],
) -> Id {
    let node = expr[id].clone();
    match node {
        Mir::Var(v) => new_nodes.add(Mir::Var(v)),
        Mir::Line(inner) => {
            let new_inner = clone_var_node(expr, inner, new_nodes, id_map);
            new_nodes.add(Mir::Line(new_inner))
        }
        Mir::Local(inner) => {
            let new_inner = clone_var_node(expr, inner, new_nodes, id_map);
            new_nodes.add(Mir::Local(new_inner))
        }
        Mir::Game(inner) => {
            let new_inner = clone_var_node(expr, inner, new_nodes, id_map);
            new_nodes.add(Mir::Game(new_inner))
        }
        Mir::Save(inner) => {
            let new_inner = clone_var_node(expr, inner, new_nodes, id_map);
            new_nodes.add(Mir::Save(new_inner))
        }
        _ => id_map[usize::from(id)],
    }
}
