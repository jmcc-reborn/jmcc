//! Document formatting implementation using `jmcc::ast::format`.

use lsp_types::{DocumentFormattingParams, Position, Range, TextEdit};

use super::state::DocumentData;

/// Formats the document using the built-in JMCC AST formatter.
#[must_use]
pub fn format_document(
    doc: &DocumentData,
    _params: &DocumentFormattingParams,
) -> Option<Vec<TextEdit>> {
    let filename = doc.path.display().to_string();
    let local_ast = jmcc::ast::parser::parse_string(&doc.text, &filename, doc.edition, 0).ok()?;
    let formatted = jmcc::ast::format::format(&local_ast, &doc.text);

    if formatted == doc.text {
        return Some(Vec::new());
    }

    let lines: Vec<&str> = doc.text.lines().collect();
    let last_line = u32::try_from(lines.len().saturating_sub(1)).unwrap_or(0);
    let last_col = u32::try_from(lines.last().map_or(0, |l| l.len())).unwrap_or(0);

    Some(vec![TextEdit {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: last_line,
                character: last_col,
            },
        },
        new_text: formatted,
    }])
}
