use std::fmt;
use std::ops::Range;
use std::path::Path;

use crate::diagnostic::{Diagnostic, DiagnosticLabel, DiagnosticRenderer, format_path};
use crate::i18n::{Lang, current_lang};

#[derive(Debug)]
pub enum JmccError {
    InternalCompilerError(String),
    InternalParserError(String),
    Generic(String),
    NumberParse {
        input: String,
        source: std::num::ParseFloatError,
    },
    UnexpectedEof {
        context: String,
    },
    UnexpectedToken {
        expected: String,
        got: String,
    },
    Pretty {
        diagnostic: Box<Diagnostic>,
        rendered: String,
    },
}

impl std::error::Error for JmccError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NumberParse { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl JmccError {
    /// Enhances a parse error with file and source context, rendering it as a pretty diagnostic.
    #[must_use]
    pub fn with_source_context(
        self,
        source: &str,
        file: &Path,
        span: Range<usize>,
        lang: Lang,
    ) -> Self {
        let (code, msg, label) = match &self {
            Self::InternalCompilerError(details) => {
                let m = match lang {
                    Lang::Ru => format!("внутренняя ошибка компилятора (ICE): {details}"),
                    Lang::En => format!("internal compiler error (ICE): {details}"),
                };
                let l = match lang {
                    Lang::Ru => "внутренняя ошибка компилятора".to_owned(),
                    Lang::En => "internal compiler error".to_owned(),
                };
                ("E0001", m, l)
            }
            Self::InternalParserError(details) => {
                let m = match lang {
                    Lang::Ru => format!("внутренняя ошибка парсера (ICE): {details}"),
                    Lang::En => format!("internal parser error (ICE): {details}"),
                };
                let l = match lang {
                    Lang::Ru => "внутренняя ошибка парсера".to_owned(),
                    Lang::En => "internal parser error".to_owned(),
                };
                ("S0001", m, l)
            }
            Self::UnexpectedToken { expected, got } => {
                let m = match lang {
                    Lang::Ru => format!("неожиданный токен: ожидался {expected}, получен {got}"),
                    Lang::En => format!("unexpected token: expected {expected}, got {got}"),
                };
                let l = match lang {
                    Lang::Ru => format!("ожидался {expected}"),
                    Lang::En => format!("expected {expected}"),
                };
                ("S0002", m, l)
            }
            Self::UnexpectedEof { context } => {
                let m = match lang {
                    Lang::Ru => format!("неожиданный конец файла в {context}"),
                    Lang::En => format!("unexpected end of file in {context}"),
                };
                let l = match lang {
                    Lang::Ru => "неожиданный конец файла".to_owned(),
                    Lang::En => "unexpected EOF".to_owned(),
                };
                ("S0003", m, l)
            }
            Self::NumberParse { input, source: e } => {
                let m = match lang {
                    Lang::Ru => format!("не удалось разобрать число '{input}': {e}"),
                    Lang::En => format!("failed to parse number '{input}': {e}"),
                };
                let l = match lang {
                    Lang::Ru => "недопустимое число".to_owned(),
                    Lang::En => "invalid number".to_owned(),
                };
                ("S0004", m, l)
            }
            Self::Generic(msg) => (
                "S0005",
                msg.clone(),
                match lang {
                    Lang::Ru => "ошибка разбора".to_owned(),
                    Lang::En => "parse error".to_owned(),
                },
            ),
            Self::Pretty { .. } => return self,
        };

        let local_start = span.start.min(source.len());
        let mut local_end = span.end.min(source.len());
        if local_end <= local_start && !source.is_empty() {
            local_end = (local_start + 1).min(source.len());
        }

        let file_display = format_path(file);
        let diag = Diagnostic::error(msg)
            .with_code(code)
            .with_file(file.to_path_buf())
            .with_label(DiagnosticLabel::primary(
                local_start..local_end,
                Some(label),
            ));

        let renderer = DiagnosticRenderer::default().with_lang(lang);
        let rendered = renderer.render_diagnostic(&diag, source, &file_display);

        Self::Pretty {
            diagnostic: Box::new(diag),
            rendered,
        }
    }

    #[must_use]
    pub fn format_localized(&self, lang: Lang) -> String {
        match self {
            Self::InternalCompilerError(details) => match lang {
                Lang::Ru => format!("Внутренняя ошибка компилятора (ICE): {details}"),
                Lang::En => format!("Internal compiler error (ICE): {details}"),
            },
            Self::InternalParserError(details) => match lang {
                Lang::Ru => format!("Внутренняя ошибка парсера (ICE): {details}"),
                Lang::En => format!("Internal parser error (ICE): {details}"),
            },
            Self::Pretty { rendered, .. } => rendered.clone(),
            Self::Generic(msg) => match lang {
                Lang::Ru => format!("Ошибка разбора: {msg}"),
                Lang::En => format!("Parse error: {msg}"),
            },
            Self::NumberParse { input, source } => match lang {
                Lang::Ru => format!("Не удалось разобрать число '{input}': {source}"),
                Lang::En => format!("Failed to parse number '{input}': {source}"),
            },
            Self::UnexpectedEof { context } => match lang {
                Lang::Ru => format!("Неожиданный конец файла в {context}"),
                Lang::En => format!("Unexpected end of file in {context}"),
            },
            Self::UnexpectedToken { expected, got } => match lang {
                Lang::Ru => format!("Неожиданный токен: ожидался {expected}, получен {got}"),
                Lang::En => format!("Unexpected token: expected {expected}, got {got}"),
            },
        }
    }
}

impl fmt::Display for JmccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_localized(current_lang()))
    }
}

pub type Result<T> = std::result::Result<T, JmccError>;
