use crate::ir::hir::Hir;
use crate::ir::walk::walk_subtree;
use egg::{Id, RecExpr, Symbol};
use std::collections::{HashMap, HashSet};

use super::advisor::FuncInfo;
use super::cost::ConstValue;

#[must_use]
pub fn extract_name(expr: &RecExpr<Hir>, id: Id) -> Option<String> {
    let node = &expr[id];
    log::trace!("extract_name: id={id:?}, node={node:?}");
    match node {
        Hir::Var(name) => Some(name.0.to_string()),
        Hir::Str(s) => Some(s.0.to_string()),
        Hir::Sel(inner) => extract_name(expr, *inner),
        _ => None,
    }
}

#[must_use]
pub fn extract_var_name(expr: &RecExpr<Hir>, id: Id) -> Option<Symbol> {
    let node = &expr[id];
    log::trace!("extract_var_name: id={id:?}, node={node:?}");
    match node {
        Hir::Var(name) => Some(name.0),
        Hir::Line(i) | Hir::Game(i) | Hir::Save(i) | Hir::Local(i) => extract_var_name(expr, *i),
        _ => None,
    }
}

#[must_use]
pub fn extract_args(expr: &RecExpr<Hir>, id: Id) -> Vec<Id> {
    let node = &expr[id];
    log::trace!("extract_args: id={id:?}, node={node:?}");
    match node {
        Hir::List(ids) => ids.to_vec(),
        _ => Vec::new(),
    }
}

#[must_use]
pub fn extract_params(expr: &RecExpr<Hir>, id: Id) -> Vec<String> {
    let node = &expr[id];
    log::trace!("extract_params: id={id:?}, node={node:?}");
    match node {
        Hir::List(ids) => ids
            .iter()
            .filter_map(|&i| match &expr[i] {
                Hir::Var(name) => Some(name.0.to_string()),
                Hir::Named([name_id, _]) => extract_name(expr, *name_id),
                Hir::List(inner_ids) => {
                    let name_id = *inner_ids.first()?;
                    extract_name(expr, name_id)
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[must_use]
pub fn eval_const_arg(expr: &RecExpr<Hir>, id: Id) -> Option<ConstValue> {
    let node = &expr[id];
    log::trace!("eval_const_arg: id={id:?}, node={node:?}");
    match node {
        Hir::Num(n) => Some(ConstValue::Number(n.0)),
        Hir::Bool(b) => Some(ConstValue::Bool(*b)),
        Hir::Str(s) => Some(ConstValue::Text(s.0.to_string())),
        _ => None,
    }
}

pub fn collect_calls_in_subtree(
    expr: &RecExpr<Hir>,
    root: Id,
    func_table: &HashMap<String, FuncInfo>,
    call_counts: &mut HashMap<String, usize>,
    callees: &mut HashSet<String>,
) {
    log::trace!("collect_calls_in_subtree: starting from root={root:?}");

    walk_subtree(expr, root, |node, id| {
        if let Hir::FuncCall([target, _args]) = node
            && let Some(name) = extract_name(expr, *target)
            && func_table.contains_key(&name)
        {
            log::trace!("  Found call to '{name}' at id={id:?}");
            *call_counts.entry(name.clone()).or_insert(0) += 1;
            callees.insert(name);
        }
        true
    });
}

#[must_use]
pub fn detect_recursive(
    call_graph: &HashMap<String, HashSet<String>>,
    func_table: &HashMap<String, FuncInfo>,
) -> HashSet<String> {
    log::trace!("detect_recursive: analyzing call graph...");
    let mut recursive = HashSet::new();

    for name in func_table.keys() {
        let mut visited = HashSet::new();
        if can_reach(name, name, call_graph, &mut visited) {
            log::trace!("  Function '{name}' is recursive");
            recursive.insert(name.clone());
        }
    }

    recursive
}

fn can_reach(
    start: &str,
    current: &str,
    call_graph: &HashMap<String, HashSet<String>>,
    visited: &mut HashSet<String>,
) -> bool {
    if let Some(callees) = call_graph.get(current) {
        for callee in callees {
            if callee == start {
                return true;
            }
            if !visited.contains(callee) {
                visited.insert(callee.clone());
                if can_reach(start, callee, call_graph, visited) {
                    return true;
                }
            }
        }
    }
    false
}
