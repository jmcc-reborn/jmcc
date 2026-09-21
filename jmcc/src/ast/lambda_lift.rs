//! Lambda lifting: transforms anonymous functions (`Expr::Lambda`) into synthetic
//! top-level inline functions and synthetic inline classes implementing `Function`.

use crate::ast::*;

#[expect(
    clippy::too_many_lines,
    reason = "Lambda lifting transforms expressions into synthetic functions and classes"
)]
pub fn lift_lambdas(ast: &mut Ast) {
    let mut lambda_ids = Vec::new();
    for (id, expr) in &ast.exprs {
        if matches!(expr, Expr::Lambda(_)) {
            lambda_ids.push(id);
        }
    }

    if lambda_ids.is_empty() {
        return;
    }

    let mut new_statements = Vec::new();

    for (counter, lambda_id) in lambda_ids.into_iter().enumerate() {
        let Expr::Lambda(lambda) = &ast.exprs[lambda_id] else {
            continue;
        };
        let lambda = lambda.clone();

        let fn_name_str = format!("__lambda_{counter}");
        let fn_name = ast.strings.get_or_intern(&fn_name_str);

        let class_name_str = format!("__LambdaClass_{counter}");
        let class_name = ast.strings.get_or_intern(&class_name_str);

        let fn_body = match lambda.body {
            LambdaBody::Expr(e) => vec![Statement::Return(ReturnStmt {
                value: Some(e),
                span: lambda.span.clone(),
            })],
            LambdaBody::Block(stmts) => stmts,
        };

        let func_decl = FunctionDecl {
            name: fn_name,
            generics: Vec::new(),
            params: lambda.params.clone(),
            return_type: lambda.return_type,
            body: fn_body,
            is_inline: true,
            is_exported: false,
            is_getter: false,
            is_setter: false,
            is_overload: false,
            aliases: vec![],
            test_attr: None,
            span: lambda.span.clone(),
        };

        let self_name = ast.strings.get_or_intern("self");
        let self_param = Param {
            name: self_name,
            ty: Some(class_name),
            default: None,
            is_ref: false,
            spread: 0,
            span: lambda.span.clone(),
        };

        let mut call_params = vec![self_param];
        call_params.extend(lambda.params.clone());

        let fn_ident = ast.exprs.alloc(Expr::Ident(fn_name, lambda.span.clone()));
        let call_args: Vec<ArgExpr> = lambda
            .params
            .iter()
            .map(|p| {
                let arg_val = ast.exprs.alloc(Expr::Ident(p.name, p.span.clone()));
                ArgExpr {
                    name: None,
                    value: arg_val,
                    spread: p.spread,
                    is_ref: p.is_ref,
                }
            })
            .collect();

        let call_expr = ast.exprs.alloc(Expr::Call(CallExpr {
            target: fn_ident,
            method: fn_name,
            args: call_args,
            span: lambda.span.clone(),
        }));

        let call_body = vec![Statement::Return(ReturnStmt {
            value: Some(call_expr),
            span: lambda.span.clone(),
        })];

        let call_method_name = ast.strings.get_or_intern("call");
        let call_method = FunctionDecl {
            name: call_method_name,
            generics: Vec::new(),
            params: call_params,
            return_type: lambda.return_type,
            body: call_body,
            is_inline: true,
            is_exported: false,
            is_getter: false,
            is_setter: false,
            is_overload: false,
            aliases: vec![],
            test_attr: None,
            span: lambda.span.clone(),
        };

        let iface_name = ast.strings.get_or_intern("Function");
        let class_decl = ClassDecl {
            name: class_name,
            generics: vec![],
            parent: None,
            implements: vec![iface_name],
            body: vec![Statement::Function(call_method)],
            is_inline: true,
            lang_item: false,
            is_dict: false,
            is_exported: false,
            aliases: vec![],
            span: lambda.span.clone(),
        };

        let class_ident = ast
            .exprs
            .alloc(Expr::Ident(class_name, lambda.span.clone()));
        ast.exprs[lambda_id] = Expr::Call(CallExpr {
            target: class_ident,
            method: class_name,
            args: vec![],
            span: lambda.span.clone(),
        });

        new_statements.push(Statement::Function(func_decl));
        new_statements.push(Statement::Class(class_decl));
    }

    ast.statements.extend(new_statements);
}
