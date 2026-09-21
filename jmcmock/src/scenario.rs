//! Декларативные JSON-сценарии для тестирования и симуляции поведения мира и игроков.
//!
//! Сценарий позволяет описать многопользовательскую последовательность действий:
//! подключение игроков с заданными никами, вызов событий от их имени, клики,
//! сообщения в чат, проверку записей в журнале и состояния мира.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, RuntimeError};
use crate::run::{EventData, Runtime};
use crate::world::{Position, Target};

/// Сценарий симуляции для `jmcmock`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scenario {
    /// Название сценария.
    #[serde(default)]
    pub name: Option<String>,

    /// Описание сценария.
    #[serde(default)]
    pub description: Option<String>,

    /// Список игроков, создаваемых до начала сценария.
    #[serde(default)]
    pub initial_players: Vec<String>,

    /// Отключить создание игрока `Dev` по умолчанию.
    #[serde(default)]
    pub no_default_player: bool,

    /// Последовательность шагов сценария.
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
}

/// Отдельный шаг сценария.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScenarioStep {
    /// Добавление нового игрока в мир.
    AddPlayer {
        /// Никнейм игрока.
        name: String,
        /// Координата X (по умолчанию 0.0).
        #[serde(default)]
        x: Option<f64>,
        /// Координата Y (по умолчанию 64.0).
        #[serde(default)]
        y: Option<f64>,
        /// Координата Z (по умолчанию 0.0).
        #[serde(default)]
        z: Option<f64>,
    },

    /// Удаление игрока из мира.
    RemovePlayer {
        /// Никнейм удаляемого игрока.
        name: String,
    },

    /// Вызов события от имени игрока или мира.
    Event {
        /// Название события (`world_start`, `player_join`, `player_chat`, `player_click_inventory` и др.).
        event: String,
        /// Никнейм игрока, от которого происходит событие (цель события).
        #[serde(default)]
        player: Option<String>,
        /// Текст сообщения в чате (`event_chat_message`).
        #[serde(default)]
        chat: Option<String>,
        /// Номер слота (`event_slot`).
        #[serde(default)]
        slot: Option<f64>,
        /// Заголовок инвентаря (`open_inventory_title`).
        #[serde(default)]
        title: Option<String>,
    },

    /// Прямой вызов функции модуля.
    CallFunction {
        /// Имя функции.
        name: String,
    },

    /// Продвижение игрового времени (в тиках).
    Wait {
        /// Количество тиков.
        #[serde(default = "default_wait_ticks")]
        ticks: u64,
    },

    /// Проверка записей в журнале мира (`log`).
    AssertLog {
        /// Подстрока, которая ОБЯЗАНА присутствовать хотя бы в одной записи журнала.
        #[serde(default)]
        contains: Option<String>,
        /// Подстрока, которой НЕ ДОЛЖНО быть ни в одной записи журнала.
        #[serde(default)]
        not_contains: Option<String>,
        /// Точная строка, которая должна присутствовать в журнале.
        #[serde(default)]
        exact: Option<String>,
    },

    /// Очистка журнала логов мира.
    ClearLog,
}

const fn default_wait_ticks() -> u64 {
    1
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScenarioInput {
    Full(Scenario),
    Steps(Vec<ScenarioStep>),
}

impl Scenario {
    /// Парсит сценарий из JSON-строки.
    ///
    /// Поддерживает как полный формат объекта `{ "name": "...", "steps": [...] }`,
    /// так и простой массив шагов `[ { "type": "..." }, ... ]`.
    ///
    /// # Errors
    /// Возвращает ошибку при невалидном формате JSON.
    pub fn parse(json_text: &str) -> Result<Self> {
        let input: ScenarioInput =
            serde_json::from_str(json_text).map_err(|source| RuntimeError::Json {
                context: "scenario <- JSON".to_owned(),
                source,
            })?;

        Ok(match input {
            ScenarioInput::Full(s) => s,
            ScenarioInput::Steps(steps) => Self {
                steps,
                ..Self::default()
            },
        })
    }

    /// Загружает и парсит сценарий из файла.
    ///
    /// # Errors
    /// Возвращает ошибку ввода-вывода или ошибку парсинга.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| RuntimeError::Io {
            context: format!("scenario <- {}", path.display()),
            source,
        })?;
        Self::parse(&content)
    }

    /// Выполняет сценарий шаг за шагом на переданном экземпляре `Runtime`.
    ///
    /// # Errors
    /// Возвращает [`RuntimeError`] при сбое события, неизвестном игроке или
    /// несработавшем утверждении `AssertLog`.
    #[tracing::instrument(level = "debug", skip(self, runtime))]
    pub fn run(&self, runtime: &mut Runtime<'_>) -> Result<()> {
        if let Some(name) = &self.name {
            tracing::info!(scenario = %name, "Running scenario");
        }

        if self.no_default_player {
            runtime.world_mut().remove_player("Dev");
        }

        for player_name in &self.initial_players {
            runtime.world_mut().add_player(player_name.clone());
        }

        for (idx, step) in self.steps.iter().enumerate() {
            let step_num = idx + 1;
            execute_step(runtime, step, step_num)?;
        }

        Ok(())
    }
}

