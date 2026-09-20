//! Depth-first traversal of a `RecExpr` subtree without visiting nodes twice.

use egg::{Id, Language, RecExpr};
use std::collections::HashSet;

/// Walks a subtree rooted at `root` depth-first, visiting each node at most once.
///
/// `f` is called with each node and its `Id`. If `f` returns `false`, traversal stops
/// immediately and returns `false`. Returns `true` if all visited nodes returned `true`.
pub fn walk_subtree<L: Language>(
    expr: &RecExpr<L>,
    root: Id,
    mut f: impl FnMut(&L, Id) -> bool,
) -> bool {
    let mut visited = HashSet::new();
    let mut stack = vec![root];

    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        if !f(&expr[id], id) {
            return false;
        }
        for child in expr[id].children() {
            stack.push(*child);
        }
    }

    true
}

/// Walks a subtree rooted at `root`, allowing `visit` to control which children to push onto the stack.
pub fn walk_subtree_with<L: Language>(
    expr: &RecExpr<L>,
    root: Id,
    mut visit: impl FnMut(&L, Id, &mut Vec<Id>),
) {
    let mut visited = HashSet::new();
    let mut stack = vec![root];

    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        visit(&expr[id], id, &mut stack);
    }
}
