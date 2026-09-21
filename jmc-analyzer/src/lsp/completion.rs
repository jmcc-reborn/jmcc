#![allow(clippy::literal_string_with_formatting_args)]

//! Autocomplete and `IntelliSense` provider for `JustCode` and `JustMC` schema.

use std::collections::HashSet;

use jmcc::ast::*;
use jmcc::i18n::Lang;
use jmcc::ir::ctx::{ClassInfo, IrCtx};
use jmcdata::generated::{
    canonicalize_object, get_actions_for_object, get_entity_selectors, get_event_defs,
    get_game_value_selectors, get_game_values, get_known_objects, get_player_selectors,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position,
};

use super::state::{DocumentData, walk_statements};

fn try_complete_scoped(doc: &DocumentData, trimmed: &str) -> Option<Vec<CompletionItem>> {
    if let Some(colon_pos) = trimmed.rfind("::") {
        let after_colons = &trimmed[colon_pos + 2..];
        if after_colons
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
        {
            let object_prefix = trimmed[..colon_pos].trim_end();
            let ident = object_prefix
                .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");

            if !ident.is_empty() {
                if let Some(canonical) = canonicalize_object(ident) {
                    if canonical == "value" {
                        return Some(complete_justmc_game_values(ident, doc.lang));
                    }
                    if canonical == "event" {
                        return Some(complete_justmc_events(doc.lang));
                    }
                    return Some(complete_justmc_actions(canonical, ident, doc.lang));
                }

                if let Some(ir_ctx) = &doc.ir_ctx
                    && let Some(def_id) = ir_ctx.enums_by_name.get(ident)
                    && let Some(enum_info) = ir_ctx.enums_by_def.get(def_id)
                {
                    return Some(complete_enum_variants(enum_info));
                }
            }
        }
    }
    None
}

/// Builds completion suggestions based on cursor context.
#[must_use]
pub fn provide_completions(doc: &DocumentData, params: &CompletionParams) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    let text = &doc.text;
    let pos = params.text_document_position.position;

    // Find current line text up to cursor position
    let line_text = get_line_prefix(text, pos);
    let trimmed = line_text.trim_end();

    // 1. Check if user typed `object::` or `object::prefix`
    if let Some(res) = try_complete_scoped(doc, trimmed) {
        return res;
    }

    // 2. Check if user typed `obj.` or `obj.member_prefix`
    if let Some(dot_pos) = trimmed.rfind('.') {
        let after_dot = &trimmed[dot_pos + 1..];
        if after_dot.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let obj_prefix = &trimmed[..dot_pos];
            let ident = obj_prefix
                .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("");

            if !ident.is_empty() {
                // Check if it's an enum (e.g. MessageType.TEXT)
                if let Some(ir_ctx) = &doc.ir_ctx
                    && let Some(def_id) = ir_ctx.enums_by_name.get(ident)
                    && let Some(enum_info) = ir_ctx.enums_by_def.get(def_id)
                {
                    return complete_enum_variants(enum_info);
                }

                if let Some(methods) = complete_class_members_by_var_name(doc, ident) {
                    return methods;
                }
            }
        }
    }

    // 3. Check if user typed `<` for selectors or event generic
    if let Some(angle_pos) = trimmed.rfind('<') {
        let after_angle = &trimmed[angle_pos + 1..];
        if after_angle.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let before_angle = trimmed[..angle_pos].trim_end();
            if before_angle.ends_with("event") || before_angle.ends_with("событие") {
                return complete_justmc_events(doc.lang);
            }
            if let Some(colon_pos) = before_angle.rfind("::") {
                let obj_part = &before_angle[..colon_pos];
                let obj_ident = obj_part
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if let Some(norm_obj) = canonicalize_object(obj_ident)
                    && let Some(selectors) = complete_selectors_for_object(norm_obj, doc.lang)
                {
                    return selectors;
                }
            }
        }
    }

    // 4. Check if user typed `@` for decorators
    if let Some(at_pos) = trimmed.rfind('@') {
        let after_at = &trimmed[at_pos + 1..];
        if after_at.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return decorator_completions(doc.lang);
        }
    }

    // 5. Check if user typed `event ` or `событие `
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if (words.len() == 1
        && (words[0] == "event" || words[0] == "событие")
        && line_text.ends_with(' '))
        || (words.len() == 2 && (words[0] == "event" || words[0] == "событие"))
    {
        return complete_justmc_events(doc.lang);
    }

    // 6. General completions: Keywords, types, JustMC objects, global symbols, local vars
    items.extend(keyword_completions(doc.lang));
    items.extend(type_completions(doc.lang));
    items.extend(builtin_object_completions(doc.lang));

    if let Some(ir_ctx) = &doc.ir_ctx {
        items.extend(global_symbol_completions(ir_ctx, doc.lang));
    }

    if let Some(ast) = &doc.ast {
        items.extend(local_variable_completions(ast, text, pos));
    }

    items
}

fn get_line_prefix(text: &str, pos: Position) -> String {
    for (current_line, line) in text.split('\n').enumerate() {
        if current_line as u32 == pos.line {
            let col = (pos.character as usize).min(line.len());
            return line[..col].trim_end_matches('\r').to_string();
        }
    }
    String::new()
}

