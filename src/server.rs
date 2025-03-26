mod cache;
mod compiler;
mod elements;
mod lsp;
mod lua;
mod parser;
mod unit;

use std::sync::Arc;

use dashmap::DashMap;
use lsp::code::CodeRangeInfo;
use lsp::conceal::ConcealInfo;
use lsp::conceal::ConcealParams;
use lsp::styles::StyleInfo;
use lsp::styles::StyleParams;
use parser::reports::Report;
use parser::source::SourceFile;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;

#[derive(Debug)]
struct Backend {
	client: Client,
	document_map: DashMap<String, String>,
	definition_map: DashMap<String, Vec<(Location, Range)>>,
	semantic_token_map: DashMap<String, Vec<SemanticToken>>,
	diagnostic_map: DashMap<String, Vec<Diagnostic>>,
	hints_map: DashMap<String, Vec<InlayHint>>,
	conceals_map: DashMap<String, Vec<ConcealInfo>>,
	styles_map: DashMap<String, Vec<StyleInfo>>,
	coderanges_map: DashMap<String, Vec<CodeRangeInfo>>,
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
		let source = Arc::new(SourceFile::with_content(
			params.uri.to_string(),
			params.text.clone(),
			None,
		));

		// Diagnostics
		self.diagnostic_map.clear();
		let parser = LangParser::new(
			false,
			Box::new(|_colors, reports| {
				Report::reports_to_diagnostics(&self.diagnostic_map, reports)
			}),
		);
		// Parse
		let (_doc, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);

		if let Some(lsp) = state.shared.lsp.as_ref() {
			let borrow = lsp.borrow();

			// Semantics
			for (source, sem) in &borrow.semantic_data {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.semantic_token_map
						.insert(path, sem.tokens.replace(vec![]));
				}
			}

