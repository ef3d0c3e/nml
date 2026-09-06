//! The NML language server.

mod actions;
mod completion;
mod convert;
mod display;
mod find;
mod lua;
mod luals;
mod navigate;
mod proxy;
mod semantic;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use lsp_server::Connection;
use lsp_server::Message;
use lsp_server::Notification;
use lsp_server::Response;
use lsp_types::CodeActionParams;
use lsp_types::CodeActionProviderCapability;
use lsp_types::CompletionOptions;
use lsp_types::CompletionParams;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::ExecuteCommandOptions;
use lsp_types::ExecuteCommandParams;
use lsp_types::GotoDefinitionParams;
use lsp_types::GotoDefinitionResponse;
use lsp_types::HoverParams;
use lsp_types::HoverProviderCapability;
use lsp_types::InitializeParams;
use lsp_types::OneOf;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::ReferenceParams;
use lsp_types::SaveOptions;
use lsp_types::SemanticTokens;
use lsp_types::SemanticTokensFullOptions;
use lsp_types::SemanticTokensOptions;
use lsp_types::SemanticTokensParams;
use lsp_types::SemanticTokensServerCapabilities;
use lsp_types::ServerCapabilities;
use lsp_types::SignatureHelpOptions;
use lsp_types::TextDocumentSyncCapability;
use lsp_types::TextDocumentSyncKind;
use lsp_types::TextDocumentSyncOptions;
use lsp_types::TextDocumentSyncSaveOptions;
use lsp_types::Uri;
use lsp_types::notification::DidChangeTextDocument;
use lsp_types::notification::DidCloseTextDocument;
use lsp_types::notification::DidOpenTextDocument;
use lsp_types::notification::DidSaveTextDocument;
use lsp_types::notification::Notification as _;
use lsp_types::notification::PublishDiagnostics;
use lsp_types::request::CodeActionRequest;
use lsp_types::request::Completion;
use lsp_types::request::DocumentHighlightRequest;
use lsp_types::request::DocumentSymbolRequest;
use lsp_types::request::ExecuteCommand;
use lsp_types::request::GotoDefinition;
use lsp_types::request::HoverRequest;
use lsp_types::request::InlayHintRequest;
use lsp_types::request::References;
use lsp_types::request::Rename;
use lsp_types::request::Request as _;
use lsp_types::request::SemanticTokensFullRequest;
use lsp_types::request::SignatureHelpRequest;
use nml_build::BuildOptions;
use nml_build::WorkspaceResolver;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::Localizer;
use nml_document::Dependency;
use nml_lang::Language;
use nml_source::Source;
use nml_source::Url;
use nml_workspace::CONFIG_FILE;
use nml_workspace::Config;

/// Analyses an included file in another includer: arguments are the file's URI and the
/// includer's workspace path.
const INCLUDE_CONTEXT_COMMAND: &str = "nml.includeContext";

/// The includers of a file: `{ current, candidates }`.
const INCLUDE_CONTEXTS: &str = "nml/includeContexts";

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

/// Serves the language server protocol on stdin and stdout.
pub fn run_stdio() -> Result<()> {
	let (connection, threads) = Connection::stdio();
	run(connection)?;
	threads.join()?;
	Ok(())
}

