//! Actions on the world: everything that is not computation, flow, a loop or a
//! selection.
//!
//! This is the largest part of the `JustMC` schema — the actions of the objects
//! `player` (205), `entity` (227) and `world` (128). A real Minecraft stands
//! behind them, and the mock cannot reproduce it: it has no blocks, no
//! inventories and no physics. So only those actions that *mean* something
//! without a server are implemented here — the ones visible in the mock's
//! journal:
//!
//! | Action | What the mock does |
//! |---|---|
//! | `player_send_message` | writes the message to the journal |
//! | `player_send_action_bar` | writes the action bar text to the journal |
//! | `player_send_title` | writes the title and subtitle to the journal |
//! | `player_play_sound` | writes the sound and its volume to the journal |
//! | `player_set_boss_bar` | writes the boss bar to the journal |
//! | `player_remove_boss_bar` | writes the removal of the bar to the journal |
//! | `game_set_event_damage` | writes the event damage to the journal |
//! | `game_cancel_event` | ends the thread, the way cancelling an event does |
//!
//! `game_cancel_event` is the only action in the group with a real consequence:
//! cancelling an event stops the running code, so it returns
//! [`Flow::EndThread`] instead of writing to the journal.
//!
//! # Everything else is declared unimplemented
//!
//! Skipping `entity_teleport` or `player_give_item` silently is not an option:
//! the program would look sound while its behaviour differed from the server's.
//! [`Runtime::unimplemented`] raises [`RuntimeError::Unimplemented`], which stops
//! execution — and that is exactly what is wanted from the mock: it says
//! honestly what it cannot do instead of guessing. The behaviour is configured
//! through [`Unimplemented`](crate::Unimplemented).
//!
//! # The journal instead of the world
//!
//! Journal entries are addressed to the frame's targets: one line per target. If
//! there are no targets, the line is still written but marked `<no target>` —
//! otherwise an action that ran to no effect would vanish from sight, and a
//! reader of the journal would think it had not happened at all.

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
    match op.action {
        ActionId::PlayerSendMessage => message(rt, stream, op),
        ActionId::PlayerSendActionBar => action_bar(rt, stream, op),
        ActionId::PlayerSendTitle => title(rt, stream, op),
        ActionId::PlayerPlaySound => sound(rt, stream, op),
        ActionId::PlayerSetBossBar => boss_bar(rt, stream, op),
        ActionId::PlayerRemoveBossBar => remove_boss_bar(rt, stream, op),
        ActionId::GameSetEventDamage => set_event_damage(rt, stream, op),
        ActionId::GameCancelEvent => Ok(Flow::EndThread),
        _other => rt.unimplemented_op(stream, op),
    }
}

/// `player_send_message`: a chat message.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) if the
/// message does not convert to text, and an error evaluating `merging`.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn message<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let items = texts(rt, stream, args, "messages")?;
    let merging = rt.optional_enum_arg(stream, args, "merging")?;
    if merging == Some("SEPARATE_LINES") {
        for item in &items {
            broadcast(rt, stream, op.action, item);
        }
    } else {
        broadcast(rt, stream, op.action, &merge(&items, merging));
    }
    Ok(Flow::Continue)
}

/// `player_send_action_bar`: the action bar text.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) and an error
/// evaluating `merging`.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn action_bar<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let items = texts(rt, stream, args, "messages")?;
    let merging = rt.optional_enum_arg(stream, args, "merging")?;
    broadcast(rt, stream, op.action, &merge(&items, merging));
    Ok(Flow::Continue)
}

/// `player_send_title`: the title and subtitle.
///
/// The compiler does not always write the display times, so they make it into
/// the journal only when they are there.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) for the title
/// and subtitle, and
/// [`RuntimeError::NotANumber`](crate::RuntimeError::NotANumber) for the times.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn title<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let title = rt.text_arg(stream, args, "title")?;
    let subtitle = rt.text_arg(stream, args, "subtitle")?;
    let mut detail = format!("title \"{title}\", subtitle \"{subtitle}\"");
    if let Some(times) = times(rt, stream, args)? {
        detail.push_str(&times);
    }
    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

