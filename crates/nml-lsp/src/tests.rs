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
