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
            // line 4 method `iter` has length 4, check other lines
            if *l != 4 {
                panic!(
                    "Word of length 4 unexpectedly marked as Method/Class on line {}",
                    l
                );
            }
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
        "Must infer 'текст' instead of 'text': {:?}",
        type_labels
    );
    assert!(
        type_labels.iter().all(|l| !l.contains("text")),
        "Must NOT contain English 'text': {:?}",
        type_labels
    );
    assert!(
        type_labels.iter().any(|l| l.contains("Пользователь")),
        "Must infer 'Пользователь': {:?}",
        type_labels
    );
    assert!(
        type_labels.iter().any(|l| l.contains("Координата")),
        "Must infer 'Координата': {:?}",
        type_labels
    );
}
