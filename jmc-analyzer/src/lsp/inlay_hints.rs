//! Inlay Hints provider for displaying inferred types and parameter names inline in the editor.

use jmcc::ast::semantic::Type;
use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use jmcc::i18n::Lang;
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
                // Parallel to `param_names`: true when the parameter is declared `ref`.
                // The keyword is fixed at the declaration, so the call site stays bare
                // and the hint is the only place the reference is visible.
                let mut param_refs = Vec::new();

                // A bare call `touch(box, 5)` has the function name as its target.
                // Its type is the callable class, whose `call` method would hide the
                // declared parameter list, so the declaration is resolved by name.
                let called_name = match &ast.exprs[c.target] {
                    Expr::Ident(name, _) => Some(ast.strings.resolve(name)),
                    _ => None,
                };
                if let Some(called_name) = called_name {
                    let mut fallback_names = Vec::new();
                    let mut fallback_refs = Vec::new();
                    walk_statements(&ast.statements, &mut |s| {
                        if let Statement::Function(f) = s {
                            let f_name = ast.strings.resolve(&f.name);
                            let short_name = f_name.rsplit("::").next().unwrap_or(f_name);
                            if f_name == called_name || short_name == called_name {
                                let mut names = Vec::new();
                                let mut refs = Vec::new();
                                for p in &f.params {
                                    let p_name = ast.strings.resolve(&p.name);
                                    if p_name != "self" && p_name != "сам" && p_name != "себя"
                                    {
                                        names.push(p_name);
                                        refs.push(p.is_ref);
                                    }
                                }
                                // The merged AST contains the standard library, so
                                // the same short name can exist elsewhere. The
                                // declaration in the open file wins.
                                let (path, _, _) = resolve_ast_span(ast, &f.span)
                                    .unwrap_or_else(|| (std::path::Path::new(""), "", 0..0));
                                if path == doc.path {
                                    param_names = names;
                                    param_refs = refs;
                                } else if fallback_names.is_empty() {
                                    fallback_names = names;
                                    fallback_refs = refs;
                                }
                            }
                        }
                    });
                    if param_names.is_empty() {
                        param_names = fallback_names;
                        param_refs = fallback_refs;
                    }
                }

                // Method resolution from target type
                if param_names.is_empty()
                    && let Some(Type::Class(def_id, _)) = doc.expr_types.get(&c.target)
                    && let Some(ir_ctx) = &doc.ir_ctx
                    && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                    && let Some(m_decl) = class_info.methods.get(method)
                {
                    for p in &m_decl.params {
                        let p_name = ast.strings.resolve(&p.name);
                        if p_name != "self" && p_name != "сам" && p_name != "себя" {
                            param_names.push(p_name);
                            param_refs.push(p.is_ref);
                        }
                    }
                } else if param_names.is_empty() {
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
                                            param_refs.push(p.is_ref);
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
                                                param_refs.push(p.is_ref);
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    });
                }

                let ref_keyword = if doc.lang == Lang::Ru {
                    "ссылка"
                } else {
                    "ref"
                };
                for (i, arg) in c.args.iter().enumerate() {
                    if arg.name.is_none() && i < param_names.len() {
                        let param_name = param_names[i];
                        let is_ref = param_refs.get(i).copied().unwrap_or(false);
                        // A reference argument is already marked by the `ref` hint, so
                        // repeating its parameter name would only add noise.
                        let label = if is_ref {
                            ref_keyword.to_owned()
                        } else {
                            format!("{param_name}:")
                        };
                        let val_expr = &ast.exprs[arg.value];
                        let val_span = val_expr.span();
                        if let Some((path, _, span)) = resolve_ast_span(ast, &val_span)
                            && path == doc.path
                            && let Some(pos) = offset_to_position(ast, &doc.path, span.start)
                        {
                            hints.push(InlayHint {
                                position: pos,
                                label: InlayHintLabel::String(label),
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
