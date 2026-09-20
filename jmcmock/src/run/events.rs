//! События: имя события, его разбор и запуск обработчиков.
//!
//! Здесь же `run_world_start` и выбор целей события по умолчанию.

use super::*;

/// An event's identifier as it is written in the module's JSON.
#[must_use]
#[tracing::instrument(level = "debug", fields(event = ?event))]
pub fn event_name(event: EventId) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{event:?}"))
}

/// Parses an event name as it is written in the module's JSON.
///
/// # Errors
///
/// Returns [`RuntimeError::UnknownEvent`] if the `JustMC` schema has no such
/// event.
#[expect(
    clippy::map_err_ignore,
    reason = "the serde error lists every known event — that is the whole JustMC schema; the \
              name of the unknown event matters more"
)]
#[tracing::instrument(level = "debug", fields(name = %name))]
pub fn parse_event(name: &str) -> Result<EventId> {
    serde_json::from_value::<EventId>(serde_json::Value::String(name.to_owned())).map_err(|_| {
        RuntimeError::UnknownEvent {
            event: name.to_owned(),
        }
    })
}

impl<'a> Runtime<'a> {
    /// Runs an event's handlers.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownEvent`] if the module does not handle such
    /// an event, and any runtime error from the handler's body.
    #[tracing::instrument(level = "debug", skip(self), fields(event = %event))]
    pub fn fire_event(&mut self, event: &str) -> Result<()> {
        self.fire_event_with(event, EventData::default())
    }

    /// Runs an event's handlers with the event's data.
    ///
    /// `JustMC` starts a thread per hat, so every handler of the event runs,
    /// each with its own stream and local variables. All of them are enqueued
    /// *before* the scheduler starts: the hats of one event are independent
    /// threads, and a `wait` in the first must not delay the second.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnknownEvent`] if the module does not handle such
    /// an event, and the first uncaught runtime error from a handler.
    #[tracing::instrument(level = "debug", skip(self, data), fields(event = %event))]
    pub fn fire_event_with(&mut self, event: &str, data: EventData<'a>) -> Result<()> {
        let id = parse_event(event)?;
        let indices = self.program.event_indices(id).to_vec();
        if indices.is_empty() {
            return Err(RuntimeError::UnknownEvent {
                event: event.to_owned(),
            });
        }
        let targets = self.resolve_targets(data.targets);
        let origin = format!("event {event}");
        for index in indices {
            let mut stream = spawn_stream(targets.clone(), origin.clone());
            stream.chat_message = data.chat_message.clone().unwrap_or_default();
            stream.event_item = data.item.clone();
            stream.event_slot = data.slot.unwrap_or(0.0);
            stream.inventory_title = data.inventory_title.clone().unwrap_or_default();
            self.enqueue_handler(stream, index, Vec::new(), None)?;
        }
        self.run_scheduler()
    }

    /// Runs the `world_start` event if the module handles it.
    ///
    /// A module without such a handler simply does nothing.
    ///
    /// # Errors
    ///
    /// Returns a runtime error from the handler's body.
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn run_world_start(&mut self) -> Result<()> {
        if self.program.has_event(EventId::WorldStart) {
            self.fire_event(&event_name(EventId::WorldStart))?;
        }
        Ok(())
    }

    /// The event's targets: the explicit ones, or the world's default target if
    /// there are none.
    #[tracing::instrument(level = "debug", skip(self), fields(targets = ?targets))]
    pub(crate) fn resolve_targets(&self, targets: Vec<Target>) -> Vec<Target> {
        if targets.is_empty() {
            self.world.primary_target().into_iter().collect()
        } else {
            targets
        }
    }
}
