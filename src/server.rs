#![feature(char_indices_offset)]
mod document;
mod compiler;
mod parser;
mod elements;
mod lua;
mod cache;
mod lsp;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use dashmap::DashMap;
use document::variable::Variable;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
	document_map: DashMap<String, String>,
	//variables: DashMap<String, HashMap<String, Arc<dyn Variable + Send + Sync + 'static>>>,
}

#[derive(Debug)]
struct TextDocumentItem {
	uri: Url,
	text: String,
	version: i32,
}

impl Backend {
	async fn on_change(&self, params: TextDocumentItem) {
		self.document_map
			.insert(params.uri.to_string(), params.text.clone());
		let ParserResult {
			ast,
			parse_errors,
			semantic_tokens,
		} = parse(&params.text);
		let diagnostics = parse_errors
			.into_iter()
			.filter_map(|item| {
				let (message, span) = match item.reason() {
					chumsky::error::SimpleReason::Unclosed { span, delimiter } => {
						(format!("Unclosed delimiter {}", delimiter), span.clone())
					}
					chumsky::error::SimpleReason::Unexpected => (
						format!(
							"{}, expected {}",
							if item.found().is_some() {
								"Unexpected token in input"
							} else {
								"Unexpected end of input"
							},
							if item.expected().len() == 0 {
								"something else".to_string()
							} else {
								item.expected()
									.map(|expected| match expected {
										Some(expected) => expected.to_string(),
										None => "end of input".to_string(),
									})
								.collect::<Vec<_>>()
									.join(", ")
							}
						),
						item.span(),
						),
						chumsky::error::SimpleReason::Custom(msg) => (msg.to_string(), item.span()),
				};

				|| -> Option<Diagnostic> {
					// let start_line = rope.try_char_to_line(span.start)?;
					// let first_char = rope.try_line_to_char(start_line)?;
					// let start_column = span.start - first_char;
					let start_position = offset_to_position(span.start, &rope)?;
					let end_position = offset_to_position(span.end, &rope)?;
					// let end_line = rope.try_char_to_line(span.end)?;
					// let first_char = rope.try_line_to_char(end_line)?;
					// let end_column = span.end - first_char;
					Some(Diagnostic::new_simple(
							Range::new(start_position, end_position),
							message,
					))
				}()
			})
		.collect::<Vec<_>>();

		self.client
			.publish_diagnostics(params.uri.clone(), diagnostics, Some(params.version))
			.await;

		if let Some(ast) = ast {
			self.ast_map.insert(params.uri.to_string(), ast);
		}
		// self.client
		//     .log_message(MessageType::INFO, &format!("{:?}", semantic_tokens))
		//     .await;
		self.semantic_token_map
			.insert(params.uri.to_string(), semantic_tokens);
		}
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
		Ok(InitializeResult {
			server_info: None,
			capabilities: ServerCapabilities {
				text_document_sync: Some(TextDocumentSyncCapability::Kind(
						TextDocumentSyncKind::FULL,
				)),
				completion_provider: Some(CompletionOptions {
					resolve_provider: Some(false),
					trigger_characters: Some(vec!["%".to_string()]),
					work_done_progress_options: Default::default(),
					all_commit_characters: None,
					completion_item: None,
				}),
				semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                        SemanticTokensRegistrationOptions {
                            text_document_registration_options: {
                                TextDocumentRegistrationOptions {
                                    document_selector: Some(vec![DocumentFilter {
                                        language: Some("nml".to_string()),
                                        scheme: Some("file".to_string()),
                                        pattern: None,
                                    }]),
                                }
                            },
                            semantic_tokens_options: SemanticTokensOptions {
                                work_done_progress_options: WorkDoneProgressOptions::default(),
                                legend: SemanticTokensLegend {
                                    token_types: vec![SemanticTokenType::COMMENT, SemanticTokenType::MACRO],
                                    token_modifiers: vec![],
                                },
                                range: None, //Some(true),
                                full: Some(SemanticTokensFullOptions::Bool(true)),
                            },
                            static_registration_options: StaticRegistrationOptions::default(),
                        },
                    ),
                ),
				..ServerCapabilities::default()
			},
		})
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

	async fn did_open(&self, params: DidOpenTextDocumentParams) {
		self.client
			.log_message(MessageType::INFO, "file opened!")
			.await;
		self.on_change(TextDocumentItem {
			uri: params.text_document.uri,
			text: params.text_document.text,
			version: params.text_document.version,
		})
		.await
	}

	async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
		self.on_change(TextDocumentItem {
			uri: params.text_document.uri,
			text: std::mem::take(&mut params.content_changes[0].text),
			version: params.text_document.version,
		})
		.await
	}

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let completions = || -> Option<Vec<CompletionItem>> {
            let mut ret = Vec::with_capacity(0);

            Some(ret)
        }();
        Ok(completions.map(CompletionResponse::Array))
    }

	async fn semantic_tokens_full(
		&self,
		params: SemanticTokensParams,
	) -> Result<Option<SemanticTokensResult>> {
		let uri = params.text_document.uri.to_string();
		self.client
			.log_message(MessageType::LOG, "semantic_token_full")
			.await;
		let semantic_tokens = || -> Option<Vec<SemanticToken>> {
			let semantic_tokens = vec![
				SemanticToken {
					delta_line: 1,
					delta_start: 2,
					length: 5,
					token_type: 1,
					token_modifiers_bitset: 0,
				}
			];
			Some(semantic_tokens)
		}();
		if let Some(semantic_token) = semantic_tokens {
			return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
				result_id: None,
				data: semantic_token,
			})));
		}
		Ok(None)
	}
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(
		|client|
			Backend {
				client
			});
    Server::new(stdin, stdout, socket).serve(service).await;
}
