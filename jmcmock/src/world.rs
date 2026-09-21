//! The mock world: players, entities, selections and the log of what happened.
//!
//! A `.jc` program lives on a server and almost always does something to the
//! world: writes to chat, applies effects, moves blocks. The mock stands in for
//! the server with a model just rich enough to run the code and see what it
//! does. Everything world-related comes down to three things:
//!
//! * [`World`] — players and entities, each with a name, a UUID and a position;
//! * [`Log`] — an ordered journal of effects: messages, sounds, world changes;
//! * [`Selection`] — the stream's selection, which the `select_*` actions work
//!   on.

use std::borrow::Cow;
use std::cell::Cell;

use jmcdata::module::Value;

use crate::value::{as_text, count_as_number, number};

/// A position in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// The X coordinate.
    pub x: f64,
    /// The Y coordinate.
    pub y: f64,
    /// The Z coordinate.
    pub z: f64,
    /// Rotation around the vertical axis, in degrees.
    pub yaw: f64,
    /// Tilt, in degrees.
    pub pitch: f64,
}

impl Default for Position {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

impl Position {
    /// A position from its coordinates alone: the rotation is zero, which is
    /// what a location argument offers.
    #[must_use]
    pub const fn coords(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// Turns the position into a location value.
    #[must_use]
    pub const fn to_value<'a>(self) -> Value<'a> {
        crate::value::location(self.x, self.y, self.z, self.yaw, self.pitch)
    }

    /// The distance to another position, ignoring rotation.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self, other))]
    pub fn distance(self, other: Self) -> f64 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dx.hypot(dy).hypot(dz)
    }
}

/// A player of the mock world.
#[derive(Debug, Clone)]
pub struct Player {
    /// The nickname.
    pub name: String,
    /// The UUID as a string.
    pub uuid: String,
    /// The position.
    pub position: Position,
    /// The health level.
    pub health: f64,
    /// The health ceiling, the value `max_health` reads.
    pub max_health: f64,
    /// The absorption level, the value `absorption_health` reads.
    pub absorption_health: f64,
    /// The game mode.
    pub game_mode: String,
    /// The food level.
    pub food: f64,
    /// The food saturation.
    pub saturation: f64,
    /// The experience level.
    pub experience: f64,
    /// The fire ticks.
    pub fire_ticks: f64,
    /// Whether the player is currently flying.
    pub is_flying: bool,
    /// Whether the player is allowed to fly.
    pub allow_flying: bool,
    /// Whether the player is sneaking.
    pub is_sneaking: bool,
    /// Whether the player is sprinting.
    pub is_sprinting: bool,
    /// Whether the player is gliding.
    pub is_gliding: bool,
    /// The spawn point, if set.
    pub spawn_point: Option<Position>,
}

impl Player {
    /// Creates a player with a deterministic UUID derived from the nickname:
    /// the mock cares about reproducibility, not cryptography.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(name))]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&name, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        Self {
            uuid: format!(
                "{hash:016x}-0000-0000-0000-{:012x}",
                hash & 0xffff_ffff_ffff
            ),
            name,
            position: Position::default(),
            health: 20.0,
            max_health: 20.0,
            absorption_health: 0.0,
            game_mode: "SURVIVAL".to_owned(),
            food: 20.0,
            saturation: 5.0,
            experience: 0.0,
            fire_ticks: 0.0,
            is_flying: false,
            allow_flying: false,
            is_sneaking: false,
            is_sprinting: false,
            is_gliding: false,
            spawn_point: None,
        }
    }

    /// Heals the player by `amount`, up to `max_health`.
    pub fn heal(&mut self, amount: f64) {
        if amount > 0.0 {
            self.health = (self.health + amount).min(self.max_health);
        }
    }

    /// Damages the player by `amount`, first depleting absorption health and then health.
    pub fn damage(&mut self, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        let mut rem = amount;
        if self.absorption_health > 0.0 {
            let absorbed = self.absorption_health.min(rem);
            self.absorption_health -= absorbed;
            rem -= absorbed;
        }
        self.health = (self.health - rem).max(0.0);
    }
}

/// An entity of the mock world.
#[derive(Debug, Clone)]
pub struct Entity {
    /// The entity kind, for example `minecraft:zombie`.
    pub kind: String,
    /// The name, if it has one.
    pub name: Option<String>,
    /// The UUID as a string.
    pub uuid: String,
    /// The position.
    pub position: Position,
    /// The health level.
    pub health: f64,
    /// The health ceiling.
    pub max_health: f64,
    /// The absorption level.
    pub absorption_health: f64,
    /// The fire ticks.
    pub fire_ticks: f64,
}

