//! A LuaLS client: the server forwards what concerns the Lua of a document to LuaLS, which
//! sees each document's Lua as one virtual Lua document (NML-DOC-6, option 1).

use std::collections::HashMap;
use std::io;
use std::io::BufReader;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use crossbeam_channel::Receiver;
use lsp_server::Message;
use lsp_server::Notification;
use lsp_server::Request;
use lsp_server::RequestId;
use lsp_server::Response;
use lsp_types::SemanticTokensLegend;
use nml_source::Url;
use serde_json::Value;

/// Finds LuaLS: `NML_LUALS`, then `lua-language-server` on the `PATH`.
pub(crate) fn find() -> Option<PathBuf> {
	if let Some(path) = std::env::var_os("NML_LUALS") {
		return Some(path.into());
	}
	let paths = std::env::var_os("PATH")?;
	std::env::split_paths(&paths)
		.map(|dir| dir.join("lua-language-server"))
		.find(|path| path.is_file())
}

/// The URL of the virtual Lua document of a `.nml` document, next to it.
pub(crate) fn virtual_url(document: &Url) -> Url {
	Url::parse(&format!("{document}.lua")).unwrap()
}

/// The `.nml` document of a virtual Lua document.
pub(crate) fn document_url(virtual_url: &Url) -> Option<Url> {
	Url::parse(virtual_url.as_str().strip_suffix(".lua")?).ok()
}

/// A running LuaLS. Messages for it wait until it is initialized; requests do not wait for
/// their response, which comes through `receiver`.
pub(crate) struct LuaLs {
	child: Child,
	stdin: BufWriter<ChildStdin>,
	/// Messages from LuaLS, read by a thread. It disconnects when LuaLS stops.
	pub receiver: Receiver<Message>,
	next_id: i32,
	/// The version of each virtual document LuaLS has, by `.nml` document.
	versions: HashMap<Url, i32>,
	/// The `initialize` request, until LuaLS answers it.
	initializing: Option<RequestId>,
	/// Messages sent before LuaLS was initialized.
	queue: Vec<Message>,
	/// The settings LuaLS is given.
	settings: Value,
	/// The legend of LuaLS's semantic tokens, once it has given it.
	pub legend: Option<SemanticTokensLegend>,
}

impl std::fmt::Debug for LuaLs {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LuaLs").finish_non_exhaustive()
	}
}

impl LuaLs {
	/// Starts LuaLS for the workspace at `root`, and asks it to initialize. The declarations
	/// of the API are written first, so that LuaLS finds them current.
	pub fn start(program: &Path, root: &Path) -> io::Result<Self> {
		nml_lua::write_types(root)?;
		let mut child = Command::new(program)
			.current_dir(root)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.spawn()?;
		let stdin = BufWriter::new(child.stdin.take().unwrap());
		let mut stdout = BufReader::new(child.stdout.take().unwrap());
		let (sender, receiver) = crossbeam_channel::unbounded();
		std::thread::spawn(move || {
			while let Ok(Some(message)) = Message::read(&mut stdout) {
				if sender.send(message).is_err() {
					break;
				}
			}
		});
		let mut luals = Self {
			child,
			stdin,
			receiver,
			next_id: 0,
			versions: HashMap::new(),
			initializing: None,
			queue: Vec::new(),
			settings: settings(root),
			legend: None,
		};
		let root_url = Url::from_directory_path(root).map_err(|()| io::ErrorKind::InvalidInput)?;
		let params = serde_json::json!({
			"processId": std::process::id(),
			"rootUri": root_url.as_str(),
			"workspaceFolders": [{ "uri": root_url.as_str(), "name": "nml" }],
			"capabilities": {
				"textDocument": {
					"hover": { "contentFormat": ["markdown", "plaintext"] },
					"completion": { "completionItem": { "snippetSupport": false } },
					"publishDiagnostics": {},
					"signatureHelp": {},
					"semanticTokens": {
						"requests": { "full": true },
						"tokenTypes": [],
						"tokenModifiers": [],
						"formats": ["relative"],
					},
				},
				"workspace": { "configuration": true, "workspaceFolders": true },
			},
		});
		let id = luals.next_id();
		Message::from(Request::new(id.clone(), "initialize".into(), params))
			.write(&mut luals.stdin)?;
		luals.initializing = Some(id);
		Ok(luals)
	}

	fn next_id(&mut self) -> RequestId {
		self.next_id += 1;
		RequestId::from(self.next_id)
	}

	fn send(&mut self, message: Message) -> io::Result<()> {
		if self.initializing.is_some() {
			self.queue.push(message);
			return Ok(());
		}
		message.write(&mut self.stdin)
	}

