//! Main language server JSON-RPC loop using `lsp-server`.

use std::error::Error;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    CompletionOptions, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, HoverProviderCapability,
    InitializeParams, OneOf, PublishDiagnosticsParams, RenameOptions, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};

use super::completion::provide_completions;
use super::formatting::format_document;
use super::goto_def::provide_definition;
use super::highlight::provide_document_highlight;
use super::hover::provide_hover;
use super::inlay_hints::provide_inlay_hints;
use super::references::find_references;
use super::rename::{prepare_rename, rename_symbol};
use super::semantic_tokens::{compute_semantic_tokens, get_legend};
use super::signature_help::provide_signature_help;
use super::state::ServerState;
use super::symbols::{provide_document_symbols, provide_workspace_symbols};

/// Runs the LSP server on stdio until shutdown.
///
/// # Errors
///
/// Returns an error if the server connection fails or receives an unrecoverable protocol error.
pub fn run_server(
    cli_std_path: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![":".to_owned(), ".".to_owned()]),
            work_done_progress_options: Default::default(),
            all_commit_characters: None,
            completion_item: None,
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        signature_help_provider: Some(lsp_types::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: get_legend(),
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        ..Default::default()
    };

    let init_value = connection.initialize(serde_json::to_value(server_capabilities)?)?;
    let init_params: Option<InitializeParams> = serde_json::from_value(init_value).ok();

    let mut state = ServerState::new();
    state.std_path = cli_std_path;
    if let Some(params) = init_params {
        if state.std_path.is_none()
            && let Some(init_options) = &params.initialization_options
            && let Some(std_val) = init_options.get("stdPath")
            && let Some(std_str) = std_val.as_str()
            && !std_str.trim().is_empty()
        {
            state.std_path = Some(std::path::PathBuf::from(std_str.trim()));
        }

        if let Some(loc) = &params.locale {
            jmcc::i18n::set_lang_by_name(loc);
            state.lang = jmcc::i18n::current_lang();
        }
        #[expect(
            deprecated,
            reason = "fallback for clients that do not send workspace_folders"
        )]
        let uri = params.root_uri.or_else(|| {
            let f = params.workspace_folders.as_ref()?;
            f.first().map(|wf| wf.uri.clone())
        });
        if let Some(uri) = uri {
            state.workspace_root = ServerState::url_to_path(&uri);
        }
    }

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                handle_request(&state, &connection, req)?;
            }
            Message::Notification(not) => {
                handle_notification(&mut state, &connection, not)?;
            }
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}

fn publish_doc_diagnostics(
    state: &ServerState,
    connection: &Connection,
    uri: &Url,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(doc) = state.documents.get(uri) {
        let not = Notification::new(
            "textDocument/publishDiagnostics".to_owned(),
            PublishDiagnosticsParams {
                uri: doc.uri.clone(),
                diagnostics: doc.diagnostics.clone(),
                version: Some(doc.version),
            },
        );
        connection.sender.send(Message::Notification(not))?;
    }
    Ok(())
}

fn handle_notification(
    state: &mut ServerState,
    connection: &Connection,
    not: Notification,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match not.method.as_str() {
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = params.text_document.uri;
            let version = params.text_document.version;
            let text = params.text_document.text;

            state.open_document(uri.clone(), version, text);
            publish_doc_diagnostics(state, connection, &uri)?;
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = params.text_document.uri;
            let version = params.text_document.version;

            if let Some(change) = params.content_changes.into_iter().last() {
                state.update_document(&uri, version, change.text);
                publish_doc_diagnostics(state, connection, &uri)?;
            }
        }
        "textDocument/didSave" => {
            let params: DidSaveTextDocumentParams = serde_json::from_value(not.params)?;
            let uri = params.text_document.uri;
            state.compile_document_by_uri(&uri);
            publish_doc_diagnostics(state, connection, &uri)?;
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
            state.close_document(&params.text_document.uri);
        }
        "workspace/didChangeWatchedFiles" => {
            let uris: Vec<_> = state.documents.keys().cloned().collect();
            for uri in uris {
                state.compile_document_by_uri(&uri);
                publish_doc_diagnostics(state, connection, &uri)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[expect(clippy::too_many_lines, reason = "Dispatches LSP request handlers")]
fn handle_request(
    state: &ServerState,
    connection: &Connection,
    req: Request,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match req.method.as_str() {
        "textDocument/definition" => {
            let params: lsp_types::GotoDefinitionParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position_params.text_document.uri)
                .and_then(|doc| provide_definition(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/completion" => {
            let params: lsp_types::CompletionParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position.text_document.uri)
                .map(|doc| provide_completions(doc, &params))
                .unwrap_or_default();

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/hover" => {
            let params: lsp_types::HoverParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position_params.text_document.uri)
                .and_then(|doc| provide_hover(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/prepareRename" => {
            let params: lsp_types::TextDocumentPositionParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document.uri)
                .and_then(|doc| prepare_rename(doc, params.position));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/rename" => {
            let params: lsp_types::RenameParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position.text_document.uri)
                .and_then(|doc| rename_symbol(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/documentSymbol" => {
            let params: lsp_types::DocumentSymbolParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document.uri)
                .and_then(|doc| provide_document_symbols(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/formatting" => {
            let params: lsp_types::DocumentFormattingParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document.uri)
                .and_then(|doc| format_document(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/references" => {
            let params: lsp_types::ReferenceParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position.text_document.uri)
                .and_then(|doc| find_references(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/inlayHint" => {
            let params: lsp_types::InlayHintParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document.uri)
                .and_then(|doc| provide_inlay_hints(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/semanticTokens/full" => {
            let params: lsp_types::SemanticTokensParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document.uri)
                .and_then(|doc| {
                    let ast = doc.ast.as_ref()?;
                    Some(compute_semantic_tokens(ast, &doc.path))
                });

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/signatureHelp" => {
            let params: lsp_types::SignatureHelpParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position_params.text_document.uri)
                .and_then(|doc| provide_signature_help(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "textDocument/documentHighlight" => {
            let params: lsp_types::DocumentHighlightParams = serde_json::from_value(req.params)?;
            let res = state
                .documents
                .get(&params.text_document_position_params.text_document.uri)
                .and_then(|doc| provide_document_highlight(doc, &params));

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        "workspace/symbol" => {
            let params: lsp_types::WorkspaceSymbolParams = serde_json::from_value(req.params)?;
            let res = provide_workspace_symbols(state, &params);

            let resp = Response::new_ok(req.id, res);
            connection.sender.send(Message::Response(resp))?;
        }
        _ => {
            let resp = Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                "Method not found".to_owned(),
            );
            connection.sender.send(Message::Response(resp))?;
        }
    }
    Ok(())
}
