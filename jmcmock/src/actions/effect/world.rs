//! Actions on the world: blocks, explosions, spawning entities, time, and weather.

use jmcdata::generated::ActionId;
use jmcdata::module::Op;

use crate::error::Result;
use crate::interp::{Args, Flow};
use crate::run::{Runtime, Stream};
use crate::world::{Position, format_number};

use super::broadcast;

/// Dispatches a world action if recognized.
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Option<Result<Flow>> {
    match op.action {
        ActionId::GameSetBlock => Some(set_block(rt, stream, op)),
        ActionId::GameBreakBlock => Some(break_block(rt, stream, op)),
        ActionId::GameCreateExplosion => Some(create_explosion(rt, stream, op)),

        ActionId::GameSpawnMob => Some(spawn_mob(rt, stream, op)),
        ActionId::GameSpawnArmorStand => Some(spawn_armor_stand(rt, stream, op)),
        ActionId::GameSpawnItem => Some(spawn_item(rt, stream, op)),
        ActionId::GameSpawnItemDisplay => Some(spawn_item_display(rt, stream, op)),
        ActionId::GameSpawnBlockDisplay => Some(spawn_block_display(rt, stream, op)),
        ActionId::GameSpawnTextDisplay => Some(spawn_text_display(rt, stream, op)),
        ActionId::GameSpawnLightningBolt => Some(spawn_lightning_bolt(rt, stream, op)),
        ActionId::GameLaunchFirework => Some(launch_firework(rt, stream, op)),

        ActionId::GameSetWorldTime => Some(set_world_time(rt, stream, op)),
        ActionId::GameSetWorldWeather => Some(set_world_weather(rt, stream, op)),

        ActionId::GameCancelEvent => {
            rt.world_mut().cancel_event();
            Some(Ok(Flow::EndThread))
        }
        ActionId::GameUncancelEvent => {
            rt.world_mut().uncancel_event();
            broadcast(rt, stream, op.action, "uncancel event");
            Some(Ok(Flow::Continue))
        }
        ActionId::GameSetEventDamage => Some(set_event_damage(rt, stream, op)),
        ActionId::GameSetEventHeal => Some(set_event_heal(rt, stream, op)),
        ActionId::GameSetEventExperience => Some(set_event_experience(rt, stream, op)),
        ActionId::GameSetEventGamemode => Some(set_event_gamemode(rt, stream, op)),
        ActionId::GameDummy => Some(Ok(Flow::Continue)),

        _ => None,
    }
}

fn set_block<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let block_name = rt
        .optional_arg(stream, args, "block")?
        .map_or_else(|| "stone".to_owned(), |v| crate::value::display(&v));

    let locations = rt.values_arg(stream, args, "locations")?;
    for loc_val in &locations {
        let [x, y, z, _, _] =
            crate::value::location_of(loc_val, &crate::interp::at_op(args, "locations"))?;
        #[expect(clippy::cast_possible_truncation, reason = "block coordinates")]
        rt.world_mut()
            .set_block(x as i64, y as i64, z as i64, block_name.clone());
    }

    let detail = if let Some(first) = locations.first() {
        let [x, y, z, _, _] =
            crate::value::location_of(first, &crate::interp::at_op(args, "locations"))?;
        format!(
            "{block_name} at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        )
    } else {
        block_name
    };

    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

fn break_block<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let locations = rt.values_arg(stream, args, "locations")?;
    for loc_val in &locations {
        let [x, y, z, _, _] =
            crate::value::location_of(loc_val, &crate::interp::at_op(args, "locations"))?;
        #[expect(clippy::cast_possible_truncation, reason = "block coordinates")]
        rt.world_mut().break_block(x as i64, y as i64, z as i64);
    }

    let detail = if let Some(first) = locations.first() {
        let [x, y, z, _, _] =
            crate::value::location_of(first, &crate::interp::at_op(args, "locations"))?;
        format!(
            "at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        )
    } else {
        "blocks".to_owned()
    };

    broadcast(rt, stream, op.action, &detail);
    Ok(Flow::Continue)
}

fn create_explosion<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, _, _] = rt.location_arg(stream, args, "location")?;
    let power = rt
        .optional_number_arg(stream, args, "power")?
        .unwrap_or(4.0);
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "power {} at {} {} {}",
            format_number(power),
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn spawn_mob<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "location")?;
    let mob = rt
        .optional_arg(stream, args, "mob")?
        .map_or_else(|| "zombie".to_owned(), |v| crate::value::display(&v));
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity(mob.clone(), pos);
    broadcast(
        rt,
        stream,
        op.action,
        &format!(
            "{mob} at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn spawn_armor_stand<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity("armor_stand", pos);
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

fn spawn_item<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity("item", pos);
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

fn spawn_item_display<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "spawn_location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity("item_display", pos);
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

fn spawn_block_display<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "spawn_location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity("block_display", pos);
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

fn spawn_text_display<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "spawn_location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    rt.world_mut().add_entity("text_display", pos);
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

fn spawn_lightning_bolt<'a>(
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
            "at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn launch_firework<'a>(
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
            "at {} {} {}",
            format_number(x),
            format_number(y),
            format_number(z)
        ),
    );
    Ok(Flow::Continue)
}

fn set_world_time<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let time = rt.number_arg(stream, args, "time")?;
    rt.world_mut().set_world_time(time);
    broadcast(rt, stream, op.action, &format_number(time));
    Ok(Flow::Continue)
}

fn set_world_weather<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let weather = rt
        .optional_enum_arg(stream, args, "weather_type")?
        .unwrap_or("CLEAR");
    rt.world_mut().set_weather(weather);
    broadcast(rt, stream, op.action, weather);
    Ok(Flow::Continue)
}

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

fn set_event_heal<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let heal = rt.number_arg(stream, args, "heal")?;
    broadcast(
        rt,
        stream,
        op.action,
        &format!("event heal {}", format_number(heal)),
    );
    Ok(Flow::Continue)
}

fn set_event_experience<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let exp = rt.number_arg(stream, args, "experience")?;
    broadcast(
        rt,
        stream,
        op.action,
        &format!("event experience {}", format_number(exp)),
    );
    Ok(Flow::Continue)
}

fn set_event_gamemode<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let mode = rt
        .optional_enum_arg(stream, args, "gamemode")?
        .unwrap_or("SURVIVAL");
    broadcast(rt, stream, op.action, &format!("event gamemode {mode}"));
    Ok(Flow::Continue)
}
