//! Parsing text literals and interpolations, `%module%name` syntax, and simple
//! string values (variable and declaration identifiers).

use super::*;

impl Parser<'_> {
    /// Reads `%name%` with optional suffix; opening `%` has already been consumed.
    ///
    /// Both `%` occurrences share identical diagnostics, hence sharing parsing
    /// and interning.
    pub(super) fn parse_mod_name(&mut self) -> Result<Spur> {
        let tok = self.bump();
        let LexedToken::Normal(Token::Ident(s), _, _) = &tok else {
            return Err(JmccError::UnexpectedToken {
                expected: "identifier after %".into(),
                got: format!("{tok:?}"),
            });
        };
        let first = *s;
        self.expect(Token::Mod)?;

        let mut formatted = format!("%{first}%");
        if let Some(Token::Ident(suffix)) = self.curr_token() {
            formatted.push_str(suffix);
            self.bump();
        }
        Ok(self.interner.get_or_intern(formatted))
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_string_value(&mut self) -> Result<Spur> {
        if matches!(
            self.curr_token(),
            Some(Token::DQuote | Token::SQuote | Token::BQuote)
        ) {
            let tv = self.parse_string(TextParsing::Legacy)?;
            match tv.parts.first() {
                Some(TextPart::Literal(s)) if tv.parts.len() == 1 => Ok(*s),
                _ => Err(JmccError::Generic(
                    "Expected literal string for string value".into(),
                )),
            }
        } else {
            self.parse_ident_str()
        }
    }

    #[instrument(skip(self, parsing), level = "trace")]
    pub(super) fn parse_string(&mut self, parsing: TextParsing) -> Result<TextValue> {
        let start_span = self.span();
        let quote_tok = self.bump();
        let _quote_char = match quote_tok {
            LexedToken::Normal(Token::DQuote, _, _) => '"',
            LexedToken::Normal(Token::SQuote, _, _) => '\'',
            LexedToken::Normal(Token::BQuote, _, _) => '`',
            _ => {
                return Err(JmccError::UnexpectedToken {
                    expected: "string token".into(),
                    got: format!("{quote_tok:?}"),
                });
            }
        };

        let mut parts = Vec::new();
        loop {
            match self.bump() {
                LexedToken::StrPart(s, _) => {
                    if !s.is_empty() {
                        parts.push(TextPart::Literal(self.interner.get_or_intern(s.as_str())));
                    }
                }
                LexedToken::InterpStart(_, _) => {
                    let expr = self.parse_expr(0)?;
                    if !matches!(self.curr(), LexedToken::InterpEnd(_)) {
                        return Err(JmccError::UnexpectedToken {
                            expected: "}".into(),
                            got: format!("{:?}", self.curr()),
                        });
                    }
                    self.bump();
                    parts.push(TextPart::Interp(expr));
                }
                LexedToken::StrEnd(_) => break,
                _ => {
                    return Err(JmccError::UnexpectedToken {
                        expected: "string content".into(),
                        got: format!("{:?}", self.curr()),
                    });
                }
            }
        }
        Ok(TextValue {
            parts,
            parsing,
            span: start_span.start..self.last_span().end,
        })
    }
}
