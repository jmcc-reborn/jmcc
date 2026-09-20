//! Pretty diagnostic reporting powered by `annotate-snippets`.
//!
//! Provides structured diagnostic types ([`Diagnostic`], [`DiagnosticLevel`], [`DiagnosticLabel`]),
//! source mapping to AST files, error code categorization, and rendering in `rustc` style.

use std::ops::Range;
use std::path::{Path, PathBuf};

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet, renderer::DecorStyle};

use crate::ast::Ast;
use crate::ast::semantic::SemanticErrorKind;
use crate::i18n::{Lang, current_lang};

pub mod style;

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Note,
    Help,
}

impl DiagnosticLevel {
    #[must_use]
    pub fn to_annotate_level(self, lang: Lang) -> Level<'static> {
        match lang {
            Lang::Ru => match self {
                Self::Error => Level::ERROR.with_name("ошибка"),
                Self::Warning => Level::WARNING.with_name("предупреждение"),
                Self::Note => Level::NOTE.with_name("примечание"),
                Self::Help => Level::HELP.with_name("помощь"),
            },
            Lang::En => match self {
                Self::Error => Level::ERROR,
                Self::Warning => Level::WARNING,
                Self::Note => Level::NOTE,
                Self::Help => Level::HELP,
            },
        }
    }
}

/// A highlighted span in a source file with an optional label.
#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub span: Range<usize>,
    pub label: Option<String>,
    pub is_primary: bool,
}

impl DiagnosticLabel {
    #[must_use]
    pub fn primary(span: Range<usize>, label: impl Into<Option<String>>) -> Self {
        Self {
            span,
            label: label.into(),
            is_primary: true,
        }
    }

    #[must_use]
    pub fn context(span: Range<usize>, label: impl Into<Option<String>>) -> Self {
        Self {
            span,
            label: label.into(),
            is_primary: false,
        }
    }
}

/// A structured diagnostic report ready for rendering.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: Option<&'static str>,
    pub message: String,
    pub file_path: Option<PathBuf>,
    pub labels: Vec<DiagnosticLabel>,
    pub notes: Vec<String>,
    pub helps: Vec<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            code: None,
            message: message.into(),
            file_path: None,
            labels: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }

    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            code: None,
            message: message.into(),
            file_path: None,
            labels: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }

    #[must_use]
    pub const fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    #[must_use]
    pub fn with_file(mut self, file: PathBuf) -> Self {
        self.file_path = Some(file);
        self
    }

    #[must_use]
    pub fn with_label(mut self, label: DiagnosticLabel) -> Self {
        self.labels.push(label);
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.helps.push(help.into());
        self
    }
}

/// Renderer for diagnostics into human-readable terminal output.
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticRenderer {
    pub decor_style: DecorStyle,
    pub use_color: bool,
    pub lang: Lang,
}

impl Default for DiagnosticRenderer {
    fn default() -> Self {
        Self {
            decor_style: DecorStyle::Ascii,
            use_color: style::ColorConfig::detect_color_support(),
            lang: current_lang(),
        }
    }
}