fn complete_justmc_actions(canonical: &str, display_obj: &str, lang: Lang) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for def in get_actions_for_object(canonical) {
        let name = def.name;
        let mut args_doc = String::new();
        let mut snippet_args = Vec::new();
        for (i, arg) in def.args.iter().enumerate() {
            let ty = arg.arg_type;
            args_doc.push_str(&format!("\n- `{}`: `{}`", arg.id, ty));
            snippet_args.push(format!("${{{}:{}}}", i + 1, arg.id));
        }

        let detail = if def.args.is_empty() {
            format!("{display_obj}::{name}()")
        } else {
            let arg_summary: Vec<_> = def
                .args
                .iter()
                .map(|a| format!("{}: {}", a.id, a.arg_type))
                .collect();
            format!("{display_obj}::{name}({})", arg_summary.join(", "))
        };

        let insert_text = if snippet_args.is_empty() {
            format!("{name}()")
        } else {
            format!("{name}({})", snippet_args.join(", "))
        };

        let doc_text = if lang == Lang::Ru {
            format!(
                "### `{display_obj}::{name}`\n**Действие JustMC**\n- Тип: `{}`\n\n**Параметры:**{}",
                def.action_type, args_doc
            )
        } else {
            format!(
                "### `{display_obj}::{name}`\n**JustMC Action**\n- Type: `{}`\n\n**Parameters:**{}",
                def.action_type, args_doc
            )
        };

        items.push(CompletionItem {
            label: (*name).to_owned(),
            kind: Some(CompletionItemKind::METHOD),
            detail: Some(detail),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc_text,
            })),
            insert_text: Some(insert_text),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn complete_justmc_game_values(display_obj: &str, lang: Lang) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let is_ru = lang == Lang::Ru;

    for gv in get_game_values() {
        let name = gv.id;
        let val_type = gv.value_type;

        let detail = if is_ru {
            format!("{display_obj}::{name} -> {val_type} (игровое значение)")
        } else {
            format!("{display_obj}::{name} -> {val_type} (game value)")
        };

        let doc_text = if is_ru {
            format!(
                "### `{display_obj}::{name}`\n**Игровое значение JustMC**\n- Тип: `{val_type}`\n- Идентификатор: `{name}`\n\n**Пример:**\n```jc\n{display_obj}::{name}\n{display_obj}::{name}<default>\n```"
            )
        } else {
            format!(
                "### `{display_obj}::{name}`\n**JustMC Game Value**\n- Type: `{val_type}`\n- Identifier: `{name}`\n\n**Example:**\n```jc\n{display_obj}::{name}\n{display_obj}::{name}<default>\n```"
            )
        };

        items.push(CompletionItem {
            label: (*name).to_owned(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(detail),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc_text,
            })),
            insert_text: Some((*name).to_owned()),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn complete_justmc_events(lang: Lang) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let is_ru = lang == Lang::Ru;

    for ev in get_event_defs() {
        let event_id = ev.id;
        let cancellable = ev.cancellable;
        let cancel_str = if cancellable {
            if is_ru {
                "отменяемое"
            } else {
                "cancellable"
            }
        } else if is_ru {
            "неотменяемое"
        } else {
            "not cancellable"
        };

        let detail = if is_ru {
            format!("Событие: {event_id} ({cancel_str})")
        } else {
            format!("Event: {event_id} ({cancel_str})")
        };

        let doc_text = if is_ru {
            format!("### Событие JustMC `{event_id}`\n- Отменяемое: `{cancellable}`")
        } else {
            format!("### JustMC Event `{event_id}`\n- Cancellable: `{cancellable}`")
        };

        items.push(CompletionItem {
            label: (*event_id).to_owned(),
            kind: Some(CompletionItemKind::EVENT),
            detail: Some(detail),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc_text,
            })),
            insert_text: Some((*event_id).to_owned()),
            ..Default::default()
        });
    }

    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

