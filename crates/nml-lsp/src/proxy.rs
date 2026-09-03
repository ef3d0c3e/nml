//! Forwarding what concerns a document's Lua to LuaLS, with positions translated both ways.

use std::collections::HashMap;

use lsp_server::Message;
use lsp_server::Notification;
use lsp_server::Request;
use lsp_server::RequestId;
use lsp_server::Response;
use lsp_types::Range;
use nml_source::Url;
use serde_json::Value;

use crate::Result;
use crate::Server;
use crate::lua::LuaDocument;
use crate::luals;
use crate::semantic;

/// The requests forwarded when their position is in Lua.
const FORWARDED: &[&str] = &[
	"textDocument/hover",
	"textDocument/completion",
	"textDocument/definition",
	"textDocument/signatureHelp",
	"textDocument/references",
	"textDocument/documentHighlight",
	"textDocument/rename",
];

/// The requests about a whole document, forwarded when it has Lua.
const FORWARDED_DOCUMENT: &[&str] = &["textDocument/documentSymbol", SEMANTIC_TOKENS];

/// Semantic tokens: LuaLS's for the Lua, merged with NML's.
const SEMANTIC_TOKENS: &str = "textDocument/semanticTokens/full";

fn range(value: &Value) -> Option<Range> {
	serde_json::from_value(value.clone()).ok()
}

/// Maps the range at `value[key]` to the `.nml` document; `false` if it is not in Lua.
fn map_range(value: &mut Value, key: &str, document: &LuaDocument) -> bool {
	let Some(lua) = value.get(key).and_then(range) else {
		return false;
	};
	match document.range_to_nml(lua) {
		Some(nml) => {
			value[key] = serde_json::to_value(nml).unwrap();
			true
		}
		None => false,
	}
}

/// A request forwarded to LuaLS, until it answers.
pub(crate) struct Forwarded {
	/// The client's request.
	id: RequestId,
	method: String,
	url: Url,
	/// The virtual document the request was made in.
	document: LuaDocument,
	/// For semantic tokens, NML's.
	tokens: Vec<semantic::Token>,
}

impl Forwarded {
	/// The result if LuaLS does not answer: NML's semantic tokens, or nothing.
	fn fallback(&self) -> Value {
		if self.method == SEMANTIC_TOKENS {
			semantic_tokens(self.tokens.clone())
		} else {
			Value::Null
		}
	}
}

fn semantic_tokens(tokens: Vec<semantic::Token>) -> Value {
	let tokens = lsp_types::SemanticTokens {
		result_id: None,
		data: semantic::encode(&tokens),
	};
	serde_json::to_value(tokens).unwrap()
}

/// The virtual documents results may be in: the one a request was made in, as it was
/// then, and the other open documents'.
struct Documents<'a> {
	forwarded: &'a Forwarded,
	open: &'a HashMap<Url, LuaDocument>,
}

impl Documents<'_> {
	/// The `.nml` document of a virtual document, and its Lua.
	fn get(&self, uri: &str) -> Option<(Url, &LuaDocument)> {
		let url = luals::document_url(&Url::parse(uri).ok()?)?;
		if url == self.forwarded.url {
			return Some((url, &self.forwarded.document));
		}
		let document = self.open.get(&url)?;
		Some((url, document))
	}

	/// Maps a location (`uri_key` and its `range_keys`) to its `.nml` document. `false` if
	/// it is in a virtual document but not in a document's own code; locations elsewhere
	/// (the declarations of the API) are kept as they are.
	fn map_location(&self, location: &mut Value, uri_key: &str, range_keys: &[&str]) -> bool {
		let Some((url, document)) = location[uri_key].as_str().and_then(|uri| self.get(uri)) else {
			return true;
		};
		location[uri_key] = Value::from(url.as_str());
		range_keys
			.iter()
			.all(|key| map_range(location, key, document))
	}
}

/// Maps the ranges of document symbols to the document, recursively. Symbols outside its
/// own code are dropped, with their children.
fn map_symbols(symbols: &mut Vec<Value>, document: &LuaDocument) {
	symbols.retain_mut(|symbol| {
		if symbol.get("location").is_some() {
			// A `SymbolInformation`: its location is in the virtual document.
			let location = &mut symbol["location"];
			return map_range(location, "range", document);
		}
		if !map_range(symbol, "range", document) || !map_range(symbol, "selectionRange", document) {
			return false;
		}
		if let Some(children) = symbol.get_mut("children").and_then(Value::as_array_mut) {
			map_symbols(children, document);
		}
		true
	});
}