impl DiagnosticRenderer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            decor_style: DecorStyle::Ascii,
            use_color: true,
            lang: Lang::En,
        }
    }

    #[must_use]
    pub const fn with_color(mut self, use_color: bool) -> Self {
        self.use_color = use_color;
        self
    }

    #[must_use]
    pub const fn with_decor_style(mut self, decor_style: DecorStyle) -> Self {
        self.decor_style = decor_style;
        self
    }

    #[must_use]
    pub const fn with_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }

    /// Renders a single diagnostic against given source text and display path.
    #[must_use]
    pub fn render_diagnostic(&self, diag: &Diagnostic, source: &str, file_display: &str) -> String {
        let level = diag.level.to_annotate_level(self.lang);
        let mut title = level.primary_title(&diag.message);
        if let Some(code) = diag.code {
            title = title.id(code);
        }

        let mut groups = Vec::new();

        if diag.labels.is_empty() {
            groups.push(Group::with_title(title));
        } else {
            let mut snippet = Snippet::source(source).line_start(1).path(file_display);
            for label in &diag.labels {
                let kind = if label.is_primary {
                    AnnotationKind::Primary
                } else {
                    AnnotationKind::Context
                };
                let mut annotation = kind.span(label.span.clone());
                if let Some(lbl) = &label.label {
                    annotation = annotation.label(lbl.as_str());
                }
                snippet = snippet.annotation(annotation);
            }
            groups.push(title.element(snippet));
        }

        let note_level = DiagnosticLevel::Note.to_annotate_level(self.lang);
        for note in &diag.notes {
            groups.push(Group::with_title(
                note_level.clone().secondary_title(note.as_str()),
            ));
        }
        let help_level = DiagnosticLevel::Help.to_annotate_level(self.lang);
        for help in &diag.helps {
            groups.push(Group::with_title(
                help_level.clone().secondary_title(help.as_str()),
            ));
        }

        let renderer = if self.use_color {
            Renderer::styled()
        } else {
            Renderer::plain()
        }
        .decor_style(self.decor_style);

        renderer.render(&groups)
    }

    /// Renders a diagnostic that references an AST containing mapped file offsets.
    #[must_use]
    pub fn render_ast_diagnostic(&self, diag: &Diagnostic, ast: &Ast) -> String {
        if let Some(path) = &diag.file_path
            && let Some(source) = ast.sources.get(path)
        {
            let display_path = format_path(path);
            return self.render_diagnostic(diag, source, &display_path);
        }

        // Try to resolve from the first label if file_path is unset
        if let Some(first_label) = diag.labels.first()
            && let Some((path, source, local_span)) = resolve_ast_span(ast, &first_label.span)
        {
            let mut resolved_diag = diag.clone();
            resolved_diag.file_path = Some(path.to_path_buf());
            if let Some(l) = resolved_diag.labels.first_mut() {
                l.span = local_span;
            }
            let display_path = format_path(path);
            return self.render_diagnostic(&resolved_diag, source, &display_path);
        }

        // Fallback: render title and notes without snippet
        let level = diag.level.to_annotate_level(self.lang);
        let mut title = level.primary_title(&diag.message);
        if let Some(code) = diag.code {
            title = title.id(code);
        }
        let mut groups = vec![Group::with_title(title)];
        let note_level = DiagnosticLevel::Note.to_annotate_level(self.lang);
        for note in &diag.notes {
            groups.push(Group::with_title(
                note_level.clone().secondary_title(note.as_str()),
            ));
        }
        let help_level = DiagnosticLevel::Help.to_annotate_level(self.lang);
        for help in &diag.helps {
            groups.push(Group::with_title(
                help_level.clone().secondary_title(help.as_str()),
            ));
        }

        let renderer = if self.use_color {
            Renderer::styled()
        } else {
            Renderer::plain()
        }
        .decor_style(self.decor_style);

        renderer.render(&groups)
    }

    /// Renders multiple diagnostics for an AST, followed by an abort summary footer.
    #[must_use]
    pub fn render_ast_diagnostics(&self, diags: &[Diagnostic], ast: &Ast, lang: Lang) -> String {
        if diags.is_empty() {
            return String::new();
        }

        let renderer = self.with_lang(lang);
        let mut rendered_blocks = Vec::with_capacity(diags.len());
        let mut error_count = 0;
        let mut warning_count = 0;

        for diag in diags {
            match diag.level {
                DiagnosticLevel::Error => error_count += 1,
                DiagnosticLevel::Warning => warning_count += 1,
                _ => {}
            }
            rendered_blocks.push(renderer.render_ast_diagnostic(diag, ast));
        }

        let summary = format_abort_summary(error_count, warning_count, lang);
        if summary.is_empty() {
            rendered_blocks.join("\n\n")
        } else {
            format!("{}\n\n{summary}", rendered_blocks.join("\n\n"))
        }
    }
}

