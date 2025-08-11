mod cache;
mod compiler;
mod elements;
mod lsp;
mod lua;
mod parser;
mod unit;
mod util;
mod layout;

use std::cmp;
use std::env::current_dir;
use std::fs::read;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::sync::Arc;

use cache::cache::Cache;
use dashmap::DashMap;
use lsp::code::CodeRangeInfo;
use lsp::completion::CompletionProvider;
use lsp::conceal::ConcealInfo;
use lsp::conceal::ConcealParams;
use lsp::hover::HoverRange;
use lsp::styles::StyleInfo;
use lsp::styles::StyleParams;
use parser::parser::Parser;
use parser::reports::Report;
use parser::source::LineCursor;
use parser::source::Source;
use parser::source::SourceFile;
use tokio::sync::mpsc;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;
use tower_lsp::Client;
use tower_lsp::LanguageServer;
use tower_lsp::LspService;
use tower_lsp::Server;
use unit::scope::ScopeAccessor;
use unit::translation::TranslationUnit;
use util::settings::ProjectSettings;

pub struct Backend {
	parser: Arc<Parser>,

	source_files: DashMap<String, Arc<dyn Source>>,
	client: Client,
	settings: ProjectSettings,
	root_path: PathBuf,
	cache: Arc<Cache>,

	completors: DashMap<String, Vec<Box<dyn CompletionProvider + 'static + Send + Sync>>>,

	units: DashMap<String, TranslationUnit>,

	document_map: DashMap<String, String>,
	definition_map: DashMap<String, Vec<(Location, Range)>>,
	semantic_token_map: DashMap<String, Vec<SemanticToken>>,
	diagnostic_map: DashMap<String, Vec<Diagnostic>>,
	hints_map: DashMap<String, Vec<InlayHint>>,
	conceals_map: DashMap<String, Vec<ConcealInfo>>,
	styles_map: DashMap<String, Vec<StyleInfo>>,
	coderanges_map: DashMap<String, Vec<CodeRangeInfo>>,
	completions_map: DashMap<String, Vec<CompletionItem>>,
	hovers_map: DashMap<String, Vec<HoverRange>>,

	// Lua ls
	lua_ranges: DashMap<String, Vec<std::ops::Range<usize>>>,
}

#[derive(Debug)]
struct TextDocumentItem {
	uri: Url,
	text: String,
}

impl Backend {
	pub fn new(
		client: Client,
		settings: ProjectSettings,
		root_path: PathBuf,
	) -> Self {
		let cache = Arc::new(Cache::new(settings.db_path.as_str()).unwrap());
		//cache.setup_tables();

		Self {
			parser: Arc::new(Parser::new()),
			source_files: DashMap::default(),
			client,
			settings,
			root_path,
			cache,

			completors: DashMap::default(),

			units: DashMap::default(),
			document_map: DashMap::default(),
			definition_map: DashMap::default(),
			semantic_token_map: DashMap::default(),
			diagnostic_map: DashMap::default(),
			hints_map: DashMap::default(),
			conceals_map: DashMap::default(),
			styles_map: DashMap::default(),
			coderanges_map: DashMap::default(),
			completions_map: DashMap::default(),
			hovers_map: DashMap::default(),

			lua_ranges: DashMap::new(),
		}
	}

	async fn on_change(&self, params: TextDocumentItem) {
		let mut external_refs = {
			let cache = self.cache.clone();
			cache.get_references().await
		};
		self.document_map
			.insert(params.uri.to_string(), params.text.clone());

		let source = Arc::new(SourceFile::with_content(
			params.uri.to_string(),
			params.text.clone(),
			None,
		));
		self.source_files
			.insert(params.uri.to_string(), source.clone());

		let path = pathdiff::diff_paths(
			params.uri.to_string().replace("file:///", "/"),
			&self.root_path,
		)
		.map(|path| path.to_str().unwrap().to_string())
		.unwrap();
		let unit = TranslationUnit::new(path, self.parser.clone(), source.clone(), true, false);

		// Set references
		unit.with_lsp(move |mut lsp| {
			lsp.external_refs.clear();
			external_refs.drain(..).for_each(|reference| {
				lsp.external_refs.insert(
					format!("{}#{}", reference.source_refkey, reference.name),
					reference,
				);
			});
		});

		let basename = PathBuf::from(params.uri.as_str())
			.file_stem()
			.map(|p| p.to_str().unwrap_or("output"))
			.unwrap_or("output")
			.to_owned();
		let output_file = format!("{}/{basename}", self.settings.output_path);
		let (reports, unit) = unit.consume(output_file);

		self.diagnostic_map.clear();
		for report in reports {
			Report::to_diagnostics(report, &self.diagnostic_map);
		}

		// Completion
		let completors = self.parser.get_completors();
		let mut items = vec![];
		for comp in &completors {
			comp.unit_items(&unit, &mut items);
		}
		self.completions_map.insert(params.uri.to_string(), items);
		self.completors.insert(params.uri.to_string(), completors);

		// TODO: Run resolver
		unit.with_lsp(|lsp| {
			// Semantics
			for (source, sem) in &lsp.semantic_data {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.semantic_token_map
						.insert(path, std::mem::take(&mut *sem.tokens.write()));
				}
			}

			// Inlay hints
			for (source, hints) in &lsp.inlay_hints {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.hints_map
						.insert(path, std::mem::take(&mut *hints.hints.write()));
				}
			}

			// Definitions
			for (source, definitions) in &lsp.definitions {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.definition_map
						.insert(path, std::mem::take(&mut *definitions.definitions.write()));
				}
			}

