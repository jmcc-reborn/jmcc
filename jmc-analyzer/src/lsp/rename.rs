//! Rename symbol (F2) and prepareRename implementation.

use std::collections::HashMap;

use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use jmcc::ir::KNOWN_OBJECTS;
use line_index::TextSize;
use lsp_types::{
    GotoDefinitionResponse, Position, PrepareRenameResponse, Range, RenameParams, TextEdit, Url,
    WorkspaceEdit,
};

use super::state::{DocumentData, ServerState, walk_statements};

/// Checks whether a given word is a reserved `JustCode` keyword.
fn is_reserved_keyword(word: &str) -> bool {
    matches!(
        word,
        "func"
            | "function"
            | "fun"
            | "def"
            | "proc"
            | "event"
            | "class"
            | "interface"
            | "enum"
            | "type"
            | "if"
            | "else"
            | "elif"
            | "while"
            | "for"
            | "in"
            | "return"
            | "break"
            | "continue"
            | "var"
            | "const"
            | "import"
            | "export"
            | "true"
            | "false"
            | "null"
            | "try"
            | "catch"
            | "throw"
            | "ref"
            | "inline"
            | "функция"
            | "процесс"
            | "событие"
            | "класс"
            | "интерфейс"
            | "перечисление"
            | "тип"
            | "если"
            | "иначе"
            | "иначеесли"
            | "пока"
            | "для"
            | "в"
            | "вернуть"
            | "прервать"
            | "продолжить"
            | "переменная"
            | "константа"
            | "импорт"
            | "экспорт"
            | "истина"
            | "правда"
            | "ложь"
            | "пусто"
    )
}

/// Validates whether the symbol under cursor can be renamed and returns its range.
#[must_use]
pub fn prepare_rename(doc: &DocumentData, pos: Position) -> Option<PrepareRenameResponse> {
    let ast = doc.ast.as_ref()?;
    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    let (start, end, word) = get_ident_range_at_offset(&doc.text, offset)?;

    // Do not allow renaming language keywords or JustMC object categories
    if is_reserved_keyword(word) || KNOWN_OBJECTS.contains(&word) {
        return None;
    }

    // Do not allow renaming symbols declared in the standard library
    if let Some(GotoDefinitionResponse::Scalar(loc)) = super::goto_def::provide_definition(
        doc,
        &lsp_types::GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: doc.uri.clone(),
                },
                position: pos,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    ) {
        let uri_str = loc.uri.as_str();
        if uri_str.contains("/std/") || uri_str.contains("\\std\\") {
            return None;
        }
    }

    let start_lc = index.try_line_col(TextSize::from(u32::try_from(start).ok()?))?;
    let end_lc = index.try_line_col(TextSize::from(u32::try_from(end).ok()?))?;

    let start_wide = index.to_wide(line_index::WideEncoding::Utf16, start_lc)?;
    let end_wide = index.to_wide(line_index::WideEncoding::Utf16, end_lc)?;

    Some(PrepareRenameResponse::Range(Range {
        start: Position {
            line: start_wide.line,
            character: start_wide.col,
        },
        end: Position {
            line: end_wide.line,
            character: end_wide.col,
        },
    }))
}