/// Resolves a global AST span to a file path, its source code, and local span range.
#[must_use]
pub fn resolve_ast_span<'a>(
    ast: &'a Ast,
    span: &Range<usize>,
) -> Option<(&'a Path, &'a str, Range<usize>)> {
    if ast.file_offsets.is_empty() {
        return None;
    }

    let (path, start_offset, _) = ast.file_offsets.iter().min_by_key(|(_, start, end)| {
        if span.start >= *start && span.start < *end {
            0
        } else if span.start < *start {
            *start - span.start
        } else {
            span.start - *end
        }
    })?;

    let source = ast.sources.get(path)?.as_str();
    let mut local_start = span.start.saturating_sub(*start_offset).min(source.len());
    while local_start > 0 && !source.is_char_boundary(local_start) {
        local_start -= 1;
    }
    let mut local_end = span.end.saturating_sub(*start_offset).min(source.len());
    while local_end < source.len() && !source.is_char_boundary(local_end) {
        local_end += 1;
    }
    if local_end <= local_start && !source.is_empty() {
        let char_len = source[local_start..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        local_end = (local_start + char_len).min(source.len());
    }

    Some((path.as_path(), source, local_start..local_end))
}

/// Formats a file path relative to current working directory if possible.
#[must_use]
pub fn format_path(path: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(rel) = path.strip_prefix(&cwd)
    {
        return rel.display().to_string();
    }
    path.display().to_string()
}

/// Helper to extract suggestion text from a formatted suggestion string like `" (did you mean '...')"`
#[must_use]
pub fn extract_suggestion(s: &str) -> Option<&str> {
    let start = s.find('\'')? + 1;
    let end = s[start..].find('\'')? + start;
    Some(&s[start..end])
}

/// Strips trailing suggestion clause like `" (did you mean '...')"` from error message.
#[must_use]
pub fn strip_suggestion(s: &str) -> String {
    let pos = s
        .rfind(" (did you mean '")
        .or_else(|| s.rfind(" (возможно, вы имели в виду '"));
    pos.map_or_else(|| s.to_owned(), |p| s[..p].to_owned())
}

/// Converts a [`SemanticErrorKind`] and its AST span into a [`Diagnostic`].
#[must_use]
pub fn semantic_to_diagnostic(
    kind: &SemanticErrorKind,
    global_span: &Range<usize>,
    ast: &Ast,
    lang: Lang,
) -> Diagnostic {
    let (code, primary_label_text) = error_code_and_label(kind, lang);

    // Resolve file and local span if possible
    let (file_path, local_span) = if let Some((path, _, local)) = resolve_ast_span(ast, global_span)
    {
        (Some(path.to_path_buf()), local)
    } else {
        (None, global_span.clone())
    };

    let raw_msg = kind.format_localized(lang);
    let clean_msg = strip_suggestion(&raw_msg);

    let mut diag = (if kind.is_warning() {
        Diagnostic::warning(clean_msg)
    } else {
        Diagnostic::error(clean_msg)
    })
    .with_code(code)
    .with_label(DiagnosticLabel::primary(local_span, primary_label_text));

    if let Some(path) = file_path {
        diag = diag.with_file(path);
    }

    // Extract suggestion for clean help section
    match kind {
        SemanticErrorKind::UnknownAction { suggestion, .. }
        | SemanticErrorKind::UnknownMethod { suggestion, .. }
        | SemanticErrorKind::UnknownProperty { suggestion, .. }
        | SemanticErrorKind::UndeclaredVariable { suggestion, .. }
        | SemanticErrorKind::UnknownParam { suggestion, .. }
        | SemanticErrorKind::MissingArgument { suggestion, .. } => {
            if let Some(suggested) = extract_suggestion(suggestion) {
                let help_text = match lang {
                    Lang::Ru => format!("возможно, вы имели в виду '{suggested}'?"),
                    Lang::En => format!("did you mean '{suggested}'?"),
                };
                diag = diag.with_help(help_text);
            }
        }
        SemanticErrorKind::DuplicateFunction { prev_span, .. } => {
            let note_text = match lang {
                Lang::Ru => format!("ранее функция была объявлена в {prev_span}"),
                Lang::En => format!("previously declared at {prev_span}"),
            };
            diag = diag.with_note(note_text);
        }
        SemanticErrorKind::DuplicateProcess { prev_span, .. } => {
            let note_text = match lang {
                Lang::Ru => format!("ранее процесс был объявлен в {prev_span}"),
                Lang::En => format!("previously declared at {prev_span}"),
            };
            diag = diag.with_note(note_text);
        }
        SemanticErrorKind::UnresolvedTypeInference => {
            let help_text = match lang {
                Lang::Ru => "добавьте явную аннотацию типа (например: 'var x: number = ...')",
                Lang::En => "add explicit type annotation (e.g., 'var x: number = ...')",
            };
            diag = diag.with_help(help_text);
        }
        SemanticErrorKind::DeprecatedElif => {
            let help_text = match lang {
                Lang::Ru => "замените цепочку 'elif' на конструкцию 'match'",
                Lang::En => "replace 'elif' chain with a 'match' statement",
            };
            diag = diag.with_help(help_text);
        }
        SemanticErrorKind::EventNotCancellable { event } => {
            let help_text = match lang {
                Lang::Ru => format!(
                    "удалите вызов cancel_event(), так как событие '{event}' не поддерживает отмену"
                ),
                Lang::En => {
                    format!("remove cancel_event() call as event '{event}' is not cancellable")
                }
            };
            diag = diag.with_help(help_text);
        }
        _ => {}
    }

    diag
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "Mapping of all semantic error variants"
)]
fn error_code_and_label(kind: &SemanticErrorKind, lang: Lang) -> (&'static str, Option<String>) {
    match kind {
        SemanticErrorKind::InternalCompilerError { .. } => (
            "E0001",
            Some(match lang {
                Lang::Ru => "внутренняя ошибка компилятора".to_owned(),
                Lang::En => "internal compiler error".to_owned(),
            }),
        ),
        SemanticErrorKind::UnknownAction { .. } => (
            "E0002",
            Some(match lang {
                Lang::Ru => "неизвестное действие".to_owned(),
                Lang::En => "unknown action".to_owned(),
            }),
        ),
        SemanticErrorKind::UnknownMethod { .. } => (
            "E0003",
            Some(match lang {
                Lang::Ru => "неизвестный метод".to_owned(),
                Lang::En => "unknown method".to_owned(),
            }),
        ),
        SemanticErrorKind::UnknownProperty { .. } => (
            "E0004",
            Some(match lang {
                Lang::Ru => "неизвестное свойство".to_owned(),
                Lang::En => "unknown property".to_owned(),
            }),
        ),
        SemanticErrorKind::UndeclaredVariable { .. } => (
            "E0005",
            Some(match lang {
                Lang::Ru => "не объявлена в этой области видимости".to_owned(),
                Lang::En => "not declared in this scope".to_owned(),
            }),
        ),
        SemanticErrorKind::TypeMismatchVarDecl { expected, actual } => (
            "E0006",
            Some(match lang {
                Lang::Ru => format!("ожидался '{expected}', получен '{actual}'"),
                Lang::En => format!("expected '{expected}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::TypeMismatchAssign { target, actual } => (
            "E0007",
            Some(match lang {
                Lang::Ru => format!("цель имеет тип '{target}', получено '{actual}'"),
                Lang::En => format!("target is '{target}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::TypeMismatchReturn { expected, actual } => (
            "E0008",
            Some(match lang {
                Lang::Ru => format!("возвращаемый тип '{expected}', получено '{actual}'"),
                Lang::En => format!("return type is '{expected}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::DuplicateFunction { .. } => (
            "E0009",
            Some(match lang {
                Lang::Ru => "повторное объявление функции".to_owned(),
                Lang::En => "duplicate function declaration".to_owned(),
            }),
        ),
        SemanticErrorKind::DuplicateProcess { .. } => (
            "E0010",
            Some(match lang {
                Lang::Ru => "повторное объявление процесса".to_owned(),
                Lang::En => "duplicate process declaration".to_owned(),
            }),
        ),
        SemanticErrorKind::BreakOutsideLoop => (
            "E0011",
            Some(match lang {
                Lang::Ru => "'break' вне цикла".to_owned(),
                Lang::En => "'break' outside loop".to_owned(),
            }),
        ),
        SemanticErrorKind::ReturnOutsideCallable => (
            "E0012",
            Some(match lang {
                Lang::Ru => "'return' вне функции или процесса".to_owned(),
                Lang::En => "'return' outside function or process".to_owned(),
            }),
        ),
        SemanticErrorKind::MissingReturnValue => (
            "E0013",
            Some(match lang {
                Lang::Ru => "требуется возвращаемое значение".to_owned(),
                Lang::En => "missing return value".to_owned(),
            }),
        ),
        SemanticErrorKind::InvalidCondition { actual } => (
            "E0014",
            Some(match lang {
                Lang::Ru => format!("ожидалось истинное условие, получено '{actual}'"),
                Lang::En => format!("expected truthy condition, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidNumericOperand { op, actual } => (
            "E0015",
            Some(match lang {
                Lang::Ru => format!("оператор '{op}' требует число, получено '{actual}'"),
                Lang::En => format!("operator '{op}' requires numeric operand, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidArithmetic { op, lty, rty } => (
            "E0016",
            Some(match lang {
                Lang::Ru => format!("операция '{op}' над нечисловыми типами '{lty}' и '{rty}'"),
                Lang::En => format!("operation '{op}' on non-numeric types '{lty}' and '{rty}'"),
            }),
        ),
        SemanticErrorKind::UnknownParam { .. } => (
            "E0017",
            Some(match lang {
                Lang::Ru => "неизвестный именованный параметр".to_owned(),
                Lang::En => "unknown named parameter".to_owned(),
            }),
        ),
        SemanticErrorKind::TooManyArgs {
            expected, actual, ..
        } => (
            "E0018",
            Some(match lang {
                Lang::Ru => format!("ожидалось не более {expected}, получено {actual}"),
                Lang::En => format!("expected at most {expected}, got {actual}"),
            }),
        ),
        SemanticErrorKind::MissingArgument { arg, .. } => (
            "E0019",
            Some(match lang {
                Lang::Ru => format!("пропущен обязательный аргумент '{arg}'"),
                Lang::En => format!("missing required argument '{arg}'"),
            }),
        ),
        SemanticErrorKind::FuncArgTypeMismatch {
            arg,
            expected,
            actual,
            ..
        } => (
            "E0020",
            Some(match lang {
                Lang::Ru => format!("аргумент '{arg}' ожидает '{expected}', получено '{actual}'"),
                Lang::En => format!("argument '{arg}' expects '{expected}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::ActionArgTypeMismatch {
            arg,
            expected,
            actual,
            ..
        } => (
            "E0021",
            Some(match lang {
                Lang::Ru => {
                    format!("аргумент действия '{arg}' ожидает '{expected}', получено '{actual}'")
                }
                Lang::En => format!("action argument '{arg}' expects '{expected}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidSelector { .. } => (
            "E0022",
            Some(match lang {
                Lang::Ru => "недопустимый селектор для этого объекта".to_owned(),
                Lang::En => "invalid selector for this object".to_owned(),
            }),
        ),
        SemanticErrorKind::CyclicInheritance { .. } => (
            "E0023",
            Some(match lang {
                Lang::Ru => "циклическое наследование".to_owned(),
                Lang::En => "cyclic inheritance".to_owned(),
            }),
        ),
        SemanticErrorKind::CyclicTypeInference => (
            "E0024",
            Some(match lang {
                Lang::Ru => "цикл при выводе типов".to_owned(),
                Lang::En => "cyclic type inference".to_owned(),
            }),
        ),
        SemanticErrorKind::NotIterable { ty } => (
            "E0025",
            Some(match lang {
                Lang::Ru => format!("тип '{ty}' не итерируем"),
                Lang::En => format!("type '{ty}' is not iterable"),
            }),
        ),
        SemanticErrorKind::MatchArmTypeMismatch { expected, actual } => (
            "E0026",
            Some(match lang {
                Lang::Ru => format!("тип ветви '{actual}' не совпадает с предыдущей '{expected}'"),
                Lang::En => {
                    format!("match arm type '{actual}' does not match previous '{expected}'")
                }
            }),
        ),
        SemanticErrorKind::MissingInterfaceMethod {
            method, interface, ..
        } => (
            "E0027",
            Some(match lang {
                Lang::Ru => format!("метод '{method}' из интерфейса '{interface}' не реализован"),
                Lang::En => format!(
                    "missing implementation of method '{method}' from interface '{interface}'"
                ),
            }),
        ),
        SemanticErrorKind::InterfaceMethodSignatureMismatch { method, .. } => (
            "E0028",
            Some(match lang {
                Lang::Ru => format!("несовпадение сигнатуры метода '{method}'"),
                Lang::En => format!("mismatched signature for method '{method}'"),
            }),
        ),
        SemanticErrorKind::AlreadyDeclared { .. } => (
            "E0029",
            Some(match lang {
                Lang::Ru => "уже объявлен в этой области видимости".to_owned(),
                Lang::En => "already declared in this scope".to_owned(),
            }),
        ),
        SemanticErrorKind::UnknownLoopLabel { .. } => (
            "E0030",
            Some(match lang {
                Lang::Ru => "неизвестная метка цикла".to_owned(),
                Lang::En => "unknown loop label".to_owned(),
            }),
        ),
        SemanticErrorKind::CompoundAssignRhs { op, actual } => (
            "E0031",
            Some(match lang {
                Lang::Ru => format!("'{op}' требует число, получено '{actual}'"),
                Lang::En => format!("'{op}' requires number, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::CompoundAssignTarget { op, target } => (
            "E0032",
            Some(match lang {
                Lang::Ru => format!("цель '{op}' должна быть числовой, получено '{target}'"),
                Lang::En => format!("target for '{op}' must be numeric, got '{target}'"),
            }),
        ),
        SemanticErrorKind::InvalidElifCondition { actual } => (
            "E0033",
            Some(match lang {
                Lang::Ru => format!("ожидалось истинное условие в elif, получено '{actual}'"),
                Lang::En => format!("expected truthy elif condition, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidTernaryCondition { actual } => (
            "E0034",
            Some(match lang {
                Lang::Ru => format!("условие тернарного оператора не истинно, получено '{actual}'"),
                Lang::En => format!("ternary condition not truthy, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidNotOperand { actual } => (
            "E0035",
            Some(match lang {
                Lang::Ru => format!("'!' требует истинное выражение, получено '{actual}'"),
                Lang::En => format!("'!' requires truthy operand, got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidBitwise { op, lty, rty } => (
            "E0036",
            Some(match lang {
                Lang::Ru => format!("побитовая операция '{op}' над '{lty}' и '{rty}'"),
                Lang::En => format!("bitwise operation '{op}' on '{lty}' and '{rty}'"),
            }),
        ),
        SemanticErrorKind::InvalidLogical { op, lty, rty } => (
            "E0037",
            Some(match lang {
                Lang::Ru => format!("логическая операция '{op}' над '{lty}' и '{rty}'"),
                Lang::En => format!("logical operation '{op}' on '{lty}' and '{rty}'"),
            }),
        ),
        SemanticErrorKind::InvalidSlice { ty } => (
            "E0038",
            Some(match lang {
                Lang::Ru => format!("срез '[:]' не поддерживается для '{ty}'"),
                Lang::En => format!("slice '[:]' not supported for '{ty}'"),
            }),
        ),
        SemanticErrorKind::InvalidSubscript { ty } => (
            "E0039",
            Some(match lang {
                Lang::Ru => format!("индексация '[]' не поддерживается для '{ty}'"),
                Lang::En => format!("subscript '[]' not supported for '{ty}'"),
            }),
        ),
        SemanticErrorKind::UnknownParent { parent } => (
            "E0040",
            Some(match lang {
                Lang::Ru => format!("родительский класс '{parent}' не найден"),
                Lang::En => format!("parent class '{parent}' not found"),
            }),
        ),
        SemanticErrorKind::UnknownConstructor { .. } => (
            "E0041",
            Some(match lang {
                Lang::Ru => "неизвестный конструктор".to_owned(),
                Lang::En => "unknown constructor".to_owned(),
            }),
        ),
        SemanticErrorKind::CtorArgTypeMismatch {
            arg,
            expected,
            actual,
            ..
        } => (
            "E0042",
            Some(match lang {
                Lang::Ru => format!(
                    "аргумент '{arg}' конструктора ожидает '{expected}', получено '{actual}'"
                ),
                Lang::En => {
                    format!("constructor argument '{arg}' expects '{expected}', got '{actual}'")
                }
            }),
        ),
        SemanticErrorKind::CtorPositionalArgTypeMismatch {
            idx,
            expected,
            actual,
            ..
        } => (
            "E0043",
            Some(match lang {
                Lang::Ru => {
                    format!("аргумент {idx} конструктора ожидает '{expected}', получено '{actual}'")
                }
                Lang::En => {
                    format!("constructor argument {idx} expects '{expected}', got '{actual}'")
                }
            }),
        ),
        SemanticErrorKind::FuncPositionalArgTypeMismatch {
            idx,
            expected,
            actual,
            ..
        } => (
            "E0044",
            Some(match lang {
                Lang::Ru => {
                    format!("аргумент {idx} функции ожидает '{expected}', получено '{actual}'")
                }
                Lang::En => format!("function argument {idx} expects '{expected}', got '{actual}'"),
            }),
        ),
        SemanticErrorKind::InvalidEnumValue { expected, .. } => (
            "E0045",
            Some(match lang {
                Lang::Ru => format!("ожидалось одно из: {expected}"),
                Lang::En => format!("expected one of: {expected}"),
            }),
        ),
        SemanticErrorKind::EnumIndexOutOfBounds { idx, max } => (
            "E0046",
            Some(match lang {
                Lang::Ru => format!("индекс {idx} выходит за пределы enum (макс. {max})"),
                Lang::En => format!("enum index {idx} out of bounds (max {max})"),
            }),
        ),
        SemanticErrorKind::InvalidBoolEnum { expected } => (
            "E0047",
            Some(match lang {
                Lang::Ru => format!("ожидалось булево значение enum: {expected}"),
                Lang::En => format!("expected bool enum value: {expected}"),
            }),
        ),
        SemanticErrorKind::UnknownGameValue { .. } => (
            "E0048",
            Some(match lang {
                Lang::Ru => "неизвестное игровое значение JustMC".to_owned(),
                Lang::En => "unknown JustMC game value".to_owned(),
            }),
        ),
        SemanticErrorKind::InfinitePropertyRecursion { .. } => (
            "E0049",
            Some(match lang {
                Lang::Ru => "бесконечная рекурсия при обращении к свойству".to_owned(),
                Lang::En => "infinite recursion in property access".to_owned(),
            }),
        ),
        SemanticErrorKind::UnknownType(..) => (
            "E0050",
            Some(match lang {
                Lang::Ru => "неизвестный тип".to_owned(),
                Lang::En => "unknown type".to_owned(),
            }),
        ),
        SemanticErrorKind::TernaryBranchTypeMismatch { then_ty, else_ty } => (
            "E0051",
            Some(match lang {
                Lang::Ru => format!("ветвь then имеет тип '{then_ty}', а else — '{else_ty}'"),
                Lang::En => format!("then branch is '{then_ty}', else branch is '{else_ty}'"),
            }),
        ),
        SemanticErrorKind::OperationOnAny { .. } => (
            "E0052",
            Some(match lang {
                Lang::Ru => "операция над типом 'any'".to_owned(),
                Lang::En => "operation on 'any' type".to_owned(),
            }),
        ),
        SemanticErrorKind::PropertyAccessOnAny { .. } => (
            "E0053",
            Some(match lang {
                Lang::Ru => "доступ к свойству у типа 'any'".to_owned(),
                Lang::En => "property access on 'any' type".to_owned(),
            }),
        ),
        SemanticErrorKind::MethodCallOnAny { .. } => (
            "E0054",
            Some(match lang {
                Lang::Ru => "вызов метода у типа 'any'".to_owned(),
                Lang::En => "method call on 'any' type".to_owned(),
            }),
        ),
        SemanticErrorKind::VoidReturnValueUsed { .. } => (
            "E0055",
            Some(match lang {
                Lang::Ru => "функция не возвращает значение".to_owned(),
                Lang::En => "function does not return a value".to_owned(),
            }),
        ),
        SemanticErrorKind::ProcessReturnValueUsed { .. } => (
            "E0056",
            Some(match lang {
                Lang::Ru => "процесс не возвращает значение".to_owned(),
                Lang::En => "process does not return a value".to_owned(),
            }),
        ),
        SemanticErrorKind::UnresolvedTypeInference => (
            "E0057",
            Some(match lang {
                Lang::Ru => "не удалось вывести тип выражения".to_owned(),
                Lang::En => "could not infer type for this expression".to_owned(),
            }),
        ),
        SemanticErrorKind::RedundantCast { ty } => (
            "E0058",
            Some(match lang {
                Lang::Ru => format!("избыточное приведение: значение уже типа '{ty}'"),
                Lang::En => format!("redundant cast: already of type '{ty}'"),
            }),
        ),
        SemanticErrorKind::CyclicInterfaceInheritance { .. } => (
            "E0059",
            Some(match lang {
                Lang::Ru => "циклическое наследование интерфейса".to_owned(),
                Lang::En => "cyclic interface inheritance".to_owned(),
            }),
        ),
        SemanticErrorKind::ReturnFromProcess => (
            "E0060",
            Some(match lang {
                Lang::Ru => "процесс не может возвращать значение".to_owned(),
                Lang::En => "process cannot return a value".to_owned(),
            }),
        ),
        SemanticErrorKind::DeprecatedElif => (
            "W0001",
            Some(match lang {
                Lang::Ru => "использование 'elif' не рекомендуется".to_owned(),
                Lang::En => "use of 'elif' is deprecated".to_owned(),
            }),
        ),
        SemanticErrorKind::EventNotCancellable { event } => (
            "W0002",
            Some(match lang {
                Lang::Ru => format!("событие '{event}' не поддерживает отмену"),
                Lang::En => format!("event '{event}' is not cancellable"),
            }),
        ),
    }
}

fn format_abort_summary(errors: usize, warnings: usize, lang: Lang) -> String {
    if errors == 0 && warnings == 0 {
        return String::new();
    }

    match lang {
        Lang::Ru => {
            if errors > 0 && warnings > 0 {
                format!(
                    "семантическая ошибка: прервано из-за {errors} предыдущих ошибок ({warnings} предупреждений)"
                )
            } else if errors == 1 {
                "семантическая ошибка: прервано из-за предыдущей ошибки".to_owned()
            } else if errors > 1 {
                format!("семантическая ошибка: прервано из-за {errors} предыдущих ошибок")
            } else {
                format!("предупреждений: {warnings}")
            }
        }
        Lang::En => {
            if errors > 0 && warnings > 0 {
                format!(
                    "semantic error: aborting due to {errors} previous errors ({warnings} warnings)"
                )
            } else if errors == 1 {
                "semantic error: aborting due to previous error".to_owned()
            } else if errors > 1 {
                format!("semantic error: aborting due to {errors} previous errors")
            } else {
                format!("warnings: {warnings}")
            }
        }
    }
}
