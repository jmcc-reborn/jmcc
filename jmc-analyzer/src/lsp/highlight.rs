//! Document Highlight provider for highlighting symbol occurrences under cursor.

use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use line_index::TextSize;
use lsp_types::{
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, Position, Range,
};

use super::rename::get_ident_range_at_offset;
use super::state::{DocumentData, walk_statements};

/// Computes highlights for symbol under cursor in the current document.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "Traverses all statements and expressions for symbol highlights"
)]
pub fn provide_document_highlight(
    doc: &DocumentData,
    params: &DocumentHighlightParams,
) -> Option<Vec<DocumentHighlight>> {
    let ast = doc.ast.as_ref()?;
    let pos = params.text_document_position_params.position;

    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    let (_start, _end, target_word) = get_ident_range_at_offset(&doc.text, offset)?;
    if target_word.is_empty() {
        return None;
    }

    let mut highlights = Vec::new();

    // 1. Declarations
    walk_statements(&ast.statements, &mut |stmt| match stmt {
        Statement::VarDecl(v) => {
            for name in &v.names {
                let s = jmcc::ast::text_value_to_string(ast, name);
                if s == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        &name.span,
                        DocumentHighlightKind::WRITE,
                    );
                }
            }
        }
        Statement::Function(f) => {
            let name = ast.strings.resolve(&f.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &f.span,
                    DocumentHighlightKind::READ,
                );
            }
            for p in &f.params {
                let p_name = ast.strings.resolve(&p.name);
                if p_name == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        &p.span,
                        DocumentHighlightKind::WRITE,
                    );
                }
            }
        }
        Statement::Process(p) => {
            let name = ast.strings.resolve(&p.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &p.span,
                    DocumentHighlightKind::READ,
                );
            }
            for p in &p.params {
                let p_name = ast.strings.resolve(&p.name);
                if p_name == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        &p.span,
                        DocumentHighlightKind::WRITE,
                    );
                }
            }
        }
        Statement::Class(c) => {
            let name = ast.strings.resolve(&c.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &c.span,
                    DocumentHighlightKind::READ,
                );
            }
        }
        Statement::Enum(e) => {
            let name = ast.strings.resolve(&e.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &e.span,
                    DocumentHighlightKind::READ,
                );
            }
        }
        Statement::Interface(i) => {
            let name = ast.strings.resolve(&i.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &i.span,
                    DocumentHighlightKind::READ,
                );
            }
        }
        Statement::TypeAlias(t) => {
            let name = ast.strings.resolve(&t.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_word {
                add_highlight_span(
                    ast,
                    doc,
                    &mut highlights,
                    &t.span,
                    DocumentHighlightKind::READ,
                );
            }
        }
        Statement::For(f) => {
            for var in &f.vars {
                let s = jmcc::ast::text_value_to_string(ast, var);
                if s == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        &var.span,
                        DocumentHighlightKind::WRITE,
                    );
                }
            }
        }
        _ => {}
    });

    // 2. Expressions
    for (_eid, expr) in &ast.exprs {
        match expr {
            Expr::Ident(str_id, span) => {
                let ident = ast.strings.resolve(str_id);
                if ident == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        span,
                        DocumentHighlightKind::READ,
                    );
                }
            }
            Expr::Variable(v) => {
                let s = jmcc::ast::text_value_to_string(ast, &v.name);
                if s == target_word {
                    add_highlight_span(
                        ast,
                        doc,
                        &mut highlights,
                        &v.span,
                        DocumentHighlightKind::READ,
                    );
                }
            }
            _ => {}
        }
    }

    highlights.sort_by_key(|h| (h.range.start.line, h.range.start.character));
    highlights.dedup_by(|a, b| a.range == b.range);

    Some(highlights)
}

fn add_highlight_span(
    ast: &Ast,
    doc: &DocumentData,
    highlights: &mut Vec<DocumentHighlight>,
    outer_span: &Span,
    kind: DocumentHighlightKind,
) {
    let Some((path, _, local_span)) = resolve_ast_span(ast, outer_span) else {
        return;
    };
    if path != doc.path {
        return;
    }
    let Some(index) = ast.line_indexes.get(&doc.path) else {
        return;
    };

    let start_pos = TextSize::from(u32::try_from(local_span.start).unwrap_or(0));
    let end_pos = TextSize::from(u32::try_from(local_span.end).unwrap_or(0));

    let Some(start_lc) = index.try_line_col(start_pos) else {
        return;
    };
    let Some(end_lc) = index.try_line_col(end_pos) else {
        return;
    };

    let Some(start_wide) = index.to_wide(line_index::WideEncoding::Utf16, start_lc) else {
        return;
    };
    let Some(end_wide) = index.to_wide(line_index::WideEncoding::Utf16, end_lc) else {
        return;
    };

    highlights.push(DocumentHighlight {
        range: Range {
            start: Position {
                line: start_wide.line,
                character: start_wide.col,
            },
            end: Position {
                line: end_wide.line,
                character: end_wide.col,
            },
        },
        kind: Some(kind),
    });
}