/// A result of LuaLS, mapped from virtual documents to `.nml` documents. What is not in a
/// document's own code is dropped; a rename that would change other code fails.
fn map_result(documents: &Documents<'_>, mut result: Value) -> std::result::Result<Value, String> {
	let forwarded = documents.forwarded;
	let document = &forwarded.document;
	match forwarded.method.as_str() {
		"textDocument/hover" => {
			if result.is_object() && !map_range(&mut result, "range", document) {
				result.as_object_mut().unwrap().remove("range");
			}
		}
		"textDocument/completion" => {
			let items = match &mut result {
				Value::Array(items) => Some(items),
				Value::Object(list) => list.get_mut("items").and_then(Value::as_array_mut),
				_ => None,
			};
			for item in items.into_iter().flatten() {
				let Some(item) = item.as_object_mut() else {
					continue;
				};
				item.remove("additionalTextEdits");
				if let Some(edit) = item.get_mut("textEdit") {
					let mapped = if edit.get("range").is_some() {
						map_range(edit, "range", document)
					} else {
						map_range(edit, "insert", document) && map_range(edit, "replace", document)
					};
					if !mapped {
						item.remove("textEdit");
					}
				}
			}
		}
		"textDocument/definition" | "textDocument/references" => {
			let mut locations = match result {
				Value::Array(locations) => locations,
				Value::Null => Vec::new(),
				location => vec![location],
			};
			locations.retain_mut(|location| {
				if location.get("targetUri").is_some() {
					documents.map_location(
						location,
						"targetUri",
						&["targetRange", "targetSelectionRange"],
					)
				} else {
					documents.map_location(location, "uri", &["range"])
				}
			});
			result = Value::Array(locations);
		}
		"textDocument/documentHighlight" => {
			if let Some(highlights) = result.as_array_mut() {
				highlights.retain_mut(|highlight| map_range(highlight, "range", document));
			}
		}
		"textDocument/documentSymbol" => {
			if let Some(symbols) = result.as_array_mut() {
				map_symbols(symbols, document);
			}
		}
		"textDocument/rename" => return map_rename(documents, result),
		_ => {}
	}
	Ok(result)
}

/// Maps the edits of a rename. Every edit must be in a document's own code: a name defined
/// or used elsewhere (a module, an includer) is not renamed in part.
fn map_rename(documents: &Documents<'_>, mut result: Value) -> std::result::Result<Value, String> {
	let refused = || "this name is also used outside the document's own Lua".to_string();
	if result.is_null() {
		return Ok(result);
	}
	if result.get("documentChanges").is_some() {
		return Err(refused());
	}
	let changes = result["changes"].as_object_mut().ok_or_else(refused)?;
	let mut mapped = serde_json::Map::new();
	for (uri, edits) in std::mem::take(changes) {
		let (url, document) = documents.get(&uri).ok_or_else(refused)?;
		let mut edits = edits.as_array().cloned().unwrap_or_default();
		if !edits
			.iter_mut()
			.all(|edit| map_range(edit, "range", document))
		{
			return Err(refused());
		}
		mapped.insert(url.to_string(), Value::Array(edits));
	}
	*changes = mapped;
	Ok(result)
}

impl Server {
	/// Forwards a request about a position in Lua to LuaLS: its response is sent when LuaLS
	/// answers. Whether the request was forwarded.
	pub(crate) fn forward(&mut self, request: &Request) -> Result<bool> {
		let (method, params) = (request.method.as_str(), &request.params);
		let whole = FORWARDED_DOCUMENT.contains(&method);
		if !(whole || FORWARDED.contains(&method)) || self.luals.is_none() {
			return Ok(false);
		}
		let Some(url) = params["textDocument"]["uri"]
			.as_str()
			.and_then(|uri| Url::parse(uri).ok())
		else {
			return Ok(false);
		};
		let Some(document) = self.lua_documents.get(&url) else {
			return Ok(false);
		};
		let mut params = params.clone();
		if whole {
			if document.fragments.is_empty() {
				return Ok(false);
			}
		} else {
			let Some(position) = serde_json::from_value(params["position"].clone()).ok() else {
				return Ok(false);
			};
			let Some(lua_position) = document.to_lua(position) else {
				return Ok(false);
			};
			params["position"] = serde_json::to_value(lua_position)?;
		}
		let document = document.clone();
		let tokens = match (method, self.analyses.get(&url)) {
			(SEMANTIC_TOKENS, Some(analysis)) => semantic::tokens(&analysis.tree, &url),
			_ => Vec::new(),
		};
		params["textDocument"]["uri"] = Value::from(luals::virtual_url(&url).as_str());
		let luals = self.luals.as_mut().unwrap();
		let Ok(lua_id) = luals.request(method, params) else {
			return Ok(false);
		};
		let forwarded = Forwarded {
			id: request.id.clone(),
			method: method.to_string(),
			url,
			document,
			tokens,
		};
		self.forwarded.insert(lua_id, forwarded);
		Ok(true)
	}