fn complete_selectors_for_object(object: &str, lang: Lang) -> Option<Vec<CompletionItem>> {
    let canonical = canonicalize_object(object).unwrap_or(object);
    let selectors: &[&str] = match canonical {
        "value" => get_game_value_selectors(),
        "player" => get_player_selectors(),
        "entity" => get_entity_selectors(),
        _ => return None,
    };

    let is_ru = lang == Lang::Ru;
    let mut items: Vec<CompletionItem> = selectors
        .iter()
        .map(|sel| {
            let detail = if is_ru {
                format!("Селектор JustMC: {sel}")
            } else {
                format!("JustMC selector: {sel}")
            };
            CompletionItem {
                label: (*sel).to_owned(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some(detail),
                insert_text: Some(format!("{sel}>")),
                ..Default::default()
            }
        })
        .collect();

    items.sort_by(|a, b| a.label.cmp(&b.label));
    Some(items)
}

fn decorator_completions(lang: Lang) -> Vec<CompletionItem> {
    let is_ru = lang == Lang::Ru;
    let decorators = [
        (
            "alias",
            "alias(\"${1:name}\")",
            desc(
                is_ru,
                "Псевдоним метода/функции",
                "Method or function alias",
            ),
        ),
        (
            "lang_item",
            "lang_item(\"${1:name}\")",
            desc(is_ru, "Языковой элемент", "Language item mapping"),
        ),
        (
            "overload",
            "overload",
            desc(is_ru, "Перегрузка метода", "Method overload"),
        ),
        (
            "getter",
            "getter",
            desc(is_ru, "Геттер свойства", "Property getter"),
        ),
        (
            "setter",
            "setter",
            desc(is_ru, "Сеттер свойства", "Property setter"),
        ),
        (
            "hidden",
            "hidden",
            desc(is_ru, "Скрытый элемент", "Hidden item"),
        ),
        (
            "description",
            "description(\"${1:text}\")",
            desc(is_ru, "Описание элемента", "Item description"),
        ),
    ];

    decorators
        .into_iter()
        .map(|(name, snippet, d)| CompletionItem {
            label: name.to_owned(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(d.to_owned()),
            insert_text: Some(snippet.to_owned()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

fn complete_enum_variants(info: &jmcc::ir::ctx::EnumInfo) -> Vec<CompletionItem> {
    info.values
        .iter()
        .map(|val| CompletionItem {
            label: val.clone(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some(format!("Вариант перечисления {}::{}", info.name, val)),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "```jc\n{}::{val}\n```\nЗначение перечисления `{}`",
                    info.name, info.name
                ),
            })),
            ..Default::default()
        })
        .collect()
}

fn complete_class_members_by_var_name(
    doc: &DocumentData,
    var_name: &str,
) -> Option<Vec<CompletionItem>> {
    let ir_ctx = doc.ir_ctx.as_ref()?;
    let ast = doc.ast.as_ref()?;

    // Search for var type in VarDecl or Function parameters
    let mut found_type_name = None;
    for stmt in &ast.statements {
        if let Statement::VarDecl(v) = stmt {
            for (i, name) in v.names.iter().enumerate() {
                let name_str = jmcc::ast::text_value_to_string(ast, name);
                if name_str == var_name
                    && let Some(Some(ty_id)) = v.tys.get(i)
                {
                    found_type_name = Some(ast.strings.resolve(ty_id).to_owned());
                }
            }
        }
    }

    let type_name = found_type_name?;
    let def_id = ir_ctx.classes_by_name.get(&type_name)?;
    let class_info = ir_ctx.classes_by_def.get(def_id)?;

    Some(collect_class_members(ast, ir_ctx, class_info))
}

fn collect_class_members(ast: &Ast, ir_ctx: &IrCtx, info: &ClassInfo) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    let mut current = Some(info);
    while let Some(cls) = current {
        for (m_name, m_decl) in &cls.methods {
            if seen.insert(m_name.clone()) {
                let mut snippet_args = Vec::new();
                let mut param_types = Vec::new();
                for (i, p) in m_decl.params.iter().enumerate() {
                    let p_name = ast.strings.resolve(&p.name);
                    let p_ty = p
                        .ty
                        .map_or_else(|| "any".to_owned(), |t| ast.strings.resolve(&t).to_owned());
                    snippet_args.push(format!("${{{}:{p_name}}}", i + 1));
                    param_types.push(format!("{p_name}: {p_ty}"));
                }
                items.push(CompletionItem {
                    label: m_name.clone(),
                    kind: Some(CompletionItemKind::METHOD),
                    detail: Some(format!("fn {}({})", m_name, param_types.join(", "))),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!(
                            "```jc\nfunction {m_name}({})\n```\nМетод класса `{}`",
                            param_types.join(", "),
                            cls.name
                        ),
                    })),
                    insert_text: Some(format!("{m_name}()")),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
        }

        for (f_name, (f_ty, _)) in &cls.fields {
            if seen.insert(f_name.clone()) {
                items.push(CompletionItem {
                    label: f_name.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format!("{f_name}: {f_ty}")),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!(
                            "```jc\nvar {f_name}: {f_ty}\n```\nПоле класса `{}`",
                            cls.name
                        ),
                    })),
                    ..Default::default()
                });
            }
        }

        current = cls.parent.and_then(|p_id| ir_ctx.classes_by_def.get(&p_id));
    }

    items
}

