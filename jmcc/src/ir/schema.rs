//! Facade over the `JustMC` schema from `jmcdata`.

use crate::ir::KNOWN_OBJECTS;
pub use jmcdata::generated::{
    ActionDef, ActionId, get_action_def as action_def, get_action_id as action_id,
};

#[must_use]
pub fn is_boolean_action(object: &str, name: &str) -> bool {
    action_def(object, name).is_some_and(|def| def.boolean)
}

/// Finds an action by bare method name across [`KNOWN_OBJECTS`].
#[must_use]
pub fn action_by_method(method: &str) -> Option<(&'static str, &'static ActionDef)> {
    KNOWN_OBJECTS
        .iter()
        .find_map(|&object| action_def(object, method).map(|def| (object, def)))
}
