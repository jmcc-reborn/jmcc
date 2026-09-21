use std::path::PathBuf;

use jmc_analyzer::lsp::goto_def::provide_definition;
use jmc_analyzer::lsp::semantic_tokens::{TokenType, compute_semantic_tokens};
use jmc_analyzer::lsp::state::ServerState;
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, PartialResultParams, Position,
    TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkDoneProgressParams,
};

fn create_test_server(uri_str: &str, content: &str) -> (ServerState, Url) {
    let mut state = ServerState::new();
    let uri = Url::parse(uri_str).unwrap();
    state.open_document(uri.clone(), 1, content.to_owned());
    (state, uri)
}

#[test]
fn test_goto_definition_on_declaration_stays_on_current_method() {
    let code = r#"
class RangeIterator {
    var start: number;

    function iter(self: RangeIterator) -> RangeIterator {
        return self;
    }
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_range_iter.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `iter` in `function iter` (line 4, character 14)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 14,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let resp = provide_definition(doc, &params).expect("definition should resolve");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            // Must stay in this file, NOT jump to array::iter in std/primitives/code/array.jc
            assert_eq!(
                loc.uri, uri,
                "Definition on declaration must resolve to current document"
            );
            assert_eq!(
                loc.range.start.line, 4,
                "Definition must point to current method declaration line"
            );
        }
        _ => panic!("Expected scalar location"),
    }
}

#[test]
fn test_goto_definition_self_resolution() {
    let code = r#"
class RangeIterator {
    var start: number;

    function reset(self: RangeIterator) {
        self.start = 0;
    }
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_self_res.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `self` in `self.start = 0` (line 5, character 9)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 9,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let resp = provide_definition(doc, &params).expect("self should resolve");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, uri, "self must resolve in current document");
            // Points to either `self` param (line 4) or `class RangeIterator` (line 1)
            assert!(
                loc.range.start.line == 4 || loc.range.start.line == 1,
                "self must point to enclosing method parameter or class declaration, got line {}",
                loc.range.start.line
            );
        }
        _ => panic!("Expected scalar location"),
    }
}

#[test]
fn test_goto_definition_mapkeyiterator_case_insensitive() {
    let code = r#"
function demo() {
    var it: MapKeyIterator = null;
    var it2: mapkeyiterator = null;
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_map_iter.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // 1. Check MapKeyIterator (line 2, character 15)
    let params_camel = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let resp_camel = provide_definition(doc, &params_camel)
        .expect("MapKeyIterator should resolve to std definition");
    if let GotoDefinitionResponse::Scalar(loc) = resp_camel {
        assert!(
            loc.uri.as_str().contains("iterator.jc"),
            "MapKeyIterator should resolve to iterator.jc, got {}",
            loc.uri
        );
    } else {
        panic!("Expected scalar location");
    }

    // 2. Check lowercase mapkeyiterator (line 3, character 15)
    let params_lower = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 3,
                character: 15,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let resp_lower = provide_definition(doc, &params_lower)
        .expect("lowercase mapkeyiterator should resolve case-insensitively");
    if let GotoDefinitionResponse::Scalar(loc) = resp_lower {
        assert!(
            loc.uri.as_str().contains("iterator.jc"),
            "mapkeyiterator should resolve to iterator.jc, got {}",
            loc.uri
        );
    } else {
        panic!("Expected scalar location");
    }
}

#[test]
fn test_semantic_tokens_only_precise_identifiers() {
    let code = r#"
class RangeIterator {
    var start: number;

    function iter(self: RangeIterator) -> RangeIterator {
        return self;
    }
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_tokens.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let ast = doc.ast.as_ref().expect("ast should exist");
    let path = PathBuf::from("/tmp/test_tokens.jc");

    let tokens = compute_semantic_tokens(ast, &path);

    // Reconstruct absolute tokens
    let mut abs_tokens = Vec::new();
    let mut line = 0;
    let mut start_char = 0;

    for tok in tokens.data {
        line += tok.delta_line;
        if tok.delta_line == 0 {
            start_char += tok.delta_start;
        } else {
            start_char = tok.delta_start;
        }
        abs_tokens.push((line, start_char, tok.length, tok.token_type));
    }

    // 1. Class token must be on line 1 ("class RangeIterator") with length = 13 ("RangeIterator"),
    // NOT the whole class span!
    let class_token = abs_tokens
        .iter()
        .find(|(l, _, _, ty)| *l == 1 && *ty == TokenType::Class as u32)
        .expect("Class token must exist");
    assert_eq!(
        class_token.2, 13,
        "Class token length must be exactly 13 ('RangeIterator'), not entire class body"
    );

    // 2. Function/Method token must have length = 4 ("iter")
    let method_token = abs_tokens
        .iter()
        .find(|(l, _, _, ty)| {
            *l == 4 && (*ty == TokenType::Method as u32 || *ty == TokenType::Function as u32)
        })
        .expect("Method token must exist on line 4");
    assert_eq!(
        method_token.2, 4,
        "Method token length must be exactly 4 ('iter'), not entire function body"
    );

    // 3. Confirm `self` is NOT assigned Method or Function or Class token
    for (l, _, len, ty) in &abs_tokens {
        if *len == 4
            && (*ty == TokenType::Method as u32
                || *ty == TokenType::Function as u32
                || *ty == TokenType::Class as u32)
        {
            assert_eq!(
                *l, 4,
                "Word of length 4 unexpectedly marked as Method/Class on line {l}"
            );
        }
    }
}

#[test]
fn test_hover_localized_doc_comments() {
    let code = r#"
/// RU:
/// Возвращает следующий элемент диапазона.
/// EN:
/// Returns the next element of the range.
@alias("следующий")
function next(self: Range) -> number {
    return 1;
}
"#;
    let (mut state, uri) = create_test_server("file:///tmp/test_hover_doc.jc", code);
    state.documents.get_mut(&uri).unwrap().lang = jmcc::i18n::Lang::Ru;
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `next` in `function next` (line 6, character 10)
    let params = lsp_types::HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 6,
                character: 10,
            },
        },
        work_done_progress_params: Default::default(),
    };

    let hover =
        jmc_analyzer::lsp::hover::provide_hover(doc, &params).expect("hover must be provided");

    if let lsp_types::HoverContents::Markup(m) = hover.contents {
        assert!(m.value.contains("следующий"), "Hover must contain alias");
        assert!(
            m.value.contains("Возвращает следующий элемент"),
            "Hover must contain Russian doc comment: {}",
            m.value
        );
        assert!(
            !m.value.contains("Returns the next element"),
            "Hover in RU mode must not contain English doc comment: {}",
            m.value
        );
        assert!(
            !m.value.contains("Функция JustCode"),
            "Hover must NOT contain generic placeholder text"
        );
    } else {
        panic!("Expected MarkupContent");
    }
}

