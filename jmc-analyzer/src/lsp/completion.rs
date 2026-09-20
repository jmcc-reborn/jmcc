#![allow(clippy::literal_string_with_formatting_args)]

//! Autocomplete and `IntelliSense` provider for `JustCode` and `JustMC` schema.

use std::collections::HashSet;

use jmcc::ast::*;
use jmcc::ir::KNOWN_OBJECTS;
use jmcc::ir::ctx::{ClassInfo, IrCtx};
use jmcdata::generated::ACTION_DEF_MAP;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position,
};

use super::state::{DocumentData, walk_statements};

/// Builds completion suggestions based on cursor context.
#[must_use]
pub fn provide_completions(doc: &DocumentData, params: &CompletionParams) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    let text = &doc.text;
    let pos = params.text_document_position.position;

    // Find current line text up to cursor position
    let line_text = get_line_prefix(text, pos);
    let trimmed = line_text.trim_end();

    // 1. Check if user typed `object::`
    if let Some(colon_pos) = trimmed.rfind("::") {
        let object_prefix = &trimmed[..colon_pos];
        let ident = object_prefix
            .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
            .next()
            .unwrap_or("");

        if !ident.is_empty() {
            // Check if it's a known JustMC object (player, variable, entity, world, etc.)
            if KNOWN_OBJECTS.contains(&ident) {
                return complete_justmc_actions(ident);
            }

            // Check if it's an enum in IrCtx
            if let Some(ir_ctx) = &doc.ir_ctx
                && let Some(def_id) = ir_ctx.enums_by_name.get(ident)
                && let Some(enum_info) = ir_ctx.enums_by_def.get(def_id)
            {
                return complete_enum_variants(enum_info);
            }
        }
    }

    // 2. Check if user typed `obj.`
    if let Some(obj_expr) = trimmed.strip_suffix('.') {
        let ident = obj_expr
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

    // 3. General completions: Keywords, snippets, JustMC objects, types, functions, local vars
    items.extend(keyword_completions());
    items.extend(builtin_object_completions());

    if let Some(ir_ctx) = &doc.ir_ctx {
        items.extend(global_symbol_completions(ir_ctx));
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
            return line[..col].to_string();
        }
    }
    String::new()
}

fn complete_justmc_actions(object: &str) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for ((obj, name), def) in ACTION_DEF_MAP.entries() {
        if *obj != object {
            continue;
        }

        let mut args_doc = String::new();
        let mut snippet_args = Vec::new();
        for (i, arg) in def.args.iter().enumerate() {
            let ty = arg.arg_type;
            args_doc.push_str(&format!("\n- `{}`: `{}`", arg.id, ty));
            snippet_args.push(format!("${{{}:{}}}", i + 1, arg.id));
        }

        let detail = if def.args.is_empty() {
            format!("{object}::{name}()")
        } else {
            let arg_summary: Vec<_> = def
                .args
                .iter()
                .map(|a| format!("{}: {}", a.id, a.arg_type))
                .collect();
            format!("{object}::{name}({})", arg_summary.join(", "))
        };

        let insert_text = if snippet_args.is_empty() {
            format!("{name}()")
        } else {
            format!("{name}({})", snippet_args.join(", "))
        };

        let doc_text = format!(
            "### `{object}::{name}`\n**Действие JustMC**\n- Тип: `{}`\n\n**Параметры:**{}",
            def.action_type, args_doc
        );

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

    items
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

fn builtin_object_completions() -> Vec<CompletionItem> {
    KNOWN_OBJECTS
        .iter()
        .map(|obj| CompletionItem {
            label: (*obj).to_owned(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(format!("Встроенный объект JustMC: {obj}")),
            insert_text: Some(format!("{obj}::")),
            ..Default::default()
        })
        .collect()
}

fn global_symbol_completions(ir_ctx: &IrCtx) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for name in ir_ctx.classes_by_name.keys() {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some(format!("Класс {name}")),
            ..Default::default()
        });
    }

    for name in ir_ctx.enums_by_name.keys() {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::ENUM),
            detail: Some(format!("Перечисление {name}")),
            ..Default::default()
        });
    }

    for name in ir_ctx.type_aliases.keys() {
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::INTERFACE),
            detail: Some(format!("Псевдоним типа {name}")),
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

