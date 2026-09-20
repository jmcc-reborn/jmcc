//! LSP server document state management, compiler cache, and overlay synchronization.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jmcc::ast::semantic::{SemanticErrorKind, Type, analyze_for_diagnostics};
use jmcc::ast::{Ast, ExprId, parse_file_with_full_options};
use jmcc::error::JmccError;
use jmcc::i18n::{Lang, current_lang};
use jmcc::ir::ctx::IrCtx;
use jmcc::project::{Manifest, Project};
use lsp_types::{Diagnostic, Range, Url};

use super::diagnostics::{compiler_diag_to_lsp, semantic_errors_to_lsp};

/// Cached state of an open `.jc` document.
pub struct DocumentData {
    pub uri: Url,
    pub path: PathBuf,
    pub version: i32,
    pub text: String,
    pub ast: Option<Ast>,
    pub ir_ctx: Option<IrCtx>,
    pub expr_types: HashMap<ExprId, Type>,
    pub diagnostics: Vec<Diagnostic>,
    pub semantic_errors: Vec<(SemanticErrorKind, std::ops::Range<usize>)>,
    pub lang: Lang,
    pub edition: u16,
}

/// Global language server workspace and document state.
#[derive(Default)]
pub struct ServerState {
    pub documents: HashMap<Url, DocumentData>,
    pub workspace_root: Option<PathBuf>,
    pub std_path: Option<PathBuf>,
    pub lang: Lang,
}

