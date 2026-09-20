//! Hover information provider for actions, symbols, types, functions, and documentation comments.

use jmcc::ast::semantic::Type;
use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use jmcc::i18n::Lang;
use jmcdata::generated::{get_action_def, get_game_value_type};
use line_index::TextSize;
use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind, Position, Range};

use super::diagnostics::span_to_range;
use super::state::{DocumentData, format_type, ru_type_name, walk_statements};

/// Computes hover information for the symbol at the cursor position.
#[must_use]
pub fn provide_hover(doc: &DocumentData, params: &HoverParams) -> Option<Hover> {
    let ast = doc.ast.as_ref()?;
    let pos = params.text_document_position_params.position;

    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    // 1. Search in expressions first, prioritizing the most specific (smallest span) expression
    let mut best_expr: Option<(usize, Hover)> = None;
    for (eid, expr) in &ast.exprs {
        let span = expr.span();
        let Some((resolved_path, _source, local_span)) = resolve_ast_span(ast, &span) else {
            continue;
        };
        if resolved_path != doc.path {
            continue;
        }

        if offset >= local_span.start && offset <= local_span.end {
            let span_len = local_span.end.saturating_sub(local_span.start);
            if let Some(h) = check_expr_hover(doc, eid, expr, &span, offset)
                && best_expr
                    .as_ref()
                    .is_none_or(|(best_len, _)| span_len < *best_len)
            {
                best_expr = Some((span_len, h));
            }
        }
    }

    if let Some((_, h)) = best_expr {
        return Some(h);
    }

    // 2. Search in statements (declarations, parameters, and block elements)
    let mut found_stmt_hover = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if found_stmt_hover.is_some() {
            return;
        }
        if let Some(h) = check_stmt_hover(doc, stmt, offset) {
            found_stmt_hover = Some(h);
        }
    });

    found_stmt_hover
}

/// Extracts doc comments (`/// ...`) directly above the declaration at `span_start`.
#[must_use]
pub fn extract_doc_comment(source: &str, span_start: usize, lang: Lang) -> Option<String> {
    if span_start > source.len() {
        return None;
    }

    let before = &source[..span_start];
    let lines: Vec<&str> = before.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let mut doc_lines = Vec::new();
    let mut in_doc = false;

    for &line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("///") {
            in_doc = true;
            let content = trimmed.strip_prefix("///").unwrap_or("").trim();
            doc_lines.push(content.to_owned());
        } else if in_doc || !is_declaration_header_or_decorator(trimmed) {
            // End of doc comment block or previous statement encountered
            break;
        }
    }

    if doc_lines.is_empty() {
        return None;
    }

    doc_lines.reverse();
    parse_localized_doc(&doc_lines, lang)
}

fn is_declaration_header_or_decorator(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with('@')
        || trimmed.starts_with("export")
        || trimmed.starts_with("inline")
        || trimmed.starts_with("function")
        || trimmed.starts_with("fun")
        || trimmed.starts_with("def")
        || trimmed.starts_with("class")
        || trimmed.starts_with("enum")
        || trimmed.starts_with("interface")
        || trimmed.starts_with("process")
        || trimmed.starts_with("event")
        || trimmed.starts_with("typealias")
        || trimmed.starts_with("type")
        || trimmed.starts_with("var")
        || trimmed.starts_with("const")
}

