//! Actions of the `select` object: selecting targets.
//!
//! The selection is what the other actions act upon. In a frame it lives as two
//! fields: `selection` (what is selected) and `targets` (what the action applies
//! to right now). After any `select_*` they coincide: the following code already
//! works with the new selection.
//!
//! # A miss of `select_*` is not an error
//!
//! `select_player_by_name("Vasya")` on a server Vasya never joined gives an
//! empty selection, and the next action does nothing. That is an ordinary course
//! of the program rather than a failure, so the mock repeats the server's
//! behaviour instead of complaining. Only what the mock cannot do is declared
//! unimplemented.
//!
//! # What is not here
//!
//! * `select_event_target` needs event data — who hit, who shot — which the mock
//!   does not have. Substituting "the server's first player" for the event
//!   target would mean performing the action on someone other than whom it would
//!   have been performed on on the server;
//! * `select_filter_by_raycast` needs the world's blocks, which the mock also
//!   does not have.
//!
//! Both actions are declared unimplemented
//! ([`RuntimeError::Unimplemented`](crate::RuntimeError::Unimplemented)).
//!
//! # Mobs and entities
//!
//! The mock world does not divide entities into mobs and non-mobs: everything in
//! [`World::entities`](crate::World::entities) counts as a mob. So
//! `select_all_mobs` and `select_all_entities` give the same thing, and "the
//! last entity" is the last one added.

use jmcdata::generated::ActionId;
use jmcdata::module::{Conditional, Op};

use crate::error::Result;
use crate::interp::{Args, Flow};
use crate::run::{Runtime, Stream};
use crate::world::{Position, Target};

/// The entry point for the `select` group.
///
/// # Errors
///
/// Returns any runtime error; an unimplemented action gives
/// [`RuntimeError::Unimplemented`](crate::RuntimeError::Unimplemented).
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let action = op.action;
    match action {
        ActionId::SelectDummy => {}
        ActionId::SelectReset => clear(stream),

        ActionId::SelectAllPlayers | ActionId::SelectAddAllPlayers => {
            select(stream, players(rt), add(action));
        }
        ActionId::SelectAllEntities
        | ActionId::SelectAllMobs
        | ActionId::SelectAddAllEntities
        | ActionId::SelectAddAllMobs => {
            select(stream, entities(rt), add(action));
        }
        ActionId::SelectInvert => {
            let inverted: Vec<Target> = everyone(rt)
                .into_iter()
                .filter(|target| !stream.selection.contains(target))
                .collect();
            select(stream, inverted, false);
        }

        ActionId::SelectPlayerByName | ActionId::SelectAddPlayerByName => {
            let wanted = rt.text_arg(stream, Args::of(op), "name_or_uuid")?;
            let found = by_name(rt, &players(rt), &wanted);
            select(stream, found, add(action));
        }
        ActionId::SelectEntityByName
        | ActionId::SelectMobByName
        | ActionId::SelectAddEntityByName
        | ActionId::SelectAddMobByName => {
            let wanted = rt.text_arg(stream, Args::of(op), "name_or_uuid")?;
            let found = by_name(rt, &entities(rt), &wanted);
            select(stream, found, add(action));
        }

        ActionId::SelectRandomPlayer | ActionId::SelectAddRandomPlayer => {
            let found = pick_random(rt, players(rt));
            select(stream, found, add(action));
        }
        ActionId::SelectRandomEntity
        | ActionId::SelectRandomMob
        | ActionId::SelectAddRandomEntity
        | ActionId::SelectAddRandomMob => {
            let found = pick_random(rt, entities(rt));
            select(stream, found, add(action));
        }

        ActionId::SelectLastEntity
        | ActionId::SelectLastMob
        | ActionId::SelectAddLastEntity
        | ActionId::SelectAddLastMob => {
            let found = last_entity(rt);
            select(stream, found, add(action));
        }

        ActionId::SelectFilterByDistance => return by_distance(rt, stream, op),
        ActionId::SelectFilterRandomly => return randomly(rt, stream, op),

        _other => return rt.unimplemented_op(stream, op),
    }
    Ok(Flow::Continue)
}