/// Executes rename across the document/workspace.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "Traverses all declarations and usages for workspace rename"
)]
pub fn rename_symbol(doc: &DocumentData, params: &RenameParams) -> Option<WorkspaceEdit> {
    let ast = doc.ast.as_ref()?;
    let pos = params.text_document_position.position;

    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    let (_start, _end, old_name) = get_ident_range_at_offset(&doc.text, offset)?;
    let new_name = &params.new_name;

    if is_reserved_keyword(old_name) || KNOWN_OBJECTS.contains(&old_name) {
        return None;
    }

    // Guard against standard library renaming
    if let Some(GotoDefinitionResponse::Scalar(loc)) = super::goto_def::provide_definition(
        doc,
        &lsp_types::GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: lsp_types::TextDocumentIdentifier {
                    uri: doc.uri.clone(),
                },
                position: pos,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        },
    ) {
        let uri_str = loc.uri.as_str();
        if uri_str.contains("/std/") || uri_str.contains("\\std\\") {
            return None;
        }
    }

    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

    // 1. Rename occurrences in declarations (including nested functions and blocks)
    walk_statements(&ast.statements, &mut |stmt| match stmt {
        Statement::VarDecl(v) => {
            for name in &v.names {
                let s = jmcc::ast::text_value_to_string(ast, name);
                if s == old_name {
                    add_name_edit_in_span(ast, &mut changes, &name.span, old_name, new_name);
                }
            }
        }
        Statement::Function(f) => {
            let name = ast.strings.resolve(&f.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &f.span, old_name, new_name);
            }
            for p in &f.params {
                let p_name = ast.strings.resolve(&p.name);
                if p_name == old_name {
                    add_name_edit_in_span(ast, &mut changes, &p.span, old_name, new_name);
                }
            }
        }
        Statement::Process(p) => {
            let name = ast.strings.resolve(&p.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &p.span, old_name, new_name);
            }
            for p in &p.params {
                let p_name = ast.strings.resolve(&p.name);
                if p_name == old_name {
                    add_name_edit_in_span(ast, &mut changes, &p.span, old_name, new_name);
                }
            }
        }
        Statement::Class(c) => {
            let name = ast.strings.resolve(&c.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &c.span, old_name, new_name);
            }
        }
        Statement::Enum(e) => {
            let name = ast.strings.resolve(&e.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &e.span, old_name, new_name);
            }
        }
        Statement::Interface(i) => {
            let name = ast.strings.resolve(&i.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &i.span, old_name, new_name);
            }
        }
        Statement::TypeAlias(t) => {
            let name = ast.strings.resolve(&t.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == old_name {
                add_name_edit_in_span(ast, &mut changes, &t.span, old_name, new_name);
            }
        }
        Statement::For(f) => {
            for var in &f.vars {
                let s = jmcc::ast::text_value_to_string(ast, var);
                if s == old_name {
                    add_name_edit_in_span(ast, &mut changes, &var.span, old_name, new_name);
                }
            }
        }
        _ => {}
    });

    // 2. Rename occurrences in expressions (references)
    for (_eid, expr) in &ast.exprs {
        match expr {
            Expr::Ident(str_id, span) => {
                let ident = ast.strings.resolve(str_id);
                if ident == old_name {
                    add_name_edit_in_span(ast, &mut changes, span, old_name, new_name);
                }
            }
            Expr::Variable(v) => {
                let s = jmcc::ast::text_value_to_string(ast, &v.name);
                if s == old_name {
                    add_name_edit_in_span(ast, &mut changes, &v.span, old_name, new_name);
                }
            }
            _ => {}
        }
    }

    // Deduplicate text edits per file
    for edits in changes.values_mut() {
        edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        edits.dedup_by(|a, b| a.range == b.range);
    }

    Some(WorkspaceEdit {
        changes: Some(changes),
        document_changes: None,
        change_annotations: None,
    })
}

fn add_name_edit_in_span(
    ast: &Ast,
    changes: &mut HashMap<Url, Vec<TextEdit>>,
    outer_span: &Span,
    name: &str,
    new_name: &str,
) {
    let Some((path, src, local_span)) = resolve_ast_span(ast, outer_span) else {
        return;
    };
    // Never edit standard library
    let path_str = path.to_string_lossy();
    if path_str.contains("/std/") || path_str.contains("\\std\\") || path.starts_with("std") {
        return;
    }
    let Some(url) = ServerState::path_to_url(path) else {
        return;
    };
    let Some(index) = ast.line_indexes.get(path) else {
        return;
    };

    if local_span.end > src.len() || local_span.start >= local_span.end {
        return;
    }
    let snippet = &src[local_span.clone()];
    let Some(rel_offset) = find_exact_word(snippet, name) else {
        return;
    };

    let word_start = local_span.start + rel_offset;
    let word_end = word_start + name.len();

    let start_pos = TextSize::from(u32::try_from(word_start).unwrap_or(0));
    let end_pos = TextSize::from(u32::try_from(word_end).unwrap_or(0));

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

    let edit = TextEdit {
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
        new_text: new_name.to_owned(),
    };

    changes.entry(url).or_default().push(edit);
}

fn find_exact_word(text: &str, word: &str) -> Option<usize> {
    if word.is_empty() || text.len() < word.len() {
        return None;
    }

    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs_pos = start + pos;
        let left_ok = text[..abs_pos]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let next_pos = abs_pos + word.len();
        let right_ok = text[next_pos..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');

        if left_ok && right_ok {
            return Some(abs_pos);
        }
        let next_char_len = text[abs_pos..].chars().next().map_or(1, |c| c.len_utf8());
        start = abs_pos + next_char_len;
    }
    None
}

pub(crate) fn get_ident_range_at_offset(text: &str, offset: usize) -> Option<(usize, usize, &str)> {
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

    if start < end {
        Some((start, end, &text[start..end]))
    } else {
        None
    }
}