#[expect(clippy::too_many_lines, reason = "Static table of keyword completions")]
fn keyword_completions() -> Vec<CompletionItem> {
    let keywords = [
        (
            "function",
            "function ${1:name}(${2:params}) {\n\t$0\n}",
            "Объявление функции",
        ),
        (
            "функция",
            "функция ${1:имя}(${2:параметры}) {\n\t$0\n}",
            "Объявление функции",
        ),
        (
            "proc",
            "proc ${1:name}(${2:params}) {\n\t$0\n}",
            "Объявление процесса",
        ),
        (
            "процесс",
            "процесс ${1:имя}(${2:параметры}) {\n\t$0\n}",
            "Объявление процесса",
        ),
        (
            "event",
            "event ${1:player_join} {\n\t$0\n}",
            "Обработчик события",
        ),
        (
            "событие",
            "событие ${1:player_join} {\n\t$0\n}",
            "Обработчик события",
        ),
        ("class", "class ${1:Name} {\n\t$0\n}", "Объявление класса"),
        ("класс", "класс ${1:Имя} {\n\t$0\n}", "Объявление класса"),
        (
            "interface",
            "interface ${1:Name} {\n\t$0\n}",
            "Объявление интерфейса",
        ),
        (
            "интерфейс",
            "интерфейс ${1:Имя} {\n\t$0\n}",
            "Объявление интерфейса",
        ),
        (
            "enum",
            "enum ${1:Name} {\n\t$0\n}",
            "Объявление перечисления",
        ),
        (
            "перечисление",
            "перечисление ${1:Имя} {\n\t$0\n}",
            "Объявление перечисления",
        ),
        ("if", "if ${1:condition} {\n\t$0\n}", "Условная конструкция"),
        (
            "если",
            "если ${1:условие} {\n\t$0\n}",
            "Условная конструкция",
        ),
        ("else", "else {\n\t$0\n}", "Ветвь else"),
        ("иначе", "иначе {\n\t$0\n}", "Ветвь иначе"),
        ("elif", "elif ${1:condition} {\n\t$0\n}", "Ветвь elif"),
        (
            "иначеесли",
            "иначеесли ${1:условие} {\n\t$0\n}",
            "Ветвь иначеесли",
        ),
        ("while", "while ${1:condition} {\n\t$0\n}", "Цикл while"),
        ("пока", "пока ${1:условие} {\n\t$0\n}", "Цикл пока"),
        (
            "for",
            "for ${1:item} in ${2:iter} {\n\t$0\n}",
            "Цикл for-in",
        ),
        (
            "для",
            "для ${1:элемент} в ${2:итератор} {\n\t$0\n}",
            "Цикл для-в",
        ),
        ("return", "return $0;", "Возврат из функции"),
        ("вернуть", "вернуть $0;", "Возврат из функции"),
        ("break", "break;", "Прерывание цикла"),
        ("прервать", "прервать;", "Прерывание цикла"),
        (
            "var",
            "var ${1:name} = ${2:value};",
            "Объявление переменной",
        ),
        ("import", "import ${1:module};", "Импорт модуля"),
        ("импорт", "импорт ${1:модуль};", "Импорт модуля"),
        ("export", "export ", "Экспорт объявления"),
        ("экспорт", "экспорт ", "Экспорт объявления"),
        ("true", "true", "Булево значение: истина"),
        ("правда", "правда", "Булево значение: правда"),
        ("false", "false", "Булево значение: ложь"),
        ("ложь", "ложь", "Булево значение: ложь"),
        ("null", "null", "Нулевой указатель/значение"),
    ];

    keywords
        .into_iter()
        .map(|(label, snippet, desc)| CompletionItem {
            label: (*label).to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some((*desc).to_owned()),
            insert_text: Some((*snippet).to_owned()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}