/// `select_*_by_conditional`: narrows or extends the selection with those for
/// whom the condition is true.
///
/// Called from `Runtime::exec_conditional`: the condition of such actions lives
/// in [`Op::conditional`] rather than in the arguments, so it is handled where
/// the branches are and reaches here already found. The condition's own
/// arguments then live in the container's `values`, not in a separate operation.
///
/// The condition is checked separately for each target, so while it is checked
/// the stream's target is swapped for the one being tested: otherwise
/// `if_player_is_near` would look at the same target for every candidate.
///
/// # Errors
///
/// Returns any error evaluating the condition.
#[tracing::instrument(level = "trace", skip(rt, stream, op, conditional))]
pub fn conditional<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
    conditional: Conditional,
) -> Result<Flow> {
    let candidates = match op.action {
        ActionId::SelectPlayerByConditional | ActionId::SelectAddPlayerByConditional => players(rt),
        _ => entities(rt),
    };
    let mut survivors = Vec::with_capacity(candidates.len());
    for target in candidates {
        stream.targets = vec![target];
        if rt.condition(stream, conditional.action, op)? != conditional.is_inverted {
            survivors.push(target);
        }
    }
    select(stream, survivors, add(op.action));
    Ok(Flow::Continue)
}

/// `select_filter_by_distance`: keeps the nearest or the farthest targets.
///
/// `ignore_y_axis` measures distance horizontally, without height. The order of
/// equal distances is preserved: the sort is stable, so with equal distances the
/// selection keeps its original order.
///
/// # Errors
///
/// Returns [`RuntimeError::NotALocation`](crate::RuntimeError::NotALocation) if
/// `location` is not a location, and an error evaluating the other arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn by_distance<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
    let center = Position::coords(x, y, z);
    let size = rt
        .optional_number_arg(stream, args, "selection_size")?
        .unwrap_or(0.0);
    let ignore_y = rt.optional_enum_arg(stream, args, "ignore_y_axis")? == Some("TRUE");
    let farthest = rt.optional_enum_arg(stream, args, "compare_mode")? == Some("FARTHEST");

    let mut measured: Vec<(f64, Target)> = std::mem::take(&mut stream.selection)
        .into_iter()
        .filter_map(|target| {
            let position = rt.world().position_of(target)?;
            Some((distance(position, center, ignore_y), target))
        })
        .collect();
    measured.sort_by(|left, right| left.0.total_cmp(&right.0));
    if farthest {
        measured.reverse();
    }
    let kept = measured
        .into_iter()
        .take(count_of(size))
        .map(|(_, target)| target)
        .collect();
    select(stream, kept, false);
    Ok(Flow::Continue)
}

/// `select_filter_randomly`: keeps a random part of the selection.
///
/// Targets are drawn one at a time, so the same target cannot end up in the
/// result twice and the result is no longer than the original selection.
///
/// # Errors
///
/// Returns an error evaluating the `size` argument.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn randomly<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let size = rt
        .optional_number_arg(stream, Args::of(op), "size")?
        .unwrap_or(0.0);
    let mut candidates = std::mem::take(&mut stream.selection);
    let mut kept = Vec::new();
    while kept.len() < count_of(size) {
        // The index comes from the length of what is left, so `None` here means
        // "the remainder ran out", and that ends the draw: there is nothing left
        // to take.
        let Some(index) = rt.world().next_index(candidates.len()) else {
            break;
        };
        kept.push(candidates.remove(index));
    }
    select(stream, kept, false);
    Ok(Flow::Continue)
}

