//! Входная схема `JustMC`: типы, в которые читаются `assets/*.json`, и загрузка.
//!
//! Здесь записана только форма JSON. Что из неё генерируется — дело остальных
//! модулей.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Событие схемы.
#[derive(Deserialize)]
pub struct RawEvent {
    /// Идентификатор события.
    pub id: String,
    /// Может ли событие быть отменено.
    #[serde(default)]
    pub cancellable: bool,
}

/// Игровая величина: `id` и её `type`.
#[derive(Deserialize)]
pub struct RawGameValue {
    /// Идентификатор величины.
    pub id: String,
    /// Тип величины; попадает строкой в `GAME_VALUE_MAP`.
    #[serde(rename = "type")]
    pub value_type: String,
}

/// Аргумент действия — из `args`, `assign` или `lambda`.
#[derive(Deserialize, Clone)]
pub struct RawArg {
    /// Имя аргумента в схеме.
    pub id: String,
    /// Тип аргумента.
    #[serde(rename = "type")]
    pub arg_type: String,
    /// Размерность массива, если аргумент массивный.
    #[serde(default)]
    pub array: Option<u32>,
    /// Фиксированный набор значений, если аргумент — перечисление.
    pub values: Option<Vec<String>>,
}

/// Действие схемы целиком.
#[derive(Deserialize, Clone)]
pub struct RawAction {
    /// Идентификатор действия.
    pub id: String,
    /// Имя действия внутри `object`.
    pub name: String,
    /// Группа действия: `variable`, `player`, `world`, …
    pub object: String,
    /// Аргументы в порядке схемы.
    pub args: Vec<RawArg>,
    /// Вид действия: `basic`, `container`, `basic_with_conditional`, …
    #[serde(rename = "type")]
    pub action_type: String,
    /// Аргумент, в который действие пишет результат.
    #[serde(default)]
    pub assign: Option<Vec<RawArg>>,
    /// Имя аргумента, в который попадает получатель вызова метода.
    #[serde(default)]
    pub origin: Option<String>,
    /// Годится ли действие как условие.
    #[serde(default)]
    pub boolean: bool,
    /// Тело, которое действие выполнит само.
    #[serde(default)]
    pub lambda: Option<Vec<RawArg>>,
}

/// Выделения схемы, разложенные по видам.
#[derive(Deserialize)]
pub struct RawSelectors {
    /// Выделения игрока.
    pub player: Vec<String>,
    /// Выделения сущности.
    pub entity: Vec<String>,
    /// Выделения игровой величины.
    pub game_value: Vec<String>,
}

/// Всё, что читается из `assets/*.json`.
pub struct Assets {
    /// События.
    pub events: Vec<RawEvent>,
    /// Игровые величины.
    pub game_values: Vec<RawGameValue>,
    /// Действия.
    pub actions: Vec<RawAction>,
    /// Выделения.
    pub selectors: RawSelectors,
}

/// Читает `assets/*.json`; каталог отсчитывается от каталога пакета.
pub fn load_assets() -> Result<Assets, Box<dyn Error>> {
    let dir = PathBuf::from("assets");
    Ok(Assets {
        events: read_json(&dir.join("events.json"))?,
        game_values: read_json(&dir.join("game_values.json"))?,
        actions: read_json(&dir.join("actions.json"))?,
        selectors: read_json(&dir.join("selectors.json"))?,
    })
}

/// Читает и разбирает один JSON-файл схемы.
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}
