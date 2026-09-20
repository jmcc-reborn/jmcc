//! LSP server document state management, compiler cache, and overlay synchronization.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jmcc::ast::semantic::{SemanticErrorKind, Type, analyze_for_diagnostics};
use jmcc::ast::{Ast, ExprId, parse_file_with_overlays};
use jmcc::error::JmccError;
use jmcc::i18n::{Lang, current_lang};
use jmcc::ir::ctx::IrCtx;
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
}

/// Global language server workspace and document state.
#[derive(Default)]
pub struct ServerState {
    pub documents: HashMap<Url, DocumentData>,
    pub workspace_root: Option<PathBuf>,
    pub lang: Lang,
}

impl ServerState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            workspace_root: None,
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

    /// Registers a newly opened document and triggers compilation.
    pub fn open_document(&mut self, uri: Url, version: i32, text: String) {
        let Some(path) = Self::url_to_path(&uri) else {
            return;
        };

        let lang = if self.lang == Lang::Ru
            || jmcc::ast::lexer::detect_lexer_kind(&text) == jmcc::ast::lexer::LexerKind::Alternate
        {
            Lang::Ru
        } else {
            self.lang
        };

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
        };

        self.compile_document(&mut doc);
        self.documents.insert(uri, doc);
    }

    /// Updates the text of an open document and triggers recompilation.
    pub fn update_document(&mut self, uri: &Url, version: i32, text: String) {
        if let Some(doc) = self.documents.get_mut(uri) {
            doc.version = version;
            doc.text = text;
            if self.lang == Lang::Ru
                || jmcc::ast::lexer::detect_lexer_kind(&doc.text)
                    == jmcc::ast::lexer::LexerKind::Alternate
            {
                doc.lang = Lang::Ru;
            } else {
                doc.lang = self.lang;
            }
            self.compile_document_by_uri(uri);
        }
    }

    /// Closes a document, removing it from active memory.
    pub fn close_document(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    /// Recompiles an open document by URI.
    pub fn compile_document_by_uri(&mut self, uri: &Url) {
        let overlays = self.overlays();
        if let Some(doc) = self.documents.get_mut(uri) {
            Self::recompile_doc(doc, overlays, doc.lang);
        }
    }

    /// Recompiles the given document using current server overlays.
    fn compile_document(&self, doc: &mut DocumentData) {
        let mut overlays = self.overlays();
        let canon = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
        overlays.insert(canon, doc.text.clone());
        overlays.insert(doc.path.clone(), doc.text.clone());

        Self::recompile_doc(doc, overlays, doc.lang);
    }

    fn recompile_doc(doc: &mut DocumentData, overlays: HashMap<PathBuf, String>, lang: Lang) {
        doc.diagnostics.clear();
        doc.semantic_errors.clear();

        // 1. Parse AST with overlays
        match parse_file_with_overlays(&doc.path, 2026, overlays) {
            Ok(ast) => {
                // 2. Build IR context
                let ir_ctx = IrCtx::new(&ast);

                // 3. Analyze semantics without early bail
                let (expr_types, semantic_errors) =
                    analyze_for_diagnostics(&ast, &doc.text, &ir_ctx, 2026);

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
                            2026,
                            0,
                        )
                        .ok();
                        if let Some(ast) = &synthetic_ast {
                            if let Some(lsp_diag) =
                                compiler_diag_to_lsp(&diagnostic, ast, &doc.path)
                            {
                                doc.diagnostics.push(lsp_diag);
                            }
                        } else {
                            doc.diagnostics.push(Diagnostic {
                                range: Range::default(),
                                severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                                code: diagnostic
                                    .code
                                    .map(|c| lsp_types::NumberOrString::String(c.to_owned())),
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
