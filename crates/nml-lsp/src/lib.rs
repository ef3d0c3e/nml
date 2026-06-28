//! The NML language server.

mod convert;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use lsp_server::Connection;
use lsp_server::Message;
use lsp_server::Notification;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidCloseTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::InitializeParams;
use lsp_types::PublishDiagnosticsParams;
use lsp_types::SaveOptions;
use lsp_types::ServerCapabilities;
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
use nml_build::BuildOptions;
use nml_build::WorkspaceResolver;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::Localizer;
use nml_lang::Language;
use nml_source::Source;
use nml_source::Url;
use nml_workspace::CONFIG_FILE;
use nml_workspace::Config;

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
		..ServerCapabilities::default()
	};
	let params = connection.initialize(serde_json::to_value(capabilities)?)?;
	let params: InitializeParams = serde_json::from_value(params)?;
	let mut server = Server::new(connection, root(&params));
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
}

impl Server {
	fn new(connection: Connection, root: Option<PathBuf>) -> Self {
		let root = root.and_then(|root| root.canonicalize().ok());
		let config = root.as_deref().and_then(|root| {
			let path = root.join(CONFIG_FILE);
			let content = std::fs::read_to_string(&path).ok()?;
			let url = Url::from_file_path(&path).ok()?;
			Config::parse(Arc::new(Source::new(url, content, 0))).ok()
		});
		Self {
			connection,
			root,
			config,
			language: nml_build::language(),
			localizer: Localizer::from_env(),
			buffers: HashMap::new(),
			built: BTreeSet::new(),
		}
	}

	fn run(&mut self) -> Result<()> {
		while let Ok(message) = self.connection.receiver.recv() {
			match message {
				Message::Request(request) => {
					if self.connection.handle_shutdown(&request)? {
						return Ok(());
					}
				}
				Message::Notification(notification) => self.notification(notification)?,
				Message::Response(_) => {}
			}
		}
		Ok(())
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
			}
			DidSaveTextDocument::METHOD => self.build()?,
			_ => {}
		}
		Ok(())
	}

	/// The path of a file in the workspace, with `/` separators.
	fn workspace_path(&self, url: &Url) -> Option<String> {
		let path = url.to_file_path().ok()?;
		let relative = path.strip_prefix(self.root.as_ref()?).ok()?;
		let parts: Vec<_> = relative
			.components()
			.map(|c| c.as_os_str().to_string_lossy())
			.collect();
		Some(parts.join("/"))
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
		let resolver = WorkspaceResolver::new(root, modules).with_overlay(overlay);
		let source = Arc::new(Source::new(url.clone(), text.clone(), 0));
		let compiled = nml_lang::compile(&self.language, source, &path, Arc::new(resolver));

		// Diagnostics located in other files (an included file) are reported with them.
		let diagnostics = compiled
			.diagnostics
			.iter()
			.filter(|diagnostic| convert::file(diagnostic) == *url)
			.map(|diagnostic| convert::diagnostic(diagnostic, &self.localizer))
			.collect();
		self.publish(url, diagnostics)
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
		for url in previous.difference(&by_file.keys().cloned().collect()) {
			self.publish(url, Vec::new())?;
		}
		for (url, diagnostics) in by_file {
			let diagnostics = diagnostics
				.into_iter()
				.map(|diagnostic| convert::diagnostic(diagnostic, &self.localizer))
				.collect();
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