/// Parses localized lines from a doc-comment block.
#[must_use]
pub fn parse_localized_doc(lines: &[String], lang: Lang) -> Option<String> {
    let has_ru = lines.iter().any(|l| {
        let s = l.trim().to_ascii_lowercase();
        s.starts_with("ru:") || s == "ru" || s.starts_with("ru ")
    });
    let has_en = lines.iter().any(|l| {
        let s = l.trim().to_ascii_lowercase();
        s.starts_with("en:") || s == "en" || s.starts_with("en ")
    });

    if !has_ru && !has_en {
        let joined = lines.join("\n").trim().to_owned();
        return if joined.is_empty() {
            None
        } else {
            Some(joined)
        };
    }

    let mut ru_lines = Vec::new();
    let mut en_lines = Vec::new();
    let mut current_lang: Option<Lang> = None;

    for line in lines {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("ru:") || lower == "ru" || lower.starts_with("ru ") {
            current_lang = Some(Lang::Ru);
            let rest = if lower.starts_with("ru:") || lower.starts_with("ru ") {
                trimmed[3..].trim()
            } else {
                ""
            };
            if !rest.is_empty() {
                ru_lines.push(rest.to_owned());
            }
        } else if lower.starts_with("en:") || lower == "en" || lower.starts_with("en ") {
            current_lang = Some(Lang::En);
            let rest = if lower.starts_with("en:") || lower.starts_with("en ") {
                trimmed[3..].trim()
            } else {
                ""
            };
            if !rest.is_empty() {
                en_lines.push(rest.to_owned());
            }
        } else {
            match current_lang {
                Some(Lang::Ru) => ru_lines.push(trimmed.to_owned()),
                Some(Lang::En) => en_lines.push(trimmed.to_owned()),
                None => {
                    ru_lines.push(trimmed.to_owned());
                    en_lines.push(trimmed.to_owned());
                }
            }
        }
    }

    let result = match lang {
        Lang::Ru => {
            if !ru_lines.is_empty() {
                ru_lines.join("\n")
            } else if !en_lines.is_empty() {
                en_lines.join("\n")
            } else {
                String::new()
            }
        }
        Lang::En => {
            if !en_lines.is_empty() {
                en_lines.join("\n")
            } else if !ru_lines.is_empty() {
                ru_lines.join("\n")
            } else {
                String::new()
            }
        }
    };

    let trimmed = result.trim().to_owned();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn render_aliases(ast: &Ast, aliases: &[StrId], lang: Lang) -> String {
    if aliases.is_empty() {
        return String::new();
    }
    let label = if lang == Lang::Ru {
        "Псевдонимы"
    } else {
        "Aliases"
    };
    let list: Vec<_> = aliases
        .iter()
        .map(|a| format!("`{}`", ast.strings.resolve(a)))
        .collect();
    format!("\n\n*{label}:* {}", list.join(", "))
}

fn render_function_signature(ast: &Ast, f: &FunctionDecl) -> String {
    let name = ast.strings.resolve(&f.name);
    let short_name = name.rsplit("::").next().unwrap_or(name);

    let prefix = if f.is_inline {
        "inline "
    } else if f.is_getter {
        "@getter "
    } else if f.is_setter {
        "@setter "
    } else {
        ""
    };

    let params_str: Vec<_> = f
        .params
        .iter()
        .map(|p| {
            let p_name = ast.strings.resolve(&p.name);
            let ty_str =
                p.ty.map_or_else(|| "any".to_owned(), |t| ast.strings.resolve(&t).to_owned());
            if p.default.is_some() {
                format!("{p_name}: {ty_str} = ...")
            } else {
                format!("{p_name}: {ty_str}")
            }
        })
        .collect();

    let ret_str = f
        .return_type
        .map_or_else(|| "void".to_owned(), |r| ast.strings.resolve(&r).to_owned());

    format!(
        "{prefix}function {short_name}({}) -> {ret_str}",
        params_str.join(", ")
    )
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "Traverses statement kinds to render contextual hovers"
)]
fn check_stmt_hover(doc: &DocumentData, stmt: &Statement, offset: usize) -> Option<Hover> {
    let ast = doc.ast.as_ref()?;
    match stmt {
        Statement::Function(f) => {
            let (path, src, span) = resolve_ast_span(ast, &f.span)?;
            if path == doc.path {
                let _name = ast.strings.resolve(&f.name);

                // Check parameter hover
                for p in &f.params {
                    let (p_path, _, p_span) = resolve_ast_span(ast, &p.span)?;
                    if p_path == doc.path && offset >= p_span.start && offset <= p_span.end {
                        let p_name = ast.strings.resolve(&p.name);
                        let ty_str = p.ty.map_or_else(
                            || "any".to_owned(),
                            |t| ast.strings.resolve(&t).to_owned(),
                        );
                        let markdown = format!("```jc\n(parameter) {p_name}: {ty_str}\n```");
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: markdown,
                            }),
                            range: span_to_range(ast, &doc.path, &p.span),
                        });
                    }
                }

                // Check function header hover (from declaration start up to function name and params)
                if offset >= span.start && offset <= span.end {
                    let sig = render_function_signature(ast, f);
                    let mut markdown = format!("```jc\n{sig}\n```");
                    let aliases = render_aliases(ast, &f.aliases, doc.lang);
                    markdown.push_str(&aliases);

                    if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                        markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                    }

                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: span_to_range(ast, &doc.path, &f.span),
                    });
                }
            }
        }
        Statement::Class(c) => {
            let (path, src, span) = resolve_ast_span(ast, &c.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&c.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);

                let generics_str = if c.generics.is_empty() {
                    String::new()
                } else {
                    let g: Vec<_> = c.generics.iter().map(|g| ast.strings.resolve(g)).collect();
                    format!("<{}>", g.join(", "))
                };

                let parent_str = c
                    .parent
                    .map_or_else(String::new, |p| format!(" : {}", ast.strings.resolve(&p)));

                let impl_str = if c.implements.is_empty() {
                    String::new()
                } else {
                    let i: Vec<_> = c
                        .implements
                        .iter()
                        .map(|im| ast.strings.resolve(im))
                        .collect();
                    format!(" implements {}", i.join(", "))
                };

                let mut prefix = String::new();
                if c.lang_item {
                    prefix.push_str("@lang_item\n");
                }
                if c.is_dict {
                    prefix.push_str("@dict\n");
                }
                if c.is_exported {
                    prefix.push_str("export ");
                }

                let mut markdown = format!(
                    "```jc\n{prefix}class {short_name}{generics_str}{parent_str}{impl_str}\n```"
                );
                let aliases = render_aliases(ast, &c.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &c.span),
                });
            }
        }
        Statement::Interface(i) => {
            let (path, src, span) = resolve_ast_span(ast, &i.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&i.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);

                let generics_str = if i.generics.is_empty() {
                    String::new()
                } else {
                    let g: Vec<_> = i.generics.iter().map(|g| ast.strings.resolve(g)).collect();
                    format!("<{}>", g.join(", "))
                };

                let parent_str = if i.parents.is_empty() {
                    String::new()
                } else {
                    let p: Vec<_> = i.parents.iter().map(|p| ast.strings.resolve(p)).collect();
                    format!(" : {}", p.join(", "))
                };

                let prefix = if i.is_exported { "export " } else { "" };
                let mut markdown =
                    format!("```jc\n{prefix}interface {short_name}{generics_str}{parent_str}\n```");
                let aliases = render_aliases(ast, &i.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &i.span),
                });
            }
        }
        Statement::Enum(e) => {
            let (path, src, span) = resolve_ast_span(ast, &e.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&e.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);

                let values: Vec<_> = e.values.iter().map(|v| ast.strings.resolve(v)).collect();
                let prefix = if e.is_exported { "export " } else { "" };
                let mut markdown = format!(
                    "```jc\n{prefix}enum {short_name} {{\n    {}\n}}\n```",
                    values.join(",\n    ")
                );
                let aliases = render_aliases(ast, &e.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &e.span),
                });
            }
        }
        Statement::TypeAlias(t) => {
            let (path, src, span) = resolve_ast_span(ast, &t.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&t.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                let target_ty = ast.strings.resolve(&t.target_ty);

                let generics_str = if t.generics.is_empty() {
                    String::new()
                } else {
                    let g: Vec<_> = t.generics.iter().map(|g| ast.strings.resolve(g)).collect();
                    format!("<{}>", g.join(", "))
                };

                let prefix = if t.is_exported { "export " } else { "" };
                let mut markdown = format!(
                    "```jc\n{prefix}typealias {short_name}{generics_str} = {target_ty}\n```"
                );
                let aliases = render_aliases(ast, &t.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &t.span),
                });
            }
        }
        Statement::Process(p) => {
            let (path, src, span) = resolve_ast_span(ast, &p.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&p.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);

                let params_str: Vec<_> = p
                    .params
                    .iter()
                    .map(|param| ast.strings.resolve(&param.name).to_owned())
                    .collect();

                let prefix = if p.is_exported { "export " } else { "" };
                let mut markdown = format!(
                    "```jc\n{prefix}process {short_name}({})\n```",
                    params_str.join(", ")
                );
                let aliases = render_aliases(ast, &p.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &p.span),
                });
            }
        }
        Statement::Event(e) => {
            let (path, src, span) = resolve_ast_span(ast, &e.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let name = ast.strings.resolve(&e.event_name);
                let mut markdown = format!("```jc\nevent {name}\n```");

                if let Some(doc_text) = extract_doc_comment(src, span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, &e.span),
                });
            }
        }
        Statement::VarDecl(v) => {
            for name in &v.names {
                let (path, _, span) = resolve_ast_span(ast, &name.span)?;
                if path == doc.path && offset >= span.start && offset <= span.end {
                    let s = jmcc::ast::text_value_to_string(ast, name);
                    let markdown = format!("```jc\nvar {s}\n```");
                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: span_to_range(ast, &doc.path, &name.span),
                    });
                }
            }
        }
        _ => {}
    }
    None
}

