//! Shared helpers for HIR optimization passes.

use crate::ir::VarName;
use crate::ir::hir::Hir;
use egg::{Id, RecExpr, Symbol};

/// Extracts the variable name from a wrapper node (`var` / `line` / `local` / `game` / `save`).
#[must_use]
pub fn get_var_name(expr: &RecExpr<Hir>, id: Id) -> Option<Symbol> {
    match &expr[id] {
        Hir::Var(VarName(name)) => Some(*name),
        Hir::Line(inner) | Hir::Local(inner) | Hir::Game(inner) | Hir::Save(inner) => {
            get_var_name(expr, *inner)
        }
        _ => None,
    }
}

/// Extracts the base variable name from a mutation target, unwrapping index or slice expressions.
#[must_use]
pub fn get_target_var_name(expr: &RecExpr<Hir>, id: Id) -> Option<Symbol> {
    match &expr[id] {
        Hir::Index([obj, _]) | Hir::Slice([obj, _, _]) => get_target_var_name(expr, *obj),
        Hir::Named([_, val]) => get_target_var_name(expr, *val),
        _ => get_var_name(expr, id),
    }
}

/// Same as [`get_var_name`], but over a slice of nodes before a `RecExpr` is finalized.
#[must_use]
pub fn get_var_name_in(nodes: &[Hir], id: Id) -> Option<Symbol> {
    match &nodes[usize::from(id)] {
        Hir::Var(VarName(name)) => Some(*name),
        Hir::Line(inner) | Hir::Local(inner) | Hir::Game(inner) | Hir::Save(inner) => {
            get_var_name_in(nodes, *inner)
        }
        _ => None,
    }
}

/// Same as [`get_target_var_name`], but over a slice of nodes before a `RecExpr` is finalized.
#[must_use]
pub fn get_target_var_name_in(nodes: &[Hir], id: Id) -> Option<Symbol> {
    match &nodes[usize::from(id)] {
        Hir::Index([obj, _]) | Hir::Slice([obj, _, _]) => get_target_var_name_in(nodes, *obj),
        Hir::Named([_, val]) => get_target_var_name_in(nodes, *val),
        _ => get_var_name_in(nodes, id),
    }
}

/// Checks if a variable name is a compiler temporary by prefix.
#[must_use]
pub fn is_temp_var(name: &str) -> bool {
    name.starts_with("__ht") || name.starts_with("__t") || name.starts_with("__inl_")
}

/// Extracts the string symbol from a `Hir::Str` node.
#[must_use]
pub fn string_lit(expr: &RecExpr<Hir>, id: Id) -> Option<Symbol> {
    match &expr[id] {
        Hir::Str(s) => Some(s.0),
        _ => None,
    }
}

/// Collects pairs of `FuncDecl`/`ProcDecl` in the module: name -> (`decl_node_id`, `body_node_id`).
#[must_use]
pub fn collect_declarations(expr: &RecExpr<Hir>) -> Vec<(Symbol, (Id, Id))> {
    let mut decls = Vec::new();
    for (i, node) in expr.as_ref().iter().enumerate() {
        if let Hir::FuncDecl([name_id, _, body_id]) | Hir::ProcDecl([name_id, _, body_id]) = node
            && let Some(name) = string_lit(expr, *name_id)
        {
            decls.push((name, (Id::from(i), *body_id)));
        }
    }
    decls
}

/// Collects the variables written by an action through its `variable`-typed arguments.
pub fn collect_action_written_vars(
    expr: &RecExpr<Hir>,
    ids: &[Id],
    out: &mut std::collections::HashSet<Symbol>,
) {
    let (Some(object), Some(name)) = (string_lit(expr, ids[0]), string_lit(expr, ids[1])) else {
        return;
    };
    let Some(def) = jmcdata::generated::get_action_def(object.as_str(), name.as_str()) else {
        return;
    };
    if def.id.starts_with("if_") || def.boolean {
        return;
    }
    let Some(&args_id) = ids.get(3) else {
        return;
    };
    let Hir::List(arg_ids) = &expr[args_id] else {
        return;
    };
    let mut used_params = std::collections::HashSet::new();
    let mut positional_idx = 0;
    for &arg_id in arg_ids {
        let (arg_name, val_id) = match &expr[arg_id] {
            Hir::Named([n, v]) => (string_lit(expr, *n).map(|s| s.to_string()), *v),
            _ => (None, arg_id),
        };
        let target_arg = crate::utils::resolve_action_arg_def(
            arg_name.as_deref(),
            Some(def),
            &mut used_params,
            &mut positional_idx,
        );
        if let Some(arg) = target_arg
            && arg.arg_type == "variable"
            && let Some(var) = get_target_var_name(expr, val_id)
        {
            out.insert(var);
        }
    }
}
