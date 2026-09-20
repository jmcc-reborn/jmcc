//! Semantic token extraction for enhanced AST-driven syntax highlighting.

use std::path::Path;

use jmcc::ast::*;
use jmcc::diagnostic::resolve_ast_span;
use line_index::TextSize;
use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};

pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::TYPE,        // 0
    SemanticTokenType::CLASS,       // 1
    SemanticTokenType::ENUM,        // 2
    SemanticTokenType::INTERFACE,   // 3
    SemanticTokenType::FUNCTION,    // 4
    SemanticTokenType::METHOD,      // 5
    SemanticTokenType::PARAMETER,   // 6
    SemanticTokenType::VARIABLE,    // 7
    SemanticTokenType::PROPERTY,    // 8
    SemanticTokenType::KEYWORD,     // 9
    SemanticTokenType::STRING,      // 10
    SemanticTokenType::NUMBER,      // 11
    SemanticTokenType::OPERATOR,    // 12
    SemanticTokenType::ENUM_MEMBER, // 13
    SemanticTokenType::EVENT,       // 14
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,     // 1 << 0
    SemanticTokenModifier::READONLY,        // 1 << 1
    SemanticTokenModifier::STATIC,          // 1 << 2
    SemanticTokenModifier::DEFAULT_LIBRARY, // 1 << 3
];

#[must_use]
pub fn get_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.to_vec(),
        token_modifiers: TOKEN_MODIFIERS.to_vec(),
    }
}

const MOD_DECLARATION: u32 = 1 << 0;
const MOD_READONLY: u32 = 1 << 1;
const MOD_STATIC: u32 = 1 << 2;
const MOD_DEFAULT_LIBRARY: u32 = 1 << 3;

#[derive(Clone, Copy, Debug)]
pub enum TokenType {
    Type = 0,
    Class = 1,
    Enum = 2,
    Interface = 3,
    Function = 4,
    Method = 5,
    Parameter = 6,
    Variable = 7,
    Property = 8,
    Keyword = 9,
    String = 10,
    Number = 11,
    Operator = 12,
    EnumMember = 13,
    Event = 14,
}

#[derive(Clone, Debug)]
struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

struct TokenCollector<'a> {
    ast: &'a Ast,
    target_path: &'a Path,
    tokens: Vec<RawToken>,
}

