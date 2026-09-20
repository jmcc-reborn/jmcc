//! Redundant Return Elimination pass: removes the final `return_func`
//! action at the end of a function body, as the runtime implicitly returns.

use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use egg::*;

#[must_use]
pub fn eliminate_redundant_return(expr: &RecExpr<Mir>) -> RecExpr<Mir> {
    if expr.is_empty() {
        return expr.clone();
    }

    // Not using NodeArena here: this is an in-place mutation of an existing graph copy
    // (`new_nodes[root_idx] = ...`). NodeArena intentionally does not provide mutable
    // indexing, so the buffer stays a Vec.
    let mut new_nodes: Vec<Mir> = expr.as_ref().to_vec();
    let root_idx = new_nodes.len() - 1;

    match &new_nodes[root_idx] {
        Mir::Block(children) => {
            if !children.is_empty() {
                let last_child_idx = usize::from(children[children.len() - 1]);

                if matches!(&new_nodes[last_child_idx], Mir::ReturnFunc(_))
                    && let Mir::Block(old_children) = new_nodes[root_idx].clone()
                {
                    let new_children = old_children[..old_children.len() - 1].to_vec();

                    if new_children.is_empty() {
                        new_nodes[root_idx] = Mir::Nop;
                    } else {
                        new_nodes[root_idx] = Mir::Block(new_children.into_boxed_slice());
                    }

                    log::trace!("[RedundantReturn] Removed trailing return_func from block.");
                    return RecExpr::from(new_nodes);
                }
            }
        }
        Mir::ReturnFunc(_) => {
            log::trace!("[RedundantReturn] Replaced single return_func body with Nop.");
            new_nodes[root_idx] = Mir::Nop;
            return RecExpr::from(new_nodes);
        }
        _ => {}
    }

    expr.clone()
}

#[derive(Debug, Clone, Default)]
pub struct RedundantReturnEliminationPass;

impl FunctionPass<Mir> for RedundantReturnEliminationPass {
    const OPT_LEVEL: u8 = 1;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        eliminate_redundant_return(expr)
    }
}
