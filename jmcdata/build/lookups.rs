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

    out
}