impl TokenCollector<'_> {
    fn add_raw_token(
        &mut self,
        line: u32,
        start_char: u32,
        length: u32,
        ty: TokenType,
        modifiers: u32,
    ) {
        if length > 0 {
            self.tokens.push(RawToken {
                line,
                start_char,
                length,
                token_type: ty as u32,
                modifiers,
            });
        }
    }

    fn add_span(&mut self, span: &Span, ty: TokenType, modifiers: u32) {
        if span.start >= span.end {
            return;
        }

        let Some((resolved_path, _source, local_span)) = resolve_ast_span(self.ast, span) else {
            return;
        };

        if resolved_path != self.target_path {
            return;
        }

        let Some(index) = self.ast.line_indexes.get(self.target_path) else {
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

        let length = if start_wide.line == end_wide.line {
            end_wide.col.saturating_sub(start_wide.col)
        } else {
            let next_line_start = index
                .to_wide(
                    line_index::WideEncoding::Utf16,
                    line_index::LineCol {
                        line: start_lc.line + 1,
                        col: 0,
                    },
                )
                .map_or(1, |w| w.col);
            next_line_start.saturating_sub(start_wide.col).max(1)
        };

        self.add_raw_token(start_wide.line, start_wide.col, length, ty, modifiers);
    }

    /// Adds a token precisely for an identifier/word within an outer span.
    fn add_ident_span(&mut self, outer_span: &Span, word: &str, ty: TokenType, modifiers: u32) {
        if word.is_empty() {
            return;
        }

        let Some((resolved_path, src, local_span)) = resolve_ast_span(self.ast, outer_span) else {
            return;
        };

        if resolved_path != self.target_path {
            return;
        }

        let Some(index) = self.ast.line_indexes.get(self.target_path) else {
            return;
        };

        if local_span.end > src.len() || local_span.start >= local_span.end {
            return;
        }

        let snippet = &src[local_span.clone()];
        let Some(rel_offset) = find_exact_word(snippet, word) else {
            return;
        };

        let word_start = local_span.start + rel_offset;
        let word_end = word_start + word.len();

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

        let length = end_wide.col.saturating_sub(start_wide.col);
        self.add_raw_token(start_wide.line, start_wide.col, length, ty, modifiers);
    }

    fn collect_statements(&mut self, stmts: &[Statement], inside_class: bool) {
        for stmt in stmts {
            self.collect_statement(stmt, inside_class);
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "Walks all AST statement kinds for semantic tokens"
    )]
    fn collect_statement(&mut self, stmt: &Statement, inside_class: bool) {
        match stmt {
            Statement::Function(f) => {
                let name = self.ast.strings.resolve(&f.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                let token_type = if inside_class {
                    TokenType::Method
                } else {
                    TokenType::Function
                };
                self.add_ident_span(&f.span, short_name, token_type, MOD_DECLARATION);

                for param in &f.params {
                    let p_name = self.ast.strings.resolve(&param.name);
                    // Do not override `self`/`сам`/`себя` with Parameter to avoid yellow coloring in VS Code
                    if p_name != "self" && p_name != "сам" && p_name != "себя" {
                        self.add_ident_span(
                            &param.span,
                            p_name,
                            TokenType::Parameter,
                            MOD_DECLARATION,
                        );
                    }
                    if let Some(ty_id) = param.ty {
                        let ty_name = self.ast.strings.resolve(&ty_id);
                        let short_name = ty_name
                            .split('<')
                            .next()
                            .unwrap_or(ty_name)
                            .rsplit("::")
                            .next()
                            .unwrap_or(ty_name);
                        self.add_ident_span(&param.span, short_name, TokenType::Type, 0);
                    }
                    if let Some(val) = param.default {
                        self.collect_expr(val);
                    }
                }
                if let Some(ret_id) = f.return_type {
                    let ty_name = self.ast.strings.resolve(&ret_id);
                    let short_name = ty_name
                        .split('<')
                        .next()
                        .unwrap_or(ty_name)
                        .rsplit("::")
                        .next()
                        .unwrap_or(ty_name);
                    self.add_ident_span(&f.span, short_name, TokenType::Type, 0);
                }
                self.collect_statements(&f.body, inside_class);
            }
            Statement::Process(p) => {
                let name = self.ast.strings.resolve(&p.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&p.span, short_name, TokenType::Function, MOD_DECLARATION);
                for param in &p.params {
                    let p_name = self.ast.strings.resolve(&param.name);
                    if p_name != "self" && p_name != "сам" && p_name != "себя" {
                        self.add_ident_span(
                            &param.span,
                            p_name,
                            TokenType::Parameter,
                            MOD_DECLARATION,
                        );
                    }
                }
                self.collect_statements(&p.body, false);
            }
            Statement::Event(e) => {
                let ev_name = self.ast.strings.resolve(&e.event_name);
                self.add_ident_span(&e.span, ev_name, TokenType::Event, MOD_DECLARATION);
                self.collect_statements(&e.body, false);
            }
            Statement::Class(c) => {
                let name = self.ast.strings.resolve(&c.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&c.span, short_name, TokenType::Class, MOD_DECLARATION);
                if let Some(parent) = c.parent {
                    let p_name = self.ast.strings.resolve(&parent);
                    let p_short = p_name.rsplit("::").next().unwrap_or(p_name);
                    self.add_ident_span(&c.span, p_short, TokenType::Class, 0);
                }
                for iface in &c.implements {
                    let if_name = self.ast.strings.resolve(iface);
                    let if_short = if_name.rsplit("::").next().unwrap_or(if_name);
                    self.add_ident_span(&c.span, if_short, TokenType::Interface, 0);
                }
                self.collect_statements(&c.body, true);
            }
            Statement::Interface(i) => {
                let name = self.ast.strings.resolve(&i.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&i.span, short_name, TokenType::Interface, MOD_DECLARATION);
                self.collect_statements(&i.body, true);
            }
            Statement::Enum(e) => {
                let name = self.ast.strings.resolve(&e.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&e.span, short_name, TokenType::Enum, MOD_DECLARATION);
                for val in &e.values {
                    let val_name = self.ast.strings.resolve(val);
                    self.add_ident_span(&e.span, val_name, TokenType::EnumMember, 0);
                }
            }
            Statement::TypeAlias(t) => {
                let name = self.ast.strings.resolve(&t.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&t.span, short_name, TokenType::Type, MOD_DECLARATION);
            }
            Statement::VarDecl(v) => {
                for name in &v.names {
                    let s = jmcc::ast::text_value_to_string(self.ast, name);
                    self.add_ident_span(&name.span, &s, TokenType::Variable, MOD_DECLARATION);
                }
                for ty_id in v.tys.iter().flatten() {
                    let ty_name = self.ast.strings.resolve(ty_id);
                    let short_name = ty_name
                        .split('<')
                        .next()
                        .unwrap_or(ty_name)
                        .rsplit("::")
                        .next()
                        .unwrap_or(ty_name);
                    self.add_ident_span(&v.span, short_name, TokenType::Type, 0);
                }
                if let Some(val) = v.value {
                    self.collect_expr(val);
                }
            }
            Statement::Assign(a) => {
                for target in &a.targets {
                    self.collect_expr(*target);
                }
                self.collect_expr(a.value);
            }
            Statement::If(i) => {
                if i.is_not {
                    self.add_ident_span(&i.span, "not", TokenType::Keyword, 0);
                    self.add_ident_span(&i.span, "не", TokenType::Keyword, 0);
                }
                self.collect_expr(i.condition);
                self.collect_statements(&i.then_body, inside_class);
                for (cond, body) in &i.elif_branches {
                    self.collect_expr(*cond);
                    self.collect_statements(body, inside_class);
                }
                if let Some(else_b) = &i.else_body {
                    self.collect_statements(else_b, inside_class);
                }
            }
            Statement::While(w) => {
                if w.is_not {
                    self.add_ident_span(&w.span, "not", TokenType::Keyword, 0);
                    self.add_ident_span(&w.span, "не", TokenType::Keyword, 0);
                }
                self.collect_expr(w.condition);
                self.collect_statements(&w.body, inside_class);
            }
            Statement::For(f) => {
                for var in &f.vars {
                    let s = jmcc::ast::text_value_to_string(self.ast, var);
                    self.add_ident_span(&var.span, &s, TokenType::Variable, MOD_DECLARATION);
                }
                self.collect_expr(f.iterable);
                self.collect_statements(&f.body, inside_class);
            }
            Statement::Return(r) => {
                if let Some(val) = r.value {
                    self.collect_expr(val);
                }
            }
            Statement::Expr(e) => {
                self.collect_expr(*e);
            }
            Statement::Match(m) => {
                self.collect_expr(m.expr);
                for arm in &m.arms {
                    for pat in &arm.patterns {
                        self.collect_expr(*pat);
                    }
                    if let Some(guard) = arm.guard {
                        self.collect_expr(guard);
                    }
                    self.collect_statements(&arm.body, inside_class);
                }
            }
            Statement::TryCatch(t) => {
                self.collect_statements(&t.try_body, inside_class);
                if let Some(var) = &t.catch_var {
                    let s = jmcc::ast::text_value_to_string(self.ast, var);
                    self.add_ident_span(&var.span, &s, TokenType::Variable, MOD_DECLARATION);
                }
                self.collect_statements(&t.catch_body, inside_class);
            }
            Statement::Throw(t) => {
                if let Some(val) = t.value {
                    self.collect_expr(val);
                }
            }
            Statement::Import(_) | Statement::Break(_) => {}
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        reason = "Walks all AST expression kinds for semantic tokens"
    )]
    fn collect_expr(&mut self, eid: ExprId) {
        let expr = &self.ast.exprs[eid];
        match expr {
            Expr::Number(n) => {
                self.add_span(&n.span, TokenType::Number, MOD_READONLY);
            }
            Expr::Bool(b) => {
                self.add_span(&b.span, TokenType::Keyword, MOD_READONLY);
            }
            Expr::Ident(str_id, span) => {
                let name = self.ast.strings.resolve(str_id);
                // Do not override `self`/`сам`/`себя` with Variable or Type; let TextMate handle `self`
                if name != "self" && name != "сам" && name != "себя" {
                    if is_type_name(name) {
                        self.add_ident_span(span, name, TokenType::Class, 0);
                    } else {
                        self.add_ident_span(span, name, TokenType::Variable, 0);
                    }
                }
            }
            Expr::Variable(v) => {
                let s = jmcc::ast::text_value_to_string(self.ast, &v.name);
                if s != "self" && s != "сам" && s != "себя" {
                    self.add_ident_span(&v.span, &s, TokenType::Variable, 0);
                }
            }
            Expr::Text(t) => {
                self.add_span(&t.span, TokenType::String, 0);
                for part in &t.parts {
                    if let TextPart::Interp(inner) = part {
                        self.collect_expr(*inner);
                    }
                }
            }
            Expr::Binary(b) => {
                self.collect_expr(b.left);
                match b.op {
                    BinOp::And => {
                        self.add_ident_span(&b.span, "and", TokenType::Keyword, 0);
                        self.add_ident_span(&b.span, "и", TokenType::Keyword, 0);
                    }
                    BinOp::Or => {
                        self.add_ident_span(&b.span, "or", TokenType::Keyword, 0);
                        self.add_ident_span(&b.span, "или", TokenType::Keyword, 0);
                    }
                    _ => {}
                }
                self.collect_expr(b.right);
            }
            Expr::Unary(u) => {
                if matches!(u.op, UnOp::Not) {
                    self.add_ident_span(&u.span, "not", TokenType::Keyword, 0);
                    self.add_ident_span(&u.span, "не", TokenType::Keyword, 0);
                }
                self.collect_expr(u.operand);
            }
            Expr::Property(p) => {
                self.collect_expr(p.object);
                let prop = self.ast.strings.resolve(&p.property);
                self.add_ident_span(&p.span, prop, TokenType::Property, 0);
            }
            Expr::Subscript(s) => {
                self.collect_expr(s.object);
                self.collect_expr(s.index);
                if let Some(end) = s.end {
                    self.collect_expr(end);
                }
            }
            Expr::Call(c) => {
                self.collect_expr(c.target);
                let method = self.ast.strings.resolve(&c.method);
                self.add_ident_span(&c.span, method, TokenType::Method, 0);
                for arg in &c.args {
                    self.collect_expr(arg.value);
                }
            }
            Expr::Action(a) => {
                let obj = self.ast.strings.resolve(&a.object);
                let name = self.ast.strings.resolve(&a.name);
                self.add_ident_span(&a.span, obj, TokenType::Class, MOD_DEFAULT_LIBRARY);
                self.add_ident_span(
                    &a.span,
                    name,
                    TokenType::Method,
                    MOD_DEFAULT_LIBRARY | MOD_STATIC,
                );
                for arg in &a.args {
                    self.collect_expr(arg.value);
                }
                if let Some(ops) = &a.operations {
                    self.collect_statements(ops, false);
                }
                if let Some(lambdas) = &a.lambda {
                    for l in lambdas {
                        self.collect_expr(*l);
                    }
                }
            }
            Expr::Constructor(c) => {
                let name = self.ast.strings.resolve(&c.name);
                let short_name = name.rsplit("::").next().unwrap_or(name);
                self.add_ident_span(&c.span, short_name, TokenType::Class, 0);
                for arg in &c.args {
                    self.collect_expr(arg.value);
                }
            }
            Expr::Cast(c) => {
                self.collect_expr(c.expr);
                let ty_name = self.ast.strings.resolve(&c.ty);
                let short_name = ty_name.rsplit("::").next().unwrap_or(ty_name);
                self.add_ident_span(&c.span, short_name, TokenType::Type, 0);
            }
            Expr::List(l) => {
                for item in &l.values {
                    self.collect_expr(*item);
                }
            }
            Expr::Map(m) => {
                for k in &m.keys {
                    self.collect_expr(*k);
                }
                for v in &m.values {
                    self.collect_expr(*v);
                }
            }
            Expr::Ternary(t) => {
                self.collect_expr(t.cond);
                self.collect_expr(t.then_val);
                self.collect_expr(t.else_val);
            }
            Expr::Match(m) => {
                self.collect_expr(m.expr);
                for arm in &m.arms {
                    for pat in &arm.patterns {
                        self.collect_expr(*pat);
                    }
                    if let Some(g) = arm.guard {
                        self.collect_expr(g);
                    }
                    self.collect_statements(&arm.body, false);
                }
            }
            Expr::Lambda(l) => {
                for param in &l.params {
                    let p_name = self.ast.strings.resolve(&param.name);
                    if p_name != "self" && p_name != "сам" && p_name != "себя" {
                        self.add_ident_span(
                            &param.span,
                            p_name,
                            TokenType::Parameter,
                            MOD_DECLARATION,
                        );
                    }
                }
                match &l.body {
                    LambdaBody::Expr(e) => self.collect_expr(*e),
                    LambdaBody::Block(stmts) => self.collect_statements(stmts, false),
                }
            }
            Expr::Nbt(n) => {
                self.add_span(&n.span, TokenType::String, 0);
            }
        }
    }
}

/// Checks whether an identifier is `PascalCase` or known type name.
fn is_type_name(name: &str) -> bool {
    let short = name.rsplit("::").next().unwrap_or(name);
    short.chars().next().is_some_and(char::is_uppercase)
        || matches!(
            short,
            "number"
                | "text"
                | "boolean"
                | "array"
                | "map"
                | "vector"
                | "item"
                | "location"
                | "entity"
                | "block"
                | "sound"
                | "potion"
                | "particle"
                | "player"
                | "iterator"
                | "function"
                | "any"
                | "любой"
                | "текст"
                | "строка"
                | "число"
                | "логическое"
                | "булево"
                | "массив"
                | "список"
                | "словарь"
                | "карта"
                | "переменная"
                | "местоположение"
                | "локация"
                | "предмет"
                | "вектор"
                | "зелье"
                | "частица"
                | "звук"
                | "блок"
                | "сущность"
                | "игрок"
                | "итератор"
                | "функция"
                | "код"
                | "значение"
        )
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

/// Computes semantic tokens for a document AST.
#[must_use]
pub fn compute_semantic_tokens(ast: &Ast, target_path: &Path) -> SemanticTokens {
    let mut collector = TokenCollector {
        ast,
        target_path,
        tokens: Vec::new(),
    };

    collector.collect_statements(&ast.statements, false);

    // Sort by line, then by start_char
    collector.tokens.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.start_char.cmp(&b.start_char))
            .then_with(|| b.length.cmp(&a.length))
    });

    // Deduplicate and resolve overlapping tokens
    let mut clean_tokens = Vec::with_capacity(collector.tokens.len());
    let mut last_line = u32::MAX;
    let mut last_end = 0;

    for tok in collector.tokens {
        if tok.line != last_line {
            last_line = tok.line;
            last_end = tok.start_char + tok.length;
            clean_tokens.push(tok);
        } else if tok.start_char >= last_end {
            last_end = tok.start_char + tok.length;
            clean_tokens.push(tok);
        }
    }

    // Convert into relative deltas
    let mut data = Vec::with_capacity(clean_tokens.len());
    let mut prev_line = 0;
    let mut prev_start = 0;

    for tok in clean_tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.start_char.saturating_sub(prev_start)
        } else {
            tok.start_char
        };

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers_bitset: tok.modifiers,
        });

        prev_line = tok.line;
        prev_start = tok.start_char;
    }

    SemanticTokens {
        result_id: None,
        data,
    }
}
