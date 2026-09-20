//! Parsing `.jc` into AST.
//!
//! Contains [`Parser`] (lexer, token lookahead buffer, expression arena, and string
//! interner), its constructor, and token inspection utilities (spans, errors), plus the
//! [`parse_string`] entry point. Specific parsing logic is organized in submodules:
//! declarations in [`decls`], statements in [`stmts`], expressions in [`exprs`],
//! types in [`types`], and strings/literals in [`text`].

use id_arena::Arena;
use lasso::{Rodeo, Spur};
use line_index::LineIndex;
use logos::Span;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, instrument, trace};

use super::lexer::{LexedToken, LexerWrapper, Token};
use crate::ast::*;
use crate::error::{JmccError, Result};

mod decls;
mod exprs;
mod stmts;
mod text;
mod types;

pub struct Parser<'a> {
    lex: LexerWrapper<'a>,
    pub lexer_kind: crate::ast::lexer::LexerKind,
    input: &'a str,
    buffer: Vec<LexedToken<'a>>,
    pos: usize,
    last_span: Span,
    arena: Arena<Expr>,
    interner: Rodeo,
    default_scope: VarScope,
    edition: u16,
    offset: usize,
}

impl<'a> Parser<'a> {
    #[must_use]
    #[instrument(skip(input), level = "debug")]
    pub fn new(input: &'a str, edition: u16, offset: usize) -> Self {
        let lex = LexerWrapper::new(input);
        let lexer_kind = lex.kind();
        let mut parser = Self {
            lex,
            lexer_kind,
            input,
            buffer: Vec::with_capacity(8),
            pos: 0,
            last_span: 0..0,
            arena: Arena::new(),
            interner: Rodeo::default(),
            default_scope: if edition < 2026 {
                VarScope::Local
            } else {
                VarScope::Line
            },
            edition,
            offset,
        };
        parser.buffer.push(parser.lex.next_token());
        parser.buffer.push(parser.lex.next_token());
        parser.buffer.push(parser.lex.next_token());
        parser
    }