/// Serves the language server protocol on a connection, until the client exits.
pub fn run(connection: Connection) -> Result<()> {
	let capabilities = ServerCapabilities {
		text_document_sync: Some(TextDocumentSyncCapability::Options(
			TextDocumentSyncOptions {
				open_close: Some(true),
				change: Some(TextDocumentSyncKind::FULL),
				save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
					include_text: Some(false),
				})),
				..TextDocumentSyncOptions::default()
			},
		)),
		completion_provider: Some(CompletionOptions {
			trigger_characters: Some(vec![
				"[".into(),
				"#".into(),
				":".into(),
				".".into(),
				",".into(),
				"=".into(),
			]),
			..CompletionOptions::default()
		}),
		inlay_hint_provider: Some(OneOf::Left(true)),
		execute_command_provider: Some(ExecuteCommandOptions {
			commands: vec![INCLUDE_CONTEXT_COMMAND.into()],
			..ExecuteCommandOptions::default()
		}),
		signature_help_provider: Some(SignatureHelpOptions {
			trigger_characters: Some(vec!["(".into(), ",".into()]),
			..SignatureHelpOptions::default()
		}),
		code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
		hover_provider: Some(HoverProviderCapability::Simple(true)),
		definition_provider: Some(OneOf::Left(true)),
		references_provider: Some(OneOf::Left(true)),
		// For Lua, from LuaLS.
		rename_provider: Some(OneOf::Left(true)),
		document_highlight_provider: Some(OneOf::Left(true)),
		document_symbol_provider: Some(OneOf::Left(true)),
		semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
			SemanticTokensOptions {
				legend: semantic::legend(),
				full: Some(SemanticTokensFullOptions::Bool(true)),
				..SemanticTokensOptions::default()
			},
		)),
		..ServerCapabilities::default()
	};
	let params = connection.initialize(serde_json::to_value(capabilities)?)?;
	let params: InitializeParams = serde_json::from_value(params)?;
	let luals = match params
		.initialization_options
		.as_ref()
		.and_then(|options| options.get("luals"))
	{
		Some(serde_json::Value::Bool(false)) => None,
		Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
		_ => luals::find(),
	};
	let mut server = Server::new(connection, root(&params), luals);
	server.snippets = (params.capabilities.text_document.as_ref())
		.and_then(|document| document.completion.as_ref())
		.and_then(|completion| completion.completion_item.as_ref())
		.and_then(|item| item.snippet_support)
		.unwrap_or(false);
	server.run()
}