fn execute_step(runtime: &mut Runtime<'_>, step: &ScenarioStep, step_num: usize) -> Result<()> {
    match step {
        ScenarioStep::AddPlayer { name, x, y, z } => {
            let pos = Position {
                x: x.unwrap_or(0.0),
                y: y.unwrap_or(64.0),
                z: z.unwrap_or(0.0),
                yaw: 0.0,
                pitch: 0.0,
            };
            runtime.world_mut().add_player_at(name.clone(), pos);
            tracing::debug!(step = step_num, player = %name, "Added player to world");
        }

        ScenarioStep::RemovePlayer { name } => {
            let removed = runtime.world_mut().remove_player(name);
            tracing::debug!(step = step_num, player = %name, removed, "Removed player from world");
        }

        ScenarioStep::Event {
            event,
            player,
            chat,
            slot,
            title,
        } => {
            execute_event(
                runtime,
                step_num,
                event,
                player.as_deref(),
                chat.as_deref(),
                *slot,
                title.as_deref(),
            )?;
        }

        ScenarioStep::CallFunction { name } => {
            tracing::debug!(step = step_num, function = %name, "Calling scenario function");
            let _ignored: Option<jmcdata::module::Value<'_>> =
                runtime.call_function(name, Vec::new())?;
        }

        ScenarioStep::Wait { ticks } => {
            runtime.world_mut().advance(*ticks);
            tracing::debug!(step = step_num, ticks = *ticks, "Advanced game time");
        }

        ScenarioStep::AssertLog {
            contains,
            not_contains,
            exact,
        } => {
            execute_assert_log(
                runtime,
                step_num,
                contains.as_deref(),
                not_contains.as_deref(),
                exact.as_deref(),
            )?;
        }

        ScenarioStep::ClearLog => {
            runtime.world_mut().log_mut().clear();
            tracing::debug!(step = step_num, "Cleared world log");
        }
    }

    Ok(())
}

fn execute_event(
    runtime: &mut Runtime<'_>,
    step_num: usize,
    event: &str,
    player: Option<&str>,
    chat: Option<&str>,
    slot: Option<f64>,
    title: Option<&str>,
) -> Result<()> {
    let targets = if let Some(player_name) = player {
        if let Some(pos) = runtime
            .world()
            .players()
            .iter()
            .position(|p| p.name == player_name)
        {
            vec![Target::Player(pos)]
        } else {
            return Err(RuntimeError::AssertionFailed {
                step: step_num,
                message: format!("player '{player_name}' not found in mock world"),
            });
        }
    } else {
        Vec::new()
    };

    let event_data = EventData {
        targets,
        chat_message: chat.map(str::to_owned),
        slot,
        inventory_title: title.map(str::to_owned),
        ..EventData::default()
    };

    tracing::debug!(
        step = step_num,
        event = %event,
        ?player,
        ?chat,
        "Firing scenario event"
    );

    runtime.fire_event_with(event, event_data)
}

fn execute_assert_log(
    runtime: &Runtime<'_>,
    step_num: usize,
    contains: Option<&str>,
    not_contains: Option<&str>,
    exact: Option<&str>,
) -> Result<()> {
    let entries = runtime.world().log().entries();

    if let Some(expected) = contains {
        let found = entries.iter().any(|entry| entry.contains(expected));
        if !found {
            return Err(RuntimeError::AssertionFailed {
                step: step_num,
                message: format!(
                    "expected log to contain '{expected}', but it was not found. Current log ({} entries):\n{}",
                    entries.len(),
                    entries.join("\n")
                ),
            });
        }
    }

    if let Some(unexpected) = not_contains {
        let found = entries.iter().any(|entry| entry.contains(unexpected));
        if !found {
            return Err(RuntimeError::AssertionFailed {
                step: step_num,
                message: format!(
                    "expected log NOT to contain '{unexpected}', but a match was found in journal"
                ),
            });
        }
    }

    if let Some(exact_str) = exact {
        let found = entries.iter().any(|entry| entry == exact_str);
        if !found {
            return Err(RuntimeError::AssertionFailed {
                step: step_num,
                message: format!(
                    "expected log to contain exact line '{exact_str}', but it was not found"
                ),
            });
        }
    }

    tracing::debug!(step = step_num, "Log assertion passed");
    Ok(())
}
