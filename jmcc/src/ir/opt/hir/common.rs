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