			// Inlay hints
			for (source, hints) in &borrow.inlay_hints {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.hints_map.insert(path, hints.hints.replace(vec![]));
				}
			}

			// Definitions
			for (source, definitions) in &borrow.definitions {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.definition_map
						.insert(path, definitions.definitions.replace(vec![]));
				}
			}

			// Conceals
			for (source, conceals) in &borrow.conceals {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.conceals_map
						.insert(path, conceals.conceals.replace(vec![]));
				}
			}

			// Styles
			for (source, styles) in &borrow.styles {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.styles_map.insert(path, styles.styles.replace(vec![]));
				}
			}

			// Code Ranges
			for (source, coderanges) in &borrow.coderanges {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.coderanges_map
						.insert(path, coderanges.coderanges.replace(vec![]));
				}
			}
		}
	}

	async fn handle_conceal_request(
		&self,
		params: ConcealParams,
	) -> jsonrpc::Result<Vec<ConcealInfo>> {
		if let Some(conceals) = self.conceals_map.get(params.text_document.uri.as_str()) {
			let (_, data) = conceals.pair();

			return Ok(data.to_vec());
		}
		Ok(vec![])
	}

	async fn handle_style_request(&self, params: StyleParams) -> jsonrpc::Result<Vec<StyleInfo>> {
		if let Some(styles) = self.styles_map.get(params.text_document.uri.as_str()) {
			let (_, data) = styles.pair();

			return Ok(data.to_vec());
		}
		Ok(vec![])
	}

	async fn handle_coderange_request(
		&self,
		params: StyleParams,
	) -> jsonrpc::Result<Vec<CodeRangeInfo>> {
		if let Some(styles) = self.coderanges_map.get(params.text_document.uri.as_str()) {
			let (_, data) = styles.pair();

			return Ok(data.to_vec());
		}
		Ok(vec![])
	}
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
	async fn initialize(
		&self,
		_params: InitializeParams,
	) -> tower_lsp::jsonrpc::Result<InitializeResult> {
		Ok(InitializeResult {
			capabilities: ServerCapabilities {
				text_document_sync: Some(TextDocumentSyncCapability::Kind(
					TextDocumentSyncKind::FULL,
				)),
				definition_provider: Some(OneOf::Left(true)),
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
										language: Some("nml".into()),
										scheme: Some("file".into()),
										pattern: Some("*.nml".into()),
									}]),
								}
							},
							semantic_tokens_options: SemanticTokensOptions {
								work_done_progress_options: WorkDoneProgressOptions::default(),
								legend: SemanticTokensLegend {
									token_types: lsp::semantic::TOKEN_TYPE.into(),
									token_modifiers: lsp::semantic::TOKEN_MODIFIERS.into(),
								},
								range: None, //Some(true),
								full: Some(SemanticTokensFullOptions::Bool(true)),
							},
							static_registration_options: StaticRegistrationOptions::default(),
						},
					),
				),
				diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
					DiagnosticOptions {
						identifier: None,
						inter_file_dependencies: true,
						workspace_diagnostics: true,
						work_done_progress_options: WorkDoneProgressOptions::default(),
					},
				)),
				inlay_hint_provider: Some(OneOf::Left(true)),
				..ServerCapabilities::default()
			},
			server_info: Some(ServerInfo {
				name: "nmlls".into(),
				version: Some("0.1".into()),
			}),
		})
	}

	async fn initialized(&self, _: InitializedParams) {
		self.client
			.log_message(MessageType::INFO, "server initialized!")
			.await;
	}

	async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> { Ok(()) }

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

	async fn goto_definition(
		&self,
		params: GotoDefinitionParams,
	) -> tower_lsp::jsonrpc::Result<Option<GotoDefinitionResponse>> {
		let uri = &params.text_document_position_params.text_document.uri;
		let pos = &params.text_document_position_params.position;

		if let Some(definitions) = self.definition_map.get(uri.as_str()) {
			let index = definitions.binary_search_by(|(_, range)| {
				if range.start.line > pos.line {
					std::cmp::Ordering::Greater
				} else if range.end.line <= pos.line {
					if range.start.line == pos.line && range.start.character <= pos.character {
						std::cmp::Ordering::Equal
					} else if range.end.line == pos.line && range.end.character >= pos.character {
						std::cmp::Ordering::Equal
					} else if range.start.line < pos.line && range.end.line > pos.line {
						std::cmp::Ordering::Equal
					} else {
						std::cmp::Ordering::Less
					}
				} else {
					std::cmp::Ordering::Less
				}
			});
			if let Ok(index) = index {
				let loc = self.definition_map.get(uri.as_str()).as_ref().unwrap()[index]
					.0
					.clone();
				return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
			}
		}

		Err(tower_lsp::jsonrpc::Error::method_not_found())
	}

	async fn completion(
		&self,
		_params: CompletionParams,
	) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
		//let uri = params.text_document_position.text_document.uri;
		//let position = params.text_document_position.position;
		let completions = || -> Option<Vec<CompletionItem>> {
			let ret = Vec::with_capacity(0);

			Some(ret)
		}();
		Ok(completions.map(CompletionResponse::Array))
	}

	async fn semantic_tokens_full(
		&self,
		params: SemanticTokensParams,
	) -> tower_lsp::jsonrpc::Result<Option<SemanticTokensResult>> {
		let uri = params.text_document.uri;
		self.client
			.log_message(MessageType::LOG, "semantic_token_full")
			.await;

		if let Some(semantic_tokens) = self.semantic_token_map.get(uri.as_str()) {
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

	async fn diagnostic(
		&self,
		params: DocumentDiagnosticParams,
	) -> tower_lsp::jsonrpc::Result<DocumentDiagnosticReportResult> {
		Ok(DocumentDiagnosticReportResult::Report(
			DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
				related_documents: None,
				full_document_diagnostic_report: FullDocumentDiagnosticReport {
					result_id: None,
					items: self
						.diagnostic_map
						.get(params.text_document.uri.as_str())
						.map_or(vec![], |v| v.to_owned()),
				},
			}),
		))
	}

	async fn inlay_hint(
		&self,
		params: InlayHintParams,
	) -> tower_lsp::jsonrpc::Result<Option<Vec<InlayHint>>> {
		if let Some(hints) = self.hints_map.get(params.text_document.uri.as_str()) {
			let (_, data) = hints.pair();

			return Ok(Some(data.to_owned()));
		}
		Ok(None)
	}
}

#[tokio::main]
async fn main() {
	let stdin = tokio::io::stdin();
	let stdout = tokio::io::stdout();

	let (service, socket) = LspService::build(|client| Backend {
		client,
		document_map: DashMap::new(),
		definition_map: DashMap::new(),
		semantic_token_map: DashMap::new(),
		diagnostic_map: DashMap::new(),
		hints_map: DashMap::new(),
		conceals_map: DashMap::new(),
		styles_map: DashMap::new(),
		coderanges_map: DashMap::new(),
	})
	.custom_method("textDocument/conceal", Backend::handle_conceal_request)
	.custom_method("textDocument/style", Backend::handle_style_request)
	.custom_method("textDocument/codeRange", Backend::handle_coderange_request)
	.finish();

	Server::new(stdin, stdout, socket).serve(service).await;
}
