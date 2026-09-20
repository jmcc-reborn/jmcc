//! Translation of compiler errors and AST spans into LSP diagnostics.

use std::path::Path;

use jmcc::ast::Ast;
use jmcc::ast::semantic::SemanticErrorKind;
use jmcc::diagnostic::{DiagnosticLevel, resolve_ast_span, semantic_to_diagnostic};
use jmcc::i18n::Lang;
use line_index::{LineIndex, TextSize, WideEncoding, WideLineCol};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

/// Converts an LSP Position (UTF-16 code units) to byte offset using `LineIndex`.
#[must_use]
pub fn position_to_offset(index: &LineIndex, pos: Position) -> Option<usize> {
    let wide_lc = WideLineCol {
        line: pos.line,
        col: pos.character,
    };
    let line_col = index.to_utf8(WideEncoding::Utf16, wide_lc)?;
    Some(usize::from(index.offset(line_col)?))
}

/// Converts a byte offset within a source file into an LSP Position (UTF-16 code units) using `LineIndex`.
#[must_use]
pub fn offset_to_position(ast: &Ast, path: &Path, offset: usize) -> Option<Position> {
    let index = ast.line_indexes.get(path)?;
    let pos = TextSize::from(u32::try_from(offset).ok()?);
    let line_col = index.try_line_col(pos)?;
    let wide = index.to_wide(WideEncoding::Utf16, line_col)?;
    Some(Position {
        line: wide.line,
        character: wide.col,
    })
}

/// Converts an AST global byte span into an LSP Range (UTF-16 code units) for a specific file.
#[must_use]
pub fn span_to_range(
    ast: &Ast,
    target_path: &Path,
    span: &std::ops::Range<usize>,
) -> Option<Range> {
    let (resolved_path, _src, local_span) = resolve_ast_span(ast, span)?;
    if resolved_path != target_path {
        return None;
    }
    let index = ast.line_indexes.get(target_path)?;

    let start_pos = TextSize::from(u32::try_from(local_span.start).ok()?);
    let end_pos = TextSize::from(u32::try_from(local_span.end).ok()?);

    let start_lc = index.try_line_col(start_pos)?;
    let end_lc = index.try_line_col(end_pos)?;

    let start_wide = index.to_wide(WideEncoding::Utf16, start_lc)?;
    let end_wide = index.to_wide(WideEncoding::Utf16, end_lc)?;

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

/// Converts a compiler [`jmcc::diagnostic::Diagnostic`] into an LSP Diagnostic.
#[must_use]
pub fn compiler_diag_to_lsp(
    diag: &jmcc::diagnostic::Diagnostic,
    ast: &Ast,
    target_path: &Path,
    lang: Lang,
) -> Option<Diagnostic> {
    let severity = match diag.level {
        DiagnosticLevel::Error => DiagnosticSeverity::ERROR,
        DiagnosticLevel::Warning => DiagnosticSeverity::WARNING,
        DiagnosticLevel::Note => DiagnosticSeverity::INFORMATION,
        DiagnosticLevel::Help => DiagnosticSeverity::HINT,
    };

    let primary_label = diag
        .labels
        .iter()
        .find(|l| l.is_primary)
        .or_else(|| diag.labels.first());

    let default_range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 1,
        },
    };

    let range = primary_label.map_or(default_range, |label| {
        span_to_range(ast, target_path, &label.span).unwrap_or(default_range)
    });

    let mut message = diag.message.clone();
    if let Some(lbl) = primary_label.and_then(|l| l.label.as_ref())
        && !lbl.is_empty()
        && !message.contains(lbl)
    {
        message.push_str(&format!(" ({lbl})"));
    }

    let note_prefix = if lang == Lang::Ru {
        "Примечание:"
    } else {
        "Note:"
    };
    let help_prefix = if lang == Lang::Ru {
        "Подсказка:"
    } else {
        "Help:"
    };
    for note in &diag.notes {
        message.push_str(&format!("\n{note_prefix} {note}"));
    }
    for help in &diag.helps {
        message.push_str(&format!("\n{help_prefix} {help}"));
    }

    Some(Diagnostic {
        range,
        severity: Some(severity),
        code: diag.code.map(|c| NumberOrString::String(c.to_owned())),
        source: Some("jmc-analyzer".to_owned()),
        message,
        related_information: None,
        tags: None,
        code_description: None,
        data: None,
    })
}

/// Converts semantic error list into LSP Diagnostics for the target file.
#[must_use]
pub fn semantic_errors_to_lsp(
    errors: &[(SemanticErrorKind, std::ops::Range<usize>)],
    ast: &Ast,
    target_path: &Path,
    lang: Lang,
) -> Vec<Diagnostic> {
    let mut results = Vec::new();

    for (kind, span) in errors {
        let Some(range) = span_to_range(ast, target_path, span) else {
            continue;
        };

        let compiler_diag = semantic_to_diagnostic(kind, span, ast, lang);

        let mut message = kind.format_localized(lang);
        let note_prefix = if lang == Lang::Ru {
            "Примечание:"
        } else {
            "Note:"
        };
        let help_prefix = if lang == Lang::Ru {
            "Подсказка:"
        } else {
            "Help:"
        };
        for note in &compiler_diag.notes {
            message.push_str(&format!("\n{note_prefix} {note}"));
        }
        for help in &compiler_diag.helps {
            message.push_str(&format!("\n{help_prefix} {help}"));
        }

        let severity = match compiler_diag.level {
            DiagnosticLevel::Error => DiagnosticSeverity::ERROR,
            DiagnosticLevel::Warning => DiagnosticSeverity::WARNING,
            DiagnosticLevel::Note => DiagnosticSeverity::INFORMATION,
            DiagnosticLevel::Help => DiagnosticSeverity::HINT,
        };

        results.push(Diagnostic {
            range,
            severity: Some(severity),
            code: compiler_diag
                .code
                .map(|c| NumberOrString::String(c.to_owned())),
            source: Some("jmc-analyzer".to_owned()),
            message,
            related_information: None,
            tags: None,
            code_description: None,
            data: None,
        });
    }

    results
}