impl Entity {
    /// Heals the entity by `amount`, up to `max_health`.
    pub fn heal(&mut self, amount: f64) {
        if amount > 0.0 {
            self.health = (self.health + amount).min(self.max_health);
        }
    }

    /// Damages the entity by `amount`.
    pub fn damage(&mut self, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        let mut rem = amount;
        if self.absorption_health > 0.0 {
            let absorbed = self.absorption_health.min(rem);
            self.absorption_health -= absorbed;
            rem -= absorbed;
        }
        self.health = (self.health - rem).max(0.0);
    }
}

/// A reference to a member of a selection: an index into the world's players or
/// entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// The player at the given index in [`World::players`].
    Player(usize),
    /// The entity at the given index in [`World::entities`].
    Entity(usize),
}

/// A selection — an ordered set of targets that the `select_*` actions work on
/// and that the `%selected%` placeholders read.
pub type Selection = Vec<Target>;

/// An ordered journal of what the program did to the world.
#[derive(Debug, Clone, Default)]
pub struct Log {
    entries: Vec<String>,
    /// The cap on the number of entries: the mock must not eat memory on loops.
    limit: Option<usize>,
    dropped: usize,
}

impl Log {
    /// A journal without a length cap.
    #[must_use]
    #[tracing::instrument(level = "trace")]
    pub fn new() -> Self {
        Self::default()
    }

    /// A journal with a cap on the number of entries.
    #[must_use]
    pub const fn with_limit(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            limit: Some(limit),
            dropped: 0,
        }
    }

    /// Appends an entry.
    #[tracing::instrument(level = "trace", skip(self, entry))]
    pub fn push(&mut self, entry: impl Into<String>) {
        if self.limit.is_some_and(|limit| self.entries.len() >= limit) {
            self.dropped += 1;
            return;
        }
        self.entries.push(entry.into());
    }

    /// All entries, in order.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// How many entries the cap kept out of the journal.
    #[must_use]
    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    /// Empties the journal.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn clear(&mut self) {
        self.entries.clear();
        self.dropped = 0;
    }
}

