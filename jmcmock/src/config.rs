//! Runtime settings.

use std::path::PathBuf;

/// What to do with an action whose semantics the mock does not implement.
///
/// `JustMC` has nearly a thousand actions, and a good half of them are about
/// the items, blocks and messages of a particular Minecraft version. The mock
/// implements everything that affects computation and control; for the rest a
/// behaviour has to be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unimplemented {
    /// Fail immediately. The default: silently skipping an action that was
    /// supposed to return something gives a wrong result without an error —
    /// that is, it hides the problem.
    #[default]
    Error,
    /// Write the action to the journal and carry on. Useful for running someone
    /// else's module to see how far it gets.
    Record,
    /// Skip silently.
    Ignore,
}

/// Execution settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// How many operations may run before the forced stop. A guard against
    /// `repeat_forever`: the mock has no server to interrupt an endless loop at
    /// the end of a tick.
    pub step_limit: usize,
    /// Maximum virtual ticks per scheduler run; sleeping tasks remain resumable.
    pub tick_limit: u64,
    /// The cap on the number of journal entries. `None` means no cap.
    pub log_limit: Option<usize>,
    /// The behaviour for unimplemented actions.
    pub unimplemented: Unimplemented,
    /// Whether to start from a world with a single player, `Dev`. If disabled,
    /// the world is empty and events without an explicit target have no one to
    /// run on.
    pub default_player: bool,
    /// The file the `save` scope's variables are stored in.
    ///
    /// `None` means the `save` variables live only until the process ends, like
    /// `game`. If a file is set, [`Runtime::new`](crate::Runtime) reads it on
    /// construction (a missing file is not an error — that is the first run),
    /// and [`Runtime::persist_save`](crate::Runtime::persist_save) writes it.
    /// The owner of the runtime decides when to save: the mock does not catch
    /// `Ctrl-C` and does not know that the program is "shutting down".
    pub save_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            step_limit: 5_000_000,
            tick_limit: 100,
            log_limit: Some(100_000),
            unimplemented: Unimplemented::default(),
            default_player: true,
            save_file: None,
        }
    }
}

impl Config {
    /// Sets the virtual tick budget for each scheduler run.
    #[must_use]
    pub const fn with_tick_limit(mut self, limit: u64) -> Self {
        self.tick_limit = limit;
        self
    }

    /// Settings with the given step limit.
    #[must_use]
    pub const fn with_step_limit(mut self, limit: usize) -> Self {
        self.step_limit = limit;
        self
    }

    /// Settings with the given mode for unimplemented actions.
    #[must_use]
    pub const fn with_unimplemented(mut self, mode: Unimplemented) -> Self {
        self.unimplemented = mode;
        self
    }

    /// Disables the default player.
    #[must_use]
    pub const fn without_default_player(mut self) -> Self {
        self.default_player = false;
        self
    }

    /// Sets the file for the `save` scope's variables.
    #[must_use]
    #[tracing::instrument(level = "trace", skip(self, path))]
    pub fn with_save_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.save_file = Some(path.into());
        self
    }
}
