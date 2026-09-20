//! Find References (Shift+F12) implementation for symbols, methods, and variables.

use jmcc::ast::*;
use lsp_types::{Location, ReferenceParams};

use super::goto_def::name_location;
use super::state::{DocumentData, walk_statements};

/// Finds all references to the symbol at the cursor position.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "Collects symbol references across all AST statement and expression kinds"
)]
pub fn find_references(doc: &DocumentData, params: &ReferenceParams) -> Option<Vec<Location>> {
    let ast = doc.ast.as_ref()?;
    let pos = params.text_document_position.position;

    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    let target_name = get_word_at_offset(&doc.text, offset)?;
    let mut locations = Vec::new();

    // 1. Check declarations if include_declaration is true
    if params.context.include_declaration {
        walk_statements(&ast.statements, &mut |stmt| match stmt {
            Statement::VarDecl(v) => {
                for name in &v.names {
                    let s = jmcc::ast::text_value_to_string(ast, name);
                    if s == target_name
                        && let Some(loc) = name_location(ast, &name.span, &s)
                    {
                        locations.push(loc);
                    }
                }
            }
            Statement::Function(f) => {
                let name = ast.strings.resolve(&f.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &f.span, short_name)
                {
                    locations.push(loc);
                }
                for p in &f.params {
                    let p_name = ast.strings.resolve(&p.name);
                    if p_name == target_name
                        && let Some(loc) = name_location(ast, &p.span, p_name)
                    {
                        locations.push(loc);
                    }
                }
            }
            Statement::Process(p) => {
                let name = ast.strings.resolve(&p.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &p.span, short_name)
                {
                    locations.push(loc);
                }
                for p in &p.params {
                    let p_name = ast.strings.resolve(&p.name);
                    if p_name == target_name
                        && let Some(loc) = name_location(ast, &p.span, p_name)
                    {
                        locations.push(loc);
                    }
                }
            }
            Statement::Class(c) => {
                let name = ast.strings.resolve(&c.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &c.span, short_name)
                {
                    locations.push(loc);
                }
            }
            Statement::Enum(e) => {
                let name = ast.strings.resolve(&e.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &e.span, short_name)
                {
                    locations.push(loc);
                }
                for val in &e.values {
                    let val_name = ast.strings.resolve(val);
                    if val_name == target_name
                        && let Some(loc) = name_location(ast, &e.span, val_name)
                    {
                        locations.push(loc);
                    }
                }
            }
            Statement::Interface(i) => {
                let name = ast.strings.resolve(&i.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &i.span, short_name)
                {
                    locations.push(loc);
                }
            }
            Statement::TypeAlias(t) => {
                let name = ast.strings.resolve(&t.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                if short_name == target_name
                    && let Some(loc) = name_location(ast, &t.span, short_name)
                {
                    locations.push(loc);
                }
            }
            Statement::For(f) => {
                for var in &f.vars {
                    let s = jmcc::ast::text_value_to_string(ast, var);
                    if s == target_name
                        && let Some(loc) = name_location(ast, &var.span, &s)
                    {
                        locations.push(loc);
                    }
                }
            }
            _ => {}
        });
    }

    // 2. Find occurrences in expressions (identifiers, variables, calls, properties)
    for (_eid, expr) in &ast.exprs {
        match expr {
            Expr::Ident(str_id, span) => {
                let ident = ast.strings.resolve(str_id);
                if ident == target_name
                    && let Some(loc) = name_location(ast, span, ident)
                {
                    locations.push(loc);
                }
            }
            Expr::Variable(v) => {
                let s = jmcc::ast::text_value_to_string(ast, &v.name);
                if s == target_name
                    && let Some(loc) = name_location(ast, &v.span, &s)
                {
                    locations.push(loc);
                }
            }
            Expr::Call(c) => {
                let m = ast.strings.resolve(&c.method);
                if m == target_name
                    && let Some(loc) = name_location(ast, &c.span, m)
                {
                    locations.push(loc);
                }
            }
            Expr::Property(p) => {
                let prop = ast.strings.resolve(&p.property);
                if prop == target_name
                    && let Some(loc) = name_location(ast, &p.span, prop)
                {
                    locations.push(loc);
                }
            }
            _ => {}
        }
    }

    // Deduplicate locations
    locations.sort_by(|a, b| {
        a.uri
            .cmp(&b.uri)
            .then_with(|| a.range.start.line.cmp(&b.range.start.line))
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
    });
    locations.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);

    Some(locations)
}

fn get_word_at_offset(text: &str, offset: usize) -> Option<&str> {
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
        Some(&text[start..end])
    } else {
        None
    }
}