    #[instrument(skip(self), level = "trace")]
    fn curr(&self) -> &LexedToken<'a> {
        &self.buffer[self.pos]
    }

    fn is_eof(&self) -> bool {
        matches!(self.curr(), LexedToken::Eof)
    }

    #[instrument(skip(self), level = "trace")]
    fn curr_token(&self) -> Option<Token<'a>> {
        if let LexedToken::Normal(t, _, _) = self.curr() {
            Some(*t)
        } else {
            None
        }
    }

    #[instrument(skip(self, n), level = "trace")]
    fn peek_token_at(&mut self, n: usize) -> Option<Token<'a>> {
        while self.pos + n >= self.buffer.len() {
            self.buffer.push(self.lex.next_token());
        }
        if let LexedToken::Normal(t, _, _) = &self.buffer[self.pos + n] {
            Some(*t)
        } else {
            None
        }
    }

    #[instrument(skip(self, n), level = "trace")]
    fn peek_span_at(&mut self, n: usize) -> Option<Span> {
        while self.pos + n >= self.buffer.len() {
            self.buffer.push(self.lex.next_token());
        }
        match &self.buffer[self.pos + n] {
            LexedToken::Normal(_, s, _)
            | LexedToken::StrPart(_, s)
            | LexedToken::InterpStart(_, s)
            | LexedToken::InterpEnd(s)
            | LexedToken::StrEnd(s) => Some(s.start + self.offset..s.end + self.offset),
            LexedToken::Eof => None,
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn peek_span(&mut self) -> Option<Span> {
        self.peek_span_at(1)
    }

    #[instrument(skip(self), level = "trace")]
    fn peek_token(&mut self) -> Option<Token<'a>> {
        self.peek_token_at(1)
    }

    #[instrument(skip(self), level = "trace")]
    fn curr_had_newline(&self) -> bool {
        if let LexedToken::Normal(_, _, nl) = self.curr() {
            *nl
        } else {
            false
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn bump(&mut self) -> LexedToken<'a> {
        self.last_span = self.span();
        let old = self.buffer[self.pos].clone();
        trace!("Bumping token: {old:?}");
        self.pos += 1;
        if self.pos + 2 >= self.buffer.len() {
            self.buffer.push(self.lex.next_token());
        }
        old
    }

    #[instrument(skip(self), level = "trace")]
    fn bump_token(&mut self) -> Option<Token<'a>> {
        if let LexedToken::Normal(t, _, _) = self.bump() {
            Some(t)
        } else {
            None
        }
    }

    #[must_use]
    fn span(&self) -> Span {
        let raw = match self.curr() {
            LexedToken::Normal(_, s, _)
            | LexedToken::StrPart(_, s)
            | LexedToken::InterpStart(_, s)
            | LexedToken::InterpEnd(s)
            | LexedToken::StrEnd(s) => s.clone(),
            LexedToken::Eof => 0..0,
        };
        raw.start + self.offset..raw.end + self.offset
    }

    #[must_use]
    const fn last_span(&self) -> Span {
        self.last_span.start..self.last_span.end
    }

    #[must_use]
    pub fn current_error_span(&self) -> Span {
        match self.curr() {
            LexedToken::Normal(_, s, _)
            | LexedToken::StrPart(_, s)
            | LexedToken::InterpStart(_, s)
            | LexedToken::InterpEnd(s)
            | LexedToken::StrEnd(s) => s.clone(),
            LexedToken::Eof => {
                if self.last_span.end > self.last_span.start {
                    self.last_span.clone()
                } else if !self.input.is_empty() {
                    self.input.len().saturating_sub(1)..self.input.len()
                } else {
                    0..0
                }
            }
        }
    }

    #[instrument(skip(self, t), level = "trace")]
    fn is_token(&self, t: Token<'a>) -> bool {
        matches!(self.curr_token(), Some(ct) if ct == t)
    }

    #[instrument(skip(self, t), level = "trace")]
    fn eat(&mut self, t: Token<'a>) -> bool {
        if self.is_token(t) {
            self.bump();
            true
        } else {
            false
        }
    }

    #[instrument(skip(self, t), level = "trace")]
    fn expect(&mut self, t: Token<'a>) -> Result<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(JmccError::UnexpectedToken {
                expected: format!("{t:?}"),
                got: format!("{:?}", self.curr()),
            })
        }
    }

    #[instrument(skip(self), level = "trace")]
    fn eat_terminator(&mut self) -> Result<()> {
        if self.eat(Token::Semicolon) {
            return Ok(());
        }
        if self.curr_had_newline()
            || self.is_eof()
            || self.is_token(Token::RBrace)
            || self.is_token(Token::Else)
            || self.is_token(Token::Elif)
        {
            return Ok(());
        }
        Err(JmccError::UnexpectedToken {
            expected: "; or newline".to_owned(),
            got: format!("{:?}", self.curr()),
        })
    }

    /// Parses comma-separated items until the `close` token.
    ///
    /// The `close` token itself is not consumed.
    fn parse_comma_separated<T>(
        &mut self,
        close: Token<'a>,
        mut parse_item: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let mut items = Vec::new();
        while !self.is_token(close) && !self.is_eof() {
            items.push(parse_item(self)?);
            if !self.eat(Token::Comma) {
                break;
            }
        }
        Ok(items)
    }

    #[instrument(skip(self), level = "trace")]
    fn parse_ident_str(&mut self) -> Result<Spur> {
        let tok = self.bump();
        match &tok {
            LexedToken::Normal(Token::Ident(s), _, _) => Ok(self.interner.get_or_intern(*s)),
            LexedToken::Normal(_, span, _) => {
                let s = &self.input[span.clone()];
                if s.chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    Ok(self.interner.get_or_intern(s))
                } else {
                    Err(JmccError::UnexpectedToken {
                        expected: "identifier".into(),
                        got: format!("{tok:?}"),
                    })
                }
            }
            _ => Err(JmccError::UnexpectedToken {
                expected: "identifier".into(),
                got: format!("{tok:?}"),
            }),
        }
    }
}

/// # Errors
///
/// Returns an error when `input` cannot be parsed as a valid source file.
#[instrument(skip(input, source), level = "info")]
pub fn parse_string(input: &str, source: &str, edition: u16, offset: usize) -> Result<Ast> {
    debug!("Parsing file: {source}");
    let mut parser = Parser::new(input, edition, offset);
    let statements = match parser.parse_program() {
        Ok(stmts) => stmts,
        Err(err) => {
            let error_span = parser.current_error_span();
            return Err(err.with_source_context(
                input,
                Path::new(source),
                error_span,
                crate::i18n::current_lang(),
            ));
        }
    };

    let path = PathBuf::from(source);
    let mut sources = HashMap::new();
    sources.insert(path.clone(), input.to_owned());
    // Line/column index is built right where text enters `Ast`: single source of truth.
    let mut line_indexes = HashMap::new();
    line_indexes.insert(path.clone(), LineIndex::new(input));
    let file_offsets = vec![(path, offset, offset + input.len() + 1)];

    Ok(Ast {
        exprs: parser.arena,
        strings: parser.interner,
        statements,
        sources,
        line_indexes,
        file_offsets,
    })
}
