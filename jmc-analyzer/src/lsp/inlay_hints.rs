//! Inlay Hints provider for displaying inferred types and parameter names inline in the editor.

use jmcc::ast::semantic::Type;
use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use jmcdata::generated::get_action_def;
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams};

use super::diagnostics::offset_to_position;
use super::state::{DocumentData, format_type, walk_statements};

/// Computes inlay hints for the document range.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "Computes inlay hints for variables and parameters"
)]
pub fn provide_inlay_hints(
    doc: &DocumentData,
    _params: &InlayHintParams,
) -> Option<Vec<InlayHint>> {
    let ast = doc.ast.as_ref()?;
    let mut hints = Vec::new();

    // 1. Inlay hints for untyped local variable declarations (`var x = ...`)
    walk_statements(&ast.statements, &mut |stmt| {
        if let Statement::VarDecl(v) = stmt {
            for (i, name) in v.names.iter().enumerate() {
                let has_explicit_type = v.tys.get(i).is_some_and(|t| t.is_some());
                if !has_explicit_type
                    && let Some(val_expr_id) = v.value
                    && let Some(ty) = doc.expr_types.get(&val_expr_id)
                    && let Some((path, _, span)) = resolve_ast_span(ast, &name.span)
                    && path == doc.path
                    && let Some(pos) = offset_to_position(ast, &doc.path, span.end)
                {
                    let ty_str = format_type(ty, doc.ir_ctx.as_ref(), doc.lang);
                    hints.push(InlayHint {
                        position: pos,
                        label: InlayHintLabel::String(format!(": {ty_str}")),
                        kind: Some(InlayHintKind::TYPE),
                        text_edits: None,
                        tooltip: None,
                        padding_left: Some(true),
                        padding_right: None,
                        data: None,
                    });
                }
            }
        }
    });

    // 2. Inlay hints for parameter names in action and function calls
    for (_eid, expr) in &ast.exprs {
        match expr {
            Expr::Action(a) => {
                let obj = ast.strings.resolve(&a.object);
                let name = ast.strings.resolve(&a.name);
                if let Some(def) = get_action_def(obj, name) {
                    let mut def_idx = 0;
                    for arg in &a.args {
                        if arg.name.is_none() {
                            while def_idx < def.args.len() {
                                let target_arg = &def.args[def_idx];
                                def_idx += 1;
                                let is_named_elsewhere = a.args.iter().any(|other| {
                                    other
                                        .name
                                        .is_some_and(|n| ast.strings.resolve(&n) == target_arg.id)
                                });
                                if !is_named_elsewhere {
                                    let val_expr = &ast.exprs[arg.value];
                                    let val_span = val_expr.span();
                                    if let Some((path, _, span)) = resolve_ast_span(ast, &val_span)
                                        && path == doc.path
                                        && let Some(pos) =
                                            offset_to_position(ast, &doc.path, span.start)
                                    {
                                        hints.push(InlayHint {
                                            position: pos,
                                            label: InlayHintLabel::String(format!(
                                                "{}:",
                                                target_arg.id
                                            )),
                                            kind: Some(InlayHintKind::PARAMETER),
                                            text_edits: None,
                                            tooltip: None,
                                            padding_left: None,
                                            padding_right: Some(true),
                                            data: None,
                                        });
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Expr::Call(c) => {
                let method = ast.strings.resolve(&c.method);
                let mut param_names = Vec::new();

                // Method resolution from target type
                if let Some(Type::Class(def_id, _)) = doc.expr_types.get(&c.target)
                    && let Some(ir_ctx) = &doc.ir_ctx
                    && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                    && let Some(m_decl) = class_info.methods.get(method)
                {
                    for p in &m_decl.params {
                        let p_name = ast.strings.resolve(&p.name);
                        if p_name != "self" && p_name != "сам" && p_name != "себя" {
                            param_names.push(p_name);
                        }
                    }
                } else {
                    // Fallback to top-level function search or class constructor search
                    walk_statements(&ast.statements, &mut |s| {
                        if !param_names.is_empty() {
                            return;
                        }
                        match s {
                            Statement::Function(f) => {
                                let f_name = ast.strings.resolve(&f.name);
                                let short_name = f_name.rsplit("::").next().unwrap_or(f_name);
                                if short_name == method {
                                    for p in &f.params {
                                        let p_name = ast.strings.resolve(&p.name);
                                        if p_name != "self" && p_name != "сам" && p_name != "себя"
                                        {
                                            param_names.push(p_name);
                                        }
                                    }
                                }
                            }
                            Statement::Class(cl) => {
                                let cl_name = ast.strings.resolve(&cl.name);
                                let short_name = cl_name.rsplit("::").next().unwrap_or(cl_name);
                                if short_name == method
                                    && let Some(ir_ctx) = &doc.ir_ctx
                                    && let Some(class_info) = ir_ctx.get_class_by_name(cl_name)
                                {
                                    let init_method = class_info
                                        .methods
                                        .get("__init__")
                                        .or_else(|| class_info.methods.get("__constructor__"))
                                        .or_else(|| class_info.methods.get("__иниц__"))
                                        .or_else(|| class_info.methods.get("__конструктор__"));
                                    if let Some(m) = init_method {
                                        for p in &m.params {
                                            let p_name = ast.strings.resolve(&p.name);
                                            if p_name != "self"
                                                && p_name != "сам"
                                                && p_name != "себя"
                                            {
                                                param_names.push(p_name);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    });
                }

                for (i, arg) in c.args.iter().enumerate() {
                    if arg.name.is_none() && i < param_names.len() {
                        let param_name = param_names[i];
                        let val_expr = &ast.exprs[arg.value];
                        let val_span = val_expr.span();
                        if let Some((path, _, span)) = resolve_ast_span(ast, &val_span)
                            && path == doc.path
                            && let Some(pos) = offset_to_position(ast, &doc.path, span.start)
                        {
                            hints.push(InlayHint {
                                position: pos,
                                label: InlayHintLabel::String(format!("{param_name}:")),
                                kind: Some(InlayHintKind::PARAMETER),
                                text_edits: None,
                                tooltip: None,
                                padding_left: None,
                                padding_right: Some(true),
                                data: None,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Some(hints)
}