#[test]
fn test_goto_definition_enum_variant_not_class_text() {
    let code = r#"
enum MessageType {
    ACTION_BAR,
    CHAT,
    TITLE,
    TEXT
}

function test_msg() {
    var m = MessageType.TEXT;
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_enum_variant.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `TEXT` in `MessageType.TEXT` (line 9, character 26)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 9,
                character: 26,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp = provide_definition(doc, &params).expect("TEXT variant should resolve");
    match resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(
                loc.uri, uri,
                "Must resolve in current file enum, not teleport to stdlib text.jc"
            );
            assert_eq!(loc.range.start.line, 5, "Must point to TEXT on line 5");
            let len = loc.range.end.character - loc.range.start.character;
            assert_eq!(
                len, 4,
                "Must highlight exactly 'TEXT' (4 chars), not whole block"
            );
        }
        _ => panic!("Expected scalar location"),
    }
}

#[test]
fn test_goto_definition_selection_range_exact_identifier_length() {
    let code = r#"
class RangeIterator {
    var start: number;

    function iter(self: RangeIterator) -> RangeIterator {
        return self;
    }
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_exact_len.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `RangeIterator` declaration (line 1, character 8)
    let params_class = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 1,
                character: 8,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp_class = provide_definition(doc, &params_class).expect("class must resolve");
    if let GotoDefinitionResponse::Scalar(loc) = resp_class {
        let len = loc.range.end.character - loc.range.start.character;
        assert_eq!(
            len, 13,
            "Class selection range must be exactly length 13 ('RangeIterator'), not entire class"
        );
    }

    // Position of `iter` method declaration (line 4, character 14)
    let params_fn = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 14,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp_fn = provide_definition(doc, &params_fn).expect("method must resolve");
    if let GotoDefinitionResponse::Scalar(loc) = resp_fn {
        let len = loc.range.end.character - loc.range.start.character;
        assert_eq!(
            len, 4,
            "Method selection range must be exactly length 4 ('iter'), not entire function body"
        );
    }
}

#[test]
fn test_document_symbols_hierarchy() {
    let code = r#"
class RangeIterator {
    var start: number;

    function iter(self: RangeIterator) -> RangeIterator {
        return self;
    }
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_symbols.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    let params = lsp_types::DocumentSymbolParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp = jmc_analyzer::lsp::symbols::provide_document_symbols(doc, &params)
        .expect("symbols must be returned");

    if let lsp_types::DocumentSymbolResponse::Nested(symbols) = resp {
        assert_eq!(symbols.len(), 1, "Must contain 1 top-level class symbol");
        let cls = &symbols[0];
        assert_eq!(cls.name, "RangeIterator");
        let children = cls.children.as_ref().expect("Class must have children");
        assert_eq!(
            children.len(),
            2,
            "Class must have 2 children (field and method)"
        );
        assert_eq!(children[0].name, "start");
        assert_eq!(children[1].name, "iter");
    } else {
        panic!("Expected Nested document symbols");
    }
}

#[test]
fn test_inlay_hints_for_untyped_variables() {
    let code = r#"
function demo() {
    var count = 10;
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_inlay.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    let params = lsp_types::InlayHintParams {
        text_document: TextDocumentIdentifier { uri },
        range: lsp_types::Range::default(),
        work_done_progress_params: Default::default(),
    };

    let hints = jmc_analyzer::lsp::inlay_hints::provide_inlay_hints(doc, &params)
        .expect("hints must be computed");

    assert_eq!(
        hints.len(),
        1,
        "Must generate 1 inlay hint for untyped variable"
    );
    if let lsp_types::InlayHintLabel::String(label) = &hints[0].label {
        assert!(
            label.contains("number") || label.contains("число"),
            "Inlay hint must show inferred type 'number' or 'число', got '{label}'"
        );
    } else {
        panic!("Expected string label");
    }
}

#[test]
fn test_document_formatting() {
    let code = "function   demo(   a:   number ){\nreturn a;\n}\n";
    let (state, uri) = create_test_server("file:///tmp/test_fmt.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    let params = lsp_types::DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri },
        options: lsp_types::FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..Default::default()
        },
        work_done_progress_params: Default::default(),
    };

    let edits = jmc_analyzer::lsp::formatting::format_document(doc, &params)
        .expect("formatting edits must be returned");

    assert!(!edits.is_empty(), "Formatting must produce text edit");
    assert!(
        edits[0].new_text.contains("function demo(a: number)"),
        "Formatted code must be clean"
    );
}

#[test]
fn test_signature_help_for_action() {
    let code = "player::message(\"hello\", ";
    let (state, uri) = create_test_server("file:///tmp/test_sig.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    let params = lsp_types::SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 24,
            },
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let sig_help = jmc_analyzer::lsp::signature_help::provide_signature_help(doc, &params)
        .expect("signature help should be provided");

    assert!(
        !sig_help.signatures.is_empty(),
        "Must have at least 1 signature"
    );
    assert!(sig_help.signatures[0].label.contains("player::message"));
    assert_eq!(
        sig_help.active_parameter,
        Some(1),
        "Active parameter must be 1 after comma"
    );
}

#[test]
fn test_document_highlight() {
    let code = "function test(x: number) {\n    var y = x + 1;\n    return y;\n}\n";
    let (state, uri) = create_test_server("file:///tmp/test_highlight.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Position of `x` in parameter declaration (line 0, col 14)
    let params = lsp_types::DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 14,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let highlights = jmc_analyzer::lsp::highlight::provide_document_highlight(doc, &params)
        .expect("highlights should be provided");

    assert_eq!(
        highlights.len(),
        2,
        "Must highlight param definition and usage in expr"
    );
}

#[test]
fn test_workspace_symbols() {
    let code = "class MyCustomService {\n}\nfunction processOrder() {\n}\n";
    let (state, _) = create_test_server("file:///tmp/test_ws_sym.jc", code);

    let params = lsp_types::WorkspaceSymbolParams {
        query: "Order".to_owned(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let res = jmc_analyzer::lsp::symbols::provide_workspace_symbols(&state, &params)
        .expect("workspace symbols should be returned");

    if let lsp_types::WorkspaceSymbolResponse::Flat(symbols) = res {
        assert_eq!(symbols.len(), 1, "Must find processOrder matching 'Order'");
        assert_eq!(symbols[0].name, "processOrder");
    } else {
        panic!("Expected flat symbols response");
    }
}

fn get_russian_code_fixture() -> (PathBuf, String, Url) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent dir")
        .join("jmcc/tests/russian_code.jc");
    let content = std::fs::read_to_string(&path).expect("read russian_code.jc");
    let uri = Url::from_file_path(&path).expect("valid file url");
    (path, content, uri)
}

#[test]
fn test_russian_code_diagnostics() {
    let (_path, content, uri) = get_russian_code_fixture();
    let (state, uri) = create_test_server(uri.as_str(), &content);
    let doc = state.documents.get(&uri).expect("doc should exist");
    assert!(
        doc.semantic_errors.is_empty(),
        "russian_code.jc should have 0 semantic errors, got: {:?}",
        doc.semantic_errors
    );
    assert!(
        doc.diagnostics.is_empty(),
        "russian_code.jc should have 0 diagnostics, got: {:?}",
        doc.diagnostics
    );
}

#[test]
fn test_russian_code_semantic_tokens() {
    let (_path, content, uri) = get_russian_code_fixture();
    let (state, uri) = create_test_server(uri.as_str(), &content);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let ast = doc.ast.as_ref().expect("ast should exist");

    let tokens = jmc_analyzer::lsp::semantic_tokens::compute_semantic_tokens(ast, &doc.path);
    assert!(
        !tokens.data.is_empty(),
        "Semantic tokens must be extracted for russian_code.jc"
    );

    // Ensure all token lengths in UTF-16 code units are positive and reasonable
    for tok in &tokens.data {
        assert!(tok.length > 0, "Token length must be > 0");
        assert!(
            tok.length < 100,
            "Token length must not be abnormally large: {}",
            tok.length
        );
    }
}

#[test]
fn test_russian_code_goto_definition() {
    let (_path, content, uri) = get_russian_code_fixture();
    let (state, uri) = create_test_server(uri.as_str(), &content);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // Line 41 (0-indexed): "    перем юзер = Пользователь("Алексей", 25);"
    // Character position on "Пользователь"
    let line_41_idx = content.lines().nth(41).expect("line 41 must exist");
    let col = line_41_idx.chars().take_while(|c| *c != 'П').count() as u32;

    let params = lsp_types::GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 41,
                character: col,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let def = jmc_analyzer::lsp::goto_def::provide_definition(doc, &params)
        .expect("definition should be found for Пользователь");

    if let lsp_types::GotoDefinitionResponse::Scalar(loc) = def {
        assert_eq!(loc.uri, uri);
        // Line 5 is "класс Пользователь {"
        assert_eq!(loc.range.start.line, 5);
        // Exact identifier selection: "Пользователь" has 12 chars
        assert_eq!(loc.range.end.character - loc.range.start.character, 12);
    } else {
        panic!("Expected scalar definition");
    }
}

#[test]
fn test_logical_operators_tokens() {
    let code = "function test(a: boolean, b: boolean) {\n    if not a and b or not b {\n        return;\n    }\n}\n";
    let (state, uri) = create_test_server("file:///tmp/test_logical.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let ast = doc.ast.as_ref().expect("ast should exist");

    let tokens = jmc_analyzer::lsp::semantic_tokens::compute_semantic_tokens(ast, &doc.path);
    // TokenType::Keyword is index 9 in legend
    let keyword_type_id = 9;
    let has_keyword_tokens = tokens.data.iter().any(|t| t.token_type == keyword_type_id);
    assert!(
        has_keyword_tokens,
        "Logical operators (not, and, or) must emit keyword semantic tokens"
    );
}

#[test]
fn test_russian_code_inlay_hints() {
    let (_path, content, uri) = get_russian_code_fixture();
    let (state, uri) = create_test_server(uri.as_str(), &content);
    let doc = state.documents.get(&uri).expect("doc should exist");

    let params = lsp_types::InlayHintParams {
        text_document: TextDocumentIdentifier { uri },
        range: lsp_types::Range::default(),
        work_done_progress_params: Default::default(),
    };

    let hints = jmc_analyzer::lsp::inlay_hints::provide_inlay_hints(doc, &params)
        .expect("hints must be computed");

    assert!(
        !hints.is_empty(),
        "Inlay hints must be present for russian_code.jc"
    );

    let type_labels: Vec<String> = hints
        .iter()
        .filter(|h| h.kind == Some(lsp_types::InlayHintKind::TYPE))
        .filter_map(|h| {
            if let lsp_types::InlayHintLabel::String(s) = &h.label {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();

    assert!(
        type_labels.iter().any(|l| l.contains("текст")),
        "Must infer 'текст' instead of 'text': {type_labels:?}"
    );
    assert!(
        type_labels.iter().all(|l| !l.contains("text")),
        "Must NOT contain English 'text': {type_labels:?}"
    );
    assert!(
        type_labels.iter().any(|l| l.contains("Пользователь")),
        "Must infer 'Пользователь': {type_labels:?}"
    );
    assert!(
        type_labels.iter().any(|l| l.contains("Координата")),
        "Must infer 'Координата': {type_labels:?}"
    );
}

#[test]
fn test_analyzer_std_diagnostics_and_symbols() {
    let code = r#"
import "std/ai/nn.jc";

function main() {
    NeuralNetwork();
}
"#;
    let (state, uri) = create_test_server("file:///tmp/test_nn_missing_args.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let diagnostics = &doc.diagnostics;
    assert!(
        doc.diagnostics
            .iter()
            .any(|d| d.message.contains("layer_sizes")),
        "Analyzer must report missing required argument 'layer_sizes' from std/ai/nn.jc. Diagnostics: {diagnostics:?}"
    );
}

#[test]
fn test_manifest_locale_en_forces_english_types() {
    let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_manifest_locale_en");
    drop(std::fs::remove_dir_all(&test_dir));
    std::fs::create_dir_all(&test_dir).unwrap();

    let manifest_path = test_dir.join("jmcc.toml");
    std::fs::write(
        &manifest_path,
        r#"
[package]
name = "test_en_pkg"
version = "0.1.0"
edition = 2026
locale = "en"
"#,
    )
    .expect("write jmcc.toml failed");

    let source_path = test_dir.join("main.jc");
    let code = r#"
function demo() {
    var count = 10;
}
"#;
    std::fs::write(&source_path, code).expect("write main.jc failed");

    let mut state = ServerState::new();
    state.lang = jmcc::i18n::Lang::Ru; // Even if server default is Russian

    let uri = Url::from_file_path(&source_path).unwrap();
    state.open_document(uri.clone(), 1, code.to_owned());
    let doc = state.documents.get(&uri).expect("doc should exist");

    assert_eq!(
        doc.lang,
        jmcc::i18n::Lang::En,
        "Manifest locale = 'en' must enforce English language"
    );

    let params = lsp_types::InlayHintParams {
        text_document: TextDocumentIdentifier { uri },
        range: lsp_types::Range::default(),
        work_done_progress_params: Default::default(),
    };
    let hints = jmc_analyzer::lsp::inlay_hints::provide_inlay_hints(doc, &params)
        .expect("hints must be computed");

    assert!(!hints.is_empty(), "Inlay hints must exist");
    if let lsp_types::InlayHintLabel::String(label) = &hints[0].label {
        assert!(
            label.contains("number"),
            "Must show 'number', got: '{label}'"
        );
        assert!(
            !label.contains("число"),
            "Must NOT show 'число', got: '{label}'"
        );
    } else {
        panic!("Expected string label");
    }
}

#[test]
fn test_manifest_locale_ru_forces_russian_types() {
    let test_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_manifest_locale_ru");
    drop(std::fs::remove_dir_all(&test_dir));
    std::fs::create_dir_all(&test_dir).unwrap();

    let manifest_path = test_dir.join("jmcc.toml");
    std::fs::write(
        &manifest_path,
        r#"
[package]
name = "test_ru_pkg"
version = "0.1.0"
edition = 2026
locale = "ru"
"#,
    )
    .expect("write jmcc.toml failed");

    let source_path = test_dir.join("main.jc");
    let code = r#"
function demo() {
    var count = 10;
}
"#;
    std::fs::write(&source_path, code).expect("write main.jc failed");

    let mut state = ServerState::new();
    state.lang = jmcc::i18n::Lang::En; // Even if server default is English

    let uri = Url::from_file_path(&source_path).unwrap();
    state.open_document(uri.clone(), 1, code.to_owned());
    let doc = state.documents.get(&uri).expect("doc should exist");

    assert_eq!(
        doc.lang,
        jmcc::i18n::Lang::Ru,
        "Manifest locale = 'ru' must enforce Russian language"
    );

    let params = lsp_types::InlayHintParams {
        text_document: TextDocumentIdentifier { uri },
        range: lsp_types::Range::default(),
        work_done_progress_params: Default::default(),
    };
    let hints = jmc_analyzer::lsp::inlay_hints::provide_inlay_hints(doc, &params)
        .expect("hints must be computed");

    assert!(!hints.is_empty(), "Inlay hints must exist");
    if let lsp_types::InlayHintLabel::String(label) = &hints[0].label {
        assert!(label.contains("число"), "Must show 'число', got: '{label}'");
    } else {
        panic!("Expected string label");
    }
}

#[test]
fn test_code_language_detection_english_and_russian() {
    let en_code = r#"
// Better Than Nothing Anticheat (BTNA)
game var check_noclip = NoClipCheck();
event<player_move> {
    var count = 10;
}
"#;
    let (state_en, uri_en) = create_test_server("file:///tmp/test_detect_en.jc", en_code);
    let doc_en = state_en.documents.get(&uri_en).expect("doc should exist");
    assert_eq!(
        doc_en.lang,
        jmcc::i18n::Lang::En,
        "English code must be detected as Lang::En"
    );

    let ru_code = r#"
// Античит
перем флаг: логическое = правда;
событие<player_move> {
    перем счетчик = 10;
}
"#;
    let (state_ru, uri_ru) = create_test_server("file:///tmp/test_detect_ru.jc", ru_code);
    let doc_ru = state_ru.documents.get(&uri_ru).expect("doc should exist");
    assert_eq!(
        doc_ru.lang,
        jmcc::i18n::Lang::Ru,
        "Russian code must be detected as Lang::Ru"
    );
}

#[test]
fn test_completion_value_and_game_values() {
    let en_code = "var x = value::";
    let (state_en, uri_en) = create_test_server("file:///tmp/test_comp_val_en.jc", en_code);
    let doc_en = state_en.documents.get(&uri_en).expect("doc should exist");
    let params_en = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_en },
            position: Position {
                line: 0,
                character: 15,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_en = jmc_analyzer::lsp::completion::provide_completions(doc_en, &params_en);
    assert!(
        !items_en.is_empty(),
        "value:: must provide game value completions"
    );
    assert!(
        items_en.iter().any(|item| item.label == "location"),
        "location must be in game values"
    );
    assert!(
        items_en.iter().any(|item| item.label == "eye_location"),
        "eye_location must be in game values"
    );
    assert!(
        items_en.iter().any(|item| item.label == "max_health"),
        "max_health must be in game values"
    );

    // Russian alias: значение::
    let ru_code = "перем x = значение::";
    let (state_ru, uri_ru) = create_test_server("file:///tmp/test_comp_val_ru.jc", ru_code);
    let doc_ru = state_ru.documents.get(&uri_ru).expect("doc should exist");
    let params_ru = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_ru },
            position: Position {
                line: 0,
                character: 19,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_ru = jmc_analyzer::lsp::completion::provide_completions(doc_ru, &params_ru);
    assert!(
        !items_ru.is_empty(),
        "значение:: must provide game value completions"
    );
    assert!(
        items_ru.iter().any(|item| item.label == "location"),
        "location must be in game values for значение::"
    );

    // Verify insert_text is identifier without parentheses
    let loc_item = items_en
        .iter()
        .find(|item| item.label == "location")
        .expect("location item");
    assert_eq!(
        loc_item.insert_text.as_deref(),
        Some("location"),
        "Game value insert_text must not include ()"
    );

    // Test with prefix typed after ::
    let prefix_code = "value::cur";
    let (state_pfx, uri_pfx) = create_test_server("file:///tmp/test_comp_val_pfx.jc", prefix_code);
    let doc_pfx = state_pfx.documents.get(&uri_pfx).expect("doc should exist");
    let params_pfx = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_pfx },
            position: Position {
                line: 0,
                character: 10,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_pfx = jmc_analyzer::lsp::completion::provide_completions(doc_pfx, &params_pfx);
    assert!(
        items_pfx.iter().any(|i| i.label.starts_with("cur")),
        "value::cur must provide matching completions"
    );
}

#[test]
fn test_completion_world_actions() {
    let en_code = "world::";
    let (state_en, uri_en) = create_test_server("file:///tmp/test_comp_world_en.jc", en_code);
    let doc_en = state_en.documents.get(&uri_en).expect("doc should exist");
    let params_en = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_en },
            position: Position {
                line: 0,
                character: 7,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_en = jmc_analyzer::lsp::completion::provide_completions(doc_en, &params_en);
    assert!(!items_en.is_empty(), "world:: must provide actions");
    assert!(
        items_en
            .iter()
            .any(|item| item.label.contains("time") || item.label.contains("weather")),
        "world actions should include time or weather actions"
    );

    // Russian alias: мир::
    let ru_code = "мир::";
    let (state_ru, uri_ru) = create_test_server("file:///tmp/test_comp_world_ru.jc", ru_code);
    let doc_ru = state_ru.documents.get(&uri_ru).expect("doc should exist");
    let params_ru = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_ru },
            position: Position {
                line: 0,
                character: 10,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_ru = jmc_analyzer::lsp::completion::provide_completions(doc_ru, &params_ru);
    assert!(!items_ru.is_empty(), "мир:: must provide actions");

    // Player actions: player:: and игрок::
    let pl_code = "player::";
    let (state_pl, uri_pl) = create_test_server("file:///tmp/test_comp_pl.jc", pl_code);
    let doc_pl = state_pl.documents.get(&uri_pl).expect("doc should exist");
    let params_pl = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri_pl },
            position: Position {
                line: 0,
                character: 8,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items_pl = jmc_analyzer::lsp::completion::provide_completions(doc_pl, &params_pl);
    assert!(
        items_pl
            .iter()
            .any(|i| i.label.contains("message") || i.label.contains("teleport")),
        "player:: must provide player actions"
    );
}

#[test]
fn test_completion_keywords_en_and_ru() {
    let code = "";
    let (state, uri) = create_test_server("file:///tmp/test_comp_kw.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let params = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 0,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = jmc_analyzer::lsp::completion::provide_completions(doc, &params);
    let labels: std::collections::HashSet<_> =
        items.iter().map(|item| item.label.as_str()).collect();

    // English keywords
    assert!(labels.contains("class"), "class keyword must be present");
    assert!(
        labels.contains("function"),
        "function keyword must be present"
    );
    assert!(labels.contains("var"), "var keyword must be present");
    assert!(labels.contains("enum"), "enum keyword must be present");
    assert!(labels.contains("if"), "if keyword must be present");

    // Russian keywords
    assert!(labels.contains("класс"), "класс keyword must be present");
    assert!(
        labels.contains("функция"),
        "функция keyword must be present"
    );
    assert!(labels.contains("перем"), "перем keyword must be present");
    assert!(
        labels.contains("переменная"),
        "переменная keyword must be present"
    );
    assert!(
        labels.contains("перечисление"),
        "перечисление keyword must be present"
    );
    assert!(labels.contains("если"), "если keyword must be present");

    // Builtin objects
    assert!(labels.contains("world"), "world object must be present");
    assert!(labels.contains("мир"), "мир object must be present");
    assert!(labels.contains("value"), "value object must be present");
    assert!(
        labels.contains("значение"),
        "значение object must be present"
    );
}

#[test]
fn test_completion_selectors_and_generic() {
    let code = "var loc = value::location<";
    let (state, uri) = create_test_server("file:///tmp/test_comp_sel.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let params = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 26,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = jmc_analyzer::lsp::completion::provide_completions(doc, &params);
    assert!(
        !items.is_empty(),
        "Selectors must be provided for value::location<"
    );
    let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    assert!(
        labels.contains(&"default") || labels.contains(&"current_selection"),
        "Must contain standard game value selectors"
    );
}

#[test]
fn test_completion_keyword_prefix_filtering() {
    let check_prefix = |code: &str, char_pos: u32, expected_matches: &[&str]| {
        let (state, uri) = create_test_server("file:///tmp/test_comp_pfx.jc", code);
        let doc = state.documents.get(&uri).expect("doc should exist");
        let params = lsp_types::CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 0,
                    character: char_pos,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        let items = jmc_analyzer::lsp::completion::provide_completions(doc, &params);
        let prefix = &code[..char_pos as usize];
        let matching: Vec<_> = items
            .iter()
            .filter(|item| item.label.starts_with(prefix))
            .map(|item| item.label.as_str())
            .collect();

        for exp in expected_matches {
            assert!(
                matching.contains(exp),
                "Typing '{prefix}' must suggest '{exp}', got: {matching:?}"
            );
        }
    };

    // English: cla -> class
    check_prefix("cla", 3, &["class"]);

    // Russian: кла -> класс
    check_prefix("кла", 6, &["класс"]);

    // fun -> fun, function
    check_prefix("fun", 3, &["fun", "function"]);

    // дей -> действие
    check_prefix("дей", 6, &["действие"]);

    // def -> def
    check_prefix("def", 3, &["def"]);

    // пер -> переменная, перем
    check_prefix("пер", 6, &["переменная", "перем"]);

    // кон -> константа
    check_prefix("кон", 6, &["константа"]);

    // выб -> выбор, выбросить
    check_prefix("выб", 6, &["выбор", "выбросить"]);
}

#[test]
fn test_completion_prelude_symbols_without_path_prefix() {
    let code = "var x = 10;";
    let (state, uri) = create_test_server("file:///tmp/test_prelude_comp.jc", code);
    let doc = state.documents.get(&uri).expect("doc should exist");
    let params = lsp_types::CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 0,
                character: 11,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };
    let items = jmc_analyzer::lsp::completion::provide_completions(doc, &params);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Symbols from prelude must NOT contain "std::primitives::"
    for label in &labels {
        assert!(
            !label.starts_with("std::primitives::"),
            "Prelude symbol '{label}' must not start with 'std::primitives::'"
        );
        assert!(
            !label.starts_with("primitives::"),
            "Prelude symbol '{label}' must not start with 'primitives::'"
        );
    }
}

#[test]
fn test_import_hover_and_goto_definition() {
    let code = r#"//! RU: Главный тестовый модуль.
//! EN: Main test module.
import "std/math/core.jc";
"#;
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent dir")
        .join("jmcc/tests/test_import_hover.jc");
    let uri_str = Url::from_file_path(&fixture_path).unwrap().to_string();
    let (state, uri) = create_test_server(&uri_str, code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // 1. Hover on import line
    let hover_params = lsp_types::HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 2,
                character: 8,
            },
        },
        work_done_progress_params: Default::default(),
    };
    let hover = jmc_analyzer::lsp::hover::provide_hover(doc, &hover_params);
    assert!(hover.is_some(), "Hover on import line must succeed");
    let hover_content = match hover.unwrap().contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup content"),
    };
    assert!(
        hover_content.contains("core.jc")
            || hover_content.contains("Математический")
            || hover_content.contains("Math utilities"),
        "Hover content must contain module documentation, got: {hover_content}"
    );

    // 2. Goto definition on import line
    let def_params = lsp_types::GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 2,
                character: 8,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let def = jmc_analyzer::lsp::goto_def::provide_definition(doc, &def_params);
    assert!(def.is_some(), "Goto definition on import must succeed");
    match def.unwrap() {
        lsp_types::GotoDefinitionResponse::Scalar(loc) => {
            let path_str = loc
                .uri
                .to_file_path()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(
                path_str.ends_with("core.jc"),
                "Goto definition must point to core.jc, got: {path_str}"
            );
        }
        _ => panic!("Expected scalar location"),
    }
}

#[test]
fn test_hover_class_doc_and_variable_type_doc() {
    let code = r#"import 'std/effects/text/bubble.jc';

fun main() {
    var b = Bubble("steve");
    b.show("hello");
}
"#;
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent dir")
        .join("jmcc/tests/case1.jc");
    let uri_str = Url::from_file_path(&fixture_path).unwrap().to_string();
    let (state, uri) = create_test_server(&uri_str, code);
    let doc = state.documents.get(&uri).expect("doc should exist");

    // 1. Hover on class identifier in constructor call `Bubble("steve")`
    let hover_class = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 3,
                    character: 14,
                },
            },
            work_done_progress_params: Default::default(),
        },
    );
    assert!(
        hover_class.is_some(),
        "Hover on Bubble constructor must succeed"
    );
    let content = match hover_class.unwrap().contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        content.contains("class Bubble"),
        "Content must contain class signature, got: {content}"
    );
    assert!(
        content.contains("Создает и управляет парящим текстовым облачком")
            || content.contains("Creates and manages a floating text bubble"),
        "Content must contain class doc-comment, got: {content}"
    );

    // 2. Hover on variable `b` with inferred class type
    let hover_var = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 4,
                    character: 4,
                },
            },
            work_done_progress_params: Default::default(),
        },
    );
    assert!(hover_var.is_some(), "Hover on variable `b` must succeed");
    let var_content = match hover_var.unwrap().contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        var_content.contains("b:"),
        "Variable hover must display var name and type, got: {var_content}"
    );
    assert!(
        var_content.contains("Создает и управляет парящим текстовым облачком")
            || var_content.contains("Creates and manages a floating text bubble"),
        "Variable hover must include class doc comment, got: {var_content}"
    );

    // 3. Hover on class method/process `show`
    let hover_method = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 4,
                    character: 7,
                },
            },
            work_done_progress_params: Default::default(),
        },
    );
    assert!(
        hover_method.is_some(),
        "Hover on method `show` must succeed"
    );
    let method_content = match hover_method.unwrap().contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        method_content.contains("process show"),
        "Method hover must display signature, got: {method_content}"
    );
    assert!(
        method_content.contains("Асинхронно отображает облачко сообщения")
            || method_content.contains("Asynchronously displays the bubble message"),
        "Method hover must include doc comment, got: {method_content}"
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "Comprehensive hover test for interfaces, type aliases, and enums"
)]
#[test]
fn test_hover_interface_typealias_enum_and_aliases() {
    let code = r#"
/// RU: Интерфейс сущности с уроном.
/// EN: Damageable entity interface.
@alias("Уязвимый")
interface Damageable {
    function damage(amount: number);
}

/// RU: Псевдоним числа для очков здоровья.
/// EN: Number alias for health points.
@alias("ОчкиЗдоровья")
typealias HP = number;

/// RU: Состояния персонажа.
/// EN: Character states.
@alias("Состояние")
enum State {
    IDLE,
    RUNNING,
}

fun test(d: Damageable, hp: HP, s: State) {
    var u: Уязвимый;
}
"#;
    let (mut state, uri) = create_test_server("file:///tmp/test_items_hover.jc", code);
    state.documents.get_mut(&uri).unwrap().lang = jmcc::i18n::Lang::Ru;
    let doc = state.documents.get(&uri).expect("doc should exist");

    // 1. Hover on `interface Damageable`
    let h1 = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 4,
                    character: 12,
                },
            },
            work_done_progress_params: Default::default(),
        },
    )
    .expect("hover on interface must succeed");
    let c1 = match h1.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        c1.contains("interface Damageable") && c1.contains("Интерфейс сущности с уроном"),
        "Interface hover failed: {c1}"
    );

    // 2. Hover on `typealias HP`
    let h2 = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 11,
                    character: 11,
                },
            },
            work_done_progress_params: Default::default(),
        },
    )
    .expect("hover on typealias must succeed");
    let c2 = match h2.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        c2.contains("typealias HP = number") && c2.contains("Псевдоним числа для очков здоровья"),
        "Typealias hover failed: {c2}"
    );

    // 3. Hover on `enum State`
    let h3 = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 16,
                    character: 6,
                },
            },
            work_done_progress_params: Default::default(),
        },
    )
    .expect("hover on enum must succeed");
    let c3 = match h3.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        c3.contains("enum State") && c3.contains("Состояния персонажа"),
        "Enum hover failed: {c3}"
    );

    // 4. Hover on russian alias `Уязвимый` in variable annotation `var u: Уязвимый`
    let h4 = jmc_analyzer::lsp::hover::provide_hover(
        doc,
        &lsp_types::HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 22,
                    character: 12,
                },
            },
            work_done_progress_params: Default::default(),
        },
    )
    .expect("hover on alias must succeed");
    let c4 = match h4.contents {
        lsp_types::HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };
    assert!(
        c4.contains("interface Damageable") && c4.contains("Интерфейс сущности с уроном"),
        "Alias hover failed: {c4}"
    );
}
