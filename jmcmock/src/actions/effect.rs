//! Actions on the world: everything that is not computation, flow, a loop or a
//! selection.
//!
//! Dispatches player, entity, and world effects.

mod entity;
mod player;
mod world;

use jmcdata::generated::ActionId;
use jmcdata::module::{Op, Value};

use crate::error::Result;
use crate::interp::{Args, Flow, at_op};
use crate::run::{Runtime, Stream};
use crate::value;
use crate::world::{Target, World, format_number};

/// The entry point for actions on the world.
///
/// # Errors
///
/// Returns an argument-evaluation error and
/// [`RuntimeError::Unimplemented`](crate::RuntimeError::Unimplemented) for an
/// action the mock does not know.
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    if let Some(flow) = player::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = entity::dispatch(rt, stream, op) {
        return flow;
    }
    if let Some(flow) = world::dispatch(rt, stream, op) {
        return flow;
    }
    rt.unimplemented_op(stream, op)
}

/// Writes an action's line to the journal — one per target of the frame.
///
/// The target names both the kind and the name: the journal shows whom the
/// message was addressed to.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(action = ?action, detail = %detail))]
pub(super) fn broadcast(rt: &mut Runtime<'_>, stream: &Stream<'_>, action: ActionId, detail: &str) {
    if stream.targets.is_empty() {
        let line = log_line(rt, None, action, detail);
        rt.world_mut().log_mut().push(line);
        return;
    }
    for target in &stream.targets {
        let line = log_line(rt, Some(*target), action, detail);
        rt.world_mut().log_mut().push(line);
    }
}

/// The journal line for a single target.
#[tracing::instrument(level = "trace", skip(rt), fields(target = ?target, action = ?action, detail = %detail))]
fn log_line(rt: &Runtime<'_>, target: Option<Target>, action: ActionId, detail: &str) -> String {
    let tick = rt.world().tick();
    let name = crate::schema::name(action);
    let target = target.map_or_else(
        || "<no target>".to_owned(),
        |target| {
            let kind = if World::is_player(target) {
                "player"
            } else {
                "entity"
            };
            format!("{kind} {}", rt.world().describe(target))
        },
    );
    format!("[{tick}] {target}: {name} {detail}")
}

/// The texts of an argument carrying several values.
#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
pub(super) fn texts<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
    name: &'static str,
) -> Result<Vec<String>> {
    let mut items = Vec::new();
    for value in rt.values_arg(stream, args, name)? {
        items.push(crate::value::text_of(&value, &at_op(args, name))?.into_owned());
    }
    Ok(items)
}

/// Collapses several messages into one line according to the `merging` mode.
#[tracing::instrument(level = "trace", fields(items = ?items, merging = ?merging))]
pub(super) fn merge(items: &[String], merging: Option<&str>) -> String {
    match merging {
        Some("SPACES") => items.join(" "),
        _ => items.concat(),
    }
}

/// The title's display times as one line, if they are written.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
pub(super) fn times<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<Option<String>> {
    let fade_in = rt.optional_number_arg(stream, args, "fade_in")?;
    let stay = rt.optional_number_arg(stream, args, "stay")?;
    let fade_out = rt.optional_number_arg(stream, args, "fade_out")?;
    let (Some(fade_in), Some(stay), Some(fade_out)) = (fade_in, stay, fade_out) else {
        return Ok(None);
    };
    Ok(Some(format!(
        " ({} {} {})",
        format_number(fade_in),
        format_number(stay),
        format_number(fade_out)
    )))
}

/// A sound as text for the journal.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
pub(super) fn sound_name<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    args: Args<'a>,
) -> Result<Option<String>> {
    let Some(raw) = args.raw("sound") else {
        return Ok(None);
    };
    let evaluated = rt.eval(stream, raw)?;
    if let Some(Value::Sound {
        sound,
        pitch,
        volume,
        ..
    }) = evaluated.as_ref()
    {
        return Ok(Some(format!(
            "{sound} (pitch {}, volume {})",
            format_number(pitch.0),
            format_number(volume.0)
        )));
    }
    if let Some(val) = evaluated {
        return Ok(Some(
            value::argument_text(&val, &at_op(args, "sound"))?.into_owned(),
        ));
    }
    Ok(None)
}
