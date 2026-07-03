use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::Request;
use lsp_server::RequestId;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::DidOpenTextDocumentParams;
use lsp_types::DidSaveTextDocumentParams;
use lsp_types::NumberOrString;
use lsp_types::TextDocumentContentChangeEvent;
use lsp_types::TextDocumentIdentifier;
use lsp_types::TextDocumentItem;
use lsp_types::VersionedTextDocumentIdentifier;
use lsp_types::WorkspaceFolder;
use lsp_types::notification::Exit;
use lsp_types::notification::Initialized;

use super::*;

/// A client talking to a server in a thread.
struct Client {
	connection: Connection,
	server: Option<JoinHandle<()>>,
}

impl Client {
	fn start(root: Option<&Path>) -> Self {
		let (server, connection) = Connection::memory();
		let server = std::thread::spawn(move || run(server).unwrap());
		let client = Self {
			connection,
			server: Some(server),
		};
		let params = InitializeParams {
			workspace_folders: root.map(|root| {
				vec![WorkspaceFolder {
					uri: Uri::from_str(Url::from_file_path(root).unwrap().as_str()).unwrap(),
					name: "test".into(),
				}]
			}),
			..InitializeParams::default()
		};
		client.request("initialize", serde_json::to_value(params).unwrap());
		client.notify::<Initialized>(serde_json::json!({}));
		client
	}

	fn request(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
		let id = RequestId::from(1);
		let request = Request::new(id.clone(), method.into(), params);
		self.connection.sender.send(request.into()).unwrap();
		loop {
			match self.receive() {
				Message::Response(response) if response.id == id => {
					return response.response_result.expect("the request succeeds");
				}
				_ => {}
			}
		}
	}

	fn notify<N: lsp_types::notification::Notification>(&self, params: impl serde::Serialize) {
		let notification = Notification::new(N::METHOD.into(), params);
		self.connection.sender.send(notification.into()).unwrap();
	}

	fn receive(&self) -> Message {
		self.connection
			.receiver
			.recv_timeout(Duration::from_secs(10))
			.expect("the server answers")
	}

	/// The next diagnostics the server publishes.
	fn diagnostics(&self) -> PublishDiagnosticsParams {
		loop {
			if let Message::Notification(notification) = self.receive()
				&& notification.method == PublishDiagnostics::METHOD
			{
				return serde_json::from_value(notification.params).unwrap();
			}
		}
	}

	fn open(&self, url: &Url, text: &str) {
		self.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
			text_document: TextDocumentItem::new(uri(url), "nml".into(), 0, text.into()),
		});
	}

	fn change(&self, url: &Url, text: &str) {
		self.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
			text_document: VersionedTextDocumentIdentifier::new(uri(url), 1),
			content_changes: vec![TextDocumentContentChangeEvent {
				range: None,
				range_length: None,
				text: text.into(),
			}],
		});
	}

	fn save(&self, url: &Url) {
		self.notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
			text_document: TextDocumentIdentifier::new(uri(url)),
			text: None,
		});
	}
}

impl Drop for Client {
	fn drop(&mut self) {
		self.request("shutdown", serde_json::Value::Null);
		self.notify::<Exit>(serde_json::Value::Null);
		self.server.take().unwrap().join().unwrap();
	}
}

fn uri(url: &Url) -> Uri {
	Uri::from_str(url.as_str()).unwrap()
}

/// A temporary workspace, removed when dropped.
struct Workspace(PathBuf);

impl Drop for Workspace {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Workspace {
	fn new(name: &str, files: &[(&str, &str)]) -> Self {
		let dir = std::env::temp_dir().join(format!("nml-lsp-{name}-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let dir = dir.canonicalize().unwrap();
		for (path, content) in files {
			let path = dir.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, content).unwrap();
		}
		Self(dir)
	}

	fn url(&self, path: &str) -> Url {
		Url::from_file_path(self.0.join(path)).unwrap()
	}
}

fn position(line: u32, character: u32) -> lsp_types::Position {
	lsp_types::Position::new(line, character)
}

#[test]
fn diagnostics_follow_the_buffer() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(&url, "é **bold\n");
	let published = client.diagnostics();
	assert_eq!(published.uri.as_str(), url.as_str());
	let [diagnostic] = published.diagnostics.as_slice() else {
		panic!("{:?}", published.diagnostics)
	};
	assert_eq!(
		diagnostic.code,
		Some(NumberOrString::String("NML1001".into()))
	);
	// `é` is one UTF-16 unit: the opener is at columns 2 to 4.
	assert_eq!(diagnostic.range.start, position(0, 2));
	assert_eq!(diagnostic.range.end, position(0, 4));
	assert_eq!(diagnostic.message, "expected `**`: bold opened here");
	let related = diagnostic.related_information.as_ref().unwrap();
	assert_eq!(related[0].message, "ended here without `**`");
	assert_eq!(related[0].location.range.start, position(0, 8));

