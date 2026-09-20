//! Code formatter for .jc files (like `cargo fmt` for Rust).
//!
//! Contains [`Formatter`], [`format`] entry point, and output infrastructure:
//! buffers, indentation, whitespace, and comments. Submodules format specific AST nodes:
//! declarations (`decls`), statements (`stmts`), expressions (`exprs`), strings and types (`text`).

use crate::ast::*;
use tracing::{Level, debug, info, instrument, span, trace, warn};

/// Formats the AST back to formatted .jc source code.
#[must_use]
#[instrument(skip(ast, source), level = "info")]
pub fn format(ast: &Ast, source: &str) -> String {
    info!("Starting AST formatting");
    let result = Formatter {
        ast,
        source,
        output: String::new(),
        indent: 0,
    }
    .format();
    debug!(output_len = result.len(), "Formatting complete");
    result
}

pub(super) struct Formatter<'a> {
    ast: &'a Ast,
    source: &'a str,
    output: String,
    indent: usize,
}

const INDENT_SIZE: usize = 4;
mod decls;
mod exprs;
mod stmts;
mod text;

impl<'a> Formatter<'a> {
    #[instrument(skip(self), level = "trace")]
    fn format(mut self) -> String {
        let _outer = span!(Level::TRACE, "formatter_run").entered();
        let mut last_end = 0;
        for stmt in &self.ast.statements {
            let s = self.stmt_span(stmt);
            self.write_gap(last_end, s.start, false);
            self.fmt_stmt(stmt);
            last_end = s.end;
        }
        self.write_gap(last_end, self.source.len(), true);
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        trace!(output_len = self.output.len(), "Format done");
        self.output
    }

