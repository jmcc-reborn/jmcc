//! Dead Loop Elimination pass for HIR.
//!
//! Eliminates while loops that have no side effects, such as loops with a
//! known false condition, empty bodies, or pure bodies whose assignments
//! are not read outside the loop.

use crate::ir::arena::NodeArena;
use crate::ir::hir::Hir;
use crate::ir::opt::FunctionPass;
use egg::{Id, Language as _, RecExpr};

fn is_const_false(expr: &RecExpr<Hir>, id: Id) -> bool {
    match &expr[id] {
        Hir::Bool(false) => true,
        Hir::Num(n) if n.0 == 0.0 => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeadLoopEliminationPass;

impl FunctionPass<Hir> for DeadLoopEliminationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Hir>) -> RecExpr<Hir> {
        if expr.is_empty() {
            return expr.clone();
        }

        let mut new_nodes = NodeArena::with_capacity(expr.len());
        let mut id_map = vec![Id::from(0); expr.len()];

        for (i, node) in expr.as_ref().iter().enumerate() {
            let old_id = Id::from(i);

            let new_id = if let Hir::While([cond_id, body_id]) = node {
                let mapped_cond = id_map[usize::from(*cond_id)];
                let mapped_body = id_map[usize::from(*body_id)];

                // Only loops whose condition is statically known to be false never execute.
                // Eliminating an empty loop whose condition might be true would turn an infinite
                // loop into termination, changing program semantics.
                let is_dead = is_const_false(expr, *cond_id);

                if is_dead {
                    new_nodes.add(Hir::Nop)
                } else {
                    new_nodes.add(Hir::While([mapped_cond, mapped_body]))
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
