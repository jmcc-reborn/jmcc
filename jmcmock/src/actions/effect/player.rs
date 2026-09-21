//! Actions on player targets: movement, stats, chat, inventory, and environment.

use jmcdata::generated::ActionId;
use jmcdata::module::Op;

use crate::error::Result;
use crate::interp::{Args, Flow};
use crate::run::{Runtime, Stream};
use crate::world::{Player, Position, Target, format_number};

use super::{broadcast, merge, sound_name, texts, times};

/// Dispatches a player action if recognized.
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Option<Result<Flow>> {
    match op.action {
        ActionId::PlayerSendMessage => Some(message(rt, stream, op)),
        ActionId::PlayerSendActionBar => Some(action_bar(rt, stream, op)),
        ActionId::PlayerSendTitle => Some(title(rt, stream, op)),
        ActionId::PlayerSendMinimessage => Some(minimessage(rt, stream, op)),
        ActionId::PlayerSendHover => Some(hover_message(rt, stream, op)),
        ActionId::PlayerSendAdvancement => Some(advancement(rt, stream, op)),
        ActionId::PlayerPlaySound => Some(sound(rt, stream, op)),
        ActionId::PlayerSetBossBar => Some(boss_bar(rt, stream, op)),
        ActionId::PlayerRemoveBossBar => Some(remove_boss_bar(rt, stream, op)),

        ActionId::PlayerTeleport | ActionId::PlayerRandomizedTeleport => {
            Some(teleport(rt, stream, op))
        }
        ActionId::PlayerSetHealth => Some(set_health(rt, stream, op)),
        ActionId::PlayerSetMaxHealth => Some(set_max_health(rt, stream, op)),
        ActionId::PlayerSetAbsorptionHealth => Some(set_absorption_health(rt, stream, op)),
        ActionId::PlayerHeal => Some(heal(rt, stream, op)),
        ActionId::PlayerDamage => Some(damage(rt, stream, op)),
        ActionId::PlayerSetGamemode => Some(set_gamemode(rt, stream, op)),

        ActionId::PlayerSetVelocity => Some(set_velocity(rt, stream, op)),
        ActionId::PlayerLaunchUp => Some(launch_up(rt, stream, op)),
        ActionId::PlayerLaunchForward => Some(launch_forward(rt, stream, op)),
        ActionId::PlayerLaunchToLocation => Some(launch_to_location(rt, stream, op)),

        ActionId::PlayerGiveItems | ActionId::PlayerGiveRandomItem => {
            Some(give_items(rt, stream, op))
        }
        ActionId::PlayerClearInventory => Some(clear_inventory(rt, stream, op)),
        ActionId::PlayerCloseInventory => Some(close_inventory(rt, stream, op)),

        ActionId::PlayerSetFireTicks => Some(set_fire_ticks(rt, stream, op)),
        ActionId::PlayerSetFood => Some(set_food(rt, stream, op)),
        ActionId::PlayerSetSaturation => Some(set_saturation(rt, stream, op)),
        ActionId::PlayerSetExperience | ActionId::PlayerGiveExperience => {
            Some(experience(rt, stream, op))
        }
        ActionId::PlayerSetAllowFlying => Some(set_allow_flying(rt, stream, op)),
        ActionId::PlayerSetFlying => Some(set_flying(rt, stream, op)),
        ActionId::PlayerKick => Some(kick(rt, stream, op)),
        ActionId::PlayerSetSpawnPoint => Some(set_spawn_point(rt, stream, op)),
        ActionId::PlayerSetCompassTarget => Some(set_compass_target(rt, stream, op)),

        ActionId::PlayerSetTime => Some(set_time(rt, stream, op)),
        ActionId::PlayerSetWeather => Some(set_weather(rt, stream, op)),
        ActionId::PlayerResetWeather => Some(reset_weather(rt, stream, op)),

        ActionId::PlayerSwingHand => Some(swing_hand(rt, stream, op)),
        ActionId::PlayerDisplayParticle => Some(display_particle(rt, stream, op)),
        ActionId::PlayerDisplayBlock => Some(display_block(rt, stream, op)),
        ActionId::PlayerDummy => Some(Ok(Flow::Continue)),

        _ => None,
    }
}

fn for_each_player(rt: &mut Runtime<'_>, stream: &Stream<'_>, mut f: impl FnMut(&mut Player)) {
    for target in &stream.targets {
        if let Target::Player(idx) = *target
            && let Some(player) = rt.world_mut().player_mut(idx)
        {
            f(player);
        }
    }
}

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

