//! Go to Definition (F12) implementation for symbols, functions, classes, enum variants, and imports.

use jmcc::ast::semantic::Type;
use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use line_index::TextSize;
use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Position, Range};

use super::state::{
    DocumentData, ServerState, extract_import_path, get_word_at_offset, resolve_import_to_file,
    walk_statements,
};

/// Finds the definition location of the symbol at the cursor position.
#[must_use]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "Traverses AST to resolve definitions with intelligent context priority"
)]
pub fn provide_definition(
    doc: &DocumentData,
    params: &GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let pos = params.text_document_position_params.position;

    // 0. Check if cursor is on an import statement line
    let line_text = doc.text.lines().nth(pos.line as usize).unwrap_or("");
    if let Some(import_path) = extract_import_path(line_text)
        && let Some(file_path) =
            resolve_import_to_file(&doc.path, &import_path, doc.ast.as_ref(), None)
        && let Some(url) = ServerState::path_to_url(&file_path)
    {
        return Some(GotoDefinitionResponse::Scalar(Location {
            uri: url,
            range: Range::default(),
        }));
    }

    let ast = doc.ast.as_ref()?;

    let index = ast.line_indexes.get(&doc.path)?;
    let offset = super::diagnostics::position_to_offset(index, pos)?;

    // 1. Check if cursor is on an import statement
    for stmt in &ast.statements {
        if let Statement::Import(imp) = stmt {
            let (path, _, span) = resolve_ast_span(ast, &imp.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let path_str = ast.strings.resolve(&imp.path);
                // Look for a source file whose path ends with this import target
                for file_path in ast.sources.keys() {
                    let file_str = file_path.to_string_lossy();
                    if file_str.contains(path_str)
                        && let Some(url) = ServerState::path_to_url(file_path)
                    {
                        return Some(GotoDefinitionResponse::Scalar(Location {
                            uri: url,
                            range: Range::default(),
                        }));
                    }
                }
            }
        }
    }

    // 2. Extract word under cursor
    let target_name = get_word_at_offset(&doc.text, offset)?;

    // 3. Declaration under cursor: if cursor is already on the declaration of a symbol,
    // stay on this definition instead of jumping to another entity with the same name.
    let mut cursor_decl = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if cursor_decl.is_some() {
            return;
        }
        match stmt {
            Statement::Function(f) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &f.span)
                    && path == doc.path
                {
                    let name = ast.strings.resolve(&f.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if offset >= span.start && offset <= span.start + short_name.len() + 10 {
                        cursor_decl = name_location(ast, &f.span, short_name);
                        return;
                    }
                }
                for p in &f.params {
                    if let Some((path, _, span)) = resolve_ast_span(ast, &p.span)
                        && path == doc.path
                    {
                        let p_name = ast.strings.resolve(&p.name);
                        if offset >= span.start && offset <= span.end {
                            cursor_decl = name_location(ast, &p.span, p_name);
                            return;
                        }
                    }
                }
            }
            Statement::Process(p) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &p.span)
                    && path == doc.path
                {
                    let name = ast.strings.resolve(&p.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if offset >= span.start && offset <= span.start + short_name.len() + 10 {
                        cursor_decl = name_location(ast, &p.span, short_name);
                    }
                }
            }
            Statement::Class(c) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &c.span)
                    && path == doc.path
                {
                    let name = ast.strings.resolve(&c.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if offset >= span.start && offset <= span.start + short_name.len() + 10 {
                        cursor_decl = name_location(ast, &c.span, short_name);
                    }
                }
            }
            Statement::Interface(i) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &i.span)
                    && path == doc.path
                {
                    let name = ast.strings.resolve(&i.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if offset >= span.start && offset <= span.start + short_name.len() + 10 {
                        cursor_decl = name_location(ast, &i.span, short_name);
                    }
                }
            }
            Statement::Enum(e) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &e.span)
                    && path == doc.path
                {
                    let name = ast.strings.resolve(&e.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if offset >= span.start && offset <= span.start + short_name.len() + 10 {
                        cursor_decl = name_location(ast, &e.span, short_name);
                        return;
                    }
                    for val in &e.values {
                        let val_name = ast.strings.resolve(val);
                        if val_name == target_name {
                            cursor_decl = name_location(ast, &e.span, val_name);
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
    });

    if let Some(loc) = cursor_decl {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 4. Special handling for `self`: jump to the current method's `self` param,
    // or to the current enclosing `class Name` declaration.
    if target_name == "self" {
        let mut self_param_loc = None;
        let mut enclosing_class_loc = None;

        walk_statements(&ast.statements, &mut |stmt| match stmt {
            Statement::Class(c) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &c.span)
                    && path == doc.path
                    && offset >= span.start
                    && offset <= span.end
                {
                    let name = ast.strings.resolve(&c.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    enclosing_class_loc = name_location(ast, &c.span, short_name);
                }
            }
            Statement::Function(f) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &f.span)
                    && path == doc.path
                    && offset >= span.start
                    && offset <= span.end
                {
                    for p in &f.params {
                        let p_name = ast.strings.resolve(&p.name);
                        if p_name == "self" {
                            self_param_loc = name_location(ast, &p.span, "self");
                        }
                    }
                }
            }
            _ => {}
        });

        if let Some(loc) = self_param_loc.or(enclosing_class_loc) {
            return Some(GotoDefinitionResponse::Scalar(loc));
        }
    }

    // 5. Property access: check if cursor is on `obj.prop` or `EnumType.VARIANT` (e.g. MessageType.TEXT)
    for (_eid, expr) in &ast.exprs {
        if let Expr::Property(p) = expr {
            let (path, _, span) = resolve_ast_span(ast, &p.span)?;
            if path == doc.path && offset >= span.start && offset <= span.end {
                let prop_name = ast.strings.resolve(&p.property);
                if prop_name == target_name {
                    // 5a. Check if object is an Enum identifier (e.g. MessageType.TEXT)
                    if let Some(Expr::Ident(obj_id, _)) = ast.exprs.get(p.object) {
                        let obj_name = ast.strings.resolve(obj_id);
                        if let Some(e) = find_enum_decl(ast, obj_name) {
                            for val in &e.values {
                                let val_name = ast.strings.resolve(val);
                                if val_name == target_name {
                                    return name_location(ast, &e.span, val_name)
                                        .map(GotoDefinitionResponse::Scalar);
                                }
                            }
                        }
                    }

                    // 5b. Check if object has class type
                    if let Some(Type::Class(def_id, _)) = doc.expr_types.get(&p.object)
                        && let Some(ir_ctx) = &doc.ir_ctx
                        && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                    {
                        // Check if it's a field
                        if class_info.fields.contains_key(target_name)
                            && let Some(loc) =
                                find_class_field_location(ast, &class_info.name, target_name)
                        {
                            return Some(GotoDefinitionResponse::Scalar(loc));
                        }
                        // Check if it's a getter method
                        if let Some(m) = class_info.methods.get(target_name) {
                            return name_location(ast, &m.span, target_name)
                                .map(GotoDefinitionResponse::Scalar);
                        }
                    }
                }
            }
        }
    }

    // 6. Method call dispatch: if cursor is on `obj.method(...)`, resolve receiver type
    // and jump to the specific class's method implementation.
    for (_eid, expr) in &ast.exprs {
        if let Expr::Call(c) = expr {
            let method_name = ast.strings.resolve(&c.method);
            if method_name == target_name
                && let Some((path, _, span)) = resolve_ast_span(ast, &c.span)
                && path == doc.path
                && offset >= span.start
                && offset <= span.end
            {
                // Check receiver type
                if let Some(Type::Class(def_id, _)) = doc.expr_types.get(&c.target)
                    && let Some(ir_ctx) = &doc.ir_ctx
                    && let Some(class_info) = ir_ctx.classes_by_def.get(def_id)
                {
                    // Find method in this class
                    if let Some(method_decl) = class_info.methods.get(target_name)
                        && let Some(loc) = name_location(ast, &method_decl.span, target_name)
                    {
                        return Some(GotoDefinitionResponse::Scalar(loc));
                    }
                }
            }
        }
    }

    // 7. Current enclosing class methods: if cursor is inside a class and calling a method
    let mut current_class_method_loc = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if current_class_method_loc.is_some() {
            return;
        }
        if let Statement::Class(c) = stmt
            && let Some((path, _, span)) = resolve_ast_span(ast, &c.span)
            && path == doc.path
            && offset >= span.start
            && offset <= span.end
        {
            // We are inside class `c`, search its body for `target_name` method
            for member in &c.body {
                if let Statement::Function(f) = member {
                    let name = ast.strings.resolve(&f.name);
                    let short_name = name.rsplit("::").next().unwrap_or(name);
                    if short_name.eq_ignore_ascii_case(target_name) {
                        current_class_method_loc = name_location(ast, &f.span, short_name);
                        return;
                    }
                }
            }
        }
    });

    if let Some(loc) = current_class_method_loc {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 8. Local variables and parameters in the CURRENT file
    let mut local_loc = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if local_loc.is_some() {
            return;
        }
        match stmt {
            Statement::VarDecl(v) => {
                if let Some((path, _, _)) = resolve_ast_span(ast, &v.span)
                    && path == doc.path
                {
                    for name in &v.names {
                        let s = jmcc::ast::text_value_to_string(ast, name);
                        if s == target_name
                            && let Some(loc) = name_location(ast, &name.span, &s)
                        {
                            local_loc = Some(loc);
                            return;
                        }
                    }
                }
            }
            Statement::Function(f) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &f.span)
                    && path == doc.path
                    && offset >= span.start
                    && offset <= span.end
                {
                    for p in &f.params {
                        let p_name = ast.strings.resolve(&p.name);
                        if p_name == target_name
                            && let Some(loc) = name_location(ast, &p.span, p_name)
                        {
                            local_loc = Some(loc);
                            return;
                        }
                    }
                }
            }
            Statement::Process(p) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &p.span)
                    && path == doc.path
                    && offset >= span.start
                    && offset <= span.end
                {
                    for p in &p.params {
                        let p_name = ast.strings.resolve(&p.name);
                        if p_name == target_name
                            && let Some(loc) = name_location(ast, &p.span, p_name)
                        {
                            local_loc = Some(loc);
                            return;
                        }
                    }
                }
            }
            Statement::For(f) => {
                if let Some((path, _, span)) = resolve_ast_span(ast, &f.span)
                    && path == doc.path
                    && offset >= span.start
                    && offset <= span.end
                {
                    for var in &f.vars {
                        let s = jmcc::ast::text_value_to_string(ast, var);
                        if s == target_name
                            && let Some(loc) = name_location(ast, &var.span, &s)
                        {
                            local_loc = Some(loc);
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
    });

    if let Some(loc) = local_loc {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 9. Exact match for symbols (classes, interfaces, enums, enum variants, functions)
    // 9a. Check enum variants across all enums first with EXACT case match
    for stmt in &ast.statements {
        if let Statement::Enum(e) = stmt {
            for val in &e.values {
                let val_name = ast.strings.resolve(val);
                if val_name == target_name
                    && let Some(loc) = name_location(ast, &e.span, val_name)
                {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }
    }

    // 9b. Check items in CURRENT file first with exact case match
    let mut current_file_exact = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if current_file_exact.is_some() {
            return;
        }
        check_item_exact_match(
            ast,
            stmt,
            target_name,
            Some(&doc.path),
            &mut current_file_exact,
        );
    });

    if let Some(loc) = current_file_exact {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 9c. Check items globally with exact case match
    let mut global_exact = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if global_exact.is_some() {
            return;
        }
        check_item_exact_match(ast, stmt, target_name, None, &mut global_exact);
    });

    if let Some(loc) = global_exact {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 10. Case-insensitive fallback (if exact match not found)
    // 10a. Case-insensitive enum variants
    for stmt in &ast.statements {
        if let Statement::Enum(e) = stmt {
            for val in &e.values {
                let val_name = ast.strings.resolve(val);
                if val_name.eq_ignore_ascii_case(target_name)
                    && let Some(loc) = name_location(ast, &e.span, val_name)
                {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }
    }

    // 10b. Case-insensitive items in current file
    let mut current_file_item = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if current_file_item.is_some() {
            return;
        }
        check_item_case_insensitive_match(
            ast,
            stmt,
            target_name,
            Some(&doc.path),
            &mut current_file_item,
        );
    });

    if let Some(loc) = current_file_item {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    // 10c. Case-insensitive items globally
    let mut global_item = None;
    walk_statements(&ast.statements, &mut |stmt| {
        if global_item.is_some() {
            return;
        }
        check_item_case_insensitive_match(ast, stmt, target_name, None, &mut global_item);
    });

    if let Some(loc) = global_item {
        return Some(GotoDefinitionResponse::Scalar(loc));
    }

    None
}

fn find_enum_decl<'a>(ast: &'a Ast, name: &str) -> Option<&'a EnumDecl> {
    for stmt in &ast.statements {
        if let Statement::Enum(e) = stmt {
            let e_name = ast.strings.resolve(&e.name);
            let short_name = e_name.rsplit("::").next().unwrap_or(e_name);
            if short_name == name || e_name == name || short_name.eq_ignore_ascii_case(name) {
                return Some(e);
            }
        }
    }
    None
}

fn find_class_field_location(ast: &Ast, class_name: &str, field_name: &str) -> Option<Location> {
    for stmt in &ast.statements {
        if let Statement::Class(c) = stmt {
            let c_name = ast.strings.resolve(&c.name);
            let short_c = c_name.rsplit("::").next().unwrap_or(c_name);
            if short_c == class_name || c_name == class_name {
                for member in &c.body {
                    if let Statement::VarDecl(v) = member {
                        for name in &v.names {
                            let s = jmcc::ast::text_value_to_string(ast, name);
                            if s == field_name {
                                return name_location(ast, &name.span, field_name);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn check_item_exact_match(
    ast: &Ast,
    stmt: &Statement,
    target_name: &str,
    target_path: Option<&std::path::Path>,
    result: &mut Option<Location>,
) {
    let matches_exact = |name_id: &StrId, aliases: &[StrId]| -> bool {
        let name = ast.strings.resolve(name_id);
        let short_name = name.rsplit("::").next().unwrap_or(name);
        if short_name == target_name || name == target_name {
            return true;
        }
        aliases
            .iter()
            .any(|a| ast.strings.resolve(a) == target_name)
    };

    let (span, name_str) = match stmt {
        Statement::Class(c) if matches_exact(&c.name, &c.aliases) => {
            let name = ast.strings.resolve(&c.name);
            (&c.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Interface(i) if matches_exact(&i.name, &i.aliases) => {
            let name = ast.strings.resolve(&i.name);
            (&i.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Enum(e) if matches_exact(&e.name, &e.aliases) => {
            let name = ast.strings.resolve(&e.name);
            (&e.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::TypeAlias(t) if matches_exact(&t.name, &t.aliases) => {
            let name = ast.strings.resolve(&t.name);
            (&t.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Function(f) if matches_exact(&f.name, &f.aliases) => {
            let name = ast.strings.resolve(&f.name);
            (&f.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Process(p) if matches_exact(&p.name, &p.aliases) => {
            let name = ast.strings.resolve(&p.name);
            (&p.span, name.rsplit("::").next().unwrap_or(name))
        }
        _ => return,
    };

    if file_matches(ast, span, target_path) {
        *result = name_location(ast, span, name_str);
    }
}

fn check_item_case_insensitive_match(
    ast: &Ast,
    stmt: &Statement,
    target_name: &str,
    target_path: Option<&std::path::Path>,
    result: &mut Option<Location>,
) {
    let matches_case_insensitive = |name_id: &StrId, aliases: &[StrId]| -> bool {
        let name = ast.strings.resolve(name_id);
        let short_name = name.rsplit("::").next().unwrap_or(name);
        if short_name.eq_ignore_ascii_case(target_name) || name.eq_ignore_ascii_case(target_name) {
            return true;
        }
        aliases
            .iter()
            .any(|a| ast.strings.resolve(a).eq_ignore_ascii_case(target_name))
    };

    let (span, name_str) = match stmt {
        Statement::Class(c) if matches_case_insensitive(&c.name, &c.aliases) => {
            let name = ast.strings.resolve(&c.name);
            (&c.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Interface(i) if matches_case_insensitive(&i.name, &i.aliases) => {
            let name = ast.strings.resolve(&i.name);
            (&i.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Enum(e) if matches_case_insensitive(&e.name, &e.aliases) => {
            let name = ast.strings.resolve(&e.name);
            (&e.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::TypeAlias(t) if matches_case_insensitive(&t.name, &t.aliases) => {
            let name = ast.strings.resolve(&t.name);
            (&t.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Function(f) if matches_case_insensitive(&f.name, &f.aliases) => {
            let name = ast.strings.resolve(&f.name);
            (&f.span, name.rsplit("::").next().unwrap_or(name))
        }
        Statement::Process(p) if matches_case_insensitive(&p.name, &p.aliases) => {
            let name = ast.strings.resolve(&p.name);
            (&p.span, name.rsplit("::").next().unwrap_or(name))
        }
        _ => return,
    };

    if file_matches(ast, span, target_path) {
        *result = name_location(ast, span, name_str);
    }
}

fn file_matches(ast: &Ast, span: &Span, target_path: Option<&std::path::Path>) -> bool {
    let Some(target) = target_path else {
        return true;
    };
    if let Some((path, _, _)) = resolve_ast_span(ast, span) {
        path == target
    } else {
        false
    }
}

/// Computes a location that selects ONLY the identifier name instead of the whole declaration block.
#[must_use]
pub fn name_location(ast: &Ast, span: &Span, name_str: &str) -> Option<Location> {
    let (path, src, local_span) = resolve_ast_span(ast, span)?;
    let url = ServerState::path_to_url(path)?;
    let index = ast.line_indexes.get(path)?;

    let snippet = src.get(local_span.clone())?;
    let (abs_start, abs_end) = find_ident_in_snippet(snippet, name_str).map_or_else(
        || {
            (
                local_span.start,
                (local_span.start + name_str.len()).min(local_span.end),
            )
        },
        |offset| {
            (
                local_span.start + offset,
                local_span.start + offset + name_str.len(),
            )
        },
    );

    let start_pos = TextSize::from(u32::try_from(abs_start).ok()?);
    let end_pos = TextSize::from(u32::try_from(abs_end).ok()?);

    let start_lc = index.try_line_col(start_pos)?;
    let end_lc = index.try_line_col(end_pos)?;

    let start_wide = index.to_wide(line_index::WideEncoding::Utf16, start_lc)?;
    let end_wide = index.to_wide(line_index::WideEncoding::Utf16, end_lc)?;

    Some(Location {
        uri: url,
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
