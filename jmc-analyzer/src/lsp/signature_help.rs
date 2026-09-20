//! Signature Help provider for interactive parameter hints when calling functions and actions.

use jmcc::ast::*;
use jmcdata::generated::get_action_def;
use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, Position,
    SignatureHelp, SignatureHelpParams, SignatureInformation,
};

use super::hover::extract_doc_comment;
use super::state::{DocumentData, walk_statements};

/// Computes signature help for function/action call at the cursor position.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "Signature resolution across actions, methods, and functions"
)]
pub fn provide_signature_help(
    doc: &DocumentData,
    params: &SignatureHelpParams,
) -> Option<SignatureHelp> {
    let text = &doc.text;
    let pos = params.text_document_position_params.position;

    let line_prefix = get_line_prefix(text, pos);
    let (target_ident, active_param) = parse_call_context(&line_prefix)?;

    // 1. Check if it's an action call (e.g. `player::message` or `player::send_message`)
    if let Some((obj, action)) = target_ident.split_once("::")
        && let Some(def) = get_action_def(obj, action)
            .or_else(|| get_action_def(obj, action.strip_prefix("send_").unwrap_or(action)))
    {
        let mut param_infos = Vec::new();
        let mut param_strs = Vec::new();

        for arg in def.args {
            let label = format!("{}: {}", arg.id, arg.arg_type);
            param_strs.push(label.clone());
            param_infos.push(ParameterInformation {
                label: ParameterLabel::Simple(label),
                documentation: None,
            });
        }

        let label = format!("{obj}::{action}({})", param_strs.join(", "));
        let doc_text = format!(
            "### `{obj}::{action}`\n**Действие JustMC** (`{}`)",
            def.action_type
        );

        let sig = SignatureInformation {
            label,
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc_text,
            })),
            parameters: Some(param_infos),
            active_parameter: Some(active_param as u32),
        };

        return Some(SignatureHelp {
            signatures: vec![sig],
            active_signature: Some(0),
            active_parameter: Some(active_param as u32),
        });
    }

    // 2. Search for user-defined function or method in AST
    let ast = doc.ast.as_ref()?;
    let mut found_func = None;

    walk_statements(&ast.statements, &mut |s| {
        if found_func.is_some() {
            return;
        }
        if let Statement::Function(f) = s {
            let name = ast.strings.resolve(&f.name);
            let short_name = name.rsplit("::").next().unwrap_or(name);
            if short_name == target_ident {
                found_func = Some(f.clone());
            }
        }
    });

    if let Some(f) = found_func {
        let mut param_infos = Vec::new();
        let mut param_strs = Vec::new();

        for p in &f.params {
            let p_name = ast.strings.resolve(&p.name);
            let ty_str =
                p.ty.map_or_else(|| "any".to_owned(), |t| ast.strings.resolve(&t).to_owned());
            let label = format!("{p_name}: {ty_str}");
            param_strs.push(label.clone());
            param_infos.push(ParameterInformation {
                label: ParameterLabel::Simple(label),
                documentation: None,
            });
        }

        let ret_str = f
            .return_type
            .map_or_else(|| "void".to_owned(), |r| ast.strings.resolve(&r).to_owned());
        let label = format!(
            "function {target_ident}({}) -> {ret_str}",
            param_strs.join(", ")
        );

        let doc_text = if let Some((_, src, span)) =
            jmcc::diagnostic::resolve_ast_span(ast, &f.span)
            && let Some(comment) = extract_doc_comment(src, span.start, doc.lang)
        {
            comment
        } else {
            String::new()
        };

        let sig = SignatureInformation {
            label,
            documentation: if doc_text.is_empty() {
                None
            } else {
                Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: doc_text,
                }))
            },
            parameters: Some(param_infos),
            active_parameter: Some(active_param as u32),
        };

        return Some(SignatureHelp {
            signatures: vec![sig],
            active_signature: Some(0),
            active_parameter: Some(active_param as u32),
        });
    }

    None
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

/// Parses the line prefix to find the enclosing call target and active parameter index.
fn parse_call_context(prefix: &str) -> Option<(String, usize)> {
    let bytes = prefix.as_bytes();
    let mut paren_depth = 0;
    let mut comma_count = 0;
    let mut open_paren_idx = None;

    // Scan backwards from cursor
    for i in (0..bytes.len()).rev() {
        let b = bytes[i];
        match b {
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    open_paren_idx = Some(i);
                    break;
                }
            }
            b',' if paren_depth == 0 => {
                comma_count += 1;
            }
            _ => {}
        }
    }

    let paren_pos = open_paren_idx?;
    let before_paren = prefix[..paren_pos].trim_end();

    // Extract identifier before paren (handles `player::send_message` or `foo`)
    let ident_start = before_paren
        .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != ':')
        .map_or(0, |idx| idx + 1);

    let ident = before_paren[ident_start..].trim();
    if ident.is_empty() {
        return None;
    }

    Some((ident.to_owned(), comma_count))
}