			// Conceals
			for (source, conceals) in &lsp.conceals {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.conceals_map
						.insert(path, std::mem::take(&mut *conceals.conceals.write()));
				}
			}

			// Styles
			for (source, styles) in &lsp.styles {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.styles_map
						.insert(path, std::mem::take(&mut *styles.styles.write()));
				}
			}

			// Code Ranges
			for (source, coderanges) in &lsp.coderanges {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.coderanges_map
						.insert(path, std::mem::take(&mut *coderanges.coderanges.write()));
				}
			}

			// Hovers
			for (source, hovers) in &lsp.hovers {
				if let Some(path) = source
					.clone()
					.downcast_ref::<SourceFile>()
					.map(|source| source.path().to_owned())
				{
					self.hovers_map
						.insert(path, std::mem::take(&mut *hovers.hovers.write()));
				}
			}
		});

		self.units.insert(source.clone().path().to_owned(), unit);
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
					trigger_characters: Some(vec![
						"%".to_string(),
						":".to_string(),
						"@".to_string(),
						"&".to_string(),
						"$".to_string(),
						"-".to_string(),
						"*".to_string(),
					]),
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
				hover_provider: Some(HoverProviderCapability::Simple(true)),
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

	async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
		Ok(())
	}

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

	async fn hover(&self, params: HoverParams) -> tower_lsp::jsonrpc::Result<Option<Hover>> {
		let uri = &params.text_document_position_params.text_document.uri;
		let pos = &params.text_document_position_params.position;

		let Some(source) = self.source_files.get(uri.as_str()).map(|v| v.clone()) else {
			return Ok(None);
		};
		let cursor = LineCursor::from_position(
			source,
			parser::source::OffsetEncoding::Utf16,
			pos.line,
			pos.character,
		);
		let hovers_from_map = || -> Option<Hover> {
			let Some(hovers) = self.hovers_map.get(uri.as_str()) else {
				return None;
			};
			let index = hovers.binary_search_by(|hover| {
				if hover.range.start() > cursor.pos {
					cmp::Ordering::Greater
				} else if hover.range.end() <= cursor.pos {
					cmp::Ordering::Less
				} else {
					cmp::Ordering::Equal
				}
			});
			let Ok(index) = index else { return None };
			Some(Hover {
				contents: HoverContents::Markup(MarkupContent {
					kind: MarkupKind::Markdown,
					value: hovers[index].content.clone(),
				}),
				range: None,
			})
		};
		if let Some(from_maps) = hovers_from_map() {
			return Ok(Some(from_maps));
		}

		// Get hovers from document
		let Some(unit) = self.units.get(uri.as_str()) else {
			return Ok(None);
		};
		let mut found = None;
		for (_, elem) in unit.get_entry_scope().content_iter(true) {
			let location = elem.original_location();
			if location.source() != unit.get_entry_scope().token().source() {
				continue;
			}
			if location.start() <= cursor.pos && location.end() > cursor.pos {
				found = Some(elem);
			}
			if location.start() > cursor.pos {
				break;
			}
		}
		let Some(hover) = found.map(|elem| elem.provide_hover()).flatten() else {
			return Ok(None);
		};
		Ok(Some(Hover {
			contents: HoverContents::Markup(MarkupContent {
				kind: MarkupKind::Markdown,
				value: hover,
			}),
			range: None,
		}))
	}

	async fn goto_definition(
		&self,
		params: GotoDefinitionParams,
	) -> tower_lsp::jsonrpc::Result<Option<GotoDefinitionResponse>> {
		let uri = &params.text_document_position_params.text_document.uri;
		let pos = &params.text_document_position_params.position;

		let Some(definitions) = self.definition_map.get(uri.as_str()) else {
			return Ok(None);
		};
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

		Ok(None)
	}

	async fn completion(
		&self,
		params: CompletionParams,
	) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
		let Some(mut completions) = self
			.completions_map
			.get(params.text_document_position.text_document.uri.as_str())
			.map(|v| v.clone())
		else {
			return Ok(None);
		};
		if let Some(completors) = self
			.completors
			.get(params.text_document_position.text_document.uri.as_str())
		{
			completors
				.iter()
				.for_each(|comp| comp.static_items(&params.context, &mut completions));
		}
		Ok(Some(CompletionResponse::Array(completions.clone())))
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
async fn main() -> anyhow::Result<()> {
	// Find project root
	let mut path = current_dir().unwrap();
	let mut settings = ProjectSettings::default();
	loop {
		let mut file = path.clone();
		file.push("nml.toml");

		if file.exists() && file.is_file() {
			let content = String::from_utf8(
				read(&file).expect(format!("Failed to read {}", file.display()).as_str()),
			)
			.expect(format!("Project file {} contains invalid UTF-8", file.display()).as_str());
			match toml::from_str::<ProjectSettings>(content.as_str()) {
				Ok(r) => settings = r,
				Err(err) => {
					eprintln!("Failed to parse {}: {err}", file.display());
					return Ok(());
				}
			}
			break;
		}
		let Some(parent) = path.parent() else { break };
		path = parent.into();
	}

	let stdin = tokio::io::stdin();
	let stdout = tokio::io::stdout();
	let (service, socket) =
		LspService::build(|client| Backend::new(client, settings, path))
			//let (service, socket) = LspService::build(|client| Backend::new(client, settings, path))
			.custom_method("textDocument/conceal", Backend::handle_conceal_request)
			.custom_method("textDocument/style", Backend::handle_style_request)
			.custom_method("textDocument/codeRange", Backend::handle_coderange_request)
			.finish();

	Server::new(stdin, stdout, socket).serve(service).await;
	Ok(())
}
