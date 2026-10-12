//! `LoopFusionPass`: merges adjacent loops with identical trip counts and bounds.
//!
//! Handles:
//! - Adjacent `while` loops with identical loop bounds/conditions and induction variable updates.
//! - Adjacent `repeat::on_range` actions over the same range.
//! - Adjacent `repeat::for_each_in_list` actions over the same list.

use super::common::{collect_action_written_vars, get_target_var_name, get_var_name};
use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use crate::ir::walk::walk_subtree;
use egg::*;
use std::collections::HashSet;

/// Checks whether an expression in HIR is pure (has no side effects or control flow).
fn is_pure_hir(expr: &RecExpr<Hir>, id: Id) -> bool {
    let mut pure = true;
    walk_subtree(expr, id, |node, _| {
        if matches!(
            node,
            Hir::Action(_)
                | Hir::FuncCall(_)
                | Hir::ProcCall(_)
                | Hir::While(_)
                | Hir::If(_)
                | Hir::Break
                | Hir::Continue
                | Hir::Return(_)
                | Hir::FuncDecl(_)
                | Hir::ProcDecl(_)
                | Hir::EventDecl(_)
        ) {
            pure = false;
            false
        } else {
            true
        }
    });
    pure
}

fn structural_eq(expr: &RecExpr<Hir>, a: Id, b: Id) -> bool {
    if a == b {
        return true;
    }
    let (na, nb) = (&expr[a], &expr[b]);
    if std::mem::discriminant(na) != std::mem::discriminant(nb) {
        return false;
    }
    let (ca, cb) = (na.children(), nb.children());
    if ca.len() != cb.len() {
        return false;
    }
    if ca.is_empty() {
        return na == nb;
    }
    ca.iter().zip(cb).all(|(&x, &y)| structural_eq(expr, x, y))
}

/// Collects all variable names written (assigned) within a HIR subtree.
fn collect_written_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |node, _| {
        match node {
            Hir::VarDecl([target, _])
            | Hir::Set([target, _])
            | Hir::Inc(target)
            | Hir::Dec(target) => {
                if let Some(name) = get_target_var_name(expr, *target) {
                    out.insert(name);
                }
            }
            Hir::Action(ids) => {
                collect_action_written_vars(expr, ids, out);
            }
            _ => {}
        }
        true
    });
}

/// Collects all variable names read within a HIR subtree.
fn collect_read_vars(expr: &RecExpr<Hir>, root: Id, out: &mut HashSet<Symbol>) {
    walk_subtree(expr, root, |_node, id| {
        if let Some(name) = get_var_name(expr, id) {
            out.insert(name);
        }
        true
    });
}

/// Unpacks a block into a flat list of statement IDs.
fn unpack_block(expr: &RecExpr<Hir>, id: Id) -> Vec<Id> {
    match &expr[id] {
        Hir::Block(stmts) => stmts.to_vec(),
        Hir::Nop => Vec::new(),
        _ => vec![id],
    }
}

/// Checks if a statement is an induction variable increment `var = var + 1` or `var += 1`.
fn is_var_increment(expr: &RecExpr<Hir>, stmt_id: Id, var_sym: Symbol) -> bool {
    match &expr[stmt_id] {
        Hir::Set([target, val]) => {
            let Some(tgt_name) = get_target_var_name(expr, *target) else {
                return false;
            };
            if tgt_name != var_sym {
                return false;
            }
            match &expr[*val] {
                Hir::Add([left, _right]) => get_var_name(expr, *left).is_some_and(|n| n == var_sym),
                _ => false,
            }
        }
        Hir::Inc(target) => get_target_var_name(expr, *target).is_some_and(|n| n == var_sym),
        _ => false,
    }
}

/// Checks if a statement is a variable reset `var = 0` (or similar initial value).
fn is_var_reset(expr: &RecExpr<Hir>, stmt_id: Id, var_sym: Symbol) -> bool {
    match &expr[stmt_id] {
        Hir::Set([target, _val]) | Hir::VarDecl([target, _val]) => {
            get_target_var_name(expr, *target).is_some_and(|n| n == var_sym)
        }
        _ => false,
    }
}

/// Extracts loop induction variable symbol from a condition like `i < n` or `i <= n`.
fn extract_induction_var(expr: &RecExpr<Hir>, cond_id: Id) -> Option<Symbol> {
    match &expr[cond_id] {
        Hir::Lt([left, _]) | Hir::Le([left, _]) => get_var_name(expr, *left),
        _ => None,
    }
}