#[expect(
    clippy::too_many_lines,
    reason = "Traverses expression kinds to render contextual hovers"
)]
fn check_expr_hover(
    doc: &DocumentData,
    eid: ExprId,
    expr: &Expr,
    span: &Span,
    offset: usize,
) -> Option<Hover> {
    let ast = doc.ast.as_ref()?;

    match expr {
        Expr::Action(a) => {
            let obj = ast.strings.resolve(&a.object);
            let name = ast.strings.resolve(&a.name);

            if let Some(def) = get_action_def(obj, name) {
                let mut args_doc = String::new();
                for arg in def.args {
                    args_doc.push_str(&format!("\n- `{}`: `{}`", arg.id, arg.arg_type));
                }

                let markdown = format!(
                    "```jc\n{obj}::{name}(...)\n```\n\n**Действие JustMC**\n- Объект: `{obj}`\n- Тип действия: `{}`\n\n**Параметры:**{}",
                    def.action_type, args_doc
                );

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, span),
                });
            }
        }
        Expr::Variable(v) => {
            let name_str = jmcc::ast::text_value_to_string(ast, &v.name);
            let (scope_str, var_kw) = if doc.lang == Lang::Ru {
                let s = match v.scope {
                    VarScope::Line => "строка",
                    VarScope::Local => "локальный",
                    VarScope::Game => "игра",
                    VarScope::Save => "сохранение",
                    VarScope::Inline => "встраиваемый",
                    VarScope::Jmcc => "jmcc",
                };
                (s, "перем")
            } else {
                let s = match v.scope {
                    VarScope::Line => "line",
                    VarScope::Local => "local",
                    VarScope::Game => "game",
                    VarScope::Save => "save",
                    VarScope::Inline => "inline",
                    VarScope::Jmcc => "jmcc",
                };
                (s, "var")
            };

            let ty_str = doc.expr_types.get(&eid).map_or_else(
                || {
                    v.value_type.map_or_else(
                        || {
                            if doc.lang == Lang::Ru {
                                "любой".to_owned()
                            } else {
                                "any".to_owned()
                            }
                        },
                        |t_id| {
                            let raw = ast.strings.resolve(&t_id);
                            if doc.lang == Lang::Ru {
                                ru_type_name(raw).unwrap_or(raw).to_owned()
                            } else {
                                raw.to_owned()
                            }
                        },
                    )
                },
                |ty| format_type(ty, doc.ir_ctx.as_ref(), doc.lang),
            );

            let markdown = format!("```jc\n({scope_str}) {var_kw} {name_str}: {ty_str}\n```");
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: markdown,
                }),
                range: span_to_range(ast, &doc.path, span),
            });
        }
        Expr::Property(p) => {
            let prop_name = ast.strings.resolve(&p.property);

            // 1. Check if object is an enum (e.g. MessageType.TEXT)
            if let Some(Expr::Ident(obj_id, _)) = ast.exprs.get(p.object) {
                let obj_name = ast.strings.resolve(obj_id);
                if let Some(e) = find_enum_by_name(ast, obj_name) {
                    for val in &e.values {
                        let val_name = ast.strings.resolve(val);
                        if val_name == prop_name {
                            let mut markdown =
                                format!("```jc\nenum variant {obj_name}::{val_name}\n```");
                            if let Some((_, src, e_span)) = resolve_ast_span(ast, &e.span)
                                && let Some(doc_text) =
                                    extract_doc_comment(src, e_span.start, doc.lang)
                            {
                                markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                            }
                            return Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: markdown,
                                }),
                                range: get_ident_range_at_offset(&doc.text, offset, ast, &doc.path)
                                    .or_else(|| span_to_range(ast, &doc.path, span)),
                            });
                        }
                    }
                }
            }

            // 2. Check if object has class type
            if let Some(Type::Class(def_id, _)) = doc.expr_types.get(&p.object)
                && let Some(ir_ctx) = &doc.ir_ctx
                && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                && let Some((field_ty, _)) = class_info.fields.get(prop_name)
            {
                let markdown = format!(
                    "```jc\n(field) {}.{prop_name}: {field_ty}\n```",
                    class_info.name
                );
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: get_ident_range_at_offset(&doc.text, offset, ast, &doc.path)
                        .or_else(|| span_to_range(ast, &doc.path, span)),
                });
            }
        }
        Expr::Ident(str_id, s) => {
            let ident = ast.strings.resolve(str_id);

            // 1. Check if it's a game value
            if let Some(gv_type) = get_game_value_type(ident) {
                let markdown =
                    format!("```jc\ngame_value::{ident}: {gv_type}\n```\nИгровое значение JustMC");
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, s),
                });
            }

            // 2. Check if it's an enum variant (e.g. TEXT)
            for stmt in &ast.statements {
                if let Statement::Enum(e) = stmt {
                    let enum_name = ast.strings.resolve(&e.name);
                    let short_enum = enum_name.rsplit("::").next().unwrap_or(enum_name);
                    for val in &e.values {
                        let val_name = ast.strings.resolve(val);
                        if val_name == ident {
                            let mut markdown =
                                format!("```jc\nenum variant {short_enum}::{val_name}\n```");
                            if let Some((_, src, e_span)) = resolve_ast_span(ast, &e.span)
                                && let Some(doc_text) =
                                    extract_doc_comment(src, e_span.start, doc.lang)
                            {
                                markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                            }
                            return Some(Hover {
                                contents: HoverContents::Markup(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: markdown,
                                }),
                                range: span_to_range(ast, &doc.path, s),
                            });
                        }
                    }
                }
            }

            // 3. Check if we have an inferred type for this ident expression
            if let Some(ty) = doc.expr_types.get(&eid) {
                let ty_str = format_type(ty, doc.ir_ctx.as_ref(), doc.lang);
                let markdown = format!("```jc\n{ident}: {ty_str}\n```");
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: span_to_range(ast, &doc.path, s),
                });
            }
        }
        Expr::Call(c) => {
            let method = ast.strings.resolve(&c.method);

            // Attempt to resolve method from receiver class type
            if let Some(Type::Class(def_id, _)) = doc.expr_types.get(&c.target)
                && let Some(ir_ctx) = &doc.ir_ctx
                && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                && let Some(method_decl) = class_info.methods.get(method)
            {
                let (decl_path, src, decl_span) = resolve_ast_span(ast, &method_decl.span)?;
                let sig = render_function_signature(ast, method_decl);
                let mut markdown = format!("```jc\n{sig}\n```");
                let aliases = render_aliases(ast, &method_decl.aliases, doc.lang);
                markdown.push_str(&aliases);

                if let Some(doc_text) = extract_doc_comment(src, decl_span.start, doc.lang) {
                    markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                }

                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: get_ident_range_at_offset(&doc.text, offset, ast, &doc.path)
                        .or_else(|| span_to_range(ast, decl_path, &method_decl.span)),
                });
            }

            // Fallback: search function with matching name in AST
            let mut found_hover = None;
            walk_statements(&ast.statements, &mut |s| {
                if found_hover.is_some() {
                    return;
                }
                if let Statement::Function(f) = s {
                    let f_name = ast.strings.resolve(&f.name);
                    let short_name = f_name.rsplit("::").next().unwrap_or(f_name);
                    if short_name == method
                        && let Some((decl_path, src, decl_span)) = resolve_ast_span(ast, &f.span)
                    {
                        let sig = render_function_signature(ast, f);
                        let mut markdown = format!("```jc\n{sig}\n```");
                        let aliases = render_aliases(ast, &f.aliases, doc.lang);
                        markdown.push_str(&aliases);

                        if let Some(doc_text) = extract_doc_comment(src, decl_span.start, doc.lang)
                        {
                            markdown.push_str(&format!("\n\n---\n\n{doc_text}"));
                        }

                        found_hover = Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: markdown,
                            }),
                            range: get_ident_range_at_offset(&doc.text, offset, ast, &doc.path)
                                .or_else(|| span_to_range(ast, decl_path, &f.span)),
                        });
                    }
                }
            });

            if let Some(h) = found_hover {
                return Some(h);
            }
        }
        _ => {}
    }

    None
}

