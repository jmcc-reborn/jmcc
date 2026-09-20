#![allow(deprecated)]

//! Document Symbols provider for Outline view, breadcrumbs, and Go to Symbol in File.

use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use line_index::TextSize;
use lsp_types::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, Location, Position, Range,
    SymbolInformation, SymbolKind, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::diagnostics::span_to_range;
use super::state::{DocumentData, ServerState, walk_statements};

/// Collects hierarchical document symbols for the outline tree.
#[must_use]
pub fn provide_document_symbols(
    doc: &DocumentData,
    _params: &DocumentSymbolParams,
) -> Option<DocumentSymbolResponse> {
    let ast = doc.ast.as_ref()?;
    let mut symbols = Vec::new();

    for stmt in &ast.statements {
        if let Some(sym) = stmt_to_symbol(ast, stmt, &doc.path) {
            symbols.push(sym);
        }
    }

    Some(DocumentSymbolResponse::Nested(symbols))
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "Traverses all statement kinds for document symbols"
)]
fn stmt_to_symbol(
    ast: &Ast,
    stmt: &Statement,
    target_path: &std::path::Path,
) -> Option<DocumentSymbol> {
    match stmt {
        Statement::Class(c) => {
            let (path, src, _span) = resolve_ast_span(ast, &c.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&c.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);

            let range = span_to_range(ast, target_path, &c.span)?;
            let selection_range =
                name_selection_range(ast, target_path, src, &c.span, short_name).unwrap_or(range);

            let mut children = Vec::new();
            for member in &c.body {
                match member {
                    Statement::Function(f) => {
                        if let Some(m_sym) = func_to_symbol(ast, f, target_path, true) {
                            children.push(m_sym);
                        }
                    }
                    Statement::VarDecl(v) => {
                        for v_name in &v.names {
                            let s = jmcc::ast::text_value_to_string(ast, v_name);
                            if let Some(f_range) = span_to_range(ast, target_path, &v_name.span) {
                                children.push(DocumentSymbol {
                                    name: s,
                                    detail: Some("field".to_owned()),
                                    kind: SymbolKind::FIELD,
                                    tags: None,
                                    deprecated: None,
                                    range: f_range,
                                    selection_range: f_range,
                                    children: None,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }

            let detail = c.parent.map(|p| format!(": {}", ast.strings.resolve(&p)));

            Some(DocumentSymbol {
                name: short_name.to_owned(),
                detail,
                kind: SymbolKind::CLASS,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: if children.is_empty() {
                    None
                } else {
                    Some(children)
                },
            })
        }
        Statement::Interface(i) => {
            let (path, src, _span) = resolve_ast_span(ast, &i.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&i.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);

            let range = span_to_range(ast, target_path, &i.span)?;
            let selection_range =
                name_selection_range(ast, target_path, src, &i.span, short_name).unwrap_or(range);

            let mut children = Vec::new();
            for member in &i.body {
                if let Statement::Function(f) = member
                    && let Some(m_sym) = func_to_symbol(ast, f, target_path, true)
                {
                    children.push(m_sym);
                }
            }

            Some(DocumentSymbol {
                name: short_name.to_owned(),
                detail: Some("interface".to_owned()),
                kind: SymbolKind::INTERFACE,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: if children.is_empty() {
                    None
                } else {
                    Some(children)
                },
            })
        }
        Statement::Enum(e) => {
            let (path, src, _span) = resolve_ast_span(ast, &e.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&e.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);

            let range = span_to_range(ast, target_path, &e.span)?;
            let selection_range =
                name_selection_range(ast, target_path, src, &e.span, short_name).unwrap_or(range);

            let mut children = Vec::new();
            for val in &e.values {
                let val_name = ast.strings.resolve(val);
                if let Some(v_range) =
                    name_selection_range(ast, target_path, src, &e.span, val_name)
                {
                    children.push(DocumentSymbol {
                        name: val_name.to_owned(),
                        detail: None,
                        kind: SymbolKind::ENUM_MEMBER,
                        tags: None,
                        deprecated: None,
                        range: v_range,
                        selection_range: v_range,
                        children: None,
                    });
                }
            }

            Some(DocumentSymbol {
                name: short_name.to_owned(),
                detail: Some("enum".to_owned()),
                kind: SymbolKind::ENUM,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: if children.is_empty() {
                    None
                } else {
                    Some(children)
                },
            })
        }
        Statement::Function(f) => func_to_symbol(ast, f, target_path, false),
        Statement::Process(p) => {
            let (path, src, _span) = resolve_ast_span(ast, &p.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&p.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);

            let range = span_to_range(ast, target_path, &p.span)?;
            let selection_range =
                name_selection_range(ast, target_path, src, &p.span, short_name).unwrap_or(range);

            let params_str: Vec<_> = p
                .params
                .iter()
                .map(|param| ast.strings.resolve(&param.name).to_owned())
                .collect();

            Some(DocumentSymbol {
                name: short_name.to_owned(),
                detail: Some(format!("process({})", params_str.join(", "))),
                kind: SymbolKind::FUNCTION,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            })
        }
        Statement::Event(e) => {
            let (path, _src, _span) = resolve_ast_span(ast, &e.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&e.event_name);
            let range = span_to_range(ast, target_path, &e.span)?;

            Some(DocumentSymbol {
                name: format!("event {name}"),
                detail: Some("event handler".to_owned()),
                kind: SymbolKind::EVENT,
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            })
        }
        Statement::TypeAlias(t) => {
            let (path, src, _span) = resolve_ast_span(ast, &t.span)?;
            if path != target_path {
                return None;
            }
            let name = ast.strings.resolve(&t.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            let target_ty = ast.strings.resolve(&t.target_ty);

            let range = span_to_range(ast, target_path, &t.span)?;
            let selection_range =
                name_selection_range(ast, target_path, src, &t.span, short_name).unwrap_or(range);

            Some(DocumentSymbol {
                name: short_name.to_owned(),
                detail: Some(format!("= {target_ty}")),
                kind: SymbolKind::TYPE_PARAMETER,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            })
        }
        Statement::VarDecl(v) => {
            for name in &v.names {
                let (path, _src, _span) = resolve_ast_span(ast, &name.span)?;
                if path == target_path {
                    let s = jmcc::ast::text_value_to_string(ast, name);
                    let range = span_to_range(ast, target_path, &name.span)?;
                    return Some(DocumentSymbol {
                        name: s,
                        detail: Some("global var".to_owned()),
                        kind: SymbolKind::VARIABLE,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn func_to_symbol(
    ast: &Ast,
    f: &FunctionDecl,
    target_path: &std::path::Path,
    is_method: bool,
) -> Option<DocumentSymbol> {
    let (path, src, _span) = resolve_ast_span(ast, &f.span)?;
    if path != target_path {
        return None;
    }
    let name = ast.strings.resolve(&f.name);
    let short_name = name.rsplit("::").next().unwrap_or(name);

    let range = span_to_range(ast, target_path, &f.span)?;
    let selection_range =
        name_selection_range(ast, target_path, src, &f.span, short_name).unwrap_or(range);

    let params_str: Vec<_> = f
        .params
        .iter()
        .map(|p| {
            let p_name = ast.strings.resolve(&p.name);
            let p_ty =
                p.ty.map_or_else(|| "any".to_owned(), |t| ast.strings.resolve(&t).to_owned());
            format!("{p_name}: {p_ty}")
        })
        .collect();

    let ret_str = f
        .return_type
        .map_or_else(|| "void".to_owned(), |r| ast.strings.resolve(&r).to_owned());

    let detail = Some(format!("({}) -> {ret_str}", params_str.join(", ")));
    let kind = if is_method {
        SymbolKind::METHOD
    } else {
        SymbolKind::FUNCTION
    };

    Some(DocumentSymbol {
        name: short_name.to_owned(),
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    })
}

fn name_selection_range(
    ast: &Ast,
    target_path: &std::path::Path,
    src: &str,
    span: &Span,
    name: &str,
) -> Option<Range> {
    let (_, _, local_span) = resolve_ast_span(ast, span)?;
    let snippet = src.get(local_span.clone())?;

    let offset = find_ident_in_snippet(snippet, name)?;
    let abs_start = local_span.start + offset;
    let abs_end = abs_start + name.len();

    let index = ast.line_indexes.get(target_path)?;
    let start_pos = TextSize::from(u32::try_from(abs_start).ok()?);
    let end_pos = TextSize::from(u32::try_from(abs_end).ok()?);

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

fn find_ident_in_snippet(snippet: &str, ident: &str) -> Option<usize> {
    if ident.is_empty() || snippet.len() < ident.len() {
        return None;
    }

    let mut start = 0;
    while let Some(pos) = snippet[start..].find(ident) {
        let abs_pos = start + pos;
        let left_ok = snippet[..abs_pos]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let next_pos = abs_pos + ident.len();
        let right_ok = snippet[next_pos..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');

        if left_ok && right_ok {
            return Some(abs_pos);
        }
        let next_char_len = snippet[abs_pos..]
            .chars()
            .next()
            .map_or(1, |c| c.len_utf8());
        start = abs_pos + next_char_len;
    }
    None
}

/// Collects workspace symbols across all loaded documents matching query.
#[must_use]
pub fn provide_workspace_symbols(
    state: &ServerState,
    params: &WorkspaceSymbolParams,
) -> Option<WorkspaceSymbolResponse> {
    let query = params.query.to_ascii_lowercase();
    let mut symbols = Vec::new();

    for doc in state.documents.values() {
        let Some(ast) = &doc.ast else { continue };
        walk_statements(&ast.statements, &mut |stmt| {
            if let Some((name, kind, span)) = stmt_brief(ast, stmt)
                && (query.is_empty() || name.to_ascii_lowercase().contains(&query))
                && let Some(range) = span_to_range(ast, &doc.path, &span)
            {
                symbols.push(SymbolInformation {
                    name,
                    kind,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: doc.uri.clone(),
                        range,
                    },
                    container_name: None,
                });
            }
        });
    }

    Some(WorkspaceSymbolResponse::Flat(symbols))
}

fn stmt_brief(ast: &Ast, stmt: &Statement) -> Option<(String, SymbolKind, Span)> {
    match stmt {
        Statement::Function(f) => {
            let name = ast.strings.resolve(&f.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::FUNCTION, f.span.clone()))
        }
        Statement::Process(p) => {
            let name = ast.strings.resolve(&p.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::FUNCTION, p.span.clone()))
        }
        Statement::Event(e) => {
            let name = ast.strings.resolve(&e.event_name);
            Some((name.to_owned(), SymbolKind::EVENT, e.span.clone()))
        }
        Statement::Class(c) => {
            let name = ast.strings.resolve(&c.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::CLASS, c.span.clone()))
        }
        Statement::Interface(i) => {
            let name = ast.strings.resolve(&i.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::INTERFACE, i.span.clone()))
        }
        Statement::Enum(e) => {
            let name = ast.strings.resolve(&e.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::ENUM, e.span.clone()))
        }
        Statement::TypeAlias(t) => {
            let name = ast.strings.resolve(&t.name);
            let short = name.rsplit("::").next().unwrap_or(name);
            Some((short.to_owned(), SymbolKind::TYPE_PARAMETER, t.span.clone()))
        }
        _ => None,
    }
}