	client.change(&url, "é **bold**\n");
	assert!(client.diagnostics().diagnostics.is_empty());
}

#[test]
fn diagnostics_in_lua_map_to_the_file() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(&url, ":lua\n```\nlocal x = 1\nlocal y = nil + x\n```\n");
	let published = client.diagnostics();
	let diagnostic = &published.diagnostics[0];
	assert_eq!(
		diagnostic.code,
		Some(NumberOrString::String("NML4001".into()))
	);
	assert_eq!(diagnostic.range.start, position(3, 0));
	assert_eq!(diagnostic.range.end, position(3, 17));
}

#[test]
fn unsaved_buffers_are_an_overlay() {
	let workspace = Workspace::new(
		"overlay",
		&[
			("nml.toml", ""),
			("data.txt", "on disk\n"),
			(
				"a.nml",
				":lua`assert(nml.read('data.txt') == 'on disk\\n', 'changed') and nil`\n",
			),
		],
	);
	let client = Client::start(Some(&workspace.0));
	client.open(
		&workspace.url("a.nml"),
		&fs::read_to_string(workspace.0.join("a.nml")).unwrap(),
	);
	assert!(client.diagnostics().diagnostics.is_empty());

	// The data file is open and edited, but not saved.
	client.open(&workspace.url("data.txt"), "in memory\n");
	client.diagnostics();
	client.change(
		&workspace.url("a.nml"),
		&fs::read_to_string(workspace.0.join("a.nml")).unwrap(),
	);
	let published = client.diagnostics();
	assert_eq!(published.diagnostics[0].message, "changed");
}

