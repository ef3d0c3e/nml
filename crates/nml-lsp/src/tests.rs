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
	assert_eq!(complete("# Home {#top}\nSee [[#"), ["install", "top"]);
	// Commands at the start of a line, roles elsewhere.
	assert_eq!(complete(":i"), [":import", ":include"]);
	assert_eq!(complete("x :l"), [":lua"]);
	assert_eq!(complete(":import st"), ["std.util"]);
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
	client.open(&url, "# Title\na **b *c*** [[x|y]] `z`\n");
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
			(22, 23)
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
fn lua_documents() {
	let client = Client::start(None);
	let url = Url::parse("file:///tmp/nml-lsp-none/a.nml").unwrap();
	client.open(
		&url,
		":lua\n```\nx = 1\nfunction f() return x end\n```\n\nA :lua`f()` and :lua`x = 2`.\n",
	);
	client.diagnostics();
	let params = serde_json::json!({ "textDocument": { "uri": url.as_str() } });

	let result = client.request(lua::CODE_RANGE, params.clone());
	let ranges: Vec<lua::CodeRangeInfo> = serde_json::from_value(result).unwrap();
	assert_eq!(ranges.len(), 3);
	assert_eq!(ranges[0].range.start, position(2, 0));
	assert_eq!(ranges[0].language, "lua");

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
