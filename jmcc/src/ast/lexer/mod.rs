//! Lexer implementation supporting Modern (English) and Alternate (Russian) keywords.

pub mod alternate;
pub mod modern;

use logos::{Lexer, Logos, Span};
use memchr::memchr3;

#[derive(Logos, Debug, PartialEq, Eq, Clone, Copy)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip(r"//[^\r\n]*", allow_greedy = true))]
#[logos(skip(r"/\*([^*]|\*+[^*/])*\*+/"))]
#[logos(skip(r"\\[ \t]*\r?\n"))]
pub enum Token<'a> {
    Import,
    Var,
    Function,
    Fun,
    Def,
    Process,
    Event,
    Class,
    Enum,
    Inline,
    Local,
    Game,
    Save,
    Line,
    Jmcc,
    Break,
    Continue,
    Ref,
    If,
    Else,
    Elif,
    Not,
    And,
    Or,
    In,
    Return,
    True,
    False,
    Plain,
    Legacy,
    Minimessage,
    Json,
    Export,
    From,
    As,
    Const,
    TypeAlias,
    Match,
    Case,
    Default,
    Try,
    Catch,
    Throw,
    Interface,
    Implements,
    Extends,
    While,
    For,

    Label(&'a str),
    LabelDecl(&'a str),

    #[token("::")]
    ScopeRes,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token(",")]
    Comma,
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token(".")]
    Dot,
    #[token(":")]
    Colon,
    #[token("=")]
    Assign,
    #[token("?")]
    Question,
    #[token(";")]
    Semicolon,

    #[token("==")]
    Eq,
    #[token("!=")]
    Ne,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("+=")]
    AddAssign,
    #[token("-=")]
    SubAssign,
    #[token("*=")]
    MulAssign,
    #[token("/=")]
    DivAssign,
    #[token("%=")]
    ModAssign,
    #[token("^=")]
    PowAssign,
    #[token("<<")]
    Shl,
    #[token(">>")]
    Shr,
    #[token("++")]
    Inc,
    #[token("--")]
    Dec,
    #[token("**")]
    DoubleStar,
    #[token("+")]
    Add,
    #[token("-")]
    Sub,
    #[token("*")]
    Mul,
    #[token("/")]
    Div,
    #[token("%")]
    Mod,
    #[token("^")]
    Pow,
    #[token("&")]
    BitAnd,
    #[token("|")]
    BitOr,
    #[token("@")]
    At,

    #[regex(r"[а-яА-ЯёЁa-zA-Z_][а-яА-ЯёЁa-zA-Z0-9_]*", |lex| lex.slice())]
    Ident(&'a str),

    #[regex(r"[0-9][0-9_]*(\.[0-9_]+)?([eE][+-]?[0-9]+)?", |lex| lex.slice())]
    Number(&'a str),

    #[token("\"")]
    DQuote,
    #[token("'")]
    SQuote,
    #[token("`")]
    BQuote,
}

#[derive(Debug, Clone)]
pub enum LexedToken<'a> {
    Normal(Token<'a>, Span, bool),
    StrPart(String, Span),
    InterpStart(char, Span),
    InterpEnd(Span),
    StrEnd(Span),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LexerKind {
    #[default]
    Modern,
    Alternate,
}

/// Automatically detects the lexer kind depending on the language of the source file.
///
/// If Cyrillic code (keywords/identifiers) is detected outside comments and strings,
/// Alternate (Russian) is chosen; otherwise, Modern (English) is chosen.
#[must_use]
pub fn detect_lexer_kind(source: &str) -> LexerKind {
    let mut chars = source.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        match c {
            '/' => {
                if let Some(&(_, '/')) = chars.peek() {
                    chars.next();
                    for (_, ch) in chars.by_ref() {
                        if ch == '\n' {
                            break;
                        }
                    }
                } else if let Some(&(_, '*')) = chars.peek() {
                    chars.next();
                    while let Some((_, ch)) = chars.next() {
                        if ch == '*' && chars.peek().is_some_and(|&(_, next_ch)| next_ch == '/') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            '"' | '\'' | '`' => {
                let quote = c;
                let mut escaped = false;
                for (_, ch) in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == quote {
                        break;
                    }
                }
            }
            'а'..='я' | 'А'..='Я' | 'ё' | 'Ё' => {
                return LexerKind::Alternate;
            }
            _ => {}
        }
    }
    LexerKind::Modern
}

pub struct LexerWrapper<'a> {
    lexer: Lexer<'a, Token<'a>>,
    kind: LexerKind,
    input: &'a str,
    string_mode: Option<char>,
    string_buffer: String,
    interp_depth: usize,
    interp_quote: char,
    last_had_newline: bool,
    last_end: usize,
    last_token: Option<Token<'a>>,
}

impl<'a> LexerWrapper<'a> {
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        let kind = detect_lexer_kind(input);
        Self::new_with_kind(input, kind)
    }

    #[must_use]
    pub fn new_with_kind(input: &'a str, kind: LexerKind) -> Self {
        Self {
            lexer: Token::lexer(input),
            kind,
            input,
            string_mode: None,
            string_buffer: String::new(),
            interp_depth: 0,
            interp_quote: '"',
            last_had_newline: false,
            last_end: 0,
            last_token: None,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> LexerKind {
        self.kind
    }

    #[must_use]
    pub fn remainder(&self) -> &'a str {
        self.lexer.remainder()
    }

    pub fn bump(&mut self, n: usize) {
        self.lexer.bump(n);
    }

    fn next_string_token(&mut self, quote: char) -> LexedToken<'a> {
        let remainder = self.lexer.remainder();
        let remainder_start = remainder.as_ptr() as usize - self.input.as_ptr() as usize;
        let quote_byte = quote as u8;
        let bytes = remainder.as_bytes();
        let mut consumed = 0;

        while consumed < bytes.len() {
            let Some(relative_position) = memchr3(quote_byte, b'\\', b'$', &bytes[consumed..])
            else {
                self.string_buffer.push_str(&remainder[consumed..]);
                self.lexer.bump(bytes.len() - consumed);
                consumed = bytes.len();
                break;
            };
            let position = consumed + relative_position;
            if relative_position > 0 {
                self.string_buffer.push_str(&remainder[consumed..position]);
            }

            match bytes[position] {
                byte if byte == quote_byte => {
                    if !self.string_buffer.is_empty() {
                        self.lexer.bump(position);
                        let part = std::mem::take(&mut self.string_buffer);
                        self.last_end = remainder_start + position;
                        return LexedToken::StrPart(
                            part,
                            remainder_start..remainder_start + position,
                        );
                    }
                    self.lexer.bump(position + 1);
                    self.string_mode = None;
                    self.last_end = remainder_start + position + 1;
                    return LexedToken::StrEnd(
                        remainder_start + position..remainder_start + position + 1,
                    );
                }
                b'\\' => {
                    if position + 1 < bytes.len() {
                        let escaped = bytes[position + 1];
                        self.string_buffer.push(match escaped {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'\'' => '\'',
                            b'`' => '`',
                            _ => escaped as char,
                        });
                        consumed = position + 2;
                    } else {
                        self.string_buffer.push('\\');
                        consumed = position + 1;
                    }
                }
                b'$' if position + 1 < bytes.len() && bytes[position + 1] == b'{' => {
                    if !self.string_buffer.is_empty() {
                        self.lexer.bump(position);
                        let part = std::mem::take(&mut self.string_buffer);
                        self.last_end = remainder_start + position;
                        return LexedToken::StrPart(
                            part,
                            remainder_start..remainder_start + position,
                        );
                    }
                    self.lexer.bump(position + 2);
                    self.string_mode = None;
                    self.interp_depth = 1;
                    self.interp_quote = quote;
                    self.last_end = remainder_start + position + 2;
                    return LexedToken::InterpStart(
                        quote,
                        remainder_start + position..remainder_start + position + 2,
                    );
                }
                b'$' => {
                    self.string_buffer.push('$');
                    consumed = position + 1;
                }
                _ => unreachable!(),
            }
        }

        self.lexer.bump(bytes.len() - consumed);
        let part = std::mem::take(&mut self.string_buffer);
        self.string_mode = None;
        self.last_end = remainder_start + bytes.len();
        LexedToken::StrPart(part, remainder_start..remainder_start + bytes.len())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "lexer loop with string and label modes"
    )]
    pub fn next_token(&mut self) -> LexedToken<'a> {
        if let Some(quote) = self.string_mode {
            return self.next_string_token(quote);
        }

        {
            let raw_tok = self.lexer.next();
            let Some(raw_tok) = raw_tok else {
                self.last_end = self.input.len();
                return LexedToken::Eof;
            };
            let span = self.lexer.span();
            let t = match raw_tok {
                Ok(Token::Ident(s)) => match self.kind {
                    LexerKind::Modern => modern::keyword(s).unwrap_or(Token::Ident(s)),
                    LexerKind::Alternate => alternate::keyword(s).unwrap_or(Token::Ident(s)),
                },
                Ok(tok) => tok,
                Err(()) => Token::Ident(self.lexer.slice()),
            };

            if span.start > self.last_end {
                let skipped = &self.input[self.last_end..span.start];
                if skipped.contains('\n') {
                    self.last_had_newline = true;
                }
            }
            self.last_end = span.end;

            if self.interp_depth > 0 {
                match t {
                    Token::LBrace => {
                        self.interp_depth += 1;
                    }
                    Token::RBrace => {
                        self.interp_depth -= 1;
                        if self.interp_depth == 0 {
                            self.string_mode = Some(self.interp_quote);
                            let _nl = self.last_had_newline;
                            self.last_had_newline = false;
                            return LexedToken::InterpEnd(span);
                        }
                    }
                    _ => {}
                }
            }

            if matches!(t, Token::SQuote) {
                let remainder = self.lexer.remainder();
                let mut chars = remainder.char_indices();
                if let Some((_, first_char)) = chars.next()
                    && (first_char.is_alphabetic() || first_char == '_')
                {
                    let mut end_ident = first_char.len_utf8();
                    for (idx, ch) in chars {
                        if ch.is_alphanumeric() || ch == '_' {
                            end_ident = idx + ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    let ident = &remainder[..end_ident];
                    let rest = &remainder[end_ident..];

                    if matches!(self.last_token, Some(Token::Break | Token::Continue)) {
                        self.lexer.bump(end_ident);
                        self.last_end = span.start + 1 + end_ident;
                        let tok = Token::Label(ident);
                        self.last_token = Some(tok);
                        let nl = self.last_had_newline;
                        self.last_had_newline = false;
                        return LexedToken::Normal(tok, span.start..self.last_end, nl);
                    }

                    if rest.starts_with(':') && !rest.starts_with("::") {
                        let mut after_colon = &rest[1..];
                        loop {
                            after_colon = after_colon.trim_start();
                            if after_colon.starts_with("//") {
                                if let Some(pos) = after_colon.find('\n') {
                                    after_colon = &after_colon[pos + 1..];
                                    continue;
                                }
                                break;
                            }
                            if after_colon.starts_with("/*") {
                                if let Some(pos) = after_colon.find("*/") {
                                    after_colon = &after_colon[pos + 2..];
                                    continue;
                                }
                                break;
                            }
                            break;
                        }
                        let is_loop = after_colon.starts_with("while")
                            || after_colon.starts_with("for")
                            || after_colon.starts_with("repeat")
                            || after_colon.starts_with("пока")
                            || after_colon.starts_with("для")
                            || after_colon.starts_with('{');
                        if is_loop {
                            self.lexer.bump(end_ident + 1);
                            self.last_end = span.start + 1 + end_ident + 1;
                            let tok = Token::LabelDecl(ident);
                            self.last_token = Some(tok);
                            let nl = self.last_had_newline;
                            self.last_had_newline = false;
                            return LexedToken::Normal(tok, span.start..self.last_end, nl);
                        }
                    }
                }
            }

            match t {
                Token::DQuote => self.string_mode = Some('"'),
                Token::SQuote => self.string_mode = Some('\''),
                Token::BQuote => self.string_mode = Some('`'),
                _ => {}
            }

            self.last_token = Some(t);
            let nl = self.last_had_newline;
            self.last_had_newline = false;
            LexedToken::Normal(t, span, nl)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_modern_lexer() {
        let code = "function hello() { return 42; }";
        assert_eq!(detect_lexer_kind(code), LexerKind::Modern);
    }

    #[test]
    fn test_detect_modern_with_cyrillic_comments_and_strings() {
        let code = r#"
            // Это комментарий на русском
            /* Блочный комментарий на русском */
            function greet() {
                var s = "Привет, мир!";
                var s2 = 'Тестовая строка';
                return s;
            }
        "#;
        assert_eq!(detect_lexer_kind(code), LexerKind::Modern);
    }

    #[test]
    fn test_detect_alternate_keywords() {
        let code = "функция привет() { вернуть 1; }";
        assert_eq!(detect_lexer_kind(code), LexerKind::Alternate);
    }

    #[test]
    fn test_detect_alternate_ident() {
        let code = "var привет = 10;";
        assert_eq!(detect_lexer_kind(code), LexerKind::Alternate);
    }

    #[test]
    fn test_tokenize_alternate() {
        let code = "функция тест() -> число { вернуть 42; }";
        let mut wrapper = LexerWrapper::new(code);
        let mut tokens = Vec::new();
        loop {
            match wrapper.next_token() {
                LexedToken::Eof => break,
                LexedToken::Normal(tok, _, _) => tokens.push(tok),
                _ => {}
            }
        }
        assert_eq!(
            tokens,
            vec![
                Token::Function,
                Token::Ident("тест"),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("число"),
                Token::LBrace,
                Token::Return,
                Token::Number("42"),
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn test_tokenize_pust_and_perem() {
        let code = "пусть х = 1; перем у = 2; иначе_если х == у { вернуть; }";
        let mut wrapper = LexerWrapper::new(code);
        let mut tokens = Vec::new();
        loop {
            match wrapper.next_token() {
                LexedToken::Eof => break,
                LexedToken::Normal(tok, _, _) => tokens.push(tok),
                _ => {}
            }
        }
        assert_eq!(
            tokens,
            vec![
                Token::Var,
                Token::Ident("х"),
                Token::Assign,
                Token::Number("1"),
                Token::Semicolon,
                Token::Var,
                Token::Ident("у"),
                Token::Assign,
                Token::Number("2"),
                Token::Semicolon,
                Token::Elif,
                Token::Ident("х"),
                Token::Eq,
                Token::Ident("у"),
                Token::LBrace,
                Token::Return,
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }
}