fn action_bar<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let items = texts(rt, stream, args, "messages")?;
    let merging = rt.optional_enum_arg(stream, args, "merging")?;
    broadcast(rt, stream, op.action, &merge(&items, merging));
    Ok(Flow::Continue)
}

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

fn minimessage<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let msg = rt.text_arg(stream, args, "minimessage")?;
    broadcast(rt, stream, op.action, &msg);
    Ok(Flow::Continue)
}

fn hover_message<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let msg = rt.text_arg(stream, args, "message")?;
    let hover = rt.text_arg(stream, args, "hover")?;
    broadcast(
        rt,
        stream,
        op.action,
        &format!("\"{msg}\" (hover \"{hover}\")"),
    );
    Ok(Flow::Continue)
}

fn advancement<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let name = rt.text_arg(stream, args, "name")?;
    broadcast(rt, stream, op.action, &format!("advancement \"{name}\""));
    Ok(Flow::Continue)
}

fn sound<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let mut detail = sound_name(rt, stream, args)?
        .map_or_else(|| "a sound".to_owned(), |sound| format!("sound {sound}"));
    if let Some(location) = rt.optional_arg(stream, args, "location")? {
        let [x, y, z, _, _] =
            crate::value::location_of(&location, &crate::interp::at_op(args, "location"))?;
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

fn teleport<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = if op.action == ActionId::PlayerRandomizedTeleport {
        let locs = rt.values_arg(stream, args, "locations")?;
        if let Some(first) = locs.first() {
            crate::value::location_of(first, &crate::interp::at_op(args, "locations"))?
        } else {
            [0.0, 64.0, 0.0, 0.0, 0.0]
        }
    } else {
        rt.location_arg(stream, args, "location")?
    };

    let target_pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    for_each_player(rt, stream, |player| player.position = target_pos);

    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "to {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn set_health<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let health = rt.number_arg(stream, args, "health")?;
    for_each_player(rt, stream, |player| {
        player.health = health.clamp(0.0, player.max_health);
    });
    broadcast(rt, stream, op.action, &format_number(health));
    Ok(Flow::Continue)
}

fn set_max_health<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let health = rt.number_arg(stream, args, "health")?;
    let heal_mode = rt.optional_enum_arg(stream, args, "heal")?;
    for_each_player(rt, stream, |player| {
        player.max_health = health;
        if matches!(heal_mode, Some("HEAL" | "TRUE")) {
            player.health = health;
        }
    });
    broadcast(rt, stream, op.action, &format_number(health));
    Ok(Flow::Continue)
}

fn set_absorption_health<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let health = rt.number_arg(stream, args, "health")?;
    for_each_player(rt, stream, |player| {
        player.absorption_health = health.max(0.0);
    });
    broadcast(rt, stream, op.action, &format_number(health));
    Ok(Flow::Continue)
}

fn heal<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let amount = rt.number_arg(stream, args, "heal")?;
    for_each_player(rt, stream, |player| {
        player.heal(amount);
    });
    broadcast(rt, stream, op.action, &format_number(amount));
    Ok(Flow::Continue)
}

fn damage<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let amount = rt.number_arg(stream, args, "damage")?;
    for_each_player(rt, stream, |player| {
        player.damage(amount);
    });
    broadcast(rt, stream, op.action, &format_number(amount));
    Ok(Flow::Continue)
}

fn set_gamemode<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let mode = rt
        .optional_enum_arg(stream, args, "gamemode")?
        .unwrap_or("SURVIVAL");
    for_each_player(rt, stream, |player| {
        player.game_mode = mode.to_owned();
    });
    broadcast(rt, stream, op.action, mode);
    Ok(Flow::Continue)
}

fn set_velocity<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let detail = rt
        .optional_arg(stream, args, "velocity")?
        .map_or_else(|| "velocity".to_owned(), |v| crate::value::display(&v));
    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

fn launch_up<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let power = rt
        .optional_number_arg(stream, args, "power")?
        .unwrap_or(1.0);
    broadcast(
        rt,
        stream,
        op.action,
        &format!("power {}", format_number(power)),
    );
    Ok(Flow::Continue)
}

