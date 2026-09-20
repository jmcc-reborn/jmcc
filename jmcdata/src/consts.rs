/// Maximum number of event, function, and process handlers per floor.
pub const MAX_HANDLERS_PER_FLOOR: u32 = 23;

/// Maximum number of actions per one line.
pub const MAX_ACTIONS_PER_LINE: u32 = 43;

/// Maximum number of floors available through `/editor floor add`.
pub const MAX_FLOORS: u32 = 15;

/// Maximum total number of handlers.
pub const MAX_HANDLERS: u32 = MAX_HANDLERS_PER_FLOOR * MAX_FLOORS;
