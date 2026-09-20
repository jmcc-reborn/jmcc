//! IR node arena storage.
//!
//! Provides a dense container for [`egg::RecExpr`] nodes where the node index
//! corresponds to its [`egg::Id`].

use egg::{Id, Language, RecExpr};
use tracing::trace;

/// Dense storage for `RecExpr` nodes: the index in `Vec` is its [`Id`].
pub struct NodeArena<L> {
    nodes: Vec<L>,
}

impl<L: Language> NodeArena<L> {
    /// Creates a new arena with capacity for `n` nodes.
    #[must_use]
    pub fn with_capacity(n: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(n),
        }
    }

    #[must_use]
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Adds a node and returns its [`Id`].
    #[inline(always)]
    pub fn add(&mut self, node: L) -> Id {
        let id = Id::from(self.nodes.len());
        self.nodes.push(node);
        id
    }

    /// Adds a node, tagging it in trace logging.
    #[inline(always)]
    pub fn add_tagged(&mut self, tag: &str, node: L) -> Id {
        let id = self.add(node);
        trace!(?id, tag, node = ?&self.nodes[usize::from(id)], "arena add");
        id
    }

    #[must_use]
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Truncates the arena to the given length.
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        self.nodes.truncate(len);
    }

    pub fn iter(&self) -> impl Iterator<Item = &L> {
        self.nodes.iter()
    }

    /// Returns a slice of all nodes in index order.
    #[must_use]
    #[inline(always)]
    pub fn as_slice(&self) -> &[L] {
        &self.nodes
    }

    /// Consumes the arena and converts it into a [`RecExpr`].
    #[must_use]
    pub fn into_recexpr(mut self) -> RecExpr<L> {
        self.nodes.shrink_to_fit();
        RecExpr::from(self.nodes)
    }
}

impl<L: Language> Default for NodeArena<L> {
    fn default() -> Self {
        Self::new()
    }
}