/// `player_play_sound`: a sound for the target.
///
/// `sound` is not text but a [`Value::Sound`] carrying pitch and volume, so it
/// is parsed as a value rather than read through `text_arg`: a sound has no text
/// representation, and `text_arg` would stop execution on it.
///
/// The sound may be left out altogether — `tests/pvp` writes
/// `player::play_sound()` with no arguments at all — and then there is nothing
/// to name, so the journal says only that a sound was played.
///
/// # Errors
///
/// Returns an argument-evaluation error.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn sound<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let mut detail = sound_name(rt, stream, args)?
        .map_or_else(|| "a sound".to_owned(), |sound| format!("sound {sound}"));
    if let Some(location) = rt.optional_arg(stream, args, "location")? {
        let [x, y, z, _, _] = value::location_of(&location, &at_op(args, "location"))?;
        detail.push_str(&format!(
            " at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ));
    }
    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

/// `player_set_boss_bar`: a boss bar.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) for `id` and
/// `title`, and an error evaluating the other arguments.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn boss_bar<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let id = rt.text_arg(stream, args, "id")?;
    let title = rt.text_arg(stream, args, "title")?;
    let progress = rt.optional_number_arg(stream, args, "progress")?;
    let mut detail = format!("boss bar '{id}' \"{title}\"");
    if let Some(progress) = progress {
        detail.push_str(&format!(" at {}", format_number(progress)));
    }
    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

/// `player_remove_boss_bar`: removing a boss bar.
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) for `id`.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn remove_boss_bar<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let id = rt.text_arg(stream, args, "id")?;
    broadcast(rt, stream, op.action, &format!("boss bar '{id}'"));
    Ok(Flow::Continue)
}

/// `game_set_event_damage`: the event damage.
///
/// # Errors
///
/// Returns [`RuntimeError::NotANumber`](crate::RuntimeError::NotANumber) if the
/// damage is not a number.
#[tracing::instrument(level = "trace", skip(rt, stream, op))]
fn set_event_damage<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let damage = rt.number_arg(stream, args, "damage")?;
    broadcast(
        rt,
        stream,
        op.action,
        &format!("event damage {}", format_number(damage)),
    );
    Ok(Flow::Continue)
}

/// Writes an action's line to the journal — one per target of the frame.
///
/// The target names both the kind and the name: the journal shows whom the
/// message was addressed to.
#[tracing::instrument(level = "trace", skip(rt, stream), fields(action = ?action, detail = %detail))]
fn broadcast(rt: &mut Runtime<'_>, stream: &Stream<'_>, action: ActionId, detail: &str) {
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
///
/// A separate function rather than a closure inside [`broadcast`]: a closure
/// would keep the world borrowed for reading until the end of the function,
/// while right after it the world is needed for writing.
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
///
/// # Errors
///
/// Returns [`RuntimeError::NotText`](crate::RuntimeError::NotText) if a value
/// does not convert to text.
#[tracing::instrument(level = "trace", skip(rt, stream, args), fields(name = %name))]
fn texts<'a>(
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
///
/// A missing mode is `CONCATENATION`: gluing without a separator. The
/// `SEPARATE_LINES` mode never reaches here — it unfolds into separate journal
/// entries.
#[tracing::instrument(level = "trace", fields(items = ?items, merging = ?merging))]
fn merge(items: &[String], merging: Option<&str>) -> String {
    match merging {
        Some("SPACES") => items.join(" "),
        _ => items.concat(),
    }
}

/// The title's display times as one line, if they are written.
///
/// # Errors
///
/// Returns [`RuntimeError::NotANumber`](crate::RuntimeError::NotANumber).
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn times<'a>(
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
///
/// A sound arrives as a ready [`Value::Sound`] rather than as a variable
/// reference: `JustMC` picks a sound from a list, and there is nothing to
/// substitute into it. If something else turned up in the sound's place, it is
/// read as text — that way the error is about a non-text value rather than about
/// an unreachable case.
///
/// # Errors
///
/// Returns an argument-evaluation error.
/// The name of the sound an operation plays, if it names one.
///
/// # Errors
///
/// Returns an error evaluating the argument.
#[tracing::instrument(level = "trace", skip(rt, stream, args))]
fn sound_name<'a>(
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