impl ServerState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            workspace_root: None,
            std_path: None,
            lang: current_lang(),
        }
    }

    /// Converts a file `Url` into a local filesystem path.
    #[must_use]
    pub fn url_to_path(url: &Url) -> Option<PathBuf> {
        url.to_file_path().ok()
    }

    /// Converts a filesystem `Path` into a file `Url`.
    #[must_use]
    pub fn path_to_url(path: &Path) -> Option<Url> {
        Url::from_file_path(path).ok()
    }

    /// Builds a map of canonical paths to text content for all open documents.
    #[must_use]
    pub fn overlays(&self) -> HashMap<PathBuf, String> {
        let mut map = HashMap::new();
        for doc in self.documents.values() {
            let canon = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
            map.insert(canon, doc.text.clone());
            map.insert(doc.path.clone(), doc.text.clone());
        }
        map
    }

    /// Searches for a `jmcc.toml` manifest for the given path or workspace.
    #[must_use]
    pub fn find_manifest_for_path(&self, path: &Path) -> Option<Manifest> {
        let search_dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        if let Ok(Some(project)) = Project::find(search_dir) {
            return Some(project.manifest);
        }
        if let Some(manifest_path) = Manifest::find_manifest(search_dir)
            && let Ok(manifest) = Manifest::from_file(&manifest_path)
        {
            return Some(manifest);
        }
        if let Some(ws_root) = &self.workspace_root {
            let candidate = ws_root.join("jmcc.toml");
            if candidate.is_file()
                && let Ok(manifest) = Manifest::from_file(&candidate)
            {
                return Some(manifest);
            }
        }
        None
    }

    /// Determines the effective language for a document:
    /// 1. If an enclosing `jmcc.toml` specifies `locale` in `[package]`, `[project]`, or `[profile.dev]`, that locale is authoritative.
    /// 2. Otherwise, detects the language from code tokens (`detect_code_language`).
    /// 3. If the code has no language-identifying tokens, falls back to the server/client locale `self.lang`.
    #[must_use]
    pub fn determine_lang(&self, path: &Path, text: &str) -> Lang {
        if let Some(manifest) = self.find_manifest_for_path(path)
            && let Some(loc) = manifest
                .locale()
                .or_else(|| manifest.profile.get("dev").and_then(|p| p.locale.clone()))
        {
            let lower = loc.trim().to_lowercase();
            if lower.starts_with("ru") {
                return Lang::Ru;
            } else if lower.starts_with("en") {
                return Lang::En;
            }
        }

        if let Some(code_lang) = detect_code_language(text) {
            return code_lang;
        }

        self.lang
    }

    /// Determines the effective language edition for a document from manifest or default (2026).
    #[must_use]
    pub fn determine_edition(&self, path: &Path) -> u16 {
        self.find_manifest_for_path(path)
            .map_or(2026, |manifest| manifest.edition())
    }

    /// Registers a newly opened document and triggers compilation.
    pub fn open_document(&mut self, uri: Url, version: i32, text: String) {
        let Some(path) = Self::url_to_path(&uri) else {
            return;
        };

        let lang = self.determine_lang(&path, &text);
        let edition = self.determine_edition(&path);

        let mut doc = DocumentData {
            uri: uri.clone(),
            path,
            version,
            text,
            ast: None,
            ir_ctx: None,
            expr_types: HashMap::new(),
            diagnostics: Vec::new(),
            semantic_errors: Vec::new(),
            lang,
            edition,
        };

        self.compile_document(&mut doc);
        self.documents.insert(uri, doc);
    }

    /// Updates the text of an open document and triggers recompilation.
    pub fn update_document(&mut self, uri: &Url, version: i32, text: String) {
        let Some(path) = self.documents.get(uri).map(|d| d.path.clone()) else {
            return;
        };
        let lang = self.determine_lang(&path, &text);
        let edition = self.determine_edition(&path);
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.version = version;
            doc.text = text;
            doc.lang = lang;
            doc.edition = edition;
        }
        self.compile_document_by_uri(uri);
    }

    /// Closes a document, removing it from active memory.
    pub fn close_document(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    /// Recompiles an open document by URI.
    pub fn compile_document_by_uri(&mut self, uri: &Url) {
        let overlays = self.overlays();
        let std_path = self.std_path.clone();
        let Some((path, text)) = self
            .documents
            .get(uri)
            .map(|d| (d.path.clone(), d.text.clone()))
        else {
            return;
        };
        let lang = self.determine_lang(&path, &text);
        let edition = self.determine_edition(&path);
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.lang = lang;
            doc.edition = edition;
            Self::recompile_doc(doc, overlays, doc.lang, std_path.as_deref());
        }
    }

    /// Recompiles the given document using current server overlays.
    fn compile_document(&self, doc: &mut DocumentData) {
        let mut overlays = self.overlays();
        let canon = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
        overlays.insert(canon, doc.text.clone());
        overlays.insert(doc.path.clone(), doc.text.clone());

        doc.lang = self.determine_lang(&doc.path, &doc.text);
        doc.edition = self.determine_edition(&doc.path);

        Self::recompile_doc(doc, overlays, doc.lang, self.std_path.as_deref());
    }

    fn recompile_doc(
        doc: &mut DocumentData,
        overlays: HashMap<PathBuf, String>,
        lang: Lang,
        custom_std: Option<&Path>,
    ) {
        doc.diagnostics.clear();
        doc.semantic_errors.clear();

        let edition = doc.edition;
        let package_roots = Project::find(&doc.path)
            .ok()
            .flatten()
            .and_then(|p| p.resolve_dependencies_ext(false, true, true).ok())
            .map(|g| g.to_package_roots())
            .unwrap_or_default();

        // 1. Parse AST with overlays and optional custom std path
        let custom_std_buf = custom_std.map(Path::to_path_buf);
        match parse_file_with_full_options(
            &doc.path,
            edition,
            package_roots,
            overlays,
            custom_std_buf,
        ) {
            Ok(ast) => {
                // 2. Build IR context
                let ir_ctx = IrCtx::new(&ast);

                // 3. Analyze semantics without early bail
                let (expr_types, semantic_errors) =
                    analyze_for_diagnostics(&ast, &doc.text, &ir_ctx, edition);

                let diags = semantic_errors_to_lsp(&semantic_errors, &ast, &doc.path, lang);
                doc.diagnostics = diags;
                doc.semantic_errors = semantic_errors;
                doc.expr_types = expr_types;
                doc.ast = Some(ast);
                doc.ir_ctx = Some(ir_ctx);
            }
            Err(err) => {
                // Preserve previous valid AST, IR context, and expr types for resilient IDE queries

                match err {
                    JmccError::Pretty { diagnostic, .. } => {
                        // Create synthetic single-file AST for span mapping if needed
                        let synthetic_ast = jmcc::ast::parser::parse_string(
                            &doc.text,
                            &doc.path.display().to_string(),
                            edition,
                            0,
                        )
                        .ok();
                        if let Some(ast) = &synthetic_ast {
                            if let Some(lsp_diag) =
                                compiler_diag_to_lsp(&diagnostic, ast, &doc.path, lang)
                            {
                                doc.diagnostics.push(lsp_diag);
                            }
                        } else {
                            doc.diagnostics.push(Diagnostic {
                                range: Range::default(),
                                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                                code: diagnostic
                                    .code
                                    .as_ref()
                                    .map(|c| lsp_types::NumberOrString::String(c.to_string())),
                                source: Some("jmc-analyzer".to_owned()),
                                message: diagnostic.message.clone(),
                                related_information: None,
                                tags: None,
                                code_description: None,
                                data: None,
                            });
                        }
                    }
                    other => {
                        doc.diagnostics.push(Diagnostic {
                            range: Range::default(),
                            severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                            code: None,
                            source: Some("jmc-analyzer".to_owned()),
                            message: other.to_string(),
                            related_information: None,
                            tags: None,
                            code_description: None,
                            data: None,
                        });
                    }
                }
            }
        }
    }
}

