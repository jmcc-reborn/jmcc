//! Actions on entity targets: teleportation, health, damage, and removal.

use jmcdata::generated::ActionId;
use jmcdata::module::Op;

use crate::error::Result;
use crate::interp::{Args, Flow};
use crate::run::{Runtime, Stream};
use crate::world::{Entity, Position, Target, format_number};

use super::broadcast;

/// Dispatches an entity action if recognized.
#[tracing::instrument(level = "debug", skip(rt, stream, op))]
pub fn dispatch<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Option<Result<Flow>> {
    match op.action {
        ActionId::EntityTeleport => Some(teleport(rt, stream, op)),
        ActionId::EntityDamage => Some(damage(rt, stream, op)),
        ActionId::EntityHeal => Some(heal(rt, stream, op)),
        ActionId::EntitySetCurrentHealth => Some(set_current_health(rt, stream, op)),
        ActionId::EntitySetMaxHealth => Some(set_max_health(rt, stream, op)),
        ActionId::EntitySetAbsorptionHealth => Some(set_absorption_health(rt, stream, op)),
        ActionId::EntitySetFireTicks => Some(set_fire_ticks(rt, stream, op)),
        ActionId::EntityRemove => Some(remove(rt, stream, op)),
        ActionId::EntityExplode => Some(explode(rt, stream, op)),
        ActionId::EntityLaunchUp => Some(launch_up(rt, stream, op)),
        ActionId::EntityLaunchForward => Some(launch_forward(rt, stream, op)),
        ActionId::EntityLaunchToLocation => Some(launch_to_location(rt, stream, op)),
        ActionId::EntityDummy => Some(Ok(Flow::Continue)),
        _ => None,
    }
}

fn for_each_entity(rt: &mut Runtime<'_>, stream: &Stream<'_>, mut f: impl FnMut(&mut Entity)) {
    for target in &stream.targets {
        if let Target::Entity(idx) = *target
            && let Some(entity) = rt.world_mut().entity_mut(idx)
        {
            f(entity);
        }
    }
}

fn teleport<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let [x, y, z, yaw, pitch] = rt.location_arg(stream, args, "location")?;
    let pos = Position {
        x,
        y,
        z,
        yaw,
        pitch,
    };
    for_each_entity(rt, stream, |entity| entity.position = pos);
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

fn damage<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let amount = rt.number_arg(stream, args, "damage")?;
    for_each_entity(rt, stream, |entity| entity.damage(amount));
    broadcast(rt, stream, op.action, &format_number(amount));
    Ok(Flow::Continue)
}

fn heal<'a>(rt: &mut Runtime<'a>, stream: &mut Stream<'a>, op: &'a Op<'a>) -> Result<Flow> {
    let args = Args::of(op);
    let amount = rt.number_arg(stream, args, "heal")?;
    for_each_entity(rt, stream, |entity| entity.heal(amount));
    broadcast(rt, stream, op.action, &format_number(amount));
    Ok(Flow::Continue)
}

fn set_current_health<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let health = rt.number_arg(stream, args, "health")?;
    for_each_entity(rt, stream, |entity| {
        entity.health = health.clamp(0.0, entity.max_health);
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
    let health = rt.number_arg(stream, args, "max_health")?;
    let heal_mode = rt.optional_enum_arg(stream, args, "heal_to_max")?;
    for_each_entity(rt, stream, |entity| {
        entity.max_health = health;
        if matches!(heal_mode, Some("HEAL" | "TRUE")) {
            entity.health = health;
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
    for_each_entity(rt, stream, |entity| {
        entity.absorption_health = health.max(0.0);
    });
    broadcast(rt, stream, op.action, &format_number(health));
    Ok(Flow::Continue)
}

fn set_fire_ticks<'a>(
    rt: &mut Runtime<'a>,
    stream: &mut Stream<'a>,
    op: &'a Op<'a>,
) -> Result<Flow> {
    let args = Args::of(op);
    let ticks = rt.number_arg(stream, args, "ticks")?;
    for_each_entity(rt, stream, |entity| {
        entity.fire_ticks = ticks;
    });
    broadcast(rt, stream, op.action, &format_number(ticks));
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn remove(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    for target in &stream.targets {
        if let Target::Entity(idx) = *target {
            rt.world_mut().remove_entity(idx);
        }
    }
    broadcast(rt, stream, op.action, "removed");
    Ok(Flow::Continue)
}

#[expect(clippy::unnecessary_wraps, reason = "uniform action handler signature")]
fn explode(rt: &mut Runtime<'_>, stream: &Stream<'_>, op: &Op<'_>) -> Result<Flow> {
    broadcast(rt, stream, op.action, "exploded");
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