fn find_enum_by_name<'a>(ast: &'a Ast, name: &str) -> Option<&'a EnumDecl> {
    for stmt in &ast.statements {
        if let Statement::Enum(e) = stmt {
            let e_name = ast.strings.resolve(&e.name);
            let short_name = e_name.rsplit("::").next().unwrap_or(e_name);
            if short_name == name || e_name == name || short_name.eq_ignore_ascii_case(name) {
                return Some(e);
            }
        }
    }
    None
}

fn get_ident_range_at_offset(
    text: &str,
    offset: usize,
    ast: &Ast,
    doc_path: &std::path::Path,
) -> Option<Range> {
    if offset > text.len() {
        return None;
    }

    let mut safe_offset = offset;
    while safe_offset > 0 && !text.is_char_boundary(safe_offset) {
        safe_offset -= 1;
    }

    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_';

    let start = text[..safe_offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident_char(*c))
        .last()
        .map_or(safe_offset, |(idx, _)| idx);

    let end = text[safe_offset..]
        .char_indices()
        .take_while(|(_, c)| is_ident_char(*c))
        .last()
        .map_or(safe_offset, |(idx, c)| safe_offset + idx + c.len_utf8());

    if start >= end {
        return None;
    }

    let index = ast.line_indexes.get(doc_path)?;
    let start_pos = TextSize::from(u32::try_from(start).ok()?);
    let end_pos = TextSize::from(u32::try_from(end).ok()?);

    let start_lc = index.try_line_col(start_pos)?;
    let end_lc = index.try_line_col(end_pos)?;

    let start_wide = index.to_wide(line_index::WideEncoding::Utf16, start_lc)?;
    let end_wide = index.to_wide(line_index::WideEncoding::Utf16, end_lc)?;

    Some(Range {
        start: Position {
            line: start_wide.line,
            character: start_wide.col,
        },
        end: Position {
            line: end_wide.line,
            character: end_wide.col,
        },
    })
}