fn has_control_flow(expr: &RecExpr<Hir>, root: Id) -> bool {
    let mut found = false;
    walk_subtree(expr, root, |node, _| {
        if matches!(node, Hir::Break | Hir::Continue | Hir::Return(_)) {
            found = true;
            false
        } else {
            true
        }
    });
    found
}

/// Tries to fuse two while loops:
/// `while i < n { body1; i += 1; }`
/// `[i = 0;]`
/// `while i < n { body2; i += 1; }`
/// into:
/// `while i < n { body1; body2; i += 1; }`
fn try_fuse_while_loops(
    expr: &RecExpr<Hir>,
    w1: Id,
    reset_stmt: Option<Id>,
    w2: Id,
    id_map: &[Id],
    nn: &mut NodeArena<Hir>,
) -> Option<Id> {
    let (cond1, body1_id) = match &expr[w1] {
        Hir::While([c, b]) => (*c, *b),
        _ => return None,
    };
    let (cond2, body2_id) = match &expr[w2] {
        Hir::While([c, b]) => (*c, *b),
        _ => return None,
    };

    if !structural_eq(expr, cond1, cond2) || !is_pure_hir(expr, cond1) {
        return None;
    }

    let ind_var = extract_induction_var(expr, cond1)?;

    if let Some(reset) = reset_stmt
        && !is_var_reset(expr, reset, ind_var)
    {
        return None;
    }

    if has_control_flow(expr, body1_id) || has_control_flow(expr, body2_id) {
        return None;
    }

    let body1_stmts = unpack_block(expr, body1_id);
    let body2_stmts = unpack_block(expr, body2_id);

    if body1_stmts.is_empty() || body2_stmts.is_empty() {
        return None;
    }

    let last1 = *body1_stmts.last()?;
    let last2 = *body2_stmts.last()?;

    if !is_var_increment(expr, last1, ind_var) || !is_var_increment(expr, last2, ind_var) {
        return None;
    }

    let body1_core = &body1_stmts[..body1_stmts.len() - 1];
    let body2_core = &body2_stmts[..body2_stmts.len() - 1];

    // Check data dependencies: body1 must not write variables that body2 depends on in conflicting ways.
    let mut written1 = HashSet::new();
    let mut read1 = HashSet::new();
    for &s in body1_core {
        collect_written_vars(expr, s, &mut written1);
        collect_read_vars(expr, s, &mut read1);
    }

    let mut read2 = HashSet::new();
    let mut written2 = HashSet::new();
    for &s in body2_core {
        collect_read_vars(expr, s, &mut read2);
        collect_written_vars(expr, s, &mut written2);
    }

    // Remove induction variable from written and read sets
    written1.remove(&ind_var);
    written2.remove(&ind_var);
    read1.remove(&ind_var);
    read2.remove(&ind_var);

    if !written1.is_disjoint(&read2)
        || !written2.is_disjoint(&read1)
        || !written1.is_disjoint(&written2)
    {
        return None;
    }

    // Build fused body: body1_core + body2_core + last1 (the increment)
    let mut fused_stmts = Vec::new();
    for &s in body1_core {
        fused_stmts.push(id_map[usize::from(s)]);
    }
    for &s in body2_core {
        fused_stmts.push(id_map[usize::from(s)]);
    }
    fused_stmts.push(id_map[usize::from(last1)]);

    let fused_body = nn.add(Hir::Block(fused_stmts.into_boxed_slice()));
    let mapped_cond = id_map[usize::from(cond1)];

    Some(nn.add(Hir::While([mapped_cond, fused_body])))
}

fn extract_loop_var(expr: &RecExpr<Hir>, id: Id) -> Option<Symbol> {
    match &expr[id] {
        Hir::Var(v) => Some(v.0),
        Hir::Line(i) | Hir::Local(i) => match &expr[*i] {
            Hir::Var(v) => Some(v.0),
            _ => None,
        },
        _ => None,
    }
}

fn check_repeat_dependencies(
    expr: &RecExpr<Hir>,
    body1: Id,
    body2: Id,
    bounds_id: Id,
    loop_var: Option<Symbol>,
) -> bool {
    let mut written1 = HashSet::new();
    collect_written_vars(expr, body1, &mut written1);

    let mut bounds_vars = HashSet::new();
    collect_read_vars(expr, bounds_id, &mut bounds_vars);

    if bounds_vars.iter().any(|b| written1.contains(b)) {
        return false;
    }

    let mut read1 = HashSet::new();
    collect_read_vars(expr, body1, &mut read1);

    let mut written2 = HashSet::new();
    collect_written_vars(expr, body2, &mut written2);

    let mut read2 = HashSet::new();
    collect_read_vars(expr, body2, &mut read2);

    if let Some(lv) = loop_var {
        written1.remove(&lv);
        written2.remove(&lv);
        read1.remove(&lv);
        read2.remove(&lv);
    }

    written1.is_disjoint(&read2) && written2.is_disjoint(&read1) && written1.is_disjoint(&written2)
}