#[test]
fn saving_builds_the_workspace() {
	let workspace = Workspace::new(
		"save",
		&[
			("nml.toml", ""),
			("a.nml", "[[#missing]]\n"),
			("b.nml", "# B\n"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	client.save(&workspace.url("a.nml"));
	let published = client.diagnostics();
	assert_eq!(published.uri.as_str(), workspace.url("a.nml").as_str());
	assert_eq!(
		published.diagnostics[0].code,
		Some(NumberOrString::String("NML3001".into()))
	);
	// The server builds without writing pages, but keeps the index.
	assert!(!workspace.0.join("public").exists());
	assert!(workspace.0.join(".nml/index.db").exists());

	// Fixed in another file: the diagnostics of `a` are cleared.
	fs::write(workspace.0.join("b.nml"), "# B {#missing}\n").unwrap();
	client.save(&workspace.url("b.nml"));
	let published = client.diagnostics();
	assert_eq!(published.uri.as_str(), workspace.url("a.nml").as_str());
	assert!(published.diagnostics.is_empty());
}

/// Semantic tokens as (line, column, length, type name).
fn decode(tokens: &[lsp_types::SemanticToken]) -> Vec<(u32, u32, u32, String)> {
	let legend = semantic::legend();
	let mut out = Vec::new();
	let (mut line, mut column) = (0, 0);
	for token in tokens {
		line += token.delta_line;
		column = if token.delta_line == 0 {
			column + token.delta_start
		} else {
			token.delta_start
		};
		let kind = legend.token_types[token.token_type as usize]
			.as_str()
			.to_string();
		out.push((line, column, token.length, kind));
	}
	out
}

#[test]
fn semantic_tokens() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(&url, "## Title {#top}\n> é **b**\n:lua`x = 1`\n");
	client.diagnostics();
	let result = client.request(
		SemanticTokensFullRequest::METHOD,
		serde_json::to_value(SemanticTokensParams {
			text_document: TextDocumentIdentifier::new(uri(&url)),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
		})
		.unwrap(),
	);
	let tokens: SemanticTokens = serde_json::from_value(result).unwrap();
	assert_eq!(
		decode(&tokens.data),
		[
			(0, 0, 2, "operator".into()),
			(0, 11, 3, "variable".into()),
			// After `> é `: `é` is one UTF-16 unit.
			(1, 4, 2, "operator".into()),
			(1, 7, 2, "operator".into()),
			(2, 0, 4, "keyword".into()),
			(2, 4, 1, "operator".into()),
		]
	);
}

fn at(url: &Url, line: u32, character: u32) -> lsp_types::TextDocumentPositionParams {
	lsp_types::TextDocumentPositionParams::new(
		TextDocumentIdentifier::new(uri(url)),
		position(line, character),
	)
}

/// A workspace for navigation; `name` keeps tests that run in parallel apart.
fn navigation_workspace(name: &str) -> Workspace {
	Workspace::new(
		name,
		&[
			(
				"nml.toml",
				"exclude = [\"parts/**\", \"modules/**\"]\n[[module]]\nname = \"std\"\nroot = \"modules/std\"\n",
			),
			(
				"index.nml",
				":import std.util\n# Home {#top}\nSee [[guide#install]], [[top]] and [[guide#]].\n:include parts/x\n",
			),
			(
				"docs/guide.nml",
				"# Guide\n## Install {#install}\nBack to [[index#top]].\n",
			),
			("other.nml", "Also [[#install]].\n"),
			("parts/x.nml", "x\n"),
			("modules/std/util.nml", "\n"),
		],
	)
}

#[test]
fn hover_and_definition() {
	let workspace = navigation_workspace("hover");
	let client = Client::start(Some(&workspace.0));
	let index = workspace.url("index.nml");
	// The server handles messages in order: the build is done before the next request.
	client.save(&index);
	client.open(
		&index,
		&fs::read_to_string(workspace.0.join("index.nml")).unwrap(),
	);
	client.diagnostics();

	let hover = |line, character| -> Option<lsp_types::Hover> {
		let result = client.request(
			HoverRequest::METHOD,
			serde_json::to_value(lsp_types::HoverParams {
				text_document_position_params: at(&index, line, character),
				work_done_progress_params: Default::default(),
			})
			.unwrap(),
		);
		serde_json::from_value(result).unwrap()
	};
	let text = |hover: lsp_types::Hover| match hover.contents {
		lsp_types::HoverContents::Markup(markup) => markup.value,
		_ => unreachable!(),
	};
	assert_eq!(
		text(hover(2, 8).unwrap()),
		"**Install**\n\n`install` in `docs/guide.nml`"
	);
	assert_eq!(
		text(hover(2, 26).unwrap()),
		"**Home**\n\n`top` in `index.nml`"
	);
	assert_eq!(text(hover(2, 38).unwrap()), "**Guide**\n\n`docs/guide.nml`");
	assert!(hover(1, 2).is_none());

	let definition = |line, character| -> Option<lsp_types::Location> {
		let result = client.request(
			GotoDefinition::METHOD,
			serde_json::to_value(lsp_types::GotoDefinitionParams {
				text_document_position_params: at(&index, line, character),
				work_done_progress_params: Default::default(),
				partial_result_params: Default::default(),
			})
			.unwrap(),
		);
		serde_json::from_value(result).unwrap()
	};
	let place = |location: lsp_types::Location| {
		(
			location.uri.as_str().to_string(),
			location.range.start,
			location.range.end,
		)
	};
	assert_eq!(
		place(definition(2, 8).unwrap()),
		(
			workspace.url("docs/guide.nml").to_string(),
			position(1, 13),
			position(1, 20)
		)
	);
	assert_eq!(
		place(definition(2, 26).unwrap()),
		(index.to_string(), position(1, 9), position(1, 12))
	);
	assert_eq!(
		place(definition(3, 12).unwrap()),
		(
			workspace.url("parts/x.nml").to_string(),
			position(0, 0),
			position(0, 0)
		)
	);
	assert_eq!(
		place(definition(0, 12).unwrap()),
		(
			workspace.url("modules/std/util.nml").to_string(),
			position(0, 0),
			position(0, 0)
		)
	);
}

#[test]
fn references() {
	let workspace = navigation_workspace("references");
	let client = Client::start(Some(&workspace.0));
	let guide = workspace.url("docs/guide.nml");
	client.save(&guide);
	client.open(
		&guide,
		&fs::read_to_string(workspace.0.join("docs/guide.nml")).unwrap(),
	);
	client.diagnostics();

	let references = |line, character, include_declaration| -> Vec<(String, u32)> {
		let result = client.request(
			References::METHOD,
			serde_json::to_value(lsp_types::ReferenceParams {
				text_document_position: at(&guide, line, character),
				context: lsp_types::ReferenceContext {
					include_declaration,
				},
				work_done_progress_params: Default::default(),
				partial_result_params: Default::default(),
			})
			.unwrap(),
		);
		let locations: Vec<lsp_types::Location> = serde_json::from_value(result).unwrap();
		locations
			.into_iter()
			.map(|l| {
				(
					l.uri.as_str().rsplit('/').next().unwrap().to_string(),
					l.range.start.line,
				)
			})
			.collect()
	};
	// On the declaration `{#install}`.
	assert_eq!(
		references(1, 14, true),
		[
			("guide.nml".to_string(), 1),
			("index.nml".into(), 2),
			("other.nml".into(), 0)
		]
	);
	assert_eq!(
		references(1, 14, false),
		[("index.nml".to_string(), 2), ("other.nml".into(), 0)]
	);
	// On a reference to `index#top`: the uses of `top`.
	assert_eq!(
		references(2, 12, false),
		[("guide.nml".to_string(), 2), ("index.nml".into(), 2)]
	);
}
