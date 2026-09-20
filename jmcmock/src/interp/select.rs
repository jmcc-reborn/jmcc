//! Выборки: цели операции и основная цель кадра.

use super::*;

impl<'a> Runtime<'a> {
    /// The frame's primary target: the first of the targets, or the world's
    /// target if there are none.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::NoTarget`] if neither the frame nor the world has
    /// a target.
    #[tracing::instrument(level = "debug", skip(self, stream))]
    pub(crate) fn primary(&self, stream: &Stream<'a>) -> Result<Target> {
        stream
            .targets
            .first()
            .copied()
            .or_else(|| self.world().primary_target())
            .ok_or_else(|| RuntimeError::NoTarget {
                context: stream.origin.clone(),
            })
    }

    /// The targets an operation's selection picks.
    ///
    /// Selection kinds are written the way they are in the source
    /// (`player::message<all_players>`), and they name not "someone in general"
    /// but a concrete set of targets. An unknown kind stops execution:
    /// substituting the frame's targets for it would apply the action to the
    /// wrong one — an error invisible in the journal.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnimplementedSelection`] for a kind the mock does
    /// not know.
    #[tracing::instrument(level = "debug", skip(self, stream), fields(selection_type = %selection_type))]
    pub(crate) fn selection_targets(
        &self,
        stream: &Stream<'a>,
        selection_type: &str,
    ) -> Result<Vec<Target>> {
        match selection_type {
            // The thread's target, or the world's default target if there is
            // none.
            "default" => Ok(self.resolve_targets(stream.targets.clone())),
            // The current target: for an action, that is the frame's targets.
            // Their absence is no reason to substitute someone else, so an empty
            // set stays empty.
            "current" => Ok(stream.targets.clone()),
            "default_player" => Ok(players_only(self.resolve_targets(stream.targets.clone()))),
            "default_entity" => Ok(entities_only(self.resolve_targets(stream.targets.clone()))),
            // `all` and `all_players` are not distinguished in the `JustMC`
            // reference: players are the only thing that can be a target of
            // either.
            "all_players" | "all" => Ok(crate::actions::select::players(self)),
            "all_entities" => Ok(crate::actions::select::entities(self)),
            // The last entity to appear. The mock never removes them, so "the
            // last" is simply the one added last.
            "last_entity" => Ok(crate::actions::select::last_entity(self)),
            // The participants of a fight. The mock plays out no combat: it has
            // no victim, damager, killer or projectile, and putting the frame's
            // target here would pass it off as one — the same reason `%victim%`
            // reads empty in `text.rs`. The selection is empty rather than
            // unknown: "nobody" is the truth, and an action over it does nothing.
            other if absent_participant(other) => Ok(Vec::new()),
            other => Err(RuntimeError::UnimplementedSelection {
                selection: other.to_owned(),
            }),
        }
    }
}

/// The selection kind from [`Value::GameValue`].
///
/// In this single field the compiler stores not a type name but a serialized
/// [`jmcdata::module::Selection`] — `{"type":"current"}`. It is parsed by the
/// same type that describes it rather than by hand-rolled string parsing.
///
/// # Errors
///
/// Returns [`RuntimeError::Json`] if the field does not parse: `Selection` is its
/// native format, and anything else means a module built by a different compiler.
#[tracing::instrument(level = "debug", fields(raw = %raw))]
pub(super) fn selection_type(raw: &str) -> Result<Cow<'_, str>> {
    let selection: jmcdata::module::Selection<'_> =
        serde_json::from_str(raw).map_err(|source| RuntimeError::Json {
            context: "a game value selection".to_owned(),
            source,
        })?;
    Ok(selection.selection_type)
}

/// Whether a selection names a participant the mock does not play out.
///
/// The mock simulates no fight: there is no victim, damager, killer or
/// projectile in its world. A selection for one of them is not unknown — it is
/// empty, and so is every value read about it (`Runtime::game_value`).
#[must_use]
#[tracing::instrument(level = "debug", fields(selection_type = %selection_type))]
pub(super) fn absent_participant(selection_type: &str) -> bool {
    matches!(
        selection_type,
        "victim_entity"
            | "victim_player"
            | "victim"
            | "damager_entity"
            | "damager_player"
            | "damager"
            | "shooter"
            | "killer"
            | "projectile"
    )
}

/// Leaves only players in the set.
#[tracing::instrument(level = "debug", fields(targets = ?targets))]
fn players_only(targets: Vec<Target>) -> Vec<Target> {
    targets
        .into_iter()
        .filter(|target| World::is_player(*target))
        .collect()
}

/// Leaves only entities in the set.
#[tracing::instrument(level = "debug", fields(targets = ?targets))]
fn entities_only(targets: Vec<Target>) -> Vec<Target> {
    targets
        .into_iter()
        .filter(|target| !World::is_player(*target))
        .collect()
}
