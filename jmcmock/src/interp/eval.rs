//! Вычисление значений: `eval`, игровые величины и их имена для ошибок.

use super::select::selection_type;
use super::*;

impl<'a> Runtime<'a> {
    /// Evaluates a value.
    ///
    /// A variable reference is read from its scope, text is rendered, a list and
    /// a dictionary are evaluated element by element, and the rest is a literal.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UndefinedVariable`] if the variable is missing,
    /// [`RuntimeError::UnexpectedValue`] for a parameter declaration, and
    /// [`RuntimeError::Unimplemented`] for `%math()`.
    #[tracing::instrument(level = "debug", skip(self, stream), fields(value = ?value), ret)]
    pub(crate) fn eval(&mut self, stream: &mut Stream<'a>, value: &'a Value<'a>) -> Result<Rt<'a>> {
        match value {
            Value::Variable { variable, scope } => {
                let name = self.variable_name(stream, variable)?;
                self.scope_store(stream, *scope)
                    .read_or_empty(&name, *scope)
            }
            Value::Text { text, parsing } => {
                let rendered = crate::text::render(self, stream, text, *parsing)?;
                Ok(Some(value::text(rendered)))
            }
            Value::Array { values } => {
                let mut items = Vec::with_capacity(values.len());
                for item in values {
                    items.push(match item {
                        Some(item) => self.eval(stream, item)?,
                        None => None,
                    });
                }
                Ok(Some(Value::Array { values: items }))
            }
            Value::Map { values } => {
                let mut entries: LiteMap<TextValue, Value<'a>> = LiteMap::new();
                for (key, item) in values.iter() {
                    if let Some(item) = self.eval(stream, item)? {
                        entries.insert(key.clone(), item);
                    }
                }
                Ok(Some(Value::Map { values: entries }))
            }
            Value::GameValue {
                game_value,
                selection,
            } => self.game_value(stream, *game_value, selection),
            Value::Number {
                number: Number::Calc(_),
            } => {
                self.unimplemented("the %math() expression")?;
                Ok(None)
            }
            Value::Parameter { name, .. } => Err(RuntimeError::UnexpectedValue {
                context: "an operation".to_owned(),
                actual: format!("the parameter declaration '{name}'"),
            }),
            other => Ok(Some(other.clone())),
        }
    }

    /// Reads a game value.
    ///
    /// The mock knows only the values that mean something without a server: the
    /// event's data and what is in the mock world. The rest — nearly a thousand
    /// fields of live Minecraft — are unimplemented and stop execution.
    ///
    /// A value's selection decides who it is about: `value::name<current>` is the
    /// current target's name, `value::location<victim_entity>` the event victim's
    /// location. The mock reads values for a single target, so of the selections
    /// it understands only the two that mean that very target; the rest stop
    /// execution rather than being substituted with the frame's target.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::UnimplementedSelection`] for an unrecognized
    /// selection and [`RuntimeError::Unimplemented`] for an unrecognized value.
    #[expect(clippy::too_many_lines, reason = "Game value evaluation dispatch")]
    #[tracing::instrument(level = "debug", skip(self, stream), fields(id = ?id, selection = %selection))]
    fn game_value(
        &mut self,
        stream: &Stream<'a>,
        id: GameValueId,
        selection: &'a str,
    ) -> Result<Rt<'a>> {
        let selection_type = selection_type(selection)?;
        // A value about a participant of a fight the mock does not play out is
        // empty, the same as the selection itself (`interp::select`).
        if super::select::absent_participant(&selection_type) {
            return Ok(None);
        }
        // `default_entity` and `default_player` name the same single target as
        // `default`: in an `entity_interact` the thread's target *is* the
        // entity. Only the selections that name several targets stay errors —
        // picking one of them would pass it off as "the" target.
        if !matches!(
            &*selection_type,
            "default" | "current" | "default_entity" | "default_player"
        ) {
            return Err(RuntimeError::UnimplementedSelection {
                selection: selection_type.into_owned(),
            });
        }
        let value = match id {
            GameValueId::EventChatMessage | GameValueId::EventMessage => {
                value::text(stream.chat_message.clone())
            }
            GameValueId::EventItem => return Ok(stream.event_item.clone()),
            GameValueId::EventSlot => value::number(stream.event_slot),
            GameValueId::OpenInventoryTitle => value::text(stream.inventory_title.clone()),
            GameValueId::SelectionSize => value::number(count_as_number(stream.selection.len())),
            // Кто есть цель: у игроков и сущностей мок-мира есть имя и UUID.
            GameValueId::Name => value::text(self.world().target_name(self.primary(stream)?)),
            GameValueId::Uuid => {
                value::text(self.world().target_uuid(self.primary(stream)?).to_owned())
            }
            // Часы мира: виртуальные тики, которые двигает планировщик, пока
            // все потоки спят.
            GameValueId::ServerCurrentTick => {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "виртуальные тики прогона далеки от 2^53"
                )]
                let tick = self.world().tick() as f64;
                value::number(tick)
            }
            // `JustMC` reports the server's processor load in percent, and
            // programs throttle on it. The mock has no processor: it reports how
            // much of `Config::step_limit` has been used. That keeps the value
            // on the same 0–100 scale as the real one — a raw step count made
            // every `%cpu_usage% > 60` fire after the first few dozen operations
            // and pinned programs that wait on it to the tick budget.
            GameValueId::CpuUsage => {
                let used = self.steps().saturating_mul(100) / self.step_limit();
                value::number(count_as_number(used.min(100)))
            }
            GameValueId::Location => {
                let target = self.primary(stream)?;
                self.world()
                    .position_of(target)
                    .ok_or_else(|| RuntimeError::NoTarget {
                        context: stream.origin.clone(),
                    })?
                    .to_value()
            }
            GameValueId::CurrentHealth => {
                let target = self.primary(stream)?;
                match target {
                    Target::Player(index) => value::number(
                        self.world_mut()
                            .player_mut(index)
                            .map_or(0.0, |player| player.health),
                    ),
                    Target::Entity(index) => value::number(
                        self.world_mut()
                            .entity_mut(index)
                            .map_or(0.0, |entity| entity.health),
                    ),
                }
            }
            GameValueId::MaxHealth => match self.primary(stream)? {
                Target::Player(index) => value::number(
                    self.world_mut()
                        .player_mut(index)
                        .map_or(0.0, |player| player.max_health),
                ),
                Target::Entity(index) => value::number(
                    self.world_mut()
                        .entity_mut(index)
                        .map_or(0.0, |entity| entity.max_health),
                ),
            },
            GameValueId::AbsorptionHealth => match self.primary(stream)? {
                Target::Player(index) => value::number(
                    self.world_mut()
                        .player_mut(index)
                        .map_or(0.0, |player| player.absorption_health),
                ),
                Target::Entity(index) => value::number(
                    self.world_mut()
                        .entity_mut(index)
                        .map_or(0.0, |entity| entity.absorption_health),
                ),
            },
            GameValueId::Gamemode => match self.primary(stream)? {
                Target::Player(index) => value::text(
                    self.world_mut()
                        .player_mut(index)
                        .map_or_else(|| "SURVIVAL".to_owned(), |p| p.game_mode.clone()),
                ),
                Target::Entity(_) => {
                    self.unimplemented("the game value 'gamemode' of an entity")?;
                    return Ok(None);
                }
            },
            GameValueId::FoodLevel => match self.primary(stream)? {
                Target::Player(index) => {
                    value::number(self.world_mut().player_mut(index).map_or(20.0, |p| p.food))
                }
                Target::Entity(_) => {
                    self.unimplemented("the game value 'food_level' of an entity")?;
                    return Ok(None);
                }
            },
            GameValueId::FoodSaturation => match self.primary(stream)? {
                Target::Player(index) => value::number(
                    self.world_mut()
                        .player_mut(index)
                        .map_or(5.0, |p| p.saturation),
                ),
                Target::Entity(_) => {
                    self.unimplemented("the game value 'food_saturation' of an entity")?;
                    return Ok(None);
                }
            },
            GameValueId::ExperienceLevel => match self.primary(stream)? {
                Target::Player(index) => value::number(
                    self.world_mut()
                        .player_mut(index)
                        .map_or(0.0, |p| p.experience),
                ),
                Target::Entity(_) => {
                    self.unimplemented("the game value 'experience_level' of an entity")?;
                    return Ok(None);
                }
            },
            GameValueId::FireTicks => match self.primary(stream)? {
                Target::Player(index) => value::number(
                    self.world_mut()
                        .player_mut(index)
                        .map_or(0.0, |p| p.fire_ticks),
                ),
                Target::Entity(index) => value::number(
                    self.world_mut()
                        .entity_mut(index)
                        .map_or(0.0, |e| e.fire_ticks),
                ),
            },
            GameValueId::WorldWeather => value::text(self.world().weather().to_owned()),
            GameValueId::WorldTime | GameValueId::WorldGameTime => {
                value::number(self.world().world_time())
            }
            other => {
                self.unimplemented(format!("the game value '{}'", game_value_name(other)))?;
                return Ok(None);
            }
        };
        Ok(Some(value))
    }
}

/// A game value's name for an error message.
///
/// Taken from the same `serde` representation the value is read from JSON by, so
/// the message ends up with exactly the name that stands in the module.
#[tracing::instrument(level = "debug", fields(id = ?id))]
fn game_value_name(id: GameValueId) -> String {
    serde_json::to_value(id)
        .ok()
        .and_then(|value| match value {
            serde_json::Value::String(name) => Some(name),
            _ => None,
        })
        .unwrap_or_else(|| "<unknown game value>".to_owned())
}
