//! Redundant Else Elimination pass: removes `else` branches that set a variable to 0
//! when the code is not inside a loop, as line variables are reset to 0 on function entry.

use super::common::{is_set_value_action, node_str_eq};
use crate::ir::arena::NodeArena;
use crate::ir::mir::Mir;
use crate::ir::opt::FunctionPass;
use egg::*;
use std::collections::HashSet;

fn is_else_action(expr: &RecExpr<Mir>, id: Id) -> bool {
    if let Mir::Action(ids) = &expr[id] {
        ids.len() >= 4 && node_str_eq(&expr[ids[0]], "code") && node_str_eq(&expr[ids[1]], "else")
    } else {
        false
    }
}

fn is_set_zero_in_else(expr: &RecExpr<Mir>, else_id: Id) -> bool {
    let Mir::Action(ids) = &expr[else_id] else {
        return false;
    };
    if ids.len() < 5 {
        return false;
    }
    let Mir::Block(ops) = &expr[ids[4]] else {
        return false;
    };
    if ops.len() != 1 || !is_set_value_action(expr, ops[0]) {
        return false;
    }
    let Mir::Action(set_ids) = &expr[ops[0]] else {
        return false;
    };
    let Mir::List(args) = &expr[set_ids[3]] else {
        return false;
    };

    args.iter()
        .find_map(|&arg_id| match &expr[arg_id] {
            Mir::Named([name_id, value_id]) if node_str_eq(&expr[*name_id], "value") => {
                Some(*value_id)
            }
            _ => None,
        })
        .is_some_and(|value_id| matches!(&expr[value_id], Mir::Num(n) if n.0 == 0.0))
}

struct ElseEliminator<'a> {
    expr: &'a RecExpr<Mir>,
    new_nodes: NodeArena<Mir>,
    id_map: Vec<Id>,
    loop_depth: u32,
}

impl<'a> ElseEliminator<'a> {
    fn new(expr: &'a RecExpr<Mir>) -> Self {
        Self {
            expr,
            new_nodes: NodeArena::with_capacity(expr.len()),
            id_map: vec![Id::from(0); expr.len()],
            loop_depth: 0,
        }
    }

    fn visit(&mut self, id: Id) -> Id {
        let node = &self.expr[id];

        // New frames (functions, processes, events) reset loop_depth
        if matches!(
            node,
            Mir::FuncDecl(_) | Mir::ProcDecl(_) | Mir::EventDecl(_)
        ) {
            let saved_depth = self.loop_depth;
            self.loop_depth = 0;
            let new_id = self.visit_node(id);
            self.loop_depth = saved_depth;
            return new_id;
        }

        // Repeat actions increase loop_depth for their execution block (ids[4])
        if let Mir::Action(ids) = node
            && ids.len() >= 5
            && node_str_eq(&self.expr[ids[0]], "repeat")
        {
            let mut new_ids = Vec::with_capacity(ids.len());
            for (i, &child) in ids.iter().enumerate() {
                if i == 4 {
                    self.loop_depth += 1;
                    new_ids.push(self.visit(child));
                    self.loop_depth -= 1;
                } else {
                    new_ids.push(self.visit(child));
                }
            }
            let new_node = Mir::Action(new_ids.into_boxed_slice());
            let new_id = self.new_nodes.add(new_node);
            self.id_map[usize::from(id)] = new_id;
            return new_id;
        }

        self.visit_node(id)
    }

    fn visit_node(&mut self, id: Id) -> Id {
        let node = &self.expr[id];
        let old_id = id;

        let new_id = if let Mir::Block(children) = node {
            let mut new_children = Vec::with_capacity(children.len());
            let mut to_remove: HashSet<Id> = HashSet::new();

            // An `else` that resets a variable to zero is dead unless a loop
            // can re-enter it, and only when an if-action precedes it.
            if self.loop_depth == 0 {
                for (i, &child) in children.iter().enumerate() {
                    if i == 0
                        || !is_else_action(self.expr, child)
                        || !is_set_zero_in_else(self.expr, child)
                    {
                        continue;
                    }
                    let Mir::Action(prev_ids) = &self.expr[children[i - 1]] else {
                        continue;
                    };
                    if prev_ids.len() >= 4 && node_str_eq(&self.expr[prev_ids[0]], "variable") {
                        to_remove.insert(child);
                    }
                }
            }

            for &child in children {
                if to_remove.contains(&child) {
                    log::trace!("[ElseElim] Removing redundant else at {child:?}");
                    continue;
                }
                let mapped_child = self.visit(child);
                new_children.push(mapped_child);
            }

            match new_children.len() {
                0 => self.new_nodes.add(Mir::Nop),
                1 => new_children[0],
                _ => self
                    .new_nodes
                    .add(Mir::Block(new_children.into_boxed_slice())),
            }
        } else {
            let mapped = node.clone().map_children(|c| self.visit(c));
            self.new_nodes.add(mapped)
        };

        self.id_map[usize::from(old_id)] = new_id;
        new_id
    }

    fn build(self) -> RecExpr<Mir> {
        self.new_nodes.into_recexpr()
    }
}

#[must_use]
pub fn eliminate_redundant_else(expr: &RecExpr<Mir>) -> RecExpr<Mir> {
    if expr.is_empty() {
        return expr.clone();
    }

    log::trace!("=== Redundant Else Elimination Start ===");
    log::trace!("Input expr ({} nodes): {expr:?}", expr.len());

    let mut eliminator = ElseEliminator::new(expr);
    let root = expr.root();
    let _new_root = eliminator.visit(root);
    let result = eliminator.build();

    log::trace!("Output expr ({} nodes): {result:?}", result.len());
    log::trace!("=== Redundant Else Elimination End ===");

    result
}

#[derive(Debug, Clone)]
pub struct RedundantElseEliminationPass {
    pub edition: u16,
}

impl Default for RedundantElseEliminationPass {
    fn default() -> Self {
        Self { edition: 2026 }
    }
}

impl FunctionPass<Mir> for RedundantElseEliminationPass {
    const OPT_LEVEL: u8 = 2;

    fn run(&self, expr: &RecExpr<Mir>) -> RecExpr<Mir> {
        if self.edition < 2026 {
            return expr.clone();
        }
        eliminate_redundant_else(expr)
    }
}