/// The workspace root: the first workspace folder, or the deprecated root URI.
#[allow(deprecated)]
fn root(params: &InitializeParams) -> Option<PathBuf> {
	let uri = params
		.workspace_folders
		.as_ref()
		.and_then(|folders| folders.first())
		.map(|folder| &folder.uri)
		.or(params.root_uri.as_ref())?;
	Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

struct Server {
	connection: Connection,
	/// The workspace root, if the client has one with an `nml.toml`.
	root: Option<PathBuf>,
	config: Option<Config>,
	language: Language,
	localizer: Localizer,
	/// The open documents: their URL and content.
	buffers: HashMap<Url, String>,
	/// The files with diagnostics from the last build, to clear them when they are fixed.
	built: BTreeSet<Url>,
	/// The last analysis of each open document.
	analyses: HashMap<Url, Analysis>,
	/// The includer chosen for an included file, by workspace path.
	contexts: HashMap<String, String>,
	luals: Option<luals::LuaLs>,
	/// The LuaLS program, if there is one.
	luals_program: Option<PathBuf>,
	/// Whether LuaLS was started again after it stopped.
	luals_restarted: bool,
	/// The requests forwarded to LuaLS, by the id LuaLS has for them.
	forwarded: HashMap<lsp_server::RequestId, proxy::Forwarded>,
	/// The virtual Lua document of each open document.
	lua_documents: HashMap<Url, lua::LuaDocument>,
	/// The diagnostics published for each document, NML's and LuaLS's.
	nml_diagnostics: HashMap<Url, Vec<lsp_types::Diagnostic>>,
	lua_diagnostics: HashMap<Url, Vec<lsp_types::Diagnostic>>,
	/// Whether the client takes snippets in completions.
	snippets: bool,
}

/// What the server knows of an open document, from its last compilation.
struct Analysis {
	source: Arc<Source>,
	/// The document's path in the workspace.
	path: String,
	/// Its reference key: `nml.reference_key`, or its basename.
	key: String,
	tree: Vec<nml_grammar::Node>,
	document: nml_document::Document,
	exports: Vec<nml_document::Export>,
	/// The diagnostics of its last compilation, for code actions.
	diagnostics: Vec<Diagnostic>,
	/// The document's grammar, if its prologue extended the base one.
	language: Option<Arc<Language>>,
	/// The names the Lua of each imported module exports, by module file.
	module_exports: std::collections::BTreeMap<String, Vec<String>>,
	/// The variables it sets and imports.
	variables: HashMap<String, nml_lang::Variable>,
	/// The includer it was analysed in, for an included file.
	context: Option<IncludeContext>,
}

/// An includer an included file is analysed in.
struct IncludeContext {
	url: Url,
	/// Its path in the workspace.
	path: String,
	tree: Vec<nml_grammar::Node>,
	module_exports: std::collections::BTreeMap<String, Vec<String>>,
}

impl Server {
	fn new(connection: Connection, root: Option<PathBuf>, luals: Option<PathBuf>) -> Self {
		let root = root.and_then(|root| root.canonicalize().ok());
		let config = root.as_deref().and_then(|root| {
			let path = root.join(CONFIG_FILE);
			let content = std::fs::read_to_string(&path).ok()?;
			let url = Url::from_file_path(&path).ok()?;
			Config::parse(Arc::new(Source::new(url, content, 0))).ok()
		});
		let mut server = Self {
			connection,
			root,
			config,
			language: nml_build::language(),
			localizer: Localizer::from_env(),
			buffers: HashMap::new(),
			built: BTreeSet::new(),
			analyses: HashMap::new(),
			contexts: HashMap::new(),
			luals: None,
			luals_program: luals,
			luals_restarted: false,
			forwarded: HashMap::new(),
			lua_documents: HashMap::new(),
			nml_diagnostics: HashMap::new(),
			lua_diagnostics: HashMap::new(),
			snippets: false,
		};
		server.start_luals();
		server
	}

	/// Starts LuaLS, if there is one and a workspace. Without it, Lua in documents gets
	/// NML's diagnostics only. The client is told if it fails to start.
	fn start_luals(&mut self) {
		let (Some(program), Some(root)) = (&self.luals_program, &self.root) else {
			return;
		};
		match luals::LuaLs::start(program, root) {
			Ok(luals) => self.luals = Some(luals),
			Err(error) => {
				let message = format!("LuaLS ({}) did not start: {error}", program.display());
				self.show_message(lsp_types::MessageType::WARNING, message);
			}
		}
	}

	/// LuaLS stopped: the requests it had are answered with no result, and it is started
	/// again with the open documents, once. The client is told if it stops again.
	fn luals_stopped(&mut self) -> Result<()> {
		self.luals = None;
		self.drop_forwarded()?;
		self.lua_diagnostics.clear();
		if self.luals_restarted {
			let message = "LuaLS stopped: Lua in documents gets NML's diagnostics only".into();
			self.show_message(lsp_types::MessageType::WARNING, message);
			return Ok(());
		}
		self.luals_restarted = true;
		self.start_luals();
		if let Some(luals) = &mut self.luals {
			for (url, document) in &self.lua_documents {
				let _ = luals.sync(url, &document.content);
			}
		}
		Ok(())
	}

	fn show_message(&self, typ: lsp_types::MessageType, message: String) {
		let params = lsp_types::ShowMessageParams { typ, message };
		let notification = Notification::new("window/showMessage".into(), params);
		let _ = self.connection.sender.send(notification.into());
	}

	fn run(&mut self) -> Result<()> {
		loop {
			let lua = match &self.luals {
				Some(luals) => luals.receiver.clone(),
				None => crossbeam_channel::never(),
			};
			let message = crossbeam_channel::select! {
				recv(self.connection.receiver) -> message => match message {
					Ok(message) => message,
					Err(_) => return Ok(()),
				},
				recv(lua) -> message => {
					match message {
						Ok(message) => self.lua_message(message)?,
						Err(_) => self.luals_stopped()?,
					}
					continue;
				}
			};
			match message {
				Message::Request(request) => {
					if self.connection.handle_shutdown(&request)? {
						return Ok(());
					}
					// Answered when LuaLS answers.
					if self.forward(&request)? {
						continue;
					}
					let id = request.id.clone();
					let response = match self.request(request) {
						Ok(result) => Response::new_ok(id, result),
						Err(error) => Response::new_err(
							id,
							lsp_server::ErrorCode::InternalError as i32,
							error.to_string(),
						),
					};
					self.connection.sender.send(response.into())?;
				}
				Message::Notification(notification) => self.notification(notification)?,
				Message::Response(_) => {}
			}
		}
	}

	fn request(&mut self, request: lsp_server::Request) -> Result<serde_json::Value> {
		match request.method.as_str() {
			// Only Lua has these, from LuaLS.
			SignatureHelpRequest::METHOD
			| Rename::METHOD
			| DocumentHighlightRequest::METHOD
			| DocumentSymbolRequest::METHOD => Ok(serde_json::Value::Null),
			SemanticTokensFullRequest::METHOD => {
				let params: SemanticTokensParams = serde_json::from_value(request.params)?;
				let url = Url::parse(params.text_document.uri.as_str())?;
				let data = match self.analyses.get(&url) {
					Some(analysis) => semantic::tokens(&analysis.tree, &url),
					None => Vec::new(),
				};
				let tokens = SemanticTokens {
					result_id: None,
					data: semantic::encode(&data),
				};
				Ok(serde_json::to_value(tokens)?)
			}
			HoverRequest::METHOD => {
				let params: HoverParams = serde_json::from_value(request.params)?;
				let position = params.text_document_position_params;
				Ok(serde_json::to_value(self.hover(&position)?)?)
			}
			GotoDefinition::METHOD => {
				let params: GotoDefinitionParams = serde_json::from_value(request.params)?;
				let position = params.text_document_position_params;
				let location = self.definition(&position)?;
				Ok(serde_json::to_value(
					location.map(GotoDefinitionResponse::Scalar),
				)?)
			}
			References::METHOD => {
				let params: ReferenceParams = serde_json::from_value(request.params)?;
				let declaration = params.context.include_declaration;
				Ok(serde_json::to_value(self.references(
					&params.text_document_position,
					declaration,
				)?)?)
			}
			Completion::METHOD => {
				let params: CompletionParams = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.completion(&params)?)?)
			}
			InlayHintRequest::METHOD => {
				let params: lsp_types::InlayHintParams = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.inlay_hints(&params)?)?)
			}
			display::CONCEAL => {
				let params = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.conceal(params)?)?)
			}
			display::STYLE => {
				let params = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.style(params)?)?)
			}
			lua::CODE_RANGE => {
				let params = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.code_ranges(params)?)?)
			}
			lua::LUA_DOCUMENT => {
				let params = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.lua_document(params)?)?)
			}
			INCLUDE_CONTEXTS => {
				let params: lua::LuaParams = serde_json::from_value(request.params)?;
				let url = Url::parse(params.text_document.uri.as_str())?;
				let path = self.workspace_path(&url).unwrap_or_default();
				Ok(serde_json::json!({
					"current": self.include_context(&path),
					"candidates": self.includers(&path),
				}))
			}
			ExecuteCommand::METHOD => {
				let params: ExecuteCommandParams = serde_json::from_value(request.params)?;
				if params.command != INCLUDE_CONTEXT_COMMAND {
					return Err(format!("unknown command `{}`", params.command).into());
				}
				let [file, includer] = params.arguments.as_slice() else {
					return Err(
						"`nml.includeContext` takes a document URI and an includer path".into(),
					);
				};
				let (Some(file), Some(includer)) = (file.as_str(), includer.as_str()) else {
					return Err("`nml.includeContext` takes two strings".into());
				};
				let url = Url::parse(file)?;
				let path = self.workspace_path(&url).unwrap_or_default();
				self.contexts.insert(path, includer.to_string());
				self.diagnose(&url)?;
				Ok(serde_json::Value::Null)
			}
			CodeActionRequest::METHOD => {
				let params: CodeActionParams = serde_json::from_value(request.params)?;
				Ok(serde_json::to_value(self.code_actions(&params)?)?)
			}
			method => Err(format!("unsupported request `{method}`").into()),
		}
	}

	fn notification(&mut self, notification: Notification) -> Result<()> {
		match notification.method.as_str() {
			DidOpenTextDocument::METHOD => {
				let params: DidOpenTextDocumentParams =
					serde_json::from_value(notification.params)?;
				let url = Url::parse(params.text_document.uri.as_str())?;
				self.buffers.insert(url.clone(), params.text_document.text);
				self.diagnose(&url)?;
			}
			DidChangeTextDocument::METHOD => {
				let params: DidChangeTextDocumentParams =
					serde_json::from_value(notification.params)?;
				let url = Url::parse(params.text_document.uri.as_str())?;
				// Full synchronization: the last change is the whole document.
				if let Some(change) = params.content_changes.into_iter().last() {
					self.buffers.insert(url.clone(), change.text);
				}
				self.diagnose(&url)?;
			}
			DidCloseTextDocument::METHOD => {
				let params: DidCloseTextDocumentParams =
					serde_json::from_value(notification.params)?;
				let url = Url::parse(params.text_document.uri.as_str())?;
				self.buffers.remove(&url);
				self.analyses.remove(&url);
				self.lua_documents.remove(&url);
				self.lua_diagnostics.remove(&url);
				// A LuaLS that stopped is noticed through its messages.
				if let Some(luals) = &mut self.luals {
					let _ = luals.close(&url);
				}
			}
			DidSaveTextDocument::METHOD => self.build()?,
			_ => {}
		}
		Ok(())
	}

	/// The path of a file in the workspace, with `/` separators.
	pub(crate) fn workspace_path(&self, url: &Url) -> Option<String> {
		let path = url.to_file_path().ok()?;
		let relative = path.strip_prefix(self.root.as_ref()?).ok()?;
		let parts: Vec<_> = relative
			.components()
			.map(|c| c.as_os_str().to_string_lossy())
			.collect();
		Some(parts.join("/"))
	}

	/// Finds the workspace's files and modules, with the open documents' content.
	fn resolver(&self) -> WorkspaceResolver {
		let overlay = self
			.buffers
			.iter()
			.filter_map(|(url, text)| Some((self.workspace_path(url)?, text.clone())))
			.collect();
		let root = self.root.clone().unwrap_or_else(|| PathBuf::from("/"));
		let modules = self
			.config
			.as_ref()
			.map(|config| config.modules.clone())
			.unwrap_or_default();
		WorkspaceResolver::new(root, modules).with_overlay(overlay)
	}

	/// Compiles an open document from its buffer, with the other open documents as an
	/// overlay, and publishes its diagnostics. The database is not written.
	fn diagnose(&mut self, url: &Url) -> Result<()> {
		let Some(text) = self.buffers.get(url) else {
			return Ok(());
		};
		let path = self.workspace_path(url).unwrap_or_else(|| {
			url.path_segments()
				.and_then(|mut segments| segments.next_back())
				.unwrap_or("document.nml")
				.to_string()
		});
		let resolver = self.resolver();
		let source = Arc::new(Source::new(url.clone(), text.clone(), 0));
		let mut compiled = nml_lang::compile(
			&self.language,
			source.clone(),
			&path,
			Arc::new(resolver.clone()),
		);

		// An included file is analysed as part of an includer: it sees its grammar and
		// context, and its diagnostics are those of that includer, in this file.
		let mut include_context = None;
		if let Some(includer) = self.include_context(&path)
			&& let Some(includer_source) = self.file(&includer)
		{
			let context = nml_lang::compile(
				&self.language,
				includer_source.clone(),
				&includer,
				Arc::new(resolver),
			);
			if let Some(language) = &context.language {
				compiled.tree = nml_grammar::parse(language.rules(), source.clone()).nodes;
				compiled.language = Some(language.clone());
			}
			compiled.diagnostics = context.diagnostics;
			include_context = Some(IncludeContext {
				url: includer_source.location().clone(),
				path: includer,
				tree: context.tree,
				module_exports: context.module_exports,
			});
		}
		let basename = path.rsplit('/').next().unwrap_or(&path);
		let key = compiled.unit.reference_key.clone().unwrap_or_else(|| {
			basename
				.strip_suffix(".nml")
				.unwrap_or(basename)
				.to_string()
		});
		self.analyses.insert(
			url.clone(),
			Analysis {
				source,
				path: path.clone(),
				key,
				tree: compiled.tree.clone(),
				document: compiled.unit.document.clone(),
				exports: compiled.unit.exports.clone(),
				diagnostics: compiled.diagnostics.clone(),
				language: compiled.language.clone(),
				module_exports: compiled.module_exports.clone(),
				variables: compiled.variables.clone(),
				context: include_context,
			},
		);

		// The Lua of the document, for LuaLS.
		let lua = self.lua_document_of(url);
		if let Some(luals) = &mut self.luals {
			let _ = luals.sync(url, &lua.content);
		}
		self.lua_documents.insert(url.clone(), lua);

		// Diagnostics located in other files (an included file) are reported with them.
		let diagnostics = compiled
			.diagnostics
			.iter()
			.filter(|diagnostic| convert::file(diagnostic) == *url)
			.map(|diagnostic| convert::diagnostic(diagnostic, &self.localizer))
			.collect();
		self.nml_diagnostics.insert(url.clone(), diagnostics);
		self.publish_document(url)
	}

	/// Publishes the diagnostics of a document: NML's, and LuaLS's for its Lua.
	fn publish_document(&self, url: &Url) -> Result<()> {
		let mut diagnostics = self.nml_diagnostics.get(url).cloned().unwrap_or_default();
		diagnostics.extend(self.lua_diagnostics.get(url).cloned().unwrap_or_default());
		self.publish(url, diagnostics)
	}

	/// The files that include the file at `path`, from the index.
	fn includers(&self, path: &str) -> Vec<String> {
		self.index()
			.and_then(|index| index.dependents(&Dependency::Include(path.into())).ok())
			.unwrap_or_default()
	}

	/// The includer a file is analysed in: the one chosen for it, or the first that includes
	/// it. Pages, and files that nothing includes, are analysed on their own.
	fn include_context(&self, path: &str) -> Option<String> {
		if self.config.as_ref()?.is_page(std::path::Path::new(path)) {
			return None;
		}
		match self.contexts.get(path) {
			Some(chosen) => Some(chosen.clone()),
			None => self.includers(path).into_iter().next(),
		}
	}

	/// Runs the build stages without writing outputs, and publishes the diagnostics of every
	/// file. Files that had diagnostics and have none now are cleared.
	fn build(&mut self) -> Result<()> {
		let Some(root) = self.root.clone() else {
			return Ok(());
		};
		let report = match nml_build::build_with(&root, BuildOptions { write: false }) {
			Ok(report) => report,
			// No workspace: open documents are still diagnosed on their own.
			Err(_) => return Ok(()),
		};
		let mut by_file: HashMap<Url, Vec<&Diagnostic>> = HashMap::new();
		for diagnostic in &report.diagnostics {
			by_file
				.entry(convert::file(diagnostic))
				.or_default()
				.push(diagnostic);
		}
		let previous = std::mem::take(&mut self.built);
		// LuaLS's diagnostics of open documents are kept.
		let lua = |url: &Url| self.lua_diagnostics.get(url).cloned().unwrap_or_default();
		for url in previous.difference(&by_file.keys().cloned().collect()) {
			self.publish(url, lua(url))?;
		}
		for (url, diagnostics) in by_file {
			let mut diagnostics: Vec<_> = diagnostics
				.into_iter()
				.map(|diagnostic| convert::diagnostic(diagnostic, &self.localizer))
				.collect();
			diagnostics.extend(lua(&url));
			self.publish(&url, diagnostics)?;
			self.built.insert(url);
		}
		Ok(())
	}

	fn publish(&self, url: &Url, diagnostics: Vec<lsp_types::Diagnostic>) -> Result<()> {
		let params = PublishDiagnosticsParams {
			uri: Uri::from_str(url.as_str())?,
			diagnostics,
			version: None,
		};
		let notification = Notification::new(PublishDiagnostics::METHOD.into(), params);
		self.connection.sender.send(notification.into())?;
		Ok(())
	}
}

#[cfg(test)]
mod tests;
