use std::borrow::Cow;

use litemap::LiteMap;

use crate::{
    generated::ActionId,
    module::{Conditional, Op, Selection, Value},
};

pub struct OpBuilder<'a> {
    action: ActionId,
    values: LiteMap<Cow<'a, str>, Value<'a>>,
    operations: Option<Vec<Op<'a>>>,
    conditional: Option<Conditional>,
    selection: Option<Selection<'a>>,
}

impl<'a> OpBuilder<'a> {
    #[inline]
    #[must_use]
    pub const fn new(action: ActionId) -> Self {
        Self {
            action,
            values: LiteMap::new(),
            operations: None,
            conditional: None,
            selection: None,
        }
    }

    #[inline]
    pub fn with_value<K: Into<Cow<'a, str>>, V: Into<Value<'a>>>(
        mut self,
        key: K,
        value: V,
    ) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    #[inline]
    #[must_use]
    pub fn with_operations(mut self, operations: Vec<Op<'a>>) -> Self {
        self.operations = Some(operations);
        self
    }

    #[inline]
    #[must_use]
    pub const fn with_conditional(mut self, conditional: Conditional) -> Self {
        self.conditional = Some(conditional);
        self
    }

    #[inline]
    #[must_use]
    pub fn with_selection(mut self, selection: Selection<'a>) -> Self {
        self.selection = Some(selection);
        self
    }

    #[inline]
    #[must_use]
    pub fn build(self) -> Op<'a> {
        Op {
            action: self.action,
            values: self.values,
            operations: self.operations,
            conditional: self.conditional,
            selection: self.selection,
            is_inverted: None,
        }
    }
}
