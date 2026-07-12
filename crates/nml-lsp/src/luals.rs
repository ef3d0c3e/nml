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
use nml_source::Url;
use serde_json::Value;

/// How long a forwarded request may take.
const TIMEOUT: Duration = Duration::from_secs(10);

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

/// A running LuaLS.
pub(crate) struct LuaLs {
	child: Child,
	stdin: BufWriter<ChildStdin>,
	/// Messages from LuaLS, read by a thread.
	pub receiver: Receiver<Message>,
	next_id: i32,
	/// The version of each virtual document LuaLS has, by `.nml` document.
	versions: HashMap<Url, i32>,
	/// Notifications received while waiting for a response.
	pending: Vec<Notification>,
}

impl std::fmt::Debug for LuaLs {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("LuaLs").finish_non_exhaustive()
	}
}

impl LuaLs {
	/// Starts LuaLS for the workspace at `root` and initializes it.
	pub fn start(program: &Path, root: &Path) -> io::Result<Self> {
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
			pending: Vec::new(),
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
				},
				"workspace": { "configuration": true, "workspaceFolders": true },
			},
		});
		luals
			.request("initialize", params)
			.ok_or(io::ErrorKind::TimedOut)?;
		luals.notify("initialized", serde_json::json!({}))?;
		Ok(luals)
	}

	fn send(&mut self, message: Message) -> io::Result<()> {
		message.write(&mut self.stdin)
	}

	pub fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
		self.send(Notification::new(method.into(), params).into())
	}

	/// Sends a request and waits for its result. Requests from LuaLS are answered and its
	/// notifications kept meanwhile.
	pub fn request(&mut self, method: &str, params: Value) -> Option<Value> {
		self.next_id += 1;
		let id = RequestId::from(self.next_id);
		self.send(Request::new(id.clone(), method.into(), params).into())
			.ok()?;
		loop {
			match self.receiver.recv_timeout(TIMEOUT).ok()? {
				Message::Response(response) if response.id == id => {
					return response.response_result.ok();
				}
				Message::Response(_) => {}
				Message::Request(request) => self.answer(request),
				Message::Notification(notification) => self.pending.push(notification),
			}
		}
	}

	/// Answers a request from LuaLS: settings are left to `.luarc.json`, and the rest needs
	/// no answer beyond an acknowledgement.
	pub fn answer(&mut self, request: Request) {
		let result = match request.method.as_str() {
			"workspace/configuration" => {
				let items = request.params["items"].as_array().map_or(0, Vec::len);
				Value::Array(vec![Value::Null; items])
			}
			_ => Value::Null,
		};
		let _ = self.send(Response::new_ok(request.id, result).into());
	}

	/// The notifications kept while waiting for responses.
	pub fn take_pending(&mut self) -> Vec<Notification> {
		std::mem::take(&mut self.pending)
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
}

impl Drop for LuaLs {
	fn drop(&mut self) {
		let _ = self.request("shutdown", Value::Null);
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