	/// Answers the client's request that a response from LuaLS is for, with its result
	/// mapped back to the `.nml` document.
	fn forwarded_response(&mut self, response: Response) -> Result<()> {
		let Some(forwarded) = self.forwarded.remove(&response.id) else {
			return Ok(());
		};
		let Ok(result) = response.response_result else {
			let response = Response::new_ok(forwarded.id.clone(), forwarded.fallback());
			self.connection.sender.send(response.into())?;
			return Ok(());
		};
		if forwarded.method == SEMANTIC_TOKENS {
			let data: Vec<u32> = serde_json::from_value(result["data"].clone()).unwrap_or_default();
			let lua = match self.luals.as_ref().and_then(|luals| luals.legend.as_ref()) {
				Some(legend) => semantic::lua_tokens(&data, legend, &forwarded.document),
				None => Vec::new(),
			};
			let tokens = semantic::merge(forwarded.tokens, lua);
			let response = Response::new_ok(forwarded.id, semantic_tokens(tokens));
			self.connection.sender.send(response.into())?;
			return Ok(());
		}
		let documents = Documents {
			forwarded: &forwarded,
			open: &self.lua_documents,
		};
		let response = match map_result(&documents, result) {
			Ok(result) => Response::new_ok(forwarded.id, result),
			Err(message) => Response::new_err(
				forwarded.id,
				lsp_server::ErrorCode::RequestFailed as i32,
				message,
			),
		};
		self.connection.sender.send(response.into())?;
		Ok(())
	}

	/// Answers the requests still waiting for LuaLS, which stopped, with no result.
	pub(crate) fn drop_forwarded(&mut self) -> Result<()> {
		for (_, forwarded) in std::mem::take(&mut self.forwarded) {
			let response = Response::new_ok(forwarded.id.clone(), forwarded.fallback());
			self.connection.sender.send(response.into())?;
		}
		Ok(())
	}

	/// Handles a message from LuaLS outside of a request.
	pub(crate) fn lua_message(&mut self, message: Message) -> Result<()> {
		match message {
			Message::Request(request) => {
				if let Some(luals) = &mut self.luals {
					luals.answer(request);
				}
			}
			Message::Notification(notification) => self.lua_notification(notification)?,
			Message::Response(response) => {
				let Some(luals) = &mut self.luals else {
					return Ok(());
				};
				if let Some(response) = luals.initialized(response)? {
					self.forwarded_response(response)?;
				}
			}
		}
		Ok(())
	}

	/// LuaLS's diagnostics, mapped to the `.nml` document and published with NML's.
	fn lua_notification(&mut self, notification: Notification) -> Result<()> {
		if notification.method != "textDocument/publishDiagnostics" {
			return Ok(());
		}
		let Some(lua_url) = notification.params["uri"]
			.as_str()
			.and_then(|uri| Url::parse(uri).ok())
		else {
			return Ok(());
		};
		let Some(url) = luals::document_url(&lua_url) else {
			return Ok(());
		};
		let Some(document) = self.lua_documents.get(&url) else {
			return Ok(());
		};
		let diagnostics: Vec<lsp_types::Diagnostic> =
			serde_json::from_value(notification.params["diagnostics"].clone()).unwrap_or_default();
		let mapped = diagnostics
			.into_iter()
			.filter_map(|mut diagnostic| {
				diagnostic.range = document.range_to_nml(diagnostic.range)?;
				diagnostic.related_information = None;
				Some(diagnostic)
			})
			.collect();
		self.lua_diagnostics.insert(url.clone(), mapped);
		self.publish_document(&url)
	}
}