fn launch_forward<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let power = rt
        .optional_number_arg(stream, args, "power")?
        .unwrap_or(1.0);
    broadcast(
        rt,
        stream,
        op.action,
        &format!("power {}", format_number(power)),
    );
    Ok(Flow::Continue)
}

fn launch_to_location<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
    let power = rt
        .optional_number_arg(stream, args, "power")?
        .unwrap_or(1.0);
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "to {} {} {} power {}",
            format_number(x),
            format_number(y),
            format_number(z),
            format_number(power)
        ),
    );
    Ok(Flow::Continue)
}

fn give_items<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let items = rt.values_arg(stream, args, "items")?;
    broadcast(rt, stream, op.action, &format!("{} items", items.len()));
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn clear_inventory(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    broadcast(rt, stream, op.action, "inventory");
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn close_inventory(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    broadcast(rt, stream, op.action, "inventory");
    Ok(Flow::Continue)
}

fn set_fire_ticks<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let ticks = rt.number_arg(stream, args, "ticks")?;
    for_each_player(rt, stream, |player| {
        player.fire_ticks = ticks;
    });
    broadcast(rt, stream, op.action, &format_number(ticks));
    Ok(Flow::Continue)
}

fn set_food<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let food = rt.number_arg(stream, args, "food")?;
    for_each_player(rt, stream, |player| {
        player.food = food;
    });
    broadcast(rt, stream, op.action, &format_number(food));
    Ok(Flow::Continue)
}

fn set_saturation<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let sat = rt.number_arg(stream, args, "saturation")?;
    for_each_player(rt, stream, |player| {
        player.saturation = sat;
    });
    broadcast(rt, stream, op.action, &format_number(sat));
    Ok(Flow::Continue)
}

fn experience<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let exp = rt.number_arg(stream, args, "experience")?;
    for_each_player(rt, stream, |player| {
        if op.action == ActionId::PlayerGiveExperience {
            player.experience += exp;
        } else {
            player.experience = exp;
        }
    });
    broadcast(rt, stream, op.action, &format_number(exp));
    Ok(Flow::Continue)
}

fn set_allow_flying<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let allowed = rt.optional_enum_arg(stream, args, "allow_flying")? == Some("TRUE");
    for_each_player(rt, stream, |player| {
        player.allow_flying = allowed;
    });
    broadcast(
        rt,
        stream,
        op.action,
        if allowed { "true" } else { "false" },
    );
    Ok(Flow::Continue)
}

fn set_flying<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let flying = rt.optional_enum_arg(stream, args, "is_flying")? == Some("TRUE");
    for_each_player(rt, stream, |player| {
        player.is_flying = flying;
    });
    broadcast(rt, stream, op.action, if flying { "true" } else { "false" });
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn kick(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    broadcast(rt, stream, op.action, "kicked");
    Ok(Flow::Continue)
}

fn set_spawn_point<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "spawn_point")?;
    for_each_player(rt, stream, |player| {
        player.spawn_point = Some(Position {
            x,
            y,
            z,
            yaw,
            pitch,
        });
    });
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "to {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn set_compass_target<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "to {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn set_time<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let time = rt.number_arg(stream, args, "time")?;
    broadcast(rt, stream, op.action, &format_number(time));
    Ok(Flow::Continue)
}

fn set_weather<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let weather = rt
        .optional_enum_arg(stream, args, "weather_type")?
        .unwrap_or("CLEAR");
    broadcast(rt, stream, op.action, weather);
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn reset_weather(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    broadcast(rt, stream, op.action, "reset");
    Ok(Flow::Continue)
}

fn swing_hand<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let hand = rt
        .optional_enum_arg(stream, args, "hand_type")?
        .unwrap_or("MAIN_HAND");
    broadcast(rt, stream, op.action, hand);
    Ok(Flow::Continue)
}

fn display_particle<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let locs = rt.values_arg(stream, args, "location")?;
    let [x, y, z, _, _] = if let Some(first) = locs.first() {
        crate::value::location_of(first, &crate::interp::at_op(args, "location"))?
    } else {
        [0.0, 64.0, 0.0, 0.0, 0.0]
    };
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn display_block<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let locs = rt.values_arg(stream, args, "location")?;
    let [x, y, z, _, _] = if let Some(first) = locs.first() {
        crate::value::location_of(first, &crate::interp::at_op(args, "location"))?
    } else {
        [0.0, 64.0, 0.0, 0.0, 0.0]
    };
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}