/// The mock world as a whole.
#[derive(Debug)]
pub struct World {
    players: Vec<Player>,
    entities: Vec<Entity>,
    blocks: std::collections::HashMap<(i64, i64, i64), String>,
    world_time: f64,
    weather: String,
    log: Log,
    tick: u64,
    world_name: String,
    /// Whether the event being played out has been cancelled by one of its
    /// handlers. It is what `if_game_event_is_canceled` reads, and only
    /// `game_cancel_event` / `game_uncancel_event` write it.
    event_cancelled: bool,
    /// The state of the random-number generator. A `Cell`, because a random
    /// draw is needed even where the world is only borrowed immutably — for
    /// instance when substituting the `%random%` placeholder in text.
    seed: Cell<u64>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// An empty world without players or entities.
    #[must_use]
    #[tracing::instrument(level = "trace")]
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
            entities: Vec::new(),
            blocks: std::collections::HashMap::new(),
            world_time: 1000.0,
            weather: "CLEAR".to_owned(),
            log: Log::new(),
            tick: 0,
            world_name: "mock".to_owned(),
            event_cancelled: false,
            seed: Cell::new(0x2545_f491_4f6c_dd1d),
        }
    }

    /// Whether the event being played out has been cancelled.
    #[must_use]
    pub const fn is_event_cancelled(&self) -> bool {
        self.event_cancelled
    }

    /// Marks the event being played out as cancelled.
    pub const fn cancel_event(&mut self) {
        self.event_cancelled = true;
    }

    /// Marks the event being played out as no longer cancelled.
    pub const fn uncancel_event(&mut self) {
        self.event_cancelled = false;
    }

    /// Looks up a block at coordinates (x, y, z).
    #[must_use]
    pub fn block_at(&self, x: i64, y: i64, z: i64) -> Option<&str> {
        self.blocks.get(&(x, y, z)).map(String::as_str)
    }

    /// Sets a block at coordinates (x, y, z).
    pub fn set_block(&mut self, x: i64, y: i64, z: i64, block: impl Into<String>) {
        self.blocks.insert((x, y, z), block.into());
    }

    /// Breaks/removes a block at coordinates (x, y, z).
    pub fn break_block(&mut self, x: i64, y: i64, z: i64) -> Option<String> {
        self.blocks.remove(&(x, y, z))
    }

    /// The time of the world.
    #[must_use]
    pub const fn world_time(&self) -> f64 {
        self.world_time
    }

    /// Sets the time of the world.
    pub const fn set_world_time(&mut self, time: f64) {
        self.world_time = time;
    }

    /// The current weather of the world.
    #[must_use]
    pub fn weather(&self) -> &str {
        &self.weather
    }

    /// Sets the current weather of the world.
    pub fn set_weather(&mut self, weather: impl Into<String>) {
        self.weather = weather.into();
    }

    /// Removes an entity by index. Returns true if it existed.
    pub fn remove_entity(&mut self, index: usize) -> bool {
        if index < self.entities.len() {
            self.entities.remove(index);
            true
        } else {
            false
        }
    }

    /// Adds a player and returns a reference to it.
    #[tracing::instrument(level = "trace", skip(self, name))]
    pub fn add_player(&mut self, name: impl Into<String>) -> Target {
        let index = self.players.len();
        self.players.push(Player::new(name));
        Target::Player(index)
    }

    /// Adds a player at a specific position.
    #[tracing::instrument(level = "trace", skip(self, name), fields(position = ?position))]
    pub fn add_player_at(&mut self, name: impl Into<String>, position: Position) -> Target {
        let index = self.players.len();
        let mut player = Player::new(name);
        player.position = position;
        self.players.push(player);
        Target::Player(index)
    }

    /// Removes a player by name. Returns true if the player existed.
    #[tracing::instrument(level = "trace", skip(self, name))]
    pub fn remove_player(&mut self, name: &str) -> bool {
        if let Some(pos) = self.players.iter().position(|p| p.name == name) {
            self.players.remove(pos);
            true
        } else {
            false
        }
    }

    /// Adds an entity and returns a reference to it.
    #[tracing::instrument(level = "trace", skip(self, kind), fields(position = ?position))]
    pub fn add_entity(&mut self, kind: impl Into<String>, position: Position) -> Target {
        let index = self.entities.len();
        let kind = kind.into();
        self.entities.push(Entity {
            uuid: format!("entity-{index}"),
            name: None,
            kind,
            position,
            health: 20.0,
            max_health: 20.0,
            absorption_health: 0.0,
            fire_ticks: 0.0,
        });
        Target::Entity(index)
    }

    /// The players of the world.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// The entities of the world.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Mutable access to a player.
    #[tracing::instrument(level = "trace", skip(self), fields(index = ?index))]
    pub fn player_mut(&mut self, index: usize) -> Option<&mut Player> {
        self.players.get_mut(index)
    }

    /// Mutable access to an entity.
    #[tracing::instrument(level = "trace", skip(self), fields(index = ?index))]
    pub fn entity_mut(&mut self, index: usize) -> Option<&mut Entity> {
        self.entities.get_mut(index)
    }

    /// The journal of effects.
    #[must_use]
    pub const fn log(&self) -> &Log {
        &self.log
    }

    /// The journal of effects, mutably.
    pub const fn log_mut(&mut self) -> &mut Log {
        &mut self.log
    }

    /// The current game tick.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Moves the game time forward.
    pub const fn advance(&mut self, ticks: u64) {
        self.tick = self.tick.saturating_add(ticks);
    }

    /// The name of the world.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn world_name(&self) -> &str {
        &self.world_name
    }

    /// Sets the name of the world.
    #[tracing::instrument(level = "trace", skip(self, name))]
    pub fn set_world_name(&mut self, name: impl Into<String>) {
        self.world_name = name.into();
    }

    /// The next pseudo-random number. The algorithm is the same linear
    /// congruential generator as in `JustMC`: the mock wants reproducibility,
    /// not quality.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn next_random(&self) -> i64 {
        let next = self
            .seed
            .get()
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.seed.set(next);
        #[expect(
            clippy::cast_possible_wrap,
            reason = "the value is cast to i64 on purpose: JustMC hands out a signed number"
        )]
        let value = (next >> 16) as i64;
        value
    }

    /// The next pseudo-random number in `[0, 1)`.
    ///
    /// Every action with a random choice (`set_variable_random_number`,
    /// `set_variable_get_list_random_value`, …) takes the same number and maps
    /// it onto its own range, so the draws in the mock go through one counter
    /// and stay reproducible.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn next_unit(&self) -> f64 {
        // `next_random` keeps the top 48 bits of the state, so its absolute
        // value is below `2^48` and dividing by `2^48` maps it onto `[0, 1)`
        // exactly: 48 bits fit an f64 mantissa, nothing is lost. The divisor has
        // to match `next_random` — dividing by `2^53` (the mantissa size) left
        // every draw below `2^-16`, which pinned `%random%` and everything built
        // on it (`set_variable_random_number`, picking from a list, …) to the
        // bottom of its range.
        #[expect(
            clippy::cast_precision_loss,
            reason = "the divisor is a power of two and the value is under it: the division is exact, nothing is lost"
        )]
        let unit = self.next_random().unsigned_abs() as f64 / 281_474_976_710_656.0;
        unit
    }

    /// A random index out of `len` items.
    ///
    /// `None` means there are no items: there is nothing to choose from, and
    /// that is not an error — `set_variable_random` puts an empty value into
    /// the variable for an empty list.
    ///
    /// The draw goes through [`Self::next_unit`], because every random-choice
    /// action — `set_variable_random`, `set_variable_get_list_random_value`,
    /// `select_random_player` — takes the same number and maps it onto its own
    /// range.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(len = ?len))]
    pub fn next_index(&self, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let scaled = (self.next_unit() * count_as_number(len)).floor();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the product lies in [0, len); len is a number of elements"
        )]
        let index = scaled as usize;
        // A guard against a wrong `len`: the range also admits its own upper
        // bound if rounding pushed it up.
        Some(index.min(len - 1))
    }

    /// The next pseudo-random UUID.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn next_random_uuid(&self) -> String {
        let high = self.next_random().unsigned_abs();
        let low = self.next_random().unsigned_abs();
        format!("{high:016x}-{low:016x}")
    }

    /// The position of a target.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(target = ?target))]
    pub fn position_of(&self, target: Target) -> Option<Position> {
        match target {
            Target::Player(index) => self.players.get(index).map(|p| p.position),
            Target::Entity(index) => self.entities.get(index).map(|e| e.position),
        }
    }

    /// The name of a target.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(target = ?target))]
    pub fn target_name(&self, target: Target) -> String {
        match target {
            Target::Player(index) => self
                .players
                .get(index)
                .map_or_else(String::new, |p| p.name.clone()),
            Target::Entity(index) => self.entities.get(index).map_or_else(String::new, |e| {
                e.name.clone().unwrap_or_else(|| e.kind.clone())
            }),
        }
    }

    /// The UUID of a target.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(target = ?target))]
    pub fn target_uuid(&self, target: Target) -> &str {
        match target {
            Target::Player(index) => self.players.get(index).map_or("", |p| p.uuid.as_str()),
            Target::Entity(index) => self.entities.get(index).map_or("", |e| e.uuid.as_str()),
        }
    }

    /// Whether the target is a player.
    #[must_use]
    pub const fn is_player(target: Target) -> bool {
        matches!(target, Target::Player(_))
    }

    /// Who counts as "the primary" target in the current branch: the first
    /// player of the selection, otherwise the first player of the world. That
    /// is who the `%player%` and similar placeholders look at.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn primary_target(&self) -> Option<Target> {
        self.players.first().map(|_| Target::Player(0))
    }

    /// Describes a target in one line — for the journal.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self), fields(target = ?target))]
    pub fn describe(&self, target: Target) -> String {
        match target {
            Target::Player(index) => self
                .players
                .get(index)
                .map_or_else(|| "<missing player>".to_owned(), |p| p.name.clone()),
            Target::Entity(index) => self
                .entities
                .get(index)
                .map_or_else(|| "<missing entity>".to_owned(), |e| e.kind.clone()),
        }
    }
}

/// The default world: one player, `Dev`. Events need a target to run on,
/// otherwise most `player_*` actions have nothing to do.
#[must_use]
#[tracing::instrument(level = "trace")]
pub fn default_world() -> World {
    let mut world = World::new();
    world.add_player("Dev");
    world
}

/// Prints a number the way `JustMC` would in chat: without a fractional part
/// when it is zero.
#[must_use]
#[tracing::instrument(level = "trace", fields(value = ?value))]
pub fn format_number(value: f64) -> String {
    as_text(&number::<'_>(value)).map_or_else(|| format!("{value}"), Cow::into_owned)
}
