mod cache;
mod compiler;
mod document;
mod elements;
mod lsp;
mod lua;
mod parser;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use dashmap::DashMap;
use document::document::Document;
use document::element::Element;
use lsp::semantic::semantic_token_from_document;
use parser::langparser::LangParser;
use parser::parser::Parser;
use parser::parser::ParserState;
use parser::semantics::Semantics;
use parser::source::SourceFile;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;

#[derive(Debug)]
struct Backend {
	client: Client,
	document_map: DashMap<String, String>,
	//ast_map: DashMap<String, Vec<Box<dyn Element>>>,
	//variables: DashMap<String, HashMap<String, Arc<dyn Variable + Send + Sync + 'static>>>,
	semantic_token_map: DashMap<String, Vec<SemanticToken>>,
}

#[derive(Debug)]
struct TextDocumentItem {
	uri: Url,
	text: String,
}

impl Backend {
	async fn on_change(&self, params: TextDocumentItem) {
		self.document_map
			.insert(params.uri.to_string(), params.text.clone());

		// TODO: Create a custom parser for the lsp
		// Which will require a dyn Document to work
		let source = Rc::new(SourceFile::with_content(params.uri.to_string(), params.text.clone(), None));
		let parser = LangParser::default();
		let (doc, state) = parser.parse(ParserState::new_with_semantics(&parser, None), source.clone(), None);

		if let Some(sems) = state.shared.semantics.as_ref().map(|sems| {
			std::cell::RefMut::filter_map(sems.borrow_mut(), |sems| sems.get_mut(&(source as Rc<dyn parser::source::Source>)))
				.ok()
				.unwrap()
		}) {
			self.semantic_token_map
				.insert(params.uri.to_string(), sems.tokens.to_owned());
		};
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
									token_types: lsp::semantic::LEGEND_TYPE.into(),
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

	async fn shutdown(&self) -> Result<()> { Ok(()) }

	async fn did_open(&self, params: DidOpenTextDocumentParams) {
		self.client
			.log_message(MessageType::INFO, "file opened!")
			.await;
		self.on_change(TextDocumentItem {
			uri: params.text_document.uri,
			text: params.text_document.text,
		})
		.await
	}

	async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
		self.on_change(TextDocumentItem {
			uri: params.text_document.uri,
			text: std::mem::take(&mut params.content_changes[0].text),
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

		if let Some(semantic_tokens) = self.semantic_token_map.get(&uri) {
			let data = semantic_tokens
				.iter()
				.filter_map(|token| Some(token.clone()))
				.collect::<Vec<_>>();

			return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
				result_id: None,
				data: data,
			})));
		}
		Ok(None)
	}
}

#[tokio::main]
async fn main() {
	let stdin = tokio::io::stdin();
	let stdout = tokio::io::stdout();

	let (service, socket) = LspService::new(|client| Backend {
		client,
		document_map: DashMap::new(),
		semantic_token_map: DashMap::new(),
	});
	Server::new(stdin, stdout, socket).serve(service).await;
}
