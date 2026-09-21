//! Таблицы поиска: `phf`-карты по действиям, игровым величинам и выделениям
//! плюс accessor-функции к ним.

use std::fmt::Write as _;

use heck::ToPascalCase as _;

use crate::actions::gen_action_defs;
use crate::assets::{Assets, RawAction};

/// Снимает приставки `player_`/`_player` и `entity_`/`_entity`, включая
/// множественное число.
fn normalize_selector(s: &str, entity_type: &str) -> String {
    let prefix = format!("{entity_type}_");
    let suffix = format!("_{entity_type}");

    let res = s.trim_start_matches(&prefix);
    res.strip_suffix(&format!("{suffix}s"))
        .map_or_else(|| res.trim_end_matches(&suffix).to_owned(), str::to_owned)
}

/// Ключ — пара `(object, name)`, поэтому поиску не нужна аллокация `String`.
fn gen_action_id_map(actions: &[RawAction]) -> phf_codegen::Map<'_, (&str, &str)> {
    actions
        .iter()
        .map(|action| {
            (
                (action.object.as_str(), action.name.as_str()),
                format!("ActionId::{}", action.id.to_pascal_case()),
            )
        })
        .collect()
}

/// Ключ — та же пара `(object, name)`; значения ссылаются на [`gen_action_defs`].
fn gen_action_def_map(actions: &[RawAction]) -> phf_codegen::Map<'_, (&str, &str)> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            (
                (action.object.as_str(), action.name.as_str()),
                format!("&ACTION_DEFS[{index}]"),
            )
        })
        .collect()
}

/// Обратный индекс: схемный id действия -> его описание.
///
/// `phf_codegen` хеширует ключи во время сборки, а `ActionId` появляется только
/// после прогона этого скрипта, поэтому карта ключуется строкой id, а
/// `ActionId::as_str` превращает вариант в этот ключ.
fn gen_action_def_by_id_map(actions: &[RawAction]) -> phf_codegen::Map<'_, &str> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| (action.id.as_str(), format!("&ACTION_DEFS[{index}]")))
        .collect()
}

/// И исходное, и приведённое написание выделения ведут к исходному, поэтому
/// поиск принимает обе формы.
fn gen_selector_map(
    selectors: &[String],
    normalize: impl Fn(&str) -> String,
) -> phf_codegen::Map<'_, String> {
    let mut map = phf_codegen::Map::new();
    for sel in selectors {
        let value = format!("\"{sel}\"");
        map.entry(sel.clone(), value.clone());
        let normalized = normalize(sel);
        if normalized != *sel {
            map.entry(normalized, value);
        }
    }
    map
}

/// `pub static`-таблица и сразу за ней функция, которая её читает, — форма,
/// в которой сделан каждый поиск здесь.
fn write_lookup(out: &mut String, decl: &str, table: String, accessor: &str) {
    writeln!(out, "pub static {decl} = {table};\n").expect("writing to String cannot fail");
    out.push_str(accessor);
}

