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
	/// Notifications received while waiting for a response.
	pending: std::cell::RefCell<std::collections::VecDeque<Message>>,
}

impl Client {
	fn start(root: Option<&Path>) -> Self {
		Self::start_with(root, serde_json::json!({ "luals": false }))
	}

	fn start_with(root: Option<&Path>, options: serde_json::Value) -> Self {
		let capabilities = serde_json::json!({
			"textDocument": { "completion": { "completionItem": { "snippetSupport": true } } },
		});
		Self::start_full(root, options, capabilities)
	}

	fn start_full(
		root: Option<&Path>,
		options: serde_json::Value,
		capabilities: serde_json::Value,
	) -> Self {
		let (server, connection) = Connection::memory();
		let server = std::thread::spawn(move || run(server).unwrap());
		let client = Self {
			connection,
			server: Some(server),
			pending: Default::default(),
		};
		let params = InitializeParams {
			workspace_folders: root.map(|root| {
				vec![WorkspaceFolder {
					uri: Uri::from_str(Url::from_file_path(root).unwrap().as_str()).unwrap(),
					name: "test".into(),
				}]
			}),
			initialization_options: Some(options),
			capabilities: serde_json::from_value(capabilities).unwrap(),
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
				Message::Notification(notification) => {
					self.pending.borrow_mut().push_back(notification.into());
				}
				// Requests of the server, such as `window/showDocument`.
				Message::Request(request) => {
					self.pending.borrow_mut().push_back(request.into());
				}
				_ => {}
			}
		}
	}

	/// A request that fails: its error message.
	fn request_error(&self, method: &str, params: serde_json::Value) -> String {
		let id = RequestId::from(2);
		let request = Request::new(id.clone(), method.into(), params);
		self.connection.sender.send(request.into()).unwrap();
		loop {
			match self.receive() {
				Message::Response(response) if response.id == id => {
					return response
						.response_result
						.expect_err("the request fails")
						.message;
				}
				Message::Notification(notification) => {
					self.pending.borrow_mut().push_back(notification.into());
				}
				// Requests of the server, such as `window/showDocument`.
				Message::Request(request) => {
					self.pending.borrow_mut().push_back(request.into());
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

	/// The next request of the server with this method, among what it sent.
	fn server_request(&self, method: &str) -> Request {
		loop {
			let message = self.pending.borrow_mut().pop_front();
			let message = message.unwrap_or_else(|| self.receive());
			if let Message::Request(request) = message
				&& request.method == method
			{
				return request;
			}
		}
	}

	/// The next diagnostics the server publishes.
	fn diagnostics(&self) -> PublishDiagnosticsParams {
		loop {
			let message = self.pending.borrow_mut().pop_front();
			let message = message.unwrap_or_else(|| self.receive());
			if let Message::Notification(notification) = message
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
			(1, 0, 2, "operator".into()),
			// After `> é `: `é` is one UTF-16 unit.
			(1, 4, 2, "operator".into()),
			(1, 7, 2, "operator".into()),
			(2, 0, 4, "keyword".into()),
			(2, 4, 1, "operator".into()),
			(2, 10, 1, "operator".into()),
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
fn hover_shows_figures() {
	let workspace = Workspace::new(
		"hover-figures",
		&[
			("nml.toml", ""),
			("a.nml", ":figure #f Flower\n![](f.png)\n\nSee [[f]].\n"),
			("f.png", "png"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let a = workspace.url("a.nml");
	client.save(&a);
	client.open(&a, &fs::read_to_string(workspace.0.join("a.nml")).unwrap());
	client.diagnostics();
	let result = client.request(
		HoverRequest::METHOD,
		serde_json::to_value(lsp_types::HoverParams {
			text_document_position_params: at(&a, 3, 6),
			work_done_progress_params: Default::default(),
		})
		.unwrap(),
	);
	let hover: lsp_types::Hover = serde_json::from_value(result).unwrap();
	let lsp_types::HoverContents::Markup(markup) = hover.contents else {
		unreachable!()
	};
	assert_eq!(
		markup.value,
		format!(
			"**Flower**\n\n`f` in `a.nml`\n\n![Flower]({})",
			workspace.url("f.png")
		)
	);
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

#[test]
fn quick_fixes() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(&url, "x\n\nsome **bold\n");
	client.diagnostics();
	let actions = |line, character| -> Vec<lsp_types::CodeActionOrCommand> {
		let params = lsp_types::CodeActionParams {
			text_document: TextDocumentIdentifier::new(uri(&url)),
			range: lsp_types::Range::new(position(line, character), position(line, character)),
			context: Default::default(),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
		};
		let result = client.request(
			CodeActionRequest::METHOD,
			serde_json::to_value(params).unwrap(),
		);
		serde_json::from_value(result).unwrap()
	};
	let found = actions(2, 6);
	let [lsp_types::CodeActionOrCommand::CodeAction(action)] = found.as_slice() else {
		panic!("{found:?}")
	};
	assert_eq!(action.title, "Escape the opening delimiter");
	// `Uri` keys, as the protocol defines them: see `Server::code_actions`.
	#[allow(clippy::mutable_key_type)]
	let changes = action.edit.as_ref().unwrap().changes.as_ref().unwrap();
	let edits = &changes[&uri(&url)];
	assert_eq!(edits[0].new_text, "\\*\\*");
	assert_eq!(
		edits[0].range,
		lsp_types::Range::new(position(2, 5), position(2, 7))
	);

	// Elsewhere, nothing.
	assert!(actions(0, 0).is_empty());
}

#[test]
fn completion() {
	let workspace = navigation_workspace("completion");
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("index.nml");
	client.save(&page);
	let complete = |text: &str| -> Vec<String> {
		client.change(&page, text);
		client.diagnostics();
		let line = text.lines().count() as u32 - 1;
		let character = text.lines().last().unwrap().encode_utf16().count() as u32;
		let params = lsp_types::CompletionParams {
			text_document_position: at(&page, line, character),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
			context: None,
		};
		let result = client.request(Completion::METHOD, serde_json::to_value(params).unwrap());
		let items: Vec<lsp_types::CompletionItem> = serde_json::from_value(result).unwrap();
		items.into_iter().map(|item| item.label).collect()
	};
	client.open(&page, "");
	client.diagnostics();

	// Unit keys, and the page's own symbols.
	assert_eq!(
		complete("# Home {#top}\nSee [["),
		["top", "guide#", "index#", "other#"]
	);
	assert_eq!(complete("# Home {#top}\nSee [[gu"), ["guide#"]);
	assert_eq!(complete("# Home {#top}\nSee [[guide#"), ["install"]);
	// The page's own symbols first: its buffer may be newer than the index.
	assert_eq!(complete("# Home {#top}\nSee [[#"), ["top", "install"]);
	// Commands at the start of a line, roles elsewhere.
	assert_eq!(complete(":i"), [":import", ":include"]);
	assert_eq!(complete("x :l"), [":lua"]);
	assert_eq!(complete(":import st"), ["std.util"]);
}

/// Completes at a position of a document: the items.
fn complete_at(
	client: &Client,
	url: &Url,
	line: u32,
	character: u32,
) -> Vec<lsp_types::CompletionItem> {
	let params = lsp_types::CompletionParams {
		text_document_position: at(url, line, character),
		work_done_progress_params: Default::default(),
		partial_result_params: Default::default(),
		context: None,
	};
	let result = client.request(Completion::METHOD, serde_json::to_value(params).unwrap());
	serde_json::from_value(result).unwrap()
}

/// An item's inserted text, its format, and its documentation.
fn inserted(item: &lsp_types::CompletionItem) -> (String, bool, String) {
	let Some(lsp_types::CompletionTextEdit::Edit(edit)) = &item.text_edit else {
		panic!("{item:?}")
	};
	let snippet = item.insert_text_format == Some(lsp_types::InsertTextFormat::SNIPPET);
	let doc = match &item.documentation {
		Some(lsp_types::Documentation::MarkupContent(markup)) => markup.value.clone(),
		_ => String::new(),
	};
	(edit.new_text.clone(), snippet, doc)
}

#[test]
fn documented_completion() {
	let workspace = navigation_workspace("documented-completion");
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("index.nml");
	client.save(&page);
	let text = ":set name = **Bob**\n:set nml.output = home.html\n\n# Home {#top}\n:figure #f Flower\nx\n\nSee [[ and [[f]], hi %na and %nml.output%.\nco\n:fig\n[[#]]\n%nml.output%\n";
	client.open(&page, text);
	client.diagnostics();
	let item = |items: Vec<lsp_types::CompletionItem>, label: &str| {
		let item = items.iter().find(|item| item.label == label);
		inserted(item.unwrap_or_else(|| panic!("no {label} in {items:?}")))
	};

	// References: their label and title, closed unless `]]` follows.
	let (text, snippet, doc) = item(complete_at(&client, &page, 7, 6), "f");
	assert_eq!((text.as_str(), snippet), ("f]]", false));
	assert_eq!(doc, "**Figure 1**: Flower\n\nfigure `f` in `index.nml`");
	let (text, _, doc) = item(complete_at(&client, &page, 7, 6), "top");
	assert_eq!(text, "top]]");
	assert_eq!(doc, "**Home**\n\nheading `top` in `index.nml`");
	let (text, _, doc) = item(complete_at(&client, &page, 7, 6), "guide#");
	assert_eq!(text, "guide#");
	assert!(doc.contains("The page `docs/guide.nml`"), "{doc}");
	assert_eq!(item(complete_at(&client, &page, 7, 14), "f").0, "f");
	assert_eq!(item(complete_at(&client, &page, 10, 3), "f").0, "f");
	let (text, _, doc) = item(complete_at(&client, &page, 10, 3), "install");
	assert_eq!(text, "install");
	assert!(doc.contains("`install` in `guide`"), "{doc}");

	// Variables: their value, and where they are set; closed unless `%` follows.
	let items = complete_at(&client, &page, 7, 24);
	assert_eq!(items.len(), 1);
	let (text, snippet, doc) = inserted(&items[0]);
	assert_eq!((text.as_str(), snippet), ("name%", false));
	assert_eq!(items[0].detail.as_deref(), Some("**Bob**"));
	assert_eq!(
		doc,
		"**`%name%`**\n\n```nml\n**Bob**\n```\n\nSet in `index.nml`, line 1."
	);
	assert_eq!(
		item(complete_at(&client, &page, 11, 11), "nml.output").0,
		"nml.output"
	);

	// A word at the start of a line: snippets, documented.
	let items = complete_at(&client, &page, 8, 2);
	let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
	assert_eq!(labels, ["code block", "code role", "code span"]);
	let (text, snippet, doc) = inserted(&items[0]);
	assert_eq!(
		(text.as_str(), snippet),
		("```${1:language} ${2:Title}\n${0}\n```", true)
	);
	assert!(doc.starts_with("**Code block**"), "{doc}");
	// The detail is plain: editors show it as code.
	assert_eq!(items[0].detail.as_deref(), Some("Code block"));

	// Commands insert their snippet.
	let (text, snippet, doc) = item(complete_at(&client, &page, 9, 4), ":figure");
	assert_eq!(
		(text.as_str(), snippet),
		(":figure #${1:name} ${2:Caption}\n${0}", true)
	);
	assert!(doc.starts_with("**Figure**"), "{doc}");
}

#[test]
fn bracket_and_quote_completion() {
	let workspace = navigation_workspace("bracket-completion");
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("index.nml");
	client.save(&page);
	let text = ":figure #f Flower\nx\n\nSee [ and [f]].\n>\n>t\n";
	client.open(&page, text);
	client.diagnostics();
	let find = |items: &[lsp_types::CompletionItem], label: &str| {
		let item = items.iter().find(|item| item.label == label);
		item.unwrap_or_else(|| panic!("no {label} in {items:?}"))
			.clone()
	};
	let range = |item: &lsp_types::CompletionItem| match &item.text_edit {
		Some(lsp_types::CompletionTextEdit::Edit(edit)) => {
			(edit.range.start.character, edit.range.end.character)
		}
		_ => panic!("{item:?}"),
	};

	// A lone `[` becomes `[[`, the reference closed unless `]]` follows.
	let items = complete_at(&client, &page, 3, 5);
	let figure = find(&items, "f");
	assert_eq!(inserted(&figure).0, "[[f]]");
	assert_eq!(figure.filter_text.as_deref(), Some("[f"));
	assert_eq!(range(&figure), (4, 5));
	assert_eq!(inserted(&find(&items, "guide#")).0, "[[guide#");
	let items = complete_at(&client, &page, 3, 12);
	assert_eq!(inserted(&find(&items, "f")).0, "[[f");

	// A `>` at the start of a line: quotes and callouts, matched after it.
	let items = complete_at(&client, &page, 4, 1);
	let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
	assert_eq!(
		labels,
		[
			"callout",
			"quote",
			"note",
			"tip",
			"important",
			"warning",
			"caution",
			"todo",
			"cited quote"
		]
	);
	let todo = find(&items, "todo");
	let (text, snippet, doc) = inserted(&todo);
	assert_eq!(
		(text.as_str(), snippet),
		("> [!todo] ${1:Title}\n> ${0}", true)
	);
	assert!(doc.starts_with("**Callout**"), "{doc}");
	assert_eq!(todo.filter_text.as_deref(), Some(">todo"));
	assert_eq!(range(&todo), (0, 1));
	let items = complete_at(&client, &page, 5, 2);
	let labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
	assert_eq!(labels, ["tip", "todo"]);
}

#[test]
fn completion_without_snippets() {
	let (server, connection) = Connection::memory();
	let thread = std::thread::spawn(move || run(server).unwrap());
	let client = Client {
		connection,
		server: Some(thread),
		pending: Default::default(),
	};
	let params = InitializeParams {
		initialization_options: Some(serde_json::json!({ "luals": false })),
		..InitializeParams::default()
	};
	client.request("initialize", serde_json::to_value(params).unwrap());
	client.notify::<Initialized>(serde_json::json!({}));
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(&url, "co\n:fig\n>\n");
	client.diagnostics();
	assert!(complete_at(&client, &url, 0, 2).is_empty());
	// Quotes and callouts insert their start.
	let items = complete_at(&client, &url, 2, 1);
	let texts: Vec<_> = items.iter().map(|item| inserted(item).0).collect();
	assert_eq!(texts[0], "> ");
	assert!(texts.contains(&"> [!todo] ".to_string()), "{texts:?}");
	assert!(items.iter().all(|item| item.label != "callout"));
	let items = complete_at(&client, &url, 1, 4);
	let (text, snippet, doc) = inserted(&items[0]);
	assert_eq!((text.as_str(), snippet), (":figure", false));
	assert!(doc.starts_with("**Figure**"));
}

#[test]
fn syntax_hover() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	let text =
		":set name = **Bob**\n\nSome **bold** and %name% %nope%.\n\n```rust\nfn main() {}\n```\n";
	client.open(&url, text);
	client.diagnostics();
	let hover = |line, character| -> Option<(String, lsp_types::Range)> {
		let result = client.request(
			HoverRequest::METHOD,
			serde_json::to_value(lsp_types::HoverParams {
				text_document_position_params: at(&url, line, character),
				work_done_progress_params: Default::default(),
			})
			.unwrap(),
		);
		let hover: Option<lsp_types::Hover> = serde_json::from_value(result).unwrap();
		hover.map(|hover| {
			let lsp_types::HoverContents::Markup(markup) = hover.contents else {
				unreachable!()
			};
			(markup.value, hover.range.unwrap())
		})
	};
	let range =
		|line, start, end| lsp_types::Range::new(position(line, start), position(line, end));

	// On a marker: the rule's documentation, over the marker.
	let (doc, at) = hover(2, 5).unwrap();
	assert!(doc.starts_with("**Bold text**: `**text**`."), "{doc}");
	assert_eq!(at, range(2, 5, 7));
	// In its content: nothing.
	assert_eq!(hover(2, 9), None);
	// Variables: where they are used, and where they are set.
	let variable =
		"**`%name%`**\n\n```nml\n**Bob**\n```\n\nSet in `/tmp/nml-lsp-none/a.nml`, line 1.";
	assert_eq!(hover(2, 21), Some((variable.to_string(), range(2, 19, 23))));
	assert_eq!(hover(0, 6), Some((variable.to_string(), range(0, 5, 9))));
	assert_eq!(hover(2, 27).unwrap().0, "`nope` is not set");
	// The command of `:set`: its documentation.
	assert!(
		hover(0, 1)
			.unwrap()
			.0
			.starts_with("**Variable**, set in the prologue")
	);
	// A code block's fence and language, not its code.
	assert!(hover(4, 1).unwrap().0.starts_with("**Code block**"));
	assert!(hover(4, 4).unwrap().0.starts_with("**Code block**"));
	assert_eq!(hover(5, 3), None);
	// Plain text.
	assert_eq!(hover(2, 1), None);

	// While a variable is written, unclosed: the failed match documents nothing (it runs to
	// the end of its paragraph), and what follows it is documented as usual.
	client.change(&url, "Hello %wh and more\n\nSome **bold**.\n");
	client.diagnostics();
	assert_eq!(hover(0, 12), None);
	assert!(hover(2, 5).unwrap().0.starts_with("**Bold text**"));
}

#[test]
fn std_module_completion() {
	let workspace = Workspace::new("std-completion", &[("nml.toml", ""), ("a.nml", "")]);
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("a.nml");
	client.open(&page, ":import std.");
	client.diagnostics();
	let params = lsp_types::CompletionParams {
		text_document_position: at(&page, 0, 12),
		work_done_progress_params: Default::default(),
		partial_result_params: Default::default(),
		context: None,
	};
	let result = client.request(Completion::METHOD, serde_json::to_value(params).unwrap());
	let items: Vec<lsp_types::CompletionItem> = serde_json::from_value(result).unwrap();
	let labels: Vec<_> = items.into_iter().map(|item| item.label).collect();
	assert_eq!(labels, ["std.nav", "std.toc"]);
}

#[test]
fn inlay_hints() {
	let workspace = navigation_workspace("hints");
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("index.nml");
	client.save(&page);
	client.open(
		&page,
		"# Home {#top}\n[[guide#install]] [[top|cap]] [[guide#]] [[nope]]\n:figure #f F\nx\n\n[[f]]\n",
	);
	client.diagnostics();
	let params = lsp_types::InlayHintParams {
		text_document: TextDocumentIdentifier::new(uri(&page)),
		range: lsp_types::Range::new(position(0, 0), position(9, 0)),
		work_done_progress_params: Default::default(),
	};
	let result = client.request(
		InlayHintRequest::METHOD,
		serde_json::to_value(params).unwrap(),
	);
	let hints: Vec<lsp_types::InlayHint> = serde_json::from_value(result).unwrap();
	let hints: Vec<_> = hints
		.into_iter()
		.map(|hint| match hint.label {
			lsp_types::InlayHintLabel::String(label) => (hint.position, label),
			_ => unreachable!(),
		})
		.collect();
	// Captioned and unresolved references get none.
	assert_eq!(
		hints,
		[
			(position(1, 17), ": Install".to_string()),
			(position(1, 40), ": Guide".into()),
			// A figure by its label, as in the page.
			(position(5, 5), ": Figure 1".into())
		]
	);
}

#[test]
fn conceal_and_style() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(
		&url,
		"# Title\na **b *c*** [[x|y]] `z`\n\n:lua`1` and\n:lua\n```\nx = 1\n```\n",
	);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });

	let result = client.request(display::CONCEAL, params.clone());
	let conceals: Vec<display::ConcealInfo> = serde_json::from_value(result).unwrap();
	let ranges: Vec<_> = conceals
		.iter()
		.map(|c| (c.range.start.character, c.range.end.character))
		.collect();
	assert_eq!(
		ranges,
		[
			(2, 4),
			(6, 7),
			(8, 9),
			(9, 11),
			(12, 14),
			(17, 19),
			(20, 21),
			(22, 23),
			// After the Lua block's opening fence, as a code block's (the fences stay); the
			// role keeps its own.
			(3, 3)
		]
	);
	assert_eq!(
		serde_json::to_value(&conceals[0]).unwrap()["concealText"],
		serde_json::json!({ "text": "" })
	);

	let result = client.request(display::STYLE, params);
	let styles: Vec<display::StyleInfo> = serde_json::from_value(result).unwrap();
	let styles: Vec<_> = styles
		.into_iter()
		.map(|s| {
			let display::StyleTarget::Group(group) = s.style;
			(s.range.start.line, s.range.start.character, group)
		})
		.collect();
	assert_eq!(
		styles,
		[
			(0, 0, "@markup.heading".to_string()),
			(1, 2, "@markup.strong".into()),
			(1, 6, "@markup.italic".into()),
			(1, 12, "@markup.link".into()),
			(1, 20, "@markup.raw".into())
		]
	);
}

#[test]
fn conceal_tokens() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	let text = " * a\n *- [x] b\n *- [-] c\n\n> q\n> r\n\n> [!warning] Careful\n> s\n\n```rust Main\nfn main() {}\n```\n\n> [!nope]\n> t\n";
	client.open(&url, text);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });
	let result = client.request(display::CONCEAL, params);
	let conceals: Vec<display::ConcealInfo> = serde_json::from_value(result).unwrap();
	let shown: Vec<_> = conceals
		.iter()
		.map(|c| {
			let (start, end) = (c.range.start, c.range.end);
			assert_eq!(start.line, end.line);
			let target = match &c.conceal_text {
				display::ConcealTarget::Text(text) => format!("{text:?}"),
				display::ConcealTarget::Token { token, params } => format!("{token} {params}"),
			};
			(start.line, start.character, end.character, target)
		})
		.collect();
	let expected = [
		(0, 1, 2, r#"bullet {"depth":0,"numbered":false}"#),
		(1, 1, 2, r#"bullet {"depth":0,"numbered":false}"#),
		(1, 2, 3, r#"bullet {"depth":1,"numbered":true}"#),
		(1, 4, 7, r#"checkbox {"state":"Checked"}"#),
		(2, 1, 2, r#"bullet {"depth":0,"numbered":false}"#),
		(2, 2, 3, r#"bullet {"depth":1,"numbered":true}"#),
		(2, 4, 7, r#"checkbox {"state":"Partial"}"#),
		(4, 0, 1, r#"block {"name":"Quote"}"#),
		(5, 0, 1, r#"block {"name":"Quote"}"#),
		(7, 0, 1, r#"block {"name":"Warning"}"#),
		(8, 0, 1, r#"block {"name":"Warning"}"#),
		(7, 2, 12, r#"block_name {"name":"Warning"}"#),
		// The fences stay: the header is drawn over what follows the opening one.
		(10, 3, 12, r#"code {"language":"rust","name":"Main"}"#),
		// An unknown callout: its markers are text.
		(14, 0, 4, r#""""#),
		(14, 8, 9, r#""""#),
	];
	let shown: Vec<_> = (shown.iter())
		.map(|(line, start, end, target)| (*line, *start, *end, target.as_str()))
		.collect();
	assert_eq!(shown, expected);

	// Nested quotes, and a quote in a list entry: each `>` at its own column.
	let url = Url::parse("file:///tmp/nml-lsp-none/b.nml").unwrap();
	client.open(&url, "> a\n> > b\n> > c\n\n * e\n   > f\n   > g\n");
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });
	let result = client.request(display::CONCEAL, params);
	let conceals: Vec<display::ConcealInfo> = serde_json::from_value(result).unwrap();
	let blocks: Vec<_> = (conceals.iter())
		.filter(
			|c| matches!(&c.conceal_text, display::ConcealTarget::Token { token, .. } if token == "block"),
		)
		.map(|c| (c.range.start.line, c.range.start.character))
		.collect();
	assert_eq!(
		blocks,
		[(0, 0), (1, 0), (2, 0), (1, 2), (2, 2), (5, 3), (6, 3)]
	);

	// A listing's name is part of the header, with or without a title.
	let url = Url::parse("file:///tmp/nml-lsp-none/c.nml").unwrap();
	client.open(
		&url,
		"```rust #main\nx\n```\n\n```rust #lib The library\ny\n```\n",
	);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });
	let result = client.request(display::CONCEAL, params);
	let conceals: Vec<display::ConcealInfo> = serde_json::from_value(result).unwrap();
	let headers: Vec<_> = (conceals.iter())
		.filter_map(|c| match &c.conceal_text {
			display::ConcealTarget::Token { token, params } if token == "code" => Some((
				c.range.start.line,
				c.range.end.character,
				params.to_string(),
			)),
			_ => None,
		})
		.collect();
	assert_eq!(
		headers,
		[
			(0, 13, r#"{"language":"rust","name":""}"#.to_string()),
			(
				4,
				24,
				r#"{"language":"rust","name":"The library"}"#.to_string()
			),
		]
	);
}

#[test]
fn rendered_blocks_show_as_code() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(
		&url,
		":lua\n```\nx = 1\n```\n\n:graph[layout=neato] #g The graph\n````dot\ndigraph {}\n````\n\n:tex\n```\n\\LaTeX\n```\n\n:typst\n```\n#rect[]\n```\n\nA :typst`$a$`.\n\n:run #p\n```R\nplot(x)\n```\n",
	);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });

	// After the opening fence, a code block's header in the block's language; the fences
	// and the command stay.
	let result = client.request(display::CONCEAL, params.clone());
	let conceals: Vec<display::ConcealInfo> = serde_json::from_value(result).unwrap();
	let shown: Vec<_> = (conceals.iter())
		.map(|c| {
			let target = match &c.conceal_text {
				display::ConcealTarget::Text(text) => format!("{text:?}"),
				display::ConcealTarget::Token { token, params } => format!("{token} {params}"),
			};
			(
				c.range.start.line,
				c.range.start.character,
				c.range.end.character,
				target,
			)
		})
		.collect();
	let shown: Vec<_> = (shown.iter())
		.map(|(line, start, end, target)| (*line, *start, *end, target.as_str()))
		.collect();
	assert_eq!(
		shown,
		[
			(1, 3, 3, r#"code {"language":"lua","name":""}"#),
			(6, 4, 7, r#"code {"language":"dot","name":""}"#),
			(11, 3, 3, r#"code {"language":"tex","name":""}"#),
			(16, 3, 3, r#"code {"language":"typst","name":""}"#),
			(23, 3, 4, r#"code {"language":"R","name":""}"#),
		]
	);

	// Their code, highlighted in its language.
	let result = client.request(lua::CODE_RANGE, params);
	let ranges: Vec<lua::CodeRangeInfo> = serde_json::from_value(result).unwrap();
	let ranges: Vec<_> = (ranges.iter())
		.map(|r| (r.range.start, r.range.end, r.language.as_str()))
		.collect();
	assert_eq!(
		ranges,
		[
			(position(2, 0), position(3, 0), "lua"),
			(position(7, 0), position(8, 0), "dot"),
			(position(12, 0), position(13, 0), "tex"),
			(position(17, 0), position(18, 0), "typst"),
			(position(20, 9), position(20, 12), "typst"),
			(position(24, 0), position(25, 0), "R"),
		]
	);
}

#[test]
fn lua_documents() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(
		&url,
		":lua\n```\nx = 1\nfunction f() return x end\n```\n\nA :lua`f()` and :lua`x = 2`.\n\n```rust\nfn f() {}\n```\n\n```\nplain\n```\n",
	);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });

	// The Lua, and the code blocks with their language, in order.
	let result = client.request(lua::CODE_RANGE, params.clone());
	let ranges: Vec<lua::CodeRangeInfo> = serde_json::from_value(result).unwrap();
	let ranges: Vec<_> = (ranges.iter())
		.map(|r| (r.range.start, r.range.end, r.language.as_str()))
		.collect();
	assert_eq!(
		ranges,
		[
			(position(2, 0), position(4, 0), "lua"),
			(position(6, 7), position(6, 10), "lua"),
			(position(6, 21), position(6, 26), "lua"),
			(position(9, 0), position(10, 0), "rust"),
			(position(13, 0), position(14, 0), ""),
		]
	);

	let result = client.request(lua::LUA_DOCUMENT, params);
	let document: lua::LuaDocument = serde_json::from_value(result).unwrap();
	assert_eq!(
		document.content,
		"x = 1\nfunction f() return x end\nlocal _ = (f())\ndo x = 2 end\n"
	);
	let fragments: Vec<_> = document
		.fragments
		.iter()
		.map(|f| (f.range.start, f.line, f.character))
		.collect();
	assert_eq!(
		fragments,
		[
			(position(2, 0), 0, 0),
			(position(6, 7), 2, 11),
			(position(6, 21), 3, 3)
		]
	);
}

#[test]
fn lua_documents_run_imported_modules() {
	let workspace = Workspace::new(
		"lua-modules",
		&[
			("nml.toml", "[[module]]\nname = \"lib\"\nroot = \"lib\"\n"),
			(
				"lib/util.nml",
				":import lib.base as b\n\n:lua\n```\n--- Greets.\nfunction greet(name) return b.prefix .. name end\n```\n\nText :lua`ignored()`.\n",
			),
			("lib/base.nml", ":lua\n```\nprefix = 'hi '\n```\n"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("a.nml");
	client.open(&page, ":import lib.util\n\n:lua`util.greet('x')`\n");
	assert!(client.diagnostics().diagnostics.is_empty());

	let params = serde_json::json!({ "textDocument": { "uri": page.as_str() } });
	let result = client.request(lua::LUA_DOCUMENT, params);
	let document: lua::LuaDocument = serde_json::from_value(result).unwrap();
	// The module's code, and its own imports, run where it is imported, and return its
	// exports: LuaLS follows them to their definitions.
	assert_eq!(
		document.content,
		"util = (function()\n\
		b = (function()\n\
		prefix = 'hi '\n\
		return { prefix = prefix, }\n\
		end)()\n\
		--- Greets.\n\
		function greet(name) return b.prefix .. name end\n\
		return { b = b, greet = greet, }\n\
		end)()\n\
		local _ = (util.greet('x'))\n"
	);
	// Only the document's own code maps back to it.
	let fragments: Vec<_> = (document.fragments.iter())
		.map(|f| (f.range.start, f.line, f.character))
		.collect();
	assert_eq!(fragments, [(position(2, 5), 9, 11)]);
	assert_eq!(document.to_nml(position(2, 0)), None);
}

#[test]
fn lua_documents_of_included_files() {
	let workspace = Workspace::new(
		"lua-includes",
		&[
			(
				"nml.toml",
				"exclude = [\"parts/**\"]\n[[module]]\nname = \"lib\"\nroot = \"lib\"\n",
			),
			("lib/util.nml", ":lua\n```\nfunction greet() end\n```\n"),
			(
				"a.nml",
				":import lib.util\n\n:lua\n```\nfunction helper() return 'A' end\n```\n\n:include parts/x\n\n:lua\n```\nafter = 1\n```\n",
			),
			("parts/x.nml", ":include y\n\nValue: :lua`helper()`\n"),
			("parts/y.nml", ":lua\n```\nfrom_y = 1\n```\n"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let part = workspace.url("parts/x.nml");
	client.save(&part);
	client.diagnostics();
	client.open(&part, ":include y\n\nValue: :lua`helper()`\n");
	assert!(client.diagnostics().diagnostics.is_empty());

	let params = serde_json::json!({ "textDocument": { "uri": part.as_str() } });
	let result = client.request(lua::LUA_DOCUMENT, params);
	let document: lua::LuaDocument = serde_json::from_value(result).unwrap();
	// The includer's Lua up to the inclusion, with its imports, then the file's, with what
	// it includes.
	assert_eq!(
		document.content,
		"util = (function()\n\
		function greet() end\n\
		return { greet = greet, }\n\
		end)()\n\
		function helper() return 'A' end\n\
		from_y = 1\n\
		local _ = (helper())\n"
	);
	let fragments: Vec<_> = (document.fragments.iter())
		.map(|f| (f.range.start, f.line, f.character))
		.collect();
	assert_eq!(fragments, [(position(2, 12), 6, 11)]);
}

#[test]
fn include_contexts() {
	let workspace = Workspace::new(
		"contexts",
		&[
			("nml.toml", "exclude = [\"parts/**\"]\n"),
			(
				"a.nml",
				":lua\n```\nfunction helper() return 'A' end\n```\n\n:include parts/x\n",
			),
			(
				"b.nml",
				":lua\n```\nfunction helper() return 'B' end\n```\n\n:include parts/x\n",
			),
			("parts/x.nml", "Value: :lua`helper()`\n"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let part = workspace.url("parts/x.nml");
	client.save(&part);
	client.diagnostics();

	// In the first includer's context, `helper` exists.
	client.open(&part, "Value: :lua`helper()`\n");
	assert!(client.diagnostics().diagnostics.is_empty());
	let result = client.request(
		INCLUDE_CONTEXTS,
		serde_json::json!({ "textDocument": { "uri": part.as_str() } }),
	);
	assert_eq!(
		result,
		serde_json::json!({ "current": "a.nml", "candidates": ["a.nml", "b.nml"] })
	);

	// `b` is edited, not saved, and chosen as the context: its helper now fails here.
	let b = workspace.url("b.nml");
	client.open(
		&b,
		":lua\n```\nfunction helper() return true end\n```\n\n:include parts/x\n",
	);
	client.diagnostics();
	let params = lsp_types::ExecuteCommandParams {
		command: INCLUDE_CONTEXT_COMMAND.into(),
		arguments: vec![part.as_str().into(), "b.nml".into()],
		work_done_progress_params: Default::default(),
	};
	client.request(
		ExecuteCommand::METHOD,
		serde_json::to_value(params).unwrap(),
	);
	let published = client.diagnostics();
	assert_eq!(published.uri.as_str(), part.as_str());
	assert_eq!(published.diagnostics.len(), 1);
	assert!(published.diagnostics[0].message.contains("not a boolean"));
	assert_eq!(published.diagnostics[0].range.start, position(0, 7));
}

/// LuaLS for the proxy test: `NML_LUALS`, or where Mason installs it.
fn luals_path() -> Option<PathBuf> {
	if let Some(path) = std::env::var_os("NML_LUALS") {
		return Some(path.into());
	}
	let home = std::env::var_os("HOME")?;
	let path = PathBuf::from(home).join(".local/share/nvim/mason/bin/lua-language-server");
	path.is_file().then_some(path)
}

#[test]
fn luals_proxy() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new("luals", &[("nml.toml", "")]);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	let text = ":lua\n```\nfunction greet(name)\n\treturn 'hi ' .. name\nend\nundefined_global()\n```\n\nSay :lua`greet('x')` and :lua`gre`.\n";
	client.open(&page, text);

	// LuaLS's diagnostics arrive later, mapped to the document.
	let lua_diagnostic = (0..30).find_map(|_| {
		let published = client.diagnostics();
		published
			.diagnostics
			.into_iter()
			.find(|d| d.source.as_deref().is_some_and(|s| s.starts_with("Lua")))
	});
	let diagnostic = lua_diagnostic.expect("LuaLS reports the undefined global");
	assert_eq!(diagnostic.range.start, position(5, 0));

	// Hover on `greet` in the role: LuaLS knows the function from the block.
	let result = client.request(
		HoverRequest::METHOD,
		serde_json::to_value(lsp_types::HoverParams {
			text_document_position_params: at(&page, 8, 10),
			work_done_progress_params: Default::default(),
		})
		.unwrap(),
	);
	let hover = result.to_string();
	assert!(hover.contains("greet"), "{hover}");
	let range: lsp_types::Range = serde_json::from_value(result["range"].clone()).unwrap();
	assert_eq!(range.start, position(8, 9));

	// Completion in the second role.
	let result = client.request(
		Completion::METHOD,
		serde_json::to_value(lsp_types::CompletionParams {
			text_document_position: at(&page, 8, 32),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
			context: None,
		})
		.unwrap(),
	);
	assert!(result.to_string().contains("\"greet\""), "{result}");

	// Definition of `greet`: its block, in the document.
	let result = client.request(
		GotoDefinition::METHOD,
		serde_json::to_value(lsp_types::GotoDefinitionParams {
			text_document_position_params: at(&page, 8, 10),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
		})
		.unwrap(),
	);
	let locations: Vec<lsp_types::Location> = serde_json::from_value(result).unwrap();
	assert!(
		locations
			.iter()
			.any(|l| l.uri.as_str() == page.as_str() && l.range.start.line == 2),
		"{locations:?}"
	);
}

impl Client {
	/// The semantic tokens of a document, decoded.
	fn semantic_tokens(&self, url: &Url) -> Vec<(u32, u32, u32, String)> {
		let result = self.request(
			SemanticTokensFullRequest::METHOD,
			serde_json::to_value(SemanticTokensParams {
				text_document: TextDocumentIdentifier::new(uri(url)),
				work_done_progress_params: Default::default(),
				partial_result_params: Default::default(),
			})
			.unwrap(),
		);
		let tokens: SemanticTokens = serde_json::from_value(result).unwrap();
		decode(&tokens.data)
	}
}

#[test]
fn luals_semantic_tokens() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new("luals-tokens", &[("nml.toml", "")]);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	let text = ":lua\n```\nlocal n = 'x' -- note\nfunction greet(name) return n end\n```\n\n# Say :lua`greet(1)`\n";
	client.open(&page, text);
	client.diagnostics();
	let tokens = client.semantic_tokens(&page);
	let expected: &[(u32, u32, u32, &str)] = &[
		// NML's: the block's command and fence.
		(0, 0, 4, "keyword"),
		(1, 0, 3, "operator"),
		// LuaLS's, in the block.
		(2, 0, 5, "keyword"),
		(2, 6, 1, "variable"),
		(2, 10, 3, "string"),
		(2, 14, 7, "comment"),
		(3, 0, 8, "keyword"),
		(3, 9, 5, "function"),
		(3, 15, 4, "parameter"),
		(3, 21, 6, "keyword"),
		(3, 28, 1, "variable"),
		(3, 30, 3, "keyword"),
		(4, 0, 3, "operator"),
		// NML's around the role, LuaLS's in it; not its wrapper.
		(6, 0, 1, "operator"),
		(6, 6, 4, "keyword"),
		(6, 10, 1, "operator"),
		(6, 11, 5, "function"),
		(6, 17, 1, "number"),
		(6, 19, 1, "operator"),
	];
	let shown: Vec<_> = (tokens.iter())
		.map(|(line, column, length, kind)| (*line, *column, *length, kind.as_str()))
		.collect();
	assert_eq!(shown, expected);
}

#[test]
fn luals_sees_imported_modules() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new(
		"luals-modules",
		&[
			("nml.toml", "[[module]]\nname = \"lib\"\nroot = \"lib\"\n"),
			(
				"lib/util.nml",
				":lua\n```\n--- Greets someone.\n---@param name string\nfunction greet(name) return 'hi ' .. name end\n```\n",
			),
		],
	);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	let text = ":import lib.util\n\n:lua`util.greet('x')` :lua`util.g` :lua`missing()`\n";
	client.open(&page, text);

	// The namespace is defined; `missing` is not.
	let lua_diagnostics = (0..30)
		.find_map(|_| {
			let published = client.diagnostics();
			let lua: Vec<_> = (published.diagnostics.into_iter())
				.filter(|d| d.source.as_deref().is_some_and(|s| s.starts_with("Lua")))
				.collect();
			(!lua.is_empty()).then_some(lua)
		})
		.expect("LuaLS reports the undefined global");
	let messages: Vec<_> = lua_diagnostics.iter().map(|d| &d.message).collect();
	assert!(messages.iter().all(|m| !m.contains("util")), "{messages:?}");
	assert!(
		messages.iter().any(|m| m.contains("missing")),
		"{messages:?}"
	);

	// Hover on `greet`: the module's documentation.
	let result = client.request(
		HoverRequest::METHOD,
		serde_json::to_value(lsp_types::HoverParams {
			text_document_position_params: at(&page, 2, 12),
			work_done_progress_params: Default::default(),
		})
		.unwrap(),
	);
	assert!(result.to_string().contains("Greets someone."), "{result}");

	// Completion of the namespace's fields.
	let result = client.request(
		Completion::METHOD,
		serde_json::to_value(lsp_types::CompletionParams {
			text_document_position: at(&page, 2, 33),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
			context: None,
		})
		.unwrap(),
	);
	assert!(result.to_string().contains("\"greet\""), "{result}");
}

#[test]
fn luals_sees_include_contexts() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new(
		"luals-includes",
		&[
			("nml.toml", "exclude = [\"parts/**\"]\n"),
			(
				"a.nml",
				":lua\n```\nfunction helper() return 'A' end\n```\n\n:include parts/x\n",
			),
			(
				"parts/x.nml",
				"Value: :lua`helper()` :lua`tostring(missing)`\n",
			),
		],
	);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let part = workspace.url("parts/x.nml");
	client.save(&part);
	client.open(&part, "Value: :lua`helper()` :lua`tostring(missing)`\n");

	// `helper` is defined by the includer; `missing` is not.
	let lua_diagnostics = (0..30)
		.find_map(|_| {
			let published = client.diagnostics();
			let lua: Vec<_> = (published.diagnostics.into_iter())
				.filter(|d| d.source.as_deref().is_some_and(|s| s.starts_with("Lua")))
				.collect();
			(!lua.is_empty()).then_some(lua)
		})
		.expect("LuaLS reports the undefined global");
	let messages: Vec<_> = lua_diagnostics.iter().map(|d| &d.message).collect();
	assert!(
		messages.iter().all(|m| !m.contains("helper")),
		"{messages:?}"
	);
	assert!(
		messages.iter().any(|m| m.contains("missing")),
		"{messages:?}"
	);
}

#[test]
fn luals_settings_come_from_the_server() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new("luals-settings", &[("nml.toml", "")]);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	client.open(&page, ":lua`tostring(nml.read)` :lua`tostring(io)`\n");
	// The sandbox has no `io`.
	let lua_diagnostics = (0..30)
		.find_map(|_| {
			let published = client.diagnostics();
			let lua: Vec<_> = (published.diagnostics.into_iter())
				.filter(|d| d.source.as_deref().is_some_and(|s| s.starts_with("Lua")))
				.collect();
			(!lua.is_empty()).then_some(lua)
		})
		.expect("LuaLS reports `io`");
	let messages: Vec<_> = lua_diagnostics.iter().map(|d| &d.message).collect();
	assert_eq!(messages, ["Undefined global `io`."]);
	// The declarations are written; the workspace gets no `.luarc.json`.
	assert!(workspace.0.join(".nml/types/nml.lua").is_file());
	assert!(!workspace.0.join(".luarc.json").exists());

	// The API is declared.
	let result = client.request(
		HoverRequest::METHOD,
		serde_json::to_value(lsp_types::HoverParams {
			text_document_position_params: at(&page, 0, 19),
			work_done_progress_params: Default::default(),
		})
		.unwrap(),
	);
	assert!(result.to_string().contains("function nml.read"), "{result}");
}

/// A stand-in for LuaLS: a shell script in the workspace.
fn fake_luals(workspace: &Workspace, script: &str) -> PathBuf {
	use std::os::unix::fs::PermissionsExt;
	let path = workspace.0.join("fake-luals");
	fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
	fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
	path
}

impl Client {
	/// The next message for which `matches` holds, skipping the others.
	fn next(&self, matches: impl Fn(&Message) -> bool) -> Message {
		loop {
			let message = self.pending.borrow_mut().pop_front();
			let message = message.unwrap_or_else(|| self.receive());
			if matches(&message) {
				return message;
			}
		}
	}

	/// The next message the server shows the user.
	fn shown_message(&self) -> String {
		let message = self.next(
			|message| matches!(message, Message::Notification(n) if n.method == "window/showMessage"),
		);
		let Message::Notification(notification) = message else {
			unreachable!()
		};
		notification.params["message"].as_str().unwrap().to_string()
	}

	/// Sends a hover request with `id`, without waiting for its response.
	fn send_hover(&self, id: i32, url: &Url, line: u32, character: u32) {
		let params = lsp_types::HoverParams {
			text_document_position_params: at(url, line, character),
			work_done_progress_params: Default::default(),
		};
		let request = Request::new(
			RequestId::from(id),
			HoverRequest::METHOD.into(),
			serde_json::to_value(params).unwrap(),
		);
		self.connection.sender.send(request.into()).unwrap();
	}
}

#[test]
fn luals_requests_do_not_block() {
	let workspace = Workspace::new("luals-silent", &[("nml.toml", "")]);
	let luals = fake_luals(&workspace, "exec sleep 60");
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	client.open(&page, ":lua`x = 1`\n");
	client.diagnostics();
	// A hover in Lua waits for LuaLS, which never answers; the server goes on meanwhile.
	client.send_hover(2, &page, 0, 6);
	let params = serde_json::json!({ "textDocument": { "uri": page.as_str() } });
	let result = client.request(lua::CODE_RANGE, params);
	assert_eq!(result.as_array().unwrap().len(), 1);
}

#[test]
fn luals_is_restarted_once() {
	let workspace = Workspace::new("luals-stops", &[("nml.toml", "")]);
	let luals = fake_luals(&workspace, "sleep 1");
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	client.open(&page, ":lua`x = 1`\n");
	client.diagnostics();
	// LuaLS stops before it answers: the request gets no result.
	client.send_hover(2, &page, 0, 6);
	let response = client
		.next(|message| matches!(message, Message::Response(r) if r.id == RequestId::from(2)));
	let Message::Response(response) = response else {
		unreachable!()
	};
	assert_eq!(response.response_result.unwrap(), serde_json::Value::Null);
	// It is started again once; the second time, the user is told.
	assert!(client.shown_message().starts_with("LuaLS stopped"));
}

#[test]
fn semantic_tokens_without_luals_answer() {
	let workspace = Workspace::new("luals-tokens-stops", &[("nml.toml", "")]);
	let luals = fake_luals(&workspace, "sleep 1");
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	client.open(&page, ":lua`x = 1`\n");
	client.diagnostics();
	// LuaLS stops before it answers: NML's tokens.
	assert_eq!(
		client.semantic_tokens(&page),
		[
			(0, 0, 4, "keyword".into()),
			(0, 4, 1, "operator".into()),
			(0, 10, 1, "operator".into()),
		]
	);
}

#[test]
fn missing_luals_is_reported() {
	let workspace = Workspace::new("luals-missing", &[("nml.toml", "")]);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": "/nonexistent/lua-language-server" }),
	);
	let message = client.shown_message();
	assert!(message.contains("did not start"), "{message}");
}

#[test]
fn luals_references_renames_and_symbols() {
	let Some(luals) = luals_path() else {
		eprintln!("skipped: LuaLS not found (set NML_LUALS)");
		return;
	};
	let workspace = Workspace::new(
		"luals-more",
		&[
			("nml.toml", "[[module]]\nname = \"lib\"\nroot = \"lib\"\n"),
			(
				"lib/util.nml",
				":lua\n```\nfunction greet() return 'hi' end\n```\n",
			),
		],
	);
	let client = Client::start_with(
		Some(&workspace.0),
		serde_json::json!({ "luals": luals.to_string_lossy() }),
	);
	let page = workspace.url("a.nml");
	let text = ":import lib.util\n\n:lua\n```\nlocal count = 1\nfunction bump() count = count + 1 end\n```\n\n:lua`bump()` :lua`bump()` :lua`util.greet()`\n";
	client.open(&page, text);
	client.diagnostics();
	let starts = |result: &serde_json::Value, key: &str| -> Vec<lsp_types::Position> {
		let mut starts: Vec<_> = (result.as_array().unwrap().iter())
			.map(|item| serde_json::from_value(item[key]["start"].clone()).unwrap())
			.collect();
		starts.sort();
		starts
	};

	// The first request waits for LuaLS to load the workspace.
	let references = |line, character| {
		let params = lsp_types::ReferenceParams {
			text_document_position: at(&page, line, character),
			work_done_progress_params: Default::default(),
			partial_result_params: Default::default(),
			context: lsp_types::ReferenceContext {
				include_declaration: true,
			},
		};
		client.request(References::METHOD, serde_json::to_value(params).unwrap())
	};
	let result = references(5, 10);
	for location in result.as_array().unwrap() {
		assert_eq!(location["uri"].as_str(), Some(page.as_str()));
	}
	assert_eq!(
		starts(&result, "range"),
		[position(5, 9), position(8, 5), position(8, 18)]
	);

	let params = lsp_types::DocumentHighlightParams {
		text_document_position_params: at(&page, 4, 7),
		work_done_progress_params: Default::default(),
		partial_result_params: Default::default(),
	};
	let result = client.request(
		DocumentHighlightRequest::METHOD,
		serde_json::to_value(params).unwrap(),
	);
	assert_eq!(
		starts(&result, "range"),
		[position(4, 6), position(5, 16), position(5, 24)]
	);

	let rename = |line, character| {
		let params = lsp_types::RenameParams {
			text_document_position: at(&page, line, character),
			new_name: "step".into(),
			work_done_progress_params: Default::default(),
		};
		let request = Request::new(
			RequestId::from(1),
			Rename::METHOD.into(),
			serde_json::to_value(params).unwrap(),
		);
		client.connection.sender.send(request.into()).unwrap();
		let response = client
			.next(|message| matches!(message, Message::Response(r) if r.id == RequestId::from(1)));
		let Message::Response(response) = response else {
			unreachable!()
		};
		response.response_result
	};
	let result = rename(5, 10).unwrap();
	let edits = &result["changes"][page.as_str()];
	assert_eq!(
		starts(edits, "range"),
		[position(5, 9), position(8, 5), position(8, 18)]
	);
	// `greet` is the module's: it is not renamed here alone.
	let error = rename(8, 36).unwrap_err();
	assert!(error.message.contains("outside"), "{}", error.message);

	let params = lsp_types::DocumentSymbolParams {
		text_document: TextDocumentIdentifier::new(uri(&page)),
		work_done_progress_params: Default::default(),
		partial_result_params: Default::default(),
	};
	let result = client.request(
		DocumentSymbolRequest::METHOD,
		serde_json::to_value(params).unwrap(),
	);
	let names: Vec<_> = (result.as_array().unwrap().iter())
		.map(|symbol| symbol["name"].as_str().unwrap())
		.collect();
	assert_eq!(names, ["count", "bump"], "{result}");
}

/// A page from the preview server.
fn fetch(url: &str) -> String {
	use std::io::Read;
	use std::io::Write;
	let rest = url.strip_prefix("http://").unwrap();
	let (host, path) = rest.split_once('/').unwrap();
	let mut stream = std::net::TcpStream::connect(host).unwrap();
	write!(stream, "GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
	let mut response = String::new();
	stream.read_to_string(&mut response).unwrap();
	assert!(response.starts_with("HTTP/1.1 200"), "{response}");
	response.split_once("\r\n\r\n").unwrap().1.to_string()
}

#[test]
fn preview() {
	let workspace = Workspace::new(
		"preview",
		&[
			("nml.toml", ""),
			("index.nml", "# Home\n"),
			("docs/a.nml", "# A\nSaved.\n"),
			("lib/part.nml", "x"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let command = |arguments: Vec<serde_json::Value>| {
		serde_json::to_value(lsp_types::ExecuteCommandParams {
			command: PREVIEW_COMMAND.into(),
			arguments,
			work_done_progress_params: Default::default(),
		})
		.unwrap()
	};
	let a = workspace.url("docs/a.nml");
	let url = client.request(ExecuteCommand::METHOD, command(vec![a.as_str().into()]));
	let url = url.as_str().unwrap().to_string();
	assert!(url.starts_with("http://127.0.0.1:"), "{url}");
	assert!(url.ends_with("/docs/a.html"), "{url}");
	assert!(fetch(&url).contains("Saved."));

	// Unsaved edits show.
	client.open(&a, "# A\nUnsaved.\n");
	client.diagnostics();
	client.change(&a, "# A\nEdited.\n");
	// Published once the preview has the edit.
	client.diagnostics();
	let page = fetch(&url);
	assert!(page.contains("Edited."), "{page}");
	assert!(page.contains("/__nml/preview.js"), "{page}");

	// The same server; the index without a page.
	let again = client.request(ExecuteCommand::METHOD, command(vec![a.as_str().into()]));
	assert_eq!(again.as_str(), Some(url.as_str()));
	let root = client.request(ExecuteCommand::METHOD, command(vec![]));
	let root = root.as_str().unwrap();
	assert!(url.starts_with(root), "{root}");
	assert!(fetch(root).contains("Home"));

	// A page the workspace does not have.
	let message = client.request_error(
		ExecuteCommand::METHOD,
		command(vec![workspace.url("nope.nml").as_str().into()]),
	);
	assert!(message.contains("not a page"), "{message}");
	let message = client.request_error(ExecuteCommand::METHOD, command(vec![1.into()]));
	assert!(message.contains("takes the URI"), "{message}");
}

#[test]
fn preview_needs_a_workspace() {
	let client = Client::start(None);
	let params = serde_json::to_value(lsp_types::ExecuteCommandParams {
		command: PREVIEW_COMMAND.into(),
		arguments: vec![],
		work_done_progress_params: Default::default(),
	})
	.unwrap();
	let message = client.request_error(ExecuteCommand::METHOD, params);
	assert!(message.contains("needs a workspace"), "{message}");
}

/// The events a browser receives from the preview, by name and data.
struct Events(std::io::BufReader<std::net::TcpStream>);

impl Events {
	fn open(page_url: &str, page: &str) -> Self {
		use std::io::BufRead;
		use std::io::Write;
		let host = page_url
			.strip_prefix("http://")
			.unwrap()
			.split('/')
			.next()
			.unwrap();
		let mut stream = std::net::TcpStream::connect(host).unwrap();
		stream
			.set_read_timeout(Some(Duration::from_secs(10)))
			.unwrap();
		write!(
			stream,
			"GET /__nml/events?page={page} HTTP/1.1\r\nHost: {host}\r\n\r\n"
		)
		.unwrap();
		let mut reader = std::io::BufReader::new(stream);
		let mut line = String::new();
		while line != "\r\n" {
			line.clear();
			reader.read_line(&mut line).unwrap();
		}
		Self(reader)
	}

	/// The data of the next event named `name`.
	fn next_named(&mut self, name: &str) -> String {
		use std::io::BufRead;
		let mut event = String::new();
		let mut data = Vec::new();
		loop {
			let mut line = String::new();
			assert!(self.0.read_line(&mut line).expect("an event in time") > 0);
			let line = line.trim_end_matches('\n');
			if let Some(value) = line.strip_prefix("event: ") {
				event = value.to_string();
			} else if let Some(value) = line.strip_prefix("data: ") {
				data.push(value.to_string());
			} else if line.is_empty() {
				if event == name {
					return data.join("\n");
				}
				event.clear();
				data.clear();
			}
		}
	}
}

#[test]
fn preview_follows_the_cursor() {
	let workspace = Workspace::new(
		"preview-cursor",
		&[("nml.toml", ""), ("a.nml", "# A\n"), ("b.nml", "# B\n")],
	);
	let client = Client::start(Some(&workspace.0));
	let a = workspace.url("a.nml");
	let params = serde_json::to_value(lsp_types::ExecuteCommandParams {
		command: PREVIEW_COMMAND.into(),
		arguments: vec![a.as_str().into()],
		work_done_progress_params: Default::default(),
	})
	.unwrap();
	let url = client.request(ExecuteCommand::METHOD, params);
	let url = url.as_str().unwrap();
	fetch(url);
	let mut events = Events::open(url, "a.nml");
	// Listening once an edit arrives.
	client.open(&a, "# A\nLine é x.\n");
	client.diagnostics();
	assert!(events.next_named("main").contains("Line é x."));

	let cursor = |url: &Url, line, character| {
		let params = TextDocumentPositionParams {
			text_document: TextDocumentIdentifier::new(uri(url)),
			position: position(line, character),
		};
		let notification = Notification::new(CURSOR.into(), params);
		client.connection.sender.send(notification.into()).unwrap();
	};
	// Positions count UTF-16 units, offsets bytes: `é` is one unit and two bytes.
	cursor(&a, 1, 7);
	assert_eq!(events.next_named("scroll"), "12");
	cursor(&a, 1, 0);
	assert_eq!(events.next_named("scroll"), "4");
	// Documents that are not open, or not shown, send nothing; the next one arrives.
	cursor(&workspace.url("b.nml"), 0, 0);
	cursor(&a, 1, 2);
	assert_eq!(events.next_named("scroll"), "6");
}

#[test]
fn std_modules_are_written() {
	let workspace = Workspace::new("std-written", &[("nml.toml", "")]);
	let client = Client::start(Some(&workspace.0));
	// Answered once the server is up.
	let uri = workspace.url("a.nml");
	client.request(
		SemanticTokensFullRequest::METHOD,
		serde_json::json!({ "textDocument": { "uri": uri.as_str() } }),
	);
	for (name, content) in nml_lua::STD {
		let path = workspace
			.0
			.join(nml_lua::STD_DIR)
			.join(format!("{name}.nml"));
		assert_eq!(fs::read_to_string(path).unwrap(), *content);
	}
}

#[test]
fn definition_of_syntax_and_modules() {
	let kbd = ":: Keys.\n:lua\n```\nnml.rule {\n\tname = \"kbd\",\n\tatoms = { \":kbd{\", nml.capture(\"keys\", nml.inline), \"}\" },\n\thandler = function(m, out) out:content(m, \"keys\") end,\n}\n```\n";
	let workspace = Workspace::new(
		"definition-syntax",
		&[
			(
				"nml.toml",
				"exclude = [\"lib/**\"]\n[[module]]\nname = \"lib\"\nroot = \"lib\"\n",
			),
			("lib/kbd.nml", kbd),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("a.nml");
	let text = ":lua\n```\nnml.rule { name = \"shout\", atoms = { \":shout\" }, handler = function() end }\n```\n:import std.toc\n:import lib.kbd\n\n:toc Contents\n\nPress :kbd{**X**} :shout.\n";
	client.open(&page, text);
	client.diagnostics();

	let definition = |line, character| -> Option<lsp_types::Location> {
		let result = client.request(
			GotoDefinition::METHOD,
			serde_json::to_value(lsp_types::GotoDefinitionParams {
				text_document_position_params: at(&page, line, character),
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
			location.range.start.line,
			location.range.start.character,
		)
	};
	let std_toc = Url::from_file_path(workspace.0.join(nml_lua::STD_DIR).join("toc.nml")).unwrap();
	let rule_line = |source: &str| {
		source
			.lines()
			.position(|line| line.starts_with("nml.rule {"))
			.unwrap() as u32
	};
	let toc_source = nml_lua::STD.iter().find(|(n, _)| *n == "toc").unwrap().1;

	// `:toc`: the rule in the std module's copy.
	assert_eq!(
		place(definition(7, 2).unwrap()),
		(std_toc.to_string(), rule_line(toc_source), 0)
	);
	// `:kbd{`: in the workspace's module, on any of its markup.
	let lib = workspace.url("lib/kbd.nml").to_string();
	assert_eq!(place(definition(9, 7).unwrap()), (lib.clone(), 3, 0));
	assert_eq!(place(definition(9, 10).unwrap()), (lib.clone(), 3, 0));
	// In its content, the content's own syntax: bold is built in.
	assert_eq!(definition(9, 13), None);
	// A rule of the page's prologue.
	assert_eq!(place(definition(9, 20).unwrap()), (page.to_string(), 2, 0));
	// The imports: the modules' files, std's copy included.
	assert_eq!(
		place(definition(4, 10).unwrap()),
		(std_toc.to_string(), 0, 0)
	);
	assert_eq!(place(definition(5, 10).unwrap()), (lib, 0, 0));
	// Built-in syntax, and plain text.
	assert_eq!(definition(9, 11), None);
	assert_eq!(definition(9, 2), None);
}

#[test]
fn modules_are_documented() {
	let workspace = Workspace::new(
		"module-docs",
		&[
			(
				"nml.toml",
				"exclude = [\"lib/**\"]\n[[module]]\nname = \"lib\"\nroot = \"lib\"\n",
			),
			("lib/plain.nml", "No documentation.\n"),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let page = workspace.url("a.nml");
	client.open(&page, ":import std.toc\n:import lib.plain\n\n:toc\n");
	client.diagnostics();
	let hover = |line, character| -> Option<String> {
		let result = client.request(
			HoverRequest::METHOD,
			serde_json::to_value(HoverParams {
				text_document_position_params: at(&page, line, character),
				work_done_progress_params: Default::default(),
			})
			.unwrap(),
		);
		let hover: Option<lsp_types::Hover> = serde_json::from_value(result).unwrap();
		hover.map(|hover| match hover.contents {
			lsp_types::HoverContents::Markup(markup) => markup.value,
			_ => unreachable!(),
		})
	};
	let toc = hover(0, 10).unwrap();
	let toc_source = nml_lua::STD.iter().find(|(n, _)| *n == "toc").unwrap().1;
	assert!(
		toc.starts_with(&format!(
			"**std.toc**\n\n`std/toc.nml`\n\n{}",
			nml_lang::module_doc(toc_source)
		)),
		"{toc}"
	);
	assert!(
		toc.ends_with("**Syntax**\n\n* `toc`: **Table of contents** of the document, with an optional title: `:toc Contents`."),
		"{toc}"
	);
	// Without documentation or syntax: where it is.
	assert_eq!(hover(1, 10).unwrap(), "**lib.plain**\n\n`lib/plain.nml`");
	// The rest of the import is the rule's own documentation.
	assert!(hover(0, 2).unwrap().contains("**Import**"));
	// The syntax a module adds is documented too.
	assert!(hover(3, 1).unwrap().starts_with("**Table of contents**"));

	// Completed modules carry their documentation.
	client.change(&page, ":import std.");
	client.diagnostics();
	let params = lsp_types::CompletionParams {
		text_document_position: at(&page, 0, 12),
		work_done_progress_params: Default::default(),
		partial_result_params: Default::default(),
		context: None,
	};
	let result = client.request(Completion::METHOD, serde_json::to_value(params).unwrap());
	let items: Vec<lsp_types::CompletionItem> = serde_json::from_value(result).unwrap();
	let toc = items.iter().find(|item| item.label == "std.toc").unwrap();
	let Some(lsp_types::Documentation::MarkupContent(doc)) = &toc.documentation else {
		panic!("{toc:?}");
	};
	assert_eq!(doc.value, nml_lang::module_doc(toc_source));
}

#[test]
fn run_blocks_on_request() {
	let workspace = Workspace::new(
		"runs",
		&[
			("nml.toml", "[run.sh]\ncommand = [\"sh\"]\n"),
			(
				"a.nml",
				"Intro.\n\n:run #plot\n```sh\necho hello\nprintf x > \"$NML_OUT/p.svg\"\n```\n\n\
				![Plot](run:plot)\n\n:run\n```sh\necho bad >&2\nexit 2\n```\n",
			),
		],
	);
	let client = Client::start_full(
		Some(&workspace.0),
		serde_json::json!({ "luals": false }),
		serde_json::json!({ "window": { "showDocument": { "support": true } } }),
	);
	let a = workspace.url("a.nml");
	// Saving builds, without running.
	client.save(&a);
	client.open(&a, &fs::read_to_string(workspace.0.join("a.nml")).unwrap());
	let diagnostics = client.diagnostics();
	assert!(diagnostics.diagnostics.is_empty(), "{diagnostics:?}");
	assert!(!workspace.0.join(".nml/runs").exists());

	let lenses = |client: &Client| -> Vec<(u32, String, String)> {
		let params = serde_json::json!({ "textDocument": { "uri": a.as_str() } });
		let result = client.request(lsp_types::request::CodeLensRequest::METHOD, params);
		let lenses: Vec<lsp_types::CodeLens> = serde_json::from_value(result).unwrap();
		(lenses.into_iter())
			.map(|lens| {
				let command = lens.command.unwrap();
				(lens.range.start.line, command.title, command.command)
			})
			.collect()
	};
	let shown = |lenses: Vec<(u32, String, String)>| -> Vec<(u32, String, String)> {
		// Durations vary.
		(lenses.into_iter())
			.map(|(line, title, command)| {
				let title = match title.split_once(" in ").or(title.split_once(" after ")) {
					Some((status, rest)) => {
						format!("{status} ·{}", rest.split_once(" ·").unwrap().1)
					}
					None => title,
				};
				(line, title, command)
			})
			.collect()
	};
	let lens =
		|line: u32, title: &str, command: &str| (line, title.to_string(), command.to_string());
	assert_eq!(
		lenses(&client),
		[
			lens(2, "▶ Run", "nml.run"),
			lens(2, "▶ Run all (2 not run)", "nml.runAll"),
			lens(10, "▶ Run", "nml.run"),
		]
	);

	// One block, then the document is diagnosed again with its result.
	let params = serde_json::json!({ "command": "nml.run", "arguments": [a.as_str(), 2] });
	client.request(ExecuteCommand::METHOD, params);
	let diagnostics = client.diagnostics();
	assert!(diagnostics.diagnostics.is_empty(), "{diagnostics:?}");
	assert_eq!(
		shown(lenses(&client)),
		[
			lens(2, "✓ Completed · ▶ Run again", "nml.run"),
			lens(2, "≡ Output", "nml.showRunOutput"),
			lens(2, "▶ Run all (1 not run)", "nml.runAll"),
			lens(10, "▶ Run", "nml.run"),
		]
	);
	let hover = |line, character| {
		let result = client.request(
			HoverRequest::METHOD,
			serde_json::to_value(lsp_types::HoverParams {
				text_document_position_params: at(&a, line, character),
				work_done_progress_params: Default::default(),
			})
			.unwrap(),
		);
		let hover: Option<lsp_types::Hover> = serde_json::from_value(result).unwrap();
		hover.map(|hover| {
			let lsp_types::HoverContents::Markup(markup) = hover.contents else {
				unreachable!()
			};
			markup.value
		})
	};
	let value = hover(2, 7).unwrap();
	assert!(
		value.starts_with("**Run** `#plot` with `sh`: completed in "),
		"{value}"
	);
	assert!(
		value.ends_with("\n\nFiles: `p.svg`\n\nOutput:\n```text\nhello\n```"),
		"{value}"
	);

	// The others: the failure is an error of the document.
	let params = serde_json::json!({ "command": "nml.runAll", "arguments": [a.as_str()] });
	client.request(ExecuteCommand::METHOD, params);
	let diagnostics = client.diagnostics();
	assert_eq!(diagnostics.diagnostics.len(), 1, "{diagnostics:?}");
	assert_eq!(
		diagnostics.diagnostics[0].code,
		Some(NumberOrString::String("NML3017".into()))
	);
	assert_eq!(diagnostics.diagnostics[0].message, "the run failed: bad");
	assert_eq!(
		shown(lenses(&client)),
		[
			lens(2, "✓ Completed · ▶ Run again", "nml.run"),
			lens(2, "≡ Output", "nml.showRunOutput"),
			lens(2, "▶ Run all (1 failed)", "nml.runAll"),
			lens(10, "✗ Failed · ▶ Run again", "nml.run"),
			lens(10, "≡ Output", "nml.showRunOutput"),
		]
	);
	let value = hover(10, 2).unwrap();
	assert!(
		value.starts_with("**Run** with `sh`: failed after "),
		"{value}"
	);
	assert!(value.ends_with("Errors:\n```text\nbad\n```"), "{value}");
	// Only on the first line of a block: the fence has the syntax's documentation.
	let value = hover(11, 2).unwrap();
	assert!(value.starts_with("**Run** code with a runner"), "{value}");

	// The whole output, for clients that show it themselves.
	let output = client.request(
		ExecuteCommand::METHOD,
		serde_json::json!({ "command": "nml.runOutput", "arguments": [a.as_str(), 10] }),
	);
	assert_eq!(output["status"], "failed");
	assert_eq!(output["runner"], "sh");
	assert_eq!(output["name"], serde_json::Value::Null);
	assert_eq!(output["stdout"], "");
	assert_eq!(output["stderr"], "bad\n");
	assert_eq!(output["message"], "bad");
	let stderr = Url::parse(output["stderr_uri"].as_str().unwrap()).unwrap();
	assert_eq!(
		fs::read_to_string(stderr.to_file_path().unwrap()).unwrap(),
		"bad\n"
	);
	let output = client.request(
		ExecuteCommand::METHOD,
		serde_json::json!({ "command": "nml.runOutput", "arguments": [a.as_str(), 2] }),
	);
	assert_eq!(output["status"], "completed");
	assert_eq!(output["name"], "plot");
	assert_eq!(output["stdout"], "hello\n");
	assert_eq!(output["files"], serde_json::json!(["p.svg"]));

	// Or the client opens the files: only those with something in them.
	client.request(
		ExecuteCommand::METHOD,
		serde_json::json!({ "command": "nml.showRunOutput", "arguments": [a.as_str(), 2] }),
	);
	let request = client.server_request("window/showDocument");
	let params: lsp_types::ShowDocumentParams = serde_json::from_value(request.params).unwrap();
	assert!(
		params.uri.as_str().ends_with("/stdout.txt"),
		"{:?}",
		params.uri
	);
	let stdout = Url::parse(params.uri.as_str())
		.unwrap()
		.to_file_path()
		.unwrap();
	assert_eq!(fs::read_to_string(stdout).unwrap(), "hello\n");

	let error = client.request_error(
		ExecuteCommand::METHOD,
		serde_json::json!({ "command": "nml.run", "arguments": [a.as_str(), 5] }),
	);
	assert_eq!(error, "no `:run` block starts on line 6");
	drop(client);

	// A client that cannot open documents is told to use `nml.runOutput`.
	let client = Client::start(Some(&workspace.0));
	client.open(&a, &fs::read_to_string(workspace.0.join("a.nml")).unwrap());
	client.diagnostics();
	let error = client.request_error(
		ExecuteCommand::METHOD,
		serde_json::json!({ "command": "nml.showRunOutput", "arguments": [a.as_str(), 2] }),
	);
	assert_eq!(
		error,
		"the editor cannot open documents: `nml.runOutput` gives the output"
	);
}

#[test]
fn workspace_symbols() {
	let workspace = Workspace::new(
		"symbols",
		&[
			("nml.toml", ""),
			(
				"guide.nml",
				"# Getting started {#start}\n\nText.\n\n:export version = 1.2\n",
			),
			(
				"lib/parts.nml",
				":set nml.reference_key = parts\n# Parts {#parts}\n\n:anchor here\n",
			),
		],
	);
	let client = Client::start(Some(&workspace.0));
	let guide = workspace.url("guide.nml");
	client.save(&guide);
	// Unsaved: a new heading, and the old one renamed.
	client.open(
		&guide,
		"# Getting started {#begin}\n\n## Install {#install}\n\n:export version = 1.2\n",
	);
	client.diagnostics();
	let symbols = |query: &str| -> Vec<(String, lsp_types::SymbolKind, String, u32)> {
		let result = client.request(
			lsp_types::request::WorkspaceSymbolRequest::METHOD,
			serde_json::json!({ "query": query }),
		);
		let symbols: Vec<lsp_types::SymbolInformation> = serde_json::from_value(result).unwrap();
		(symbols.into_iter())
			.map(|symbol| {
				let uri = symbol.location.uri.as_str().to_string();
				let file = uri.rsplit('/').next().unwrap().to_string();
				(
					symbol.name,
					symbol.kind,
					file,
					symbol.location.range.start.line,
				)
			})
			.collect()
	};
	use lsp_types::SymbolKind as K;
	let symbol =
		|name: &str, kind, file: &str, line| (name.to_string(), kind, file.to_string(), line);
	assert_eq!(
		symbols(""),
		[
			symbol("guide.nml (Getting started)", K::FILE, "guide.nml", 0),
			symbol("lib/parts.nml (Parts)", K::FILE, "parts.nml", 0),
			symbol("Getting started (guide#begin)", K::STRING, "guide.nml", 0),
			symbol("Install (guide#install)", K::STRING, "guide.nml", 2),
			symbol("guide.version = 1.2", K::VARIABLE, "guide.nml", 4),
			symbol("Parts (parts#parts)", K::STRING, "parts.nml", 1),
			symbol("here (parts#here)", K::KEY, "parts.nml", 3),
		]
	);
	assert_eq!(
		symbols("install"),
		[symbol("Install (guide#install)", K::STRING, "guide.nml", 2)]
	);
	assert_eq!(
		symbols("parts.nml"),
		[symbol("lib/parts.nml (Parts)", K::FILE, "parts.nml", 0)]
	);
}

#[test]
fn folding_ranges() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/fold.nml").unwrap();
	client.open(
		&url,
		"# One\n\nText.\n\n## Two\n\n * a\n * b\n ** c\n\n:: x\n:: y\n\n\
		```rust\nfn f() {}\n```\n\n\
		> quoted\n> more\n\n\
		# Three\n\nA **bold\nline**.\n\n\n",
	);
	client.diagnostics();
	let result = client.request(
		lsp_types::request::FoldingRangeRequest::METHOD,
		serde_json::json!({ "textDocument": { "uri": url.as_str() } }),
	);
	let folds: Vec<lsp_types::FoldingRange> = serde_json::from_value(result).unwrap();
	let folds: Vec<_> = (folds.iter())
		.map(|fold| {
			let kind = match &fold.kind {
				Some(lsp_types::FoldingRangeKind::Region) => "section",
				Some(lsp_types::FoldingRangeKind::Comment) => "comment",
				_ => "block",
			};
			(fold.start_line, fold.end_line, kind)
		})
		.collect();
	assert_eq!(
		folds,
		[
			// Up to the next heading of its level, without the blank lines.
			(0, 18, "section"),
			(4, 18, "section"),
			(6, 8, "block"),
			(10, 11, "comment"),
			(13, 15, "block"),
			(17, 18, "block"),
			// To the last line with content; bold across lines does not fold.
			(20, 23, "section"),
		]
	);
}

/// Applies the edits of a `workspace/applyEdit` to an ASCII text.
fn apply_edits(text: &str, request: Request) -> String {
	let params: lsp_types::ApplyWorkspaceEditParams =
		serde_json::from_value(request.params).unwrap();
	let edits = params.edit.changes.unwrap().into_values().next().unwrap();
	let offset = |position: lsp_types::Position| {
		let start: usize = text
			.split_inclusive('\n')
			.take(position.line as usize)
			.map(str::len)
			.sum();
		start + position.character as usize
	};
	let mut edits: Vec<_> = (edits.into_iter())
		.map(|edit| {
			(
				offset(edit.range.start)..offset(edit.range.end),
				edit.new_text,
			)
		})
		.collect();
	edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
	let mut out = text.to_string();
	for (range, new) in edits {
		out.replace_range(range, &new);
	}
	out
}

#[test]
fn section_structure() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/outline.nml").unwrap();
	let text = "# A\n\na\n\n## A1\n\n```\n# not a heading\n```\n\n## A2\n\n### A2x\n\n# B\n\nb";
	client.open(&url, text);
	client.diagnostics();
	let run = |command: &str, line: u32| -> String {
		let params = serde_json::json!({ "command": command, "arguments": [url.as_str(), line] });
		client.request(ExecuteCommand::METHOD, params);
		apply_edits(text, client.server_request("workspace/applyEdit"))
	};
	// From inside a section: the heading above. The code block's `#` is not a heading.
	assert_eq!(
		run("nml.demote", 5),
		"# A\n\na\n\n### A1\n\n```\n# not a heading\n```\n\n## A2\n\n### A2x\n\n# B\n\nb"
	);
	assert_eq!(
		run("nml.promote", 10),
		"# A\n\na\n\n## A1\n\n```\n# not a heading\n```\n\n# A2\n\n## A2x\n\n# B\n\nb"
	);
	assert_eq!(
		run("nml.moveDown", 4),
		"# A\n\na\n\n## A2\n\n### A2x\n\n## A1\n\n```\n# not a heading\n```\n\n# B\n\nb"
	);
	assert_eq!(
		run("nml.moveUp", 10),
		"# A\n\na\n\n## A2\n\n### A2x\n\n## A1\n\n```\n# not a heading\n```\n\n# B\n\nb"
	);
	// The last section, without a final line break.
	// The last section, without a final line break: the blank lines between sections stay.
	assert_eq!(
		run("nml.moveUp", 14),
		"# B\n\nb\n\n# A\n\na\n\n## A1\n\n```\n# not a heading\n```\n\n## A2\n\n### A2x"
	);
	let error = |command: &str, line: u32| {
		let params = serde_json::json!({ "command": command, "arguments": [url.as_str(), line] });
		client.request_error(ExecuteCommand::METHOD, params)
	};
	assert_eq!(
		error("nml.promote", 0),
		"a level 1 heading cannot be promoted"
	);
	assert_eq!(
		error("nml.moveUp", 4),
		"no section of the same level before this one"
	);
	assert_eq!(
		error("nml.moveDown", 12),
		"no section of the same level after this one"
	);

	// Code actions on a heading's line, those that apply.
	let actions = |line: u32| -> Vec<String> {
		let result = client.request(
			CodeActionRequest::METHOD,
			serde_json::json!({
				"textDocument": { "uri": url.as_str() },
				"range": { "start": { "line": line, "character": 0 }, "end": { "line": line, "character": 0 } },
				"context": { "diagnostics": [] },
			}),
		);
		let actions: Vec<lsp_types::CodeActionOrCommand> = serde_json::from_value(result).unwrap();
		(actions.into_iter())
			.map(|action| match action {
				lsp_types::CodeActionOrCommand::CodeAction(action) => action.title,
				lsp_types::CodeActionOrCommand::Command(command) => command.title,
			})
			.collect()
	};
	assert_eq!(
		actions(4),
		[
			"Promote the section",
			"Demote the section",
			"Move the section down"
		]
		.map(String::from)
	);
	assert!(actions(5).is_empty());
}

#[test]
fn tables_are_aligned() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/tables.nml").unwrap();
	let text = "Text | not a table.\n\n:table #t Prices\n|Tea|3|\n|Coffee|4|\n\n\
		| a |[hspan=2] b |\n| c | d | e |\n\n> quoted:\n> |x|yy|\n\n|done   |ok |\n";
	client.open(&url, text);
	client.diagnostics();
	let result = client.request(
		lsp_types::request::Formatting::METHOD,
		serde_json::json!({
			"textDocument": { "uri": url.as_str() },
			"options": { "tabSize": 4, "insertSpaces": false },
		}),
	);
	let edits: Vec<lsp_types::TextEdit> = serde_json::from_value(result).unwrap();
	let params = serde_json::json!({
		"edit": { "changes": { url.as_str(): edits } },
	});
	let formatted = apply_edits(
		text,
		Request::new(RequestId::from(0), String::new(), params),
	);
	assert_eq!(
		formatted,
		"Text | not a table.\n\n:table #t Prices\n| Tea    | 3   |\n| Coffee | 4   |\n\n\
		| a |[hspan=2] b |\n| c | d | e |\n\n> quoted:\n> | x   | yy  |\n\n| done | ok  |\n"
	);

	// Only the table in the range.
	let result = client.request(
		lsp_types::request::RangeFormatting::METHOD,
		serde_json::json!({
			"textDocument": { "uri": url.as_str() },
			"range": { "start": { "line": 12, "character": 0 }, "end": { "line": 12, "character": 2 } },
			"options": { "tabSize": 4, "insertSpaces": false },
		}),
	);
	let edits: Vec<lsp_types::TextEdit> = serde_json::from_value(result).unwrap();
	assert_eq!(edits.len(), 1);
	assert_eq!(edits[0].new_text, "| done | ok  |");
}

#[test]
fn progress_and_checkboxes() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/tasks.nml").unwrap();
	let text = "# Plan [%]\n\n * Ship [/]\n ** [x] a\n ** [ ] b\n ** [X] c\n";
	client.open(&url, text);
	client.diagnostics();
	let params = lsp_types::InlayHintParams {
		text_document: TextDocumentIdentifier::new(uri(&url)),
		range: lsp_types::Range::new(position(0, 0), position(9, 0)),
		work_done_progress_params: Default::default(),
	};
	let result = client.request(
		InlayHintRequest::METHOD,
		serde_json::to_value(params).unwrap(),
	);
	let hints: Vec<lsp_types::InlayHint> = serde_json::from_value(result).unwrap();
	let hints: Vec<_> = (hints.into_iter())
		.map(|hint| {
			let lsp_types::InlayHintLabel::String(label) = hint.label else {
				unreachable!()
			};
			(hint.position, label)
		})
		.collect();
	assert_eq!(
		hints,
		[
			(position(0, 10), "[66%]".to_string()),
			(position(2, 11), "[2/3]".to_string())
		]
	);

	// Toggling a checkbox: done if it was not, not done if it was.
	let toggle = |line: u32| -> Vec<(lsp_types::Range, String)> {
		let result = client.request(
			CodeActionRequest::METHOD,
			serde_json::json!({
				"textDocument": { "uri": url.as_str() },
				"range": { "start": { "line": line, "character": 3 }, "end": { "line": line, "character": 3 } },
				"context": { "diagnostics": [] },
			}),
		);
		let actions: Vec<lsp_types::CodeActionOrCommand> = serde_json::from_value(result).unwrap();
		(actions.into_iter())
			.filter_map(|action| match action {
				lsp_types::CodeActionOrCommand::CodeAction(action)
					if action.title == "Toggle the checkbox" =>
				{
					let edits = action
						.edit
						.unwrap()
						.changes
						.unwrap()
						.into_values()
						.next()
						.unwrap();
					Some((edits[0].range, edits[0].new_text.clone()))
				}
				_ => None,
			})
			.collect()
	};
	let range = |line| lsp_types::Range::new(position(line, 4), position(line, 7));
	assert_eq!(toggle(3), [(range(3), "[ ]".to_string())]);
	assert_eq!(toggle(4), [(range(4), "[x]".to_string())]);
	assert_eq!(toggle(5), [(range(5), "[ ]".to_string())]);
	assert!(toggle(2).is_empty());
}
