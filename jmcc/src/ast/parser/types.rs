//! Parsing types: base name, generic arguments (`<T, U>`), and type annotations
//! following `:` or `->`.

use super::*;

impl<'a> Parser<'a> {
    /// Type following `:` or `->` if present.
    pub(super) fn parse_optional_type(&mut self, after: Token<'a>) -> Result<Option<StrId>> {
        if self.eat(after) {
            Ok(Some(self.parse_type_str()?))
        } else {
            Ok(None)
        }
    }

    #[instrument(skip(self), level = "trace")]
    pub(super) fn parse_type_str(&mut self) -> Result<Spur> {
        let base_sym = self.parse_ident_str()?;
        if !self.is_token(Token::Lt) {
            return Ok(base_sym);
        }

        let mut type_str = self.interner.resolve(&base_sym).to_owned();
        let mut depth = 0;

        loop {
            if self.is_eof() {
                return Err(JmccError::UnexpectedEof {
                    context: "type generic".into(),
                });
            }

            if self.is_token(Token::Lt) {
                depth += 1;
                type_str.push('<');
                self.bump();
            } else if self.is_token(Token::Shl) {
                // The lexer emits `<<` as `Shl`, but nested generics need two `<` tokens.
                depth += 2;
                type_str.push_str("<<");
                self.bump();
            } else if self.is_token(Token::Gt) {
                depth -= 1;
                type_str.push('>');
                self.bump();
                if depth == 0 {
                    break;
                }
            } else if self.is_token(Token::Shr) {
                // The lexer emits `>>` as `Shr`, but nested generics need two `>` tokens.
                depth -= 2;
                type_str.push_str(">>");
                self.bump();
                if depth <= 0 {
                    break;
                }
            } else if self.is_token(Token::Comma) {
                type_str.push_str(", ");
                self.bump();
            } else {
                let span = self.span();
                type_str.push_str(&self.input[span.start - self.offset..span.end - self.offset]);
                self.bump();
            }
        }
        Ok(self.interner.get_or_intern(type_str))
    }
}
