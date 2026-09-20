//! Machine-readable `JustCode` syntax.
//!
//! The JSON in `data/syntax.json` is the source of truth for keywords, builtin
//! objects, operators and the future LSP handshake. The VS Code grammar is a
//! `TextMate` rendering of the same language; keep them in sync when the lexer
//! grows new tokens.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Keywords grouped the way a highlighter / LSP client typically wants them.
#[derive(Debug, Clone, Deserialize)]
pub struct KeywordGroups {
    pub declaration: Vec<String>,
    pub storage: Vec<String>,
    pub control: Vec<String>,
    #[serde(rename = "operatorWord")]
    pub operator_word: Vec<String>,
    pub literal: Vec<String>,
    #[serde(rename = "textStyle")]
    pub text_style: Vec<String>,
}

/// Variable-scope keywords and one-letter prefixes.
#[derive(Debug, Clone, Deserialize)]
pub struct VariableScopes {
    pub keywords: Vec<String>,
    pub prefixes: BTreeMap<String, String>,
}

/// Operator tables matching [`jmcc::ast::lexer::Token`].
#[derive(Debug, Clone, Deserialize)]
pub struct Operators {
    pub assignment: Vec<String>,
    pub comparison: Vec<String>,
    pub arithmetic: Vec<String>,
    pub bitwise: Vec<String>,
    pub increment: Vec<String>,
    pub punctuation: Vec<String>,
}

/// How the VS Code client will spawn the language server later.
#[derive(Debug, Clone, Deserialize)]
pub struct LspSpec {
    pub command: String,
    pub args: Vec<String>,
    pub status: String,
}

/// Top-level catalogue loaded from `data/syntax.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct SyntaxCatalog {
    pub language: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "scopeName")]
    pub scope_name: String,
    pub extensions: Vec<String>,
    pub keywords: KeywordGroups,
    #[serde(rename = "variableScopes")]
    pub variable_scopes: VariableScopes,
    #[serde(rename = "textPrefixes")]
    pub text_prefixes: BTreeMap<String, String>,
    #[serde(rename = "builtinObjects")]
    pub builtin_objects: Vec<String>,
    #[serde(rename = "builtinTypes")]
    pub builtin_types: Vec<String>,
    pub attributes: Vec<String>,
    #[serde(default, rename = "dunderMethods")]
    pub dunder_methods: Vec<String>,
    pub operators: Operators,
    pub lsp: LspSpec,
}

impl SyntaxCatalog {
    /// Parse the catalogue that is compiled into the crate.
    ///
    /// # Panics
    ///
    /// Panics only if the in-tree `data/syntax.json` is not valid JSON matching
    /// this schema — that is a crate bug, not a runtime input error.
    #[must_use]
    pub fn builtin() -> Self {
        serde_json::from_str(include_str!("../data/syntax.json"))
            .expect("data/syntax.json must deserialize into SyntaxCatalog")
    }

    /// Every reserved word the lexer currently treats as a keyword.
    #[must_use]
    pub fn all_keywords(&self) -> Vec<&str> {
        self.keywords
            .declaration
            .iter()
            .chain(&self.keywords.storage)
            .chain(&self.keywords.control)
            .chain(&self.keywords.operator_word)
            .chain(&self.keywords.literal)
            .chain(&self.keywords.text_style)
            .map(String::as_str)
            .collect()
    }

    /// Checks if the given word is a registered keyword (either English or Russian).
    #[must_use]
    pub fn is_keyword(&self, word: &str) -> bool {
        self.all_keywords().contains(&word)
    }

    /// Checks if the given name is a known dunder method (either English or Russian).
    #[must_use]
    pub fn is_dunder_method(&self, name: &str) -> bool {
        self.dunder_methods.iter().any(|d| d == name)
    }

    /// Checks if the given name is a known builtin type (either English or Russian).
    #[must_use]
    pub fn is_builtin_type(&self, ty: &str) -> bool {
        self.builtin_types.iter().any(|t| t == ty)
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxCatalog;

    #[test]
    fn builtin_catalog_loads() {
        let catalog = SyntaxCatalog::builtin();
        assert_eq!(catalog.language, "jc");
        assert!(catalog.all_keywords().contains(&"while"));
        assert!(catalog.all_keywords().contains(&"interface"));
        assert!(catalog.builtin_objects.iter().any(|o| o == "player"));
        assert_eq!(catalog.lsp.command, "jmc-analyzer");
    }

    #[test]
    #[expect(
        clippy::cognitive_complexity,
        reason = "Test asserts multiple keywords"
    )]
    fn builtin_catalog_russian_syntax() {
        let catalog = SyntaxCatalog::builtin();
        // Keywords
        assert!(catalog.is_keyword("функция"));
        assert!(catalog.is_keyword("пусть"));
        assert!(catalog.is_keyword("перем"));
        assert!(catalog.is_keyword("переменная"));
        assert!(catalog.is_keyword("если"));
        assert!(catalog.is_keyword("иначе"));
        assert!(catalog.is_keyword("иначе_если"));
        assert!(catalog.is_keyword("пока"));
        assert!(catalog.is_keyword("для"));
        assert!(catalog.is_keyword("выбор"));
        assert!(catalog.is_keyword("вариант"));
        assert!(catalog.is_keyword("попытка"));
        assert!(catalog.is_keyword("исключение"));
        assert!(catalog.is_keyword("интерфейс"));
        assert!(catalog.is_keyword("класс"));
        assert!(catalog.is_keyword("истина"));
        assert!(catalog.is_keyword("ложь"));

        // Dunder methods
        assert!(catalog.is_dunder_method("__init__"));
        assert!(catalog.is_dunder_method("__конструктор__"));
        assert!(catalog.is_dunder_method("__иниц__"));
        assert!(catalog.is_dunder_method("__сложить__"));
        assert!(catalog.is_dunder_method("__индекс__"));

        // Builtin types
        assert!(catalog.is_builtin_type("number"));
        assert!(catalog.is_builtin_type("число"));
        assert!(catalog.is_builtin_type("строка"));
        assert!(catalog.is_builtin_type("массив"));
        assert!(catalog.is_builtin_type("словарь"));

        // Builtin objects
        assert!(catalog.builtin_objects.iter().any(|o| o == "игрок"));
        assert!(catalog.builtin_objects.iter().any(|o| o == "мир"));

        // Prefixes and scopes
        assert_eq!(
            catalog.variable_scopes.prefixes.get("л"),
            Some(&"local".to_string())
        );
        assert_eq!(
            catalog.variable_scopes.prefixes.get("с"),
            Some(&"save".to_string())
        );
        assert_eq!(catalog.text_prefixes.get("у"), Some(&"legacy".to_string()));
    }
}