/// Tries to fuse two repeat actions (such as `repeat::on_range` or `repeat::for_each_in_list`).
fn try_fuse_repeat_actions(
    expr: &RecExpr<Hir>,
    act1: Id,
    act2: Id,
    id_map: &[Id],
    nn: &mut NodeArena<Hir>,
) -> Option<Id> {
    let ids1 = match &expr[act1] {
        Hir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };
    let ids2 = match &expr[act2] {
        Hir::Action(ids) if ids.len() >= 7 => ids,
        _ => return None,
    };

    let obj1 = match &expr[ids1[0]] {
        Hir::Str(s) => s.0.as_str(),
        _ => return None,
    };
    let obj2 = match &expr[ids2[0]] {
        Hir::Str(s) => s.0.as_str(),
        _ => return None,
    };
    if obj1 != "repeat" || obj2 != "repeat" {
        return None;
    }

    let name1 = match &expr[ids1[1]] {
        Hir::Str(s) => s.0.as_str(),
        _ => return None,
    };
    let name2 = match &expr[ids2[1]] {
        Hir::Str(s) => s.0.as_str(),
        _ => return None,
    };
    if name1 != name2 {
        return None;
    }

    // Matching selectors, conditions
    if !structural_eq(expr, ids1[2], ids2[2]) || !structural_eq(expr, ids1[6], ids2[6]) {
        return None;
    }

    // Matching loop arguments (range or list) and loop variable (lambda)
    if !structural_eq(expr, ids1[3], ids2[3]) || !structural_eq(expr, ids1[5], ids2[5]) {
        return None;
    }

    let body1 = ids1[4];
    let body2 = ids2[4];

    if has_control_flow(expr, body1) || has_control_flow(expr, body2) {
        return None;
    }

    let loop_var = extract_loop_var(expr, ids1[5]);
    if !check_repeat_dependencies(expr, body1, body2, ids1[3], loop_var) {
        return None;
    }

    // Combine bodies
    let body1_stmts = unpack_block(expr, body1);
    let body2_stmts = unpack_block(expr, body2);

    let mut combined = Vec::new();
    for &s in &body1_stmts {
        combined.push(id_map[usize::from(s)]);
    }
    for &s in &body2_stmts {
        combined.push(id_map[usize::from(s)]);
    }
    let fused_body = nn.add(Hir::Block(combined.into_boxed_slice()));

    let mapped_obj = id_map[usize::from(ids1[0])];
    let mapped_name = id_map[usize::from(ids1[1])];
    let mapped_sel = id_map[usize::from(ids1[2])];
    let mapped_args = id_map[usize::from(ids1[3])];
    let mapped_lambda = id_map[usize::from(ids1[5])];
    let mapped_cond = id_map[usize::from(ids1[6])];

    Some(
        nn.add(Hir::Action(
            vec![
                mapped_obj,
                mapped_name,
                mapped_sel,
                mapped_args,
                fused_body,
                mapped_lambda,
                mapped_cond,
            ]
            .into_boxed_slice(),
        )),
    )
}

#[derive(Debug, Clone, Default)]
pub struct LoopFusionPass;

impl FunctionPass<Hir> for LoopFusionPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::Block(children) = node {
                let mut new_children = Vec::with_capacity(children.len());
                let mut idx = 0;

                while idx < children.len() {
                    let child = children[idx];

                    // 1. Try fusing two while loops with an optional reset statement in between.
                    if idx + 2 < children.len() {
                        let mid = children[idx + 1];
                        let next = children[idx + 2];
                        if let Some(fused) = try_fuse_while_loops(
                            expr,
                            child,
                            Some(mid),
                            next,
                            &id_map,
                            &mut new_nodes,
                        ) {
                            new_children.push(fused);
                            idx += 3;
                            continue;
                        }
                    }

                    // 2. Try fusing two while loops immediately adjacent.
                    if idx + 1 < children.len() {
                        let next = children[idx + 1];
                        if let Some(fused) =
                            try_fuse_while_loops(expr, child, None, next, &id_map, &mut new_nodes)
                        {
                            new_children.push(fused);
                            idx += 2;
                            continue;
                        }

                        // 3. Try fusing two repeat actions.
                        if let Some(fused) =
                            try_fuse_repeat_actions(expr, child, next, &id_map, &mut new_nodes)
                        {
                            new_children.push(fused);
                            idx += 2;
                            continue;
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