	pub fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
		self.send(Notification::new(method.into(), params).into())
	}

	/// Sends a request, and returns its id: its response comes through `receiver`.
	pub fn request(&mut self, method: &str, params: Value) -> io::Result<RequestId> {
		let id = self.next_id();
		self.send(Request::new(id.clone(), method.into(), params).into())?;
		Ok(id)
	}

	/// Handles the response to `initialize`: LuaLS is told it is initialized, and gets the
	/// messages that waited. Other responses are returned.
	pub fn initialized(&mut self, response: Response) -> io::Result<Option<Response>> {
		if self.initializing.as_ref() != Some(&response.id) {
			return Ok(Some(response));
		}
		self.initializing = None;
		if let Ok(result) = &response.response_result {
			self.found_legend(&result["capabilities"]);
		}
		self.notify("initialized", serde_json::json!({}))?;
		for message in std::mem::take(&mut self.queue) {
			message.write(&mut self.stdin)?;
		}
		Ok(None)
	}

	/// Answers a request from LuaLS: the `Lua` settings are NML's (a `.luarc.json` takes
	/// precedence over them), and the rest needs no answer beyond an acknowledgement.
	pub fn answer(&mut self, request: Request) {
		let result = match request.method.as_str() {
			"client/registerCapability" => {
				let registrations = request.params["registrations"]
					.as_array()
					.cloned()
					.unwrap_or_default();
				for registration in registrations {
					self.found_legend(&registration["registerOptions"]);
				}
				Value::Null
			}
			"workspace/configuration" => {
				let items = request.params["items"]
					.as_array()
					.cloned()
					.unwrap_or_default();
				let section = |item: &Value| match item["section"].as_str() {
					Some("Lua") => self.settings.clone(),
					_ => Value::Null,
				};
				items.iter().map(section).collect()
			}
			_ => Value::Null,
		};
		// Answers go out even before LuaLS is initialized: it may ask during `initialize`.
		let _ = Message::from(Response::new_ok(request.id, result)).write(&mut self.stdin);
	}

	/// Keeps the legend of semantic tokens in `options`, if it has one: a server's
	/// capabilities, or the options of a registration.
	fn found_legend(&mut self, options: &Value) {
		let provider = options.get("semanticTokensProvider").unwrap_or(options);
		if let Ok(legend) = serde_json::from_value(provider["legend"].clone()) {
			self.legend = Some(legend);
		}
	}

	/// Gives LuaLS the current virtual Lua document of a `.nml` document.
	pub fn sync(&mut self, document: &Url, content: &str) -> io::Result<()> {
		let uri = virtual_url(document);
		match self.versions.get_mut(document) {
			Some(version) => {
				*version += 1;
				let params = serde_json::json!({
					"textDocument": { "uri": uri.as_str(), "version": *version },
					"contentChanges": [{ "text": content }],
				});
				self.notify("textDocument/didChange", params)
			}
			None => {
				self.versions.insert(document.clone(), 0);
				let params = serde_json::json!({
					"textDocument": {
						"uri": uri.as_str(),
						"languageId": "lua",
						"version": 0,
						"text": content,
					},
				});
				self.notify("textDocument/didOpen", params)
			}
		}
	}

	/// Closes the virtual document of a `.nml` document.
	pub fn close(&mut self, document: &Url) -> io::Result<()> {
		if self.versions.remove(document).is_some() {
			let uri = virtual_url(document);
			let params = serde_json::json!({ "textDocument": { "uri": uri.as_str() } });
			self.notify("textDocument/didClose", params)?;
		}
		Ok(())
	}

	/// Asks LuaLS to shut down and waits a moment for its answer.
	fn shutdown(&mut self) {
		if self.initializing.is_some() {
			return;
		}
		let Ok(id) = self.request("shutdown", Value::Null) else {
			return;
		};
		while let Ok(message) = self.receiver.recv_timeout(Duration::from_secs(2)) {
			if let Message::Response(response) = message
				&& response.id == id
			{
				break;
			}
		}
	}
}

/// The `Lua` settings of LuaLS: Lua 5.4 without what the sandbox leaves out, and the
/// declarations of the API.
fn settings(root: &Path) -> Value {
	let builtin: serde_json::Map<String, Value> = (nml_lua::DISABLED_LIBRARIES.iter())
		.map(|library| (library.to_string(), Value::from("disable")))
		.collect();
	serde_json::json!({
		"runtime": { "version": "Lua 5.4", "builtin": builtin },
		"workspace": { "library": [root.join(nml_lua::TYPES_DIR).to_string_lossy()] },
		"diagnostics": { "globals": ["nml"] },
		// Keywords, strings and numbers too: the editor knows no Lua in a `.nml` document.
		"semantic": { "keyword": true },
	})
}

impl Drop for LuaLs {
	fn drop(&mut self) {
		self.shutdown();
		let _ = self.notify("exit", Value::Null);
		// LuaLS gets a moment to exit, then is stopped.
		for _ in 0..20 {
			if let Ok(Some(_)) = self.child.try_wait() {
				return;
			}
			std::thread::sleep(Duration::from_millis(100));
		}
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn virtual_urls() {
		let document = Url::parse("file:///w/docs/a.nml").unwrap();
		let lua = virtual_url(&document);
		assert_eq!(lua.as_str(), "file:///w/docs/a.nml.lua");
		assert_eq!(document_url(&lua), Some(document));
		assert_eq!(document_url(&Url::parse("file:///w/x.txt").unwrap()), None);
	}
}