/// Detects the language of code tokens in a source string.
///
/// Skips comments (`//` and `/* */`) and string literals (`"..."`, `'...'`, `` `...` ``).
/// - If any Cyrillic character is found in code tokens, returns `Some(Lang::Ru)`.
/// - If any ASCII Latin letter is found in code tokens (and no Cyrillic), returns `Some(Lang::En)`.
/// - If no language-identifying letters are present in code tokens, returns `None`.
#[must_use]
pub fn detect_code_language(source: &str) -> Option<Lang> {
    let mut chars = source.char_indices().peekable();
    let mut has_latin = false;
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
                return Some(Lang::Ru);
            }
            'a'..='z' | 'A'..='Z' => {
                has_latin = true;
            }
            _ => {}
        }
    }
    if has_latin { Some(Lang::En) } else { None }
}

/// Recursively walks all statements in a hierarchy, invoking `cb` for each statement.
pub fn walk_statements(stmts: &[jmcc::ast::Statement], cb: &mut impl FnMut(&jmcc::ast::Statement)) {
    for stmt in stmts {
        cb(stmt);
        match stmt {
            jmcc::ast::Statement::Function(f) => walk_statements(&f.body, cb),
            jmcc::ast::Statement::Process(p) => walk_statements(&p.body, cb),
            jmcc::ast::Statement::Event(e) => walk_statements(&e.body, cb),
            jmcc::ast::Statement::Class(c) => walk_statements(&c.body, cb),
            jmcc::ast::Statement::Interface(i) => walk_statements(&i.body, cb),
            jmcc::ast::Statement::If(i) => {
                walk_statements(&i.then_body, cb);
                for (_, branch) in &i.elif_branches {
                    walk_statements(branch, cb);
                }
                if let Some(else_b) = &i.else_body {
                    walk_statements(else_b, cb);
                }
            }
            jmcc::ast::Statement::While(w) => walk_statements(&w.body, cb),
            jmcc::ast::Statement::For(f) => walk_statements(&f.body, cb),
            jmcc::ast::Statement::TryCatch(t) => {
                walk_statements(&t.try_body, cb);
                walk_statements(&t.catch_body, cb);
            }
            jmcc::ast::Statement::Match(m) => {
                for arm in &m.arms {
                    walk_statements(&arm.body, cb);
                }
            }
            _ => {}
        }
    }
}

/// Canonical Russian names for standard types.
#[must_use]
pub fn ru_type_name(name: &str) -> Option<&'static str> {
    let short = name.rsplit("::").next().unwrap_or(name);
    match short {
        "text" | "string" | "str" => Some("текст"),
        "number" | "int" | "float" => Some("число"),
        "boolean" | "bool" => Some("логическое"),
        "array" | "list" => Some("массив"),
        "map" | "dict" => Some("словарь"),
        "vector" | "vec" => Some("вектор"),
        "location" | "loc" => Some("локация"),
        "block" => Some("блок"),
        "entity" => Some("сущность"),
        "item" => Some("предмет"),
        "player" => Some("игрок"),
        "particle" => Some("частица"),
        "potion" => Some("зелье"),
        "sound" => Some("звук"),
        "world" => Some("мир"),
        "variable" => Some("переменная"),
        "value" => Some("значение"),
        "function" | "fn" => Some("функция"),
        "iterator" => Some("итератор"),
        "any" => Some("любой"),
        _ => None,
    }
}

fn localize_type_name(short: &str, aliases: &[String], lang: Lang) -> String {
    if lang != Lang::Ru {
        return short.to_owned();
    }
    if let Some(ru) = ru_type_name(short) {
        return ru.to_owned();
    }
    for alias in aliases {
        if alias
            .chars()
            .any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
        {
            return alias.clone();
        }
    }
    short.to_owned()
}

/// Formats a semantic type into a human-readable type name using IR context symbol tables.
#[must_use]
pub fn format_type(ty: &Type, ir_ctx: Option<&IrCtx>, lang: Lang) -> String {
    match ty {
        Type::Class(id, args) => {
            let class_name = ir_ctx
                .and_then(|ctx| ctx.classes_by_def.get(id))
                .map_or_else(
                    || format!("Class#{id}"),
                    |info| {
                        let short = info.name.rsplit("::").next().unwrap_or(&info.name);
                        localize_type_name(short, &info.aliases, lang)
                    },
                );
            if args.is_empty() {
                class_name
            } else {
                let args_str: Vec<_> = args.iter().map(|a| format_type(a, ir_ctx, lang)).collect();
                format!("{class_name}<{}>", args_str.join(", "))
            }
        }
        Type::Enum(id) => ir_ctx.and_then(|ctx| ctx.enums_by_def.get(id)).map_or_else(
            || format!("Enum#{id}"),
            |info| {
                let short = info.name.rsplit("::").next().unwrap_or(&info.name);
                localize_type_name(short, &info.aliases, lang)
            },
        ),
        Type::InferVar(_) | Type::Param(_) | Type::Unknown => {
            if lang == Lang::Ru {
                "любой".to_owned()
            } else {
                "any".to_owned()
            }
        }
        Type::Never => {
            if lang == Lang::Ru {
                "никогда".to_owned()
            } else {
                "never".to_owned()
            }
        }
    }
}