/// Replaces the selection or extends it.
///
/// Extension skips what is already selected: a repeated `select_add_all_players`
/// must not double the targets, otherwise the action would run on a player
/// twice.
#[tracing::instrument(level = "trace", skip(stream), fields(targets = ?targets, add = ?add))]
fn select(stream: &mut Stream<'_>, targets: Vec<Target>, add: bool) {
    if add {
        for target in targets {
            if !stream.selection.contains(&target) {
                stream.selection.push(target);
            }
        }
    } else {
        stream.selection = targets;
    }
    stream.targets.clone_from(&stream.selection);
}

/// Resets the selection.
///
/// An empty selection is not "nobody": in that case `primary` falls back to the
/// world's player, and the action runs on someone anyway. The server behaves the
/// same way, where a thread always has a default target.
#[tracing::instrument(level = "trace", skip(stream))]
fn clear(stream: &mut Stream<'_>) {
    stream.selection.clear();
    stream.targets.clear();
}

/// Whether the action extends the selection instead of replacing it.
///
/// The flag comes from the action's identifier: the `JustMC` schema has no
/// separate field for it, and the `select_add_*` name is its only expression.
#[tracing::instrument(level = "trace", fields(action = ?action))]
fn add(action: ActionId) -> bool {
    crate::schema::name(action).starts_with("select_add_")
}

/// All players of the world.
///
/// The mock's one definition of "all players": the `all_players` and `all`
/// selections in `interp` are answered with it too.
#[tracing::instrument(level = "trace", skip(rt))]
pub fn players(rt: &Runtime<'_>) -> Vec<Target> {
    (0..rt.world().players().len())
        .map(Target::Player)
        .collect()
}

/// All entities of the world.
#[tracing::instrument(level = "trace", skip(rt))]
pub fn entities(rt: &Runtime<'_>) -> Vec<Target> {
    (0..rt.world().entities().len())
        .map(Target::Entity)
        .collect()
}

/// Players and entities together — what a full selection consists of.
#[tracing::instrument(level = "trace", skip(rt))]
fn everyone(rt: &Runtime<'_>) -> Vec<Target> {
    let mut all = players(rt);
    all.extend(entities(rt));
    all
}

/// The last entity added.
#[tracing::instrument(level = "trace", skip(rt))]
pub fn last_entity(rt: &Runtime<'_>) -> Vec<Target> {
    rt.world()
        .entities()
        .len()
        .checked_sub(1)
        .map(|index| vec![Target::Entity(index)])
        .unwrap_or_default()
}

/// The targets among the candidates whose name or UUID matches the requested
/// one.
///
/// Both the UUID and the name compare as strings: the compiler passes both in
/// the single `name_or_uuid` argument without marking which was meant.
#[tracing::instrument(level = "trace", skip(rt), fields(candidates = ?candidates, wanted = %wanted))]
fn by_name(rt: &Runtime<'_>, candidates: &[Target], wanted: &str) -> Vec<Target> {
    candidates
        .iter()
        .copied()
        .filter(|target| {
            rt.world().target_name(*target) == wanted || rt.world().target_uuid(*target) == wanted
        })
        .collect()
}

/// One random target out of the candidates.
///
/// An empty set stays empty: there is nothing to choose from.
#[tracing::instrument(level = "trace", skip(rt), fields(candidates = ?candidates))]
fn pick_random(rt: &Runtime<'_>, candidates: Vec<Target>) -> Vec<Target> {
    rt.world()
        .next_index(candidates.len())
        .map_or_else(Vec::new, |index| vec![candidates[index]])
}

/// The distance between two positions, optionally ignoring height.
#[tracing::instrument(level = "trace", fields(left = ?left, right = ?right, ignore_y = ?ignore_y))]
fn distance(left: Position, right: Position, ignore_y: bool) -> f64 {
    if ignore_y {
        (left.x - right.x).hypot(left.z - right.z)
    } else {
        left.distance(right)
    }
}

/// The size of a selection as a number of targets.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a selection size is an integer; a negative or fractional one is floored to zero"
)]
#[tracing::instrument(level = "trace", fields(size = ?size))]
fn count_of(size: f64) -> usize {
    if size.is_finite() && size > 0.0 {
        size.floor() as usize
    } else {
        0
    }
}