fn type_completions(lang: Lang) -> Vec<CompletionItem> {
    let is_ru = lang == Lang::Ru;
    let types = [
        ("number", desc(is_ru, "Числовой тип", "Number type")),
        ("число", desc(is_ru, "Числовой тип", "Number type")),
        ("text", desc(is_ru, "Текстовый тип", "Text type")),
        ("текст", desc(is_ru, "Текстовый тип", "Text type")),
        ("boolean", desc(is_ru, "Логический тип", "Boolean type")),
        ("логическое", desc(is_ru, "Логический тип", "Boolean type")),
        ("булево", desc(is_ru, "Логический тип", "Boolean type")),
        ("array", desc(is_ru, "Тип массива", "Array type")),
        ("массив", desc(is_ru, "Тип массива", "Array type")),
        (
            "список",
            desc(is_ru, "Тип массива/списка", "Array/list type"),
        ),
        ("map", desc(is_ru, "Тип словаря", "Map type")),
        ("словарь", desc(is_ru, "Тип словаря", "Map type")),
        ("карта", desc(is_ru, "Тип словаря/карты", "Map type")),
        (
            "location",
            desc(is_ru, "Тип местоположения", "Location type"),
        ),
        (
            "местоположение",
            desc(is_ru, "Тип местоположения", "Location type"),
        ),
        ("локация", desc(is_ru, "Тип локации", "Location type")),
        ("item", desc(is_ru, "Тип предмета", "Item type")),
        ("предмет", desc(is_ru, "Тип предмета", "Item type")),
        ("vector", desc(is_ru, "Тип вектора", "Vector type")),
        ("вектор", desc(is_ru, "Тип вектора", "Vector type")),
        ("sound", desc(is_ru, "Тип звука", "Sound type")),
        ("звук", desc(is_ru, "Тип звука", "Sound type")),
        ("particle", desc(is_ru, "Тип частицы", "Particle type")),
        ("частица", desc(is_ru, "Тип частицы", "Particle type")),
        ("potion", desc(is_ru, "Тип зелья", "Potion type")),
        ("зелье", desc(is_ru, "Тип зелья", "Potion type")),
        ("block", desc(is_ru, "Тип блока", "Block type")),
        ("блок", desc(is_ru, "Тип блока", "Block type")),
        ("entity", desc(is_ru, "Тип сущности", "Entity type")),
        ("сущность", desc(is_ru, "Тип сущности", "Entity type")),
        ("player", desc(is_ru, "Тип игрока", "Player type")),
        ("игрок", desc(is_ru, "Тип игрока", "Player type")),
        ("any", desc(is_ru, "Любой тип", "Any type")),
        ("любой", desc(is_ru, "Любой тип", "Any type")),
        ("iterator", desc(is_ru, "Тип итератора", "Iterator type")),
        ("итератор", desc(is_ru, "Тип итератора", "Iterator type")),
    ];

    types
        .into_iter()
        .map(|(label, d)| CompletionItem {
            label: (*label).to_owned(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some((*d).to_owned()),
            insert_text: Some((*label).to_owned()),
            ..Default::default()
        })
        .collect()
}

fn builtin_object_completions(lang: Lang) -> Vec<CompletionItem> {
    let is_ru = lang == Lang::Ru;
    let mut items = Vec::new();

    for &(en, ru) in get_known_objects() {
        let en_desc = if is_ru {
            format!("Встроенный объект JustMC: {en}")
        } else {
            format!("Built-in JustMC object: {en}")
        };
        let ru_desc = if is_ru {
            format!("Встроенный объект JustMC: {ru} ({en})")
        } else {
            format!("Built-in JustMC object: {ru} ({en})")
        };

        items.push(CompletionItem {
            label: en.to_owned(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(en_desc),
            insert_text: Some(format!("{en}::")),
            sort_text: Some(if is_ru {
                format!("1_{en}")
            } else {
                format!("0_{en}")
            }),
            ..Default::default()
        });

        items.push(CompletionItem {
            label: ru.to_owned(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(ru_desc),
            insert_text: Some(format!("{ru}::")),
            sort_text: Some(if is_ru {
                format!("0_{ru}")
            } else {
                format!("1_{ru}")
            }),
            ..Default::default()
        });
    }

    items
}

fn global_symbol_completions(ir_ctx: &IrCtx, lang: Lang) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    let is_prelude = |name: &str| -> bool {
        name.starts_with("std::primitives::")
            || name.starts_with("primitives::")
            || name.starts_with("std::math::core::")
            || name.starts_with("math::core::")
            || name.starts_with("std::prelude")
            || name.starts_with("prelude")
    };

    for name in ir_ctx.classes_by_name.keys() {
        let (display_name, origin) = if is_prelude(name) {
            let short = name.rsplit("::").next().unwrap_or(name);
            (short, Some(name.as_str()))
        } else {
            (name.as_str(), None)
        };

        if !seen.insert(display_name.to_owned()) {
            continue;
        }

        let detail = match (lang, origin) {
            (Lang::Ru, Some(orig)) => format!("Класс {display_name} ({orig})"),
            (Lang::Ru, None) => format!("Класс {display_name}"),
            (Lang::En, Some(orig)) => format!("Class {display_name} ({orig})"),
            (Lang::En, None) => format!("Class {display_name}"),
        };

        items.push(CompletionItem {
            label: display_name.to_owned(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(detail),
            insert_text: Some(display_name.to_owned()),
            ..Default::default()
        });
    }

    for name in ir_ctx.enums_by_name.keys() {
        let (display_name, origin) = if is_prelude(name) {
            let short = name.rsplit("::").next().unwrap_or(name);
            (short, Some(name.as_str()))
        } else {
            (name.as_str(), None)
        };

        if !seen.insert(display_name.to_owned()) {
            continue;
        }

        let detail = match (lang, origin) {
            (Lang::Ru, Some(orig)) => format!("Перечисление {display_name} ({orig})"),
            (Lang::Ru, None) => format!("Перечисление {display_name}"),
            (Lang::En, Some(orig)) => format!("Enum {display_name} ({orig})"),
            (Lang::En, None) => format!("Enum {display_name}"),
        };

        items.push(CompletionItem {
            label: display_name.to_owned(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some(detail),
            insert_text: Some(display_name.to_owned()),
            ..Default::default()
        });
    }

    for name in ir_ctx.type_aliases.keys() {
        let (display_name, origin) = if is_prelude(name) {
            let short = name.rsplit("::").next().unwrap_or(name);
            (short, Some(name.as_str()))
        } else {
            (name.as_str(), None)
        };

        if !seen.insert(display_name.to_owned()) {
            continue;
        }

        let detail = match (lang, origin) {
            (Lang::Ru, Some(orig)) => format!("Псевдоним типа {display_name} ({orig})"),
            (Lang::Ru, None) => format!("Псевдоним типа {display_name}"),
            (Lang::En, Some(orig)) => format!("Type alias {display_name} ({orig})"),
            (Lang::En, None) => format!("Type alias {display_name}"),
        };

        items.push(CompletionItem {
            label: display_name.to_owned(),
            kind: Some(CompletionItemKind::INTERFACE),
            detail: Some(detail),
            insert_text: Some(display_name.to_owned()),
            ..Default::default()
        });
    }

    items
}

fn local_variable_completions(ast: &Ast, _text: &str, _pos: Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    walk_statements(&ast.statements, &mut |stmt| match stmt {
        Statement::VarDecl(v) => {
            for name in &v.names {
                let s = jmcc::ast::text_value_to_string(ast, name);
                if seen.insert(s.clone()) {
                    items.push(CompletionItem {
                        label: s,
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some("Переменная".to_owned()),
                        ..Default::default()
                    });
                }
            }
        }
        Statement::For(f) => {
            for var in &f.vars {
                let s = jmcc::ast::text_value_to_string(ast, var);
                if seen.insert(s.clone()) {
                    items.push(CompletionItem {
                        label: s,
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some("Переменная цикла".to_owned()),
                        ..Default::default()
                    });
                }
            }
        }
        Statement::Function(f) => {
            let name = ast.strings.resolve(&f.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if seen.insert(short_name.to_owned()) {
                items.push(CompletionItem {
                    label: short_name.to_owned(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("Функция".to_owned()),
                    insert_text: Some(format!("{short_name}()")),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
            for p in &f.params {
                let p_name = ast.strings.resolve(&p.name);
                if seen.insert(p_name.to_owned()) {
                    items.push(CompletionItem {
                        label: p_name.to_owned(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some("Параметр".to_owned()),
                        ..Default::default()
                    });
                }
            }
        }
        Statement::Process(p) => {
            let name = ast.strings.resolve(&p.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if seen.insert(short_name.to_owned()) {
                items.push(CompletionItem {
                    label: short_name.to_owned(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("Процесс".to_owned()),
                    insert_text: Some(format!("{short_name}()")),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
            for p in &p.params {
                let p_name = ast.strings.resolve(&p.name);
                if seen.insert(p_name.to_owned()) {
                    items.push(CompletionItem {
                        label: p_name.to_owned(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some("Параметр".to_owned()),
                        ..Default::default()
                    });
                }
            }
        }
        _ => {}
    });

    items
}

const fn desc(is_ru: bool, ru: &'static str, en: &'static str) -> &'static str {
    if is_ru { ru } else { en }
}

#[expect(clippy::too_many_lines, reason = "Static table of keyword completions")]
fn keyword_completions(lang: Lang) -> Vec<CompletionItem> {
    let is_ru = lang == Lang::Ru;
    let keywords = [
        // Declarations
        (
            "function",
            "function ${1:name}(${2:params}) {\n\t$0\n}",
            desc(is_ru, "Объявление функции", "Function declaration"),
        ),
        (
            "функция",
            "функция ${1:имя}(${2:параметры}) {\n\t$0\n}",
            desc(is_ru, "Объявление функции", "Function declaration"),
        ),
        (
            "fun",
            "fun ${1:name}(${2:params}) {\n\t$0\n}",
            desc(is_ru, "Объявление действия/функции", "Action declaration"),
        ),
        (
            "действие",
            "действие ${1:имя}(${2:параметры}) {\n\t$0\n}",
            desc(is_ru, "Объявление действия/функции", "Action declaration"),
        ),
        (
            "def",
            "def ${1:name}(${2:params}) {\n\t$0\n}",
            desc(is_ru, "Определение функции", "Function definition"),
        ),
        (
            "определение",
            "определение ${1:имя}(${2:параметры}) {\n\t$0\n}",
            desc(is_ru, "Определение функции", "Function definition"),
        ),
        (
            "process",
            "process ${1:name}(${2:params}) {\n\t$0\n}",
            desc(is_ru, "Объявление процесса", "Process declaration"),
        ),
        (
            "proc",
            "proc ${1:name}(${2:params}) {\n\t$0\n}",
            desc(is_ru, "Объявление процесса", "Process declaration"),
        ),
        (
            "процесс",
            "процесс ${1:имя}(${2:параметры}) {\n\t$0\n}",
            desc(is_ru, "Объявление процесса", "Process declaration"),
        ),
        (
            "event",
            "event ${1:player_join} {\n\t$0\n}",
            desc(is_ru, "Обработчик события", "Event handler"),
        ),
        (
            "событие",
            "событие ${1:player_join} {\n\t$0\n}",
            desc(is_ru, "Обработчик события", "Event handler"),
        ),
        (
            "class",
            "class ${1:Name} {\n\t$0\n}",
            desc(is_ru, "Объявление класса", "Class declaration"),
        ),
        (
            "класс",
            "класс ${1:Имя} {\n\t$0\n}",
            desc(is_ru, "Объявление класса", "Class declaration"),
        ),
        (
            "interface",
            "interface ${1:Name} {\n\t$0\n}",
            desc(is_ru, "Объявление интерфейса", "Interface declaration"),
        ),
        (
            "интерфейс",
            "интерфейс ${1:Имя} {\n\t$0\n}",
            desc(is_ru, "Объявление интерфейса", "Interface declaration"),
        ),
        (
            "enum",
            "enum ${1:Name} {\n\t$0\n}",
            desc(is_ru, "Объявление перечисления", "Enum declaration"),
        ),
        (
            "перечисление",
            "перечисление ${1:Имя} {\n\t$0\n}",
            desc(is_ru, "Объявление перечисления", "Enum declaration"),
        ),
        (
            "typealias",
            "typealias ${1:Alias} = ${2:Type};",
            desc(is_ru, "Псевдоним типа", "Type alias declaration"),
        ),
        (
            "type",
            "type ${1:Alias} = ${2:Type};",
            desc(is_ru, "Псевдоним типа", "Type alias declaration"),
        ),
        (
            "тип",
            "тип ${1:Имя} = ${2:Тип};",
            desc(is_ru, "Псевдоним типа", "Type alias declaration"),
        ),
        (
            "псевдоним",
            "псевдоним ${1:Имя} = ${2:Тип};",
            desc(is_ru, "Псевдоним типа", "Type alias declaration"),
        ),
        (
            "import",
            "import ${1:module};",
            desc(is_ru, "Импорт модуля", "Module import"),
        ),
        (
            "импорт",
            "импорт ${1:модуль};",
            desc(is_ru, "Импорт модуля", "Module import"),
        ),
        (
            "export",
            "export ",
            desc(is_ru, "Экспорт объявления", "Export declaration"),
        ),
        (
            "экспорт",
            "экспорт ",
            desc(is_ru, "Экспорт объявления", "Export declaration"),
        ),
        (
            "from",
            "from ${1:module} import ${2:item};",
            desc(is_ru, "Импорт из модуля", "Import from module"),
        ),
        (
            "из",
            "из ${1:модуль} импорт ${2:элемент};",
            desc(is_ru, "Импорт из модуля", "Import from module"),
        ),
        // Variables and modifiers
        (
            "var",
            "var ${1:name} = ${2:value};",
            desc(is_ru, "Объявление переменной", "Variable declaration"),
        ),
        (
            "переменная",
            "переменная ${1:имя} = ${2:значение};",
            desc(is_ru, "Объявление переменной", "Variable declaration"),
        ),
        (
            "перем",
            "перем ${1:имя} = ${2:значение};",
            desc(is_ru, "Объявление переменной", "Variable declaration"),
        ),
        (
            "пусть",
            "пусть ${1:имя} = ${2:значение};",
            desc(is_ru, "Объявление переменной", "Variable declaration"),
        ),
        (
            "const",
            "const ${1:NAME} = ${2:value};",
            desc(is_ru, "Объявление константы", "Constant declaration"),
        ),
        (
            "константа",
            "константа ${1:ИМЯ} = ${2:значение};",
            desc(is_ru, "Объявление константы", "Constant declaration"),
        ),
        (
            "inline",
            "inline ",
            desc(is_ru, "Встраиваемая функция", "Inline function modifier"),
        ),
        (
            "встраиваемый",
            "встраиваемый ",
            desc(is_ru, "Встраиваемая функция", "Inline function modifier"),
        ),
        (
            "local",
            "local ",
            desc(is_ru, "Локальная переменная", "Local scope modifier"),
        ),
        (
            "локальный",
            "локальный ",
            desc(is_ru, "Локальная переменная", "Local scope modifier"),
        ),
        (
            "game",
            "game ",
            desc(
                is_ru,
                "Игровая (глобальная) переменная",
                "Game scope modifier",
            ),
        ),
        (
            "игра",
            "игра ",
            desc(
                is_ru,
                "Игровая (глобальная) переменная",
                "Game scope modifier",
            ),
        ),
        (
            "save",
            "save ",
            desc(is_ru, "Сохраняемая переменная", "Save scope modifier"),
        ),
        (
            "сохранить",
            "сохранить ",
            desc(is_ru, "Сохраняемая переменная", "Save scope modifier"),
        ),
        (
            "сохранение",
            "сохранение ",
            desc(is_ru, "Сохраняемая переменная", "Save scope modifier"),
        ),
        (
            "line",
            "line ",
            desc(
                is_ru,
                "Строковая переменная (текущая строка)",
                "Line scope modifier",
            ),
        ),
        (
            "строка",
            "строка ",
            desc(
                is_ru,
                "Строковая переменная (текущая строка)",
                "Line scope modifier",
            ),
        ),
        (
            "ref",
            "ref ",
            desc(is_ru, "Параметр по ссылке", "Reference parameter"),
        ),
        (
            "ссылка",
            "ссылка ",
            desc(is_ru, "Параметр по ссылке", "Reference parameter"),
        ),
        // Control flow
        (
            "if",
            "if ${1:condition} {\n\t$0\n}",
            desc(is_ru, "Условная конструкция", "If condition"),
        ),
        (
            "если",
            "если ${1:условие} {\n\t$0\n}",
            desc(is_ru, "Условная конструкция", "If condition"),
        ),
        (
            "else",
            "else {\n\t$0\n}",
            desc(is_ru, "Ветвь else", "Else branch"),
        ),
        (
            "иначе",
            "иначе {\n\t$0\n}",
            desc(is_ru, "Ветвь иначе", "Else branch"),
        ),
        (
            "elif",
            "elif ${1:condition} {\n\t$0\n}",
            desc(is_ru, "Ветвь elif", "Elif branch"),
        ),
        (
            "иначеесли",
            "иначеесли ${1:условие} {\n\t$0\n}",
            desc(is_ru, "Ветвь иначе-если", "Elif branch"),
        ),
        (
            "иначе_если",
            "иначе_если ${1:условие} {\n\t$0\n}",
            desc(is_ru, "Ветвь иначе-если", "Elif branch"),
        ),
        (
            "while",
            "while ${1:condition} {\n\t$0\n}",
            desc(is_ru, "Цикл while", "While loop"),
        ),
        (
            "пока",
            "пока ${1:условие} {\n\t$0\n}",
            desc(is_ru, "Цикл пока", "While loop"),
        ),
        (
            "for",
            "for ${1:item} in ${2:iter} {\n\t$0\n}",
            desc(is_ru, "Цикл for-in", "For-in loop"),
        ),
        (
            "для",
            "для ${1:элемент} в ${2:итератор} {\n\t$0\n}",
            desc(is_ru, "Цикл для-в", "For-in loop"),
        ),
        (
            "match",
            "match ${1:value} {\n\tcase ${2:pattern} => $0\n}",
            desc(is_ru, "Сопоставление с образцом", "Match expression"),
        ),
        (
            "выбор",
            "выбор ${1:значение} {\n\tслучай ${2:образец} => $0\n}",
            desc(is_ru, "Сопоставление с образцом", "Match expression"),
        ),
        (
            "сопоставить",
            "сопоставить ${1:значение} {\n\tслучай ${2:образец} => $0\n}",
            desc(is_ru, "Сопоставление с образцом", "Match expression"),
        ),
        (
            "case",
            "case ${1:pattern} => $0",
            desc(is_ru, "Ветвь сопоставления case", "Case branch"),
        ),
        (
            "случай",
            "случай ${1:образец} => $0",
            desc(is_ru, "Ветвь сопоставления случай", "Case branch"),
        ),
        (
            "вариант",
            "вариант ${1:образец} => $0",
            desc(is_ru, "Ветвь сопоставления вариант", "Case branch"),
        ),
        (
            "default",
            "default => $0",
            desc(is_ru, "Ветвь по умолчанию", "Default branch"),
        ),
        (
            "по_умолчанию",
            "по_умолчанию => $0",
            desc(is_ru, "Ветвь по умолчанию", "Default branch"),
        ),
        (
            "break",
            "break;",
            desc(is_ru, "Прерывание цикла", "Break statement"),
        ),
        (
            "прервать",
            "прервать;",
            desc(is_ru, "Прерывание цикла", "Break statement"),
        ),
        (
            "continue",
            "continue;",
            desc(is_ru, "Переход к следующей итерации", "Continue statement"),
        ),
        (
            "продолжить",
            "продолжить;",
            desc(is_ru, "Переход к следующей итерации", "Continue statement"),
        ),
        (
            "return",
            "return $0;",
            desc(is_ru, "Возврат из функции", "Return statement"),
        ),
        (
            "вернуть",
            "вернуть $0;",
            desc(is_ru, "Возврат из функции", "Return statement"),
        ),
        (
            "возврат",
            "возврат $0;",
            desc(is_ru, "Возврат из функции", "Return statement"),
        ),
        (
            "try",
            "try {\n\t$1\n} catch ${2:e} {\n\t$0\n}",
            desc(is_ru, "Блок перехвата исключений", "Try-catch block"),
        ),
        (
            "попытка",
            "попытка {\n\t$1\n} поймать ${2:ошибка} {\n\t$0\n}",
            desc(is_ru, "Блок перехвата исключений", "Try-catch block"),
        ),
        (
            "catch",
            "catch ${1:e} {\n\t$0\n}",
            desc(is_ru, "Перехват исключения", "Catch block"),
        ),
        (
            "поймать",
            "поймать ${1:ошибка} {\n\t$0\n}",
            desc(is_ru, "Перехват исключения", "Catch block"),
        ),
        (
            "перехват",
            "перехват ${1:ошибка} {\n\t$0\n}",
            desc(is_ru, "Перехват исключения", "Catch block"),
        ),
        (
            "исключение",
            "исключение ${1:ошибка} {\n\t$0\n}",
            desc(is_ru, "Перехват исключения", "Catch block"),
        ),
        (
            "throw",
            "throw ${1:error};",
            desc(is_ru, "Выброс исключения", "Throw statement"),
        ),
        (
            "выбросить",
            "выбросить ${1:ошибка};",
            desc(is_ru, "Выброс исключения", "Throw statement"),
        ),
        (
            "бросить",
            "бросить ${1:ошибка};",
            desc(is_ru, "Выброс исключения", "Throw statement"),
        ),
        // Operators and relations
        ("not", "not ", desc(is_ru, "Логическое НЕ", "Logical NOT")),
        ("не", "не ", desc(is_ru, "Логическое НЕ", "Logical NOT")),
        ("and", "and ", desc(is_ru, "Логическое И", "Logical AND")),
        ("и", "и ", desc(is_ru, "Логическое И", "Logical AND")),
        ("or", "or ", desc(is_ru, "Логическое ИЛИ", "Logical OR")),
        ("или", "или ", desc(is_ru, "Логическое ИЛИ", "Logical OR")),
        (
            "in",
            "in ",
            desc(is_ru, "Оператор вхождения", "Membership operator"),
        ),
        (
            "в",
            "в ",
            desc(is_ru, "Оператор вхождения", "Membership operator"),
        ),
        (
            "as",
            "as ${1:Type}",
            desc(is_ru, "Приведение типа", "Type cast"),
        ),
        (
            "как",
            "как ${1:Тип}",
            desc(is_ru, "Приведение типа", "Type cast"),
        ),
        (
            "implements",
            "implements ${1:Interface}",
            desc(is_ru, "Реализация интерфейса", "Interface implementation"),
        ),
        (
            "реализует",
            "реализует ${1:Интерфейс}",
            desc(is_ru, "Реализация интерфейса", "Interface implementation"),
        ),
        (
            "extends",
            "extends ${1:BaseClass}",
            desc(is_ru, "Наследование класса", "Class inheritance"),
        ),
        (
            "расширяет",
            "расширяет ${1:БазовыйКласс}",
            desc(is_ru, "Наследование класса", "Class inheritance"),
        ),
        // String formatting prefixes
        (
            "plain",
            "plain\"$0\"",
            desc(
                is_ru,
                "Обычный текст без форматирования",
                "Plain unformatted text",
            ),
        ),
        (
            "простой",
            "простой\"$0\"",
            desc(
                is_ru,
                "Обычный текст без форматирования",
                "Plain unformatted text",
            ),
        ),
        (
            "legacy",
            "legacy\"$0\"",
            desc(
                is_ru,
                "Текст с цветовыми кодами Minecraft §",
                "Legacy Minecraft § color codes",
            ),
        ),
        (
            "устаревший",
            "устаревший\"$0\"",
            desc(
                is_ru,
                "Текст с цветовыми кодами Minecraft §",
                "Legacy Minecraft § color codes",
            ),
        ),
        (
            "minimessage",
            "minimessage\"$0\"",
            desc(
                is_ru,
                "Форматирование текста MiniMessage <color>",
                "MiniMessage text formatting <color>",
            ),
        ),
        (
            "минисообщение",
            "минисообщение\"$0\"",
            desc(
                is_ru,
                "Форматирование текста MiniMessage <color>",
                "MiniMessage text formatting <color>",
            ),
        ),
        (
            "json",
            "json\"$0\"",
            desc(is_ru, "Текст в формате JSON", "JSON formatted text"),
        ),
        (
            "джсон",
            "джсон\"$0\"",
            desc(is_ru, "Текст в формате JSON", "JSON formatted text"),
        ),
        // Literals
        (
            "true",
            "true",
            desc(is_ru, "Булево значение: истина", "Boolean true"),
        ),
        (
            "истина",
            "истина",
            desc(is_ru, "Булево значение: истина", "Boolean true"),
        ),
        (
            "правда",
            "правда",
            desc(is_ru, "Булево значение: правда", "Boolean true"),
        ),
        (
            "false",
            "false",
            desc(is_ru, "Булево значение: ложь", "Boolean false"),
        ),
        (
            "ложь",
            "ложь",
            desc(is_ru, "Булево значение: ложь", "Boolean false"),
        ),
        (
            "null",
            "null",
            desc(is_ru, "Нулевой указатель/значение", "Null value"),
        ),
    ];

    keywords
        .into_iter()
        .map(|(label, snippet, desc)| {
            let is_cyrillic = label
                .chars()
                .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
            let sort_prefix = if is_ru {
                if is_cyrillic { "0_" } else { "1_" }
            } else if is_cyrillic {
                "1_"
            } else {
                "0_"
            };
            CompletionItem {
                label: (*label).to_owned(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some((*desc).to_owned()),
                insert_text: Some((*snippet).to_owned()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some(format!("{sort_prefix}{label}")),
                ..Default::default()
            }
        })
        .collect()
}