    fn r(&self, s: StrId) -> &'a str {
        self.ast.strings.resolve(&s)
    }

    fn ind(&mut self) {
        for _ in 0..self.indent * INDENT_SIZE {
            self.output.push(' ');
        }
    }

    fn w(&mut self, s: &str) {
        self.output.push_str(s);
    }

    #[instrument(skip(self, stmt), level = "trace")]
    pub(super) fn stmt_span(&self, stmt: &Statement) -> Span {
        match stmt {
            Statement::Import(i) => i.span.clone(),
            Statement::Function(f) => f.span.clone(),
            Statement::Process(p) => p.span.clone(),
            Statement::Event(e) => e.span.clone(),
            Statement::Class(c) => c.span.clone(),
            Statement::Interface(i) => i.span.clone(),
            Statement::TypeAlias(ta) => ta.span.clone(),
            Statement::Enum(e) => e.span.clone(),
            Statement::If(i) => i.span.clone(),
            Statement::While(w) => w.span.clone(),
            Statement::For(f) => f.span.clone(),
            Statement::Break(b) => b.span.clone(),
            Statement::VarDecl(v) => v.span.clone(),
            Statement::Assign(a) => a.span.clone(),
            Statement::Return(r) => r.span.clone(),
            Statement::Expr(eid) => self.ast.exprs[*eid].span(),
            Statement::Match(m) => m.span.clone(),
            Statement::TryCatch(tc) => tc.span.clone(),
            Statement::Throw(th) => th.span.clone(),
        }
    }

    fn read_line_comment(chars: &mut impl Iterator<Item = char>) -> (String, bool) {
        let mut comment = String::from("//");
        let mut has_newline = false;
        for character in chars {
            if character == '\n' {
                has_newline = true;
                break;
            }
            comment.push(character);
        }
        (comment, has_newline)
    }

    fn read_block_comment(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> (String, bool) {
        let mut comment = String::from("/*");
        while let Some(character) = chars.next() {
            comment.push(character);
            if character == '*' && chars.peek() == Some(&'/') {
                comment.push(chars.next().unwrap_or('/'));
                return (comment, true);
            }
        }
        (comment, false)
    }

    fn write_line_comment(
        &mut self,
        comment: &str,
        has_newline: bool,
        is_first_line: &mut bool,
        inline_comment_printed: &mut bool,
        blank_lines: &mut usize,
    ) {
        let is_inline = *is_first_line
            && !*inline_comment_printed
            && !self.output.is_empty()
            && !self.output.ends_with('\n');
        if is_inline {
            self.output.push(' ');
            self.output.push_str(comment.trim());
            if has_newline {
                self.output.push('\n');
                *is_first_line = false;
            }
            *inline_comment_printed = true;
        } else {
            if !self.output.is_empty() && !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            if *blank_lines >= 1 && !self.output.ends_with("\n\n") {
                self.output.push('\n');
            }
            self.ind();
            self.output.push_str(comment.trim_start());
            self.output.push('\n');
            *blank_lines = 0;
            if has_newline {
                *is_first_line = false;
            }
        }
    }

    fn write_block_comment(
        &mut self,
        comment: &str,
        is_first_line: &mut bool,
        inline_comment_printed: &mut bool,
        blank_lines: &mut usize,
    ) {
        let is_inline = *is_first_line
            && !*inline_comment_printed
            && !comment.contains('\n')
            && !self.output.is_empty()
            && !self.output.ends_with('\n');
        if is_inline {
            self.output.push(' ');
            self.output.push_str(comment.trim());
            *inline_comment_printed = true;
            return;
        }
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        if *blank_lines >= 1 && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }
        for line in comment.lines() {
            let trimmed_end = line.trim_end();
            if !trimmed_end.is_empty() {
                self.ind();
                self.output.push_str(trimmed_end.trim_start());
            }
            self.output.push('\n');
        }
        if !comment.ends_with('\n') {
            self.output.push('\n');
        }
        *blank_lines = 0;
        *is_first_line = false;
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn write_gap(&mut self, start: usize, end: usize, is_eof: bool) {
        if start >= end {
            if !is_eof && !self.output.is_empty() && !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            return;
        }
        let text = &self.source[start..end];
        let mut blank_lines = 0;
        let mut current_line_blank = true;
        let mut is_first_line = true;
        let mut inline_comment_printed = false;
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '/' {
                if chars.peek() == Some(&'/') {
                    chars.next();
                    let (comment, has_newline) = Self::read_line_comment(&mut chars);
                    self.write_line_comment(
                        &comment,
                        has_newline,
                        &mut is_first_line,
                        &mut inline_comment_printed,
                        &mut blank_lines,
                    );
                    current_line_blank = has_newline;
                } else if chars.peek() == Some(&'*') {
                    chars.next();
                    let (comment, closed) = Self::read_block_comment(&mut chars);
                    if !closed {
                        warn!(start, end, "Unterminated block comment in gap");
                    }
                    self.write_block_comment(
                        &comment,
                        &mut is_first_line,
                        &mut inline_comment_printed,
                        &mut blank_lines,
                    );
                    current_line_blank = false;
                }
            } else if c == '\n' {
                if is_first_line {
                    is_first_line = false;
                    if !self.output.is_empty() && !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    current_line_blank = true;
                    blank_lines = 0;
                    inline_comment_printed = false;
                } else {
                    if current_line_blank {
                        blank_lines += 1;
                    }
                    current_line_blank = true;
                }
            } else if !c.is_whitespace() {
                current_line_blank = false;
            }
        }

        if !is_eof {
            if self.output.is_empty() {
                return;
            }
            let trailing_newlines = if self.output.ends_with("\n\n") {
                2
            } else {
                usize::from(self.output.ends_with('\n'))
            };
            let needed = if blank_lines >= 1 { 2 } else { 1 };
            for _ in trailing_newlines..needed {
                self.output.push('\n');
            }
        } else if blank_lines >= 1 && !self.output.ends_with("\n\n") {
            self.output.push('\n');
        }
    }

    pub(super) const MAX_WIDTH: usize = 100;

    /// Returns the character length of the current (last) line being written.
    pub(super) fn current_line_width(&self) -> usize {
        self.output.rfind('\n').map_or_else(
            || self.indent * INDENT_SIZE + self.output.len(),
            |pos| self.output.len() - pos - 1,
        )
    }

    /// Executes `f` into an isolated temporary buffer and returns the formatted text,
    /// restoring the previous output buffer.
    pub(super) fn render_to_string(&mut self, f: impl FnOnce(&mut Self)) -> String {
        let saved = std::mem::take(&mut self.output);
        f(self);
        std::mem::replace(&mut self.output, saved)
    }

    /// Formats a delimited list using rustfmt-style heuristics:
    /// uses a single line if it fits within [`Self::MAX_WIDTH`], otherwise splits
    /// into a block layout with each element on its own line and a trailing comma.
    pub(super) fn fmt_delimited_list(
        &mut self,
        open: &str,
        close: &str,
        rendered_items: &[String],
    ) {
        self.fmt_delimited_list_with_extra(open, close, rendered_items, 0);
    }

    pub(super) fn fmt_delimited_list_with_extra(
        &mut self,
        open: &str,
        close: &str,
        rendered_items: &[String],
        extra_len: usize,
    ) {
        if rendered_items.is_empty() {
            self.w(open);
            self.w(close);
            return;
        }

        let curr_len = self.current_line_width();
        let any_multiline = rendered_items.iter().any(|s| s.contains('\n'));
        let total_items_len: usize = rendered_items.iter().map(|s| s.len()).sum();
        let commas_len = 2 * (rendered_items.len() - 1);
        let oneline_len =
            curr_len + open.len() + total_items_len + commas_len + close.len() + extra_len;

        self.w(open);
        if !any_multiline && oneline_len <= Self::MAX_WIDTH {
            for (i, item) in rendered_items.iter().enumerate() {
                if i > 0 {
                    self.w(", ");
                }
                self.w(item);
            }
        } else {
            self.w("\n");
            self.indent += 1;
            for item in rendered_items {
                self.ind();
                self.w(item);
                self.w(",\n");
            }
            self.indent -= 1;
            self.ind();
        }
        self.w(close);
    }

    /// Formats elements separated by `, ` inside the given delimiters.
    ///
    /// Shared framework for [`Formatter::fmt_comma_list`], [`Formatter::fmt_list`],
    /// and [`Formatter::fmt_map`].
    pub(super) fn fmt_list_like<T>(
        &mut self,
        open: &str,
        close: &str,
        items: impl IntoIterator<Item = T>,
        mut fmt_item: impl FnMut(&mut Self, T),
    ) {
        self.w(open);
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            fmt_item(self, item);
        }
        self.w(close);
    }

    /// Formats elements separated by `, ` without surrounding delimiters.
    pub(super) fn fmt_comma_separated<T>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        fmt_item: impl FnMut(&mut Self, T),
    ) {
        self.fmt_list_like("", "", items, fmt_item);
    }
}
