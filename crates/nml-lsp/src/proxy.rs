//! Forwarding what concerns a document's Lua to LuaLS, with positions translated both ways.

use lsp_server::Message;
use lsp_server::Notification;
use lsp_types::Range;
use nml_source::Url;
use serde_json::Value;

use crate::Result;
use crate::Server;
use crate::lua::LuaDocument;
use crate::luals;

/// The requests forwarded when their position is in Lua.
const FORWARDED: &[&str] = &[
	"textDocument/hover",
	"textDocument/completion",
	"textDocument/definition",
	"textDocument/signatureHelp",
];

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

impl Server {
	/// Forwards a request about a position in Lua to LuaLS, and maps its result back.
	/// `None` if the request is not about Lua.
	pub(crate) fn forward(&mut self, method: &str, params: &Value) -> Result<Option<Value>> {
		if !FORWARDED.contains(&method) || self.luals.is_none() {
			return Ok(None);
		}
		let Some(url) = params["textDocument"]["uri"]
			.as_str()
			.and_then(|uri| Url::parse(uri).ok())
		else {
			return Ok(None);
		};
		let Some(document) = self.lua_documents.get(&url) else {
			return Ok(None);
		};
		let Some(position) = serde_json::from_value(params["position"].clone()).ok() else {
			return Ok(None);
		};
		let Some(lua_position) = document.to_lua(position) else {
			return Ok(None);
		};
		let document = document.clone();
		let virtual_url = luals::virtual_url(&url);
		let mut params = params.clone();
		params["textDocument"]["uri"] = Value::from(virtual_url.as_str());
		params["position"] = serde_json::to_value(lua_position)?;

		let luals = self.luals.as_mut().unwrap();
		let mut result = luals.request(method, params).unwrap_or(Value::Null);
		let pending = luals.take_pending();
		for notification in pending {
			self.lua_notification(notification)?;
		}

		match method {
			"textDocument/hover" => {
				if result.is_object() && !map_range(&mut result, "range", &document) {
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
					let item = item.as_object_mut().unwrap();
					item.remove("additionalTextEdits");
					if let Some(edit) = item.get_mut("textEdit") {
						let mapped = if edit.get("range").is_some() {
							map_range(edit, "range", &document)
						} else {
							map_range(edit, "insert", &document)
								&& map_range(edit, "replace", &document)
						};
						if !mapped {
							item.remove("textEdit");
						}
					}
				}
			}
			"textDocument/definition" => {
				let mut locations = match result {
					Value::Array(locations) => locations,
					Value::Null => Vec::new(),
					location => vec![location],
				};
				locations.retain_mut(|location| {
					let in_document =
						|key: &str| location[key].as_str() == Some(virtual_url.as_str());
					if in_document("uri") {
						location["uri"] = Value::from(url.as_str());
						map_range(location, "range", &document)
					} else if in_document("targetUri") {
						location["targetUri"] = Value::from(url.as_str());
						map_range(location, "targetRange", &document)
							&& map_range(location, "targetSelectionRange", &document)
					} else {
						// Elsewhere: the declarations of the API, or other Lua.
						true
					}
				});
				result = Value::Array(locations);
			}
			_ => {}
		}
		Ok(Some(result))
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
			Message::Response(_) => {}
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