/// Все `phf`-таблицы и accessor-функции к ним одним текстом.
#[expect(
    clippy::too_many_lines,
    reason = "Generates all lookup maps and accessors for the schema"
)]
pub fn gen_lookup_maps(assets: &Assets) -> String {
    let mut out = gen_action_defs(&assets.actions);

    write_lookup(
        &mut out,
        "ACTION_ID_MAP: phf::Map<(&'static str, &'static str), ActionId>",
        gen_action_id_map(&assets.actions).build().to_string(),
        "#[must_use]\n\
         pub fn get_action_id(object: &str, name: &str) -> Option<ActionId> {\n    \
         ACTION_ID_MAP.get(&(object, name)).copied()\n}\n\n",
    );

    write_lookup(
        &mut out,
        "ACTION_DEF_MAP: phf::Map<(&'static str, &'static str), &'static ActionDef>",
        gen_action_def_map(&assets.actions).build().to_string(),
        "#[must_use]\n\
         pub fn get_action_def(object: &str, name: &str) -> Option<&'static ActionDef> {\n    \
         ACTION_DEF_MAP.get(&(object, name)).copied()\n}\n\n",
    );

    write_lookup(
        &mut out,
        "ACTION_DEF_BY_ID: phf::Map<&'static str, &'static ActionDef>",
        gen_action_def_by_id_map(&assets.actions)
            .build()
            .to_string(),
        "#[must_use]\n\
         pub fn get_action_def_by_id(id: ActionId) -> Option<&'static ActionDef> {\n    \
         ACTION_DEF_BY_ID.get(id.as_str()).copied()\n}\n\n",
    );

    let gv_defs = assets
        .game_values
        .iter()
        .map(|gv| {
            format!(
                "GameValueDef {{ id: \"{}\", value_type: \"{}\" }}",
                gv.id, gv.value_type
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        &mut out,
        "pub static GAME_VALUE_DEFS: [GameValueDef; {}] = [{gv_defs}];\n\n\
         #[must_use]\n\
         pub fn get_game_values() -> &'static [GameValueDef] {{\n    \
             &GAME_VALUE_DEFS\n\
         }}\n",
        assets.game_values.len()
    )
    .expect("writing to String cannot fail");

    write_lookup(
        &mut out,
        "GAME_VALUE_DEF_MAP: phf::Map<&'static str, &'static GameValueDef>",
        assets
            .game_values
            .iter()
            .enumerate()
            .map(|(index, gv)| (gv.id.as_str(), format!("&GAME_VALUE_DEFS[{index}]")))
            .collect::<phf_codegen::Map<'_, &str>>()
            .build()
            .to_string(),
        "#[must_use]\n\
         pub fn get_game_value(id: &str) -> Option<&'static GameValueDef> {\n    \
         GAME_VALUE_DEF_MAP.get(id).copied()\n}\n\n",
    );

    write_lookup(
        &mut out,
        "GAME_VALUE_MAP: phf::Map<&'static str, &'static str>",
        assets
            .game_values
            .iter()
            .map(|gv| (gv.id.clone(), format!("\"{}\"", gv.value_type)))
            .collect::<phf_codegen::Map<'_, String>>()
            .build()
            .to_string(),
        "#[must_use]\n\
         pub fn get_game_value_type(id: &str) -> Option<&'static str> {\n    \
         GAME_VALUE_MAP.get(id).copied()\n}\n\n",
    );

    let ev_defs = assets
        .events
        .iter()
        .map(|ev| {
            format!(
                "EventDef {{ id: \"{}\", cancellable: {} }}",
                ev.id, ev.cancellable
            )
        })
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        &mut out,
        "pub static EVENT_DEFS: [EventDef; {}] = [{ev_defs}];\n\n\
         #[must_use]\n\
         pub fn get_event_defs() -> &'static [EventDef] {{\n    \
             &EVENT_DEFS\n\
         }}\n",
        assets.events.len()
    )
    .expect("writing to String cannot fail");

    write_lookup(
        &mut out,
        "EVENT_DEF_MAP: phf::Map<&'static str, &'static EventDef>",
        assets
            .events
            .iter()
            .enumerate()
            .map(|(index, ev)| (ev.id.as_str(), format!("&EVENT_DEFS[{index}]")))
            .collect::<phf_codegen::Map<'_, &str>>()
            .build()
            .to_string(),
        "#[must_use]\n\
         pub fn get_event_def(id: &str) -> Option<&'static EventDef> {\n    \
         EVENT_DEF_MAP.get(id).copied()\n}\n\n",
    );

    write_lookup(
        &mut out,
        "EVENT_CANCELLABLE_MAP: phf::Map<&'static str, bool>",
        assets
            .events
            .iter()
            .map(|ev| {
                (
                    ev.id.clone(),
                    if ev.cancellable { "true" } else { "false" }.to_string(),
                )
            })
            .collect::<phf_codegen::Map<'_, String>>()
            .build()
            .to_string(),
        "#[must_use]\n\
         pub fn is_event_cancellable(event_id: &str) -> bool {\n    \
         EVENT_CANCELLABLE_MAP.get(event_id).copied().unwrap_or(false)\n}\n\n",
    );

    let p_sels = assets
        .selectors
        .player
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let e_sels = assets
        .selectors
        .entity
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let gv_sels = assets
        .selectors
        .game_value
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(
        &mut out,
        "pub static SELECTORS_PLAYER: &[&str] = &[{p_sels}];\n\
         pub static SELECTORS_ENTITY: &[&str] = &[{e_sels}];\n\
         pub static SELECTORS_GAME_VALUE: &[&str] = &[{gv_sels}];\n\n\
         #[must_use]\n\
         pub fn get_player_selectors() -> &'static [&'static str] {{\n    \
             SELECTORS_PLAYER\n\
         }}\n\
         #[must_use]\n\
         pub fn get_entity_selectors() -> &'static [&'static str] {{\n    \
             SELECTORS_ENTITY\n\
         }}\n\
         #[must_use]\n\
         pub fn get_game_value_selectors() -> &'static [&'static str] {{\n    \
             SELECTORS_GAME_VALUE\n\
         }}\n"
    )
    .expect("writing to String cannot fail");

    write_lookup(
        &mut out,
        "SELECTOR_PLAYER_MAP: phf::Map<&'static str, &'static str>",
        gen_selector_map(&assets.selectors.player, |s| {
            normalize_selector(s, "player")
        })
        .build()
        .to_string(),
        "",
    );
    write_lookup(
        &mut out,
        "SELECTOR_ENTITY_MAP: phf::Map<&'static str, &'static str>",
        gen_selector_map(&assets.selectors.entity, |s| {
            normalize_selector(s, "entity")
        })
        .build()
        .to_string(),
        "",
    );
    write_lookup(
        &mut out,
        "SELECTOR_GAME_VALUE_MAP: phf::Map<&'static str, &'static str>",
        gen_selector_map(&assets.selectors.game_value, |sel| {
            if sel == "default_entity" {
                sel.to_owned()
            } else {
                normalize_selector(sel, "entity")
            }
        })
        .build()
        .to_string(),
        "",
    );

    let object_aliases: &[(&str, &str)] = &[
        ("player", "player"),
        ("игрок", "player"),
        ("entity", "entity"),
        ("сущность", "entity"),
        ("world", "world"),
        ("мир", "world"),
        ("variable", "variable"),
        ("переменная", "variable"),
        ("value", "value"),
        ("значение", "value"),
        ("code", "code"),
        ("код", "code"),
        ("select", "select"),
        ("выборка", "select"),
        ("repeat", "repeat"),
        ("повтор", "repeat"),
        ("повторение", "repeat"),
        ("controller", "controller"),
        ("контроллер", "controller"),
        ("event", "event"),
        ("событие", "event"),
        ("item", "item"),
        ("предмет", "item"),
        ("block", "block"),
        ("блок", "block"),
    ];

    let mut obj_map = phf_codegen::Map::new();
    for (alias, canonical) in object_aliases {
        obj_map.entry(alias.to_string(), format!("\"{canonical}\""));
    }

    write_lookup(
        &mut out,
        "OBJECT_CANONICAL_MAP: phf::Map<&'static str, &'static str>",
        obj_map.build().to_string(),
        "#[must_use]\n\
         pub fn canonicalize_object(name: &str) -> Option<&'static str> {\n    \
         OBJECT_CANONICAL_MAP.get(name).copied()\n}\n\n\
         pub fn get_actions_for_object(object: &str) -> impl Iterator<Item = &'static ActionDef> {\n    \
         let canonical = canonicalize_object(object).unwrap_or(object);\n    \
         ACTION_DEFS.iter().filter(move |a| a.object == canonical)\n}\n\n\
         pub static KNOWN_OBJECTS_PAIRS: &[(&str, &str)] = &[\n    \
             (\"player\", \"игрок\"),\n    \
             (\"entity\", \"сущность\"),\n    \
             (\"world\", \"мир\"),\n    \
             (\"variable\", \"переменная\"),\n    \
             (\"value\", \"значение\"),\n    \
             (\"code\", \"код\"),\n    \
             (\"select\", \"выборка\"),\n    \
             (\"repeat\", \"повтор\"),\n    \
             (\"controller\", \"контроллер\"),\n    \
             (\"item\", \"предмет\"),\n    \
             (\"block\", \"блок\"),\n\
         ];\n\n\
         #[must_use]\n\
         pub fn get_known_objects() -> &'static [(&'static str, &'static str)] {\n    \
             KNOWN_OBJECTS_PAIRS\n\
         }\n\n",
    );

    out
}
