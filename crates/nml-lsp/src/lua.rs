//! The Lua of a document, for LuaLS: the original's `textDocument/codeRange`, and the
//! virtual Lua document, `nml/luaDocument`.

use lsp_types::Range;
use lsp_types::TextDocumentIdentifier;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Token;
use nml_source::Url;
use serde::Deserialize;
use serde::Serialize;

use crate::Result;
use crate::Server;
use crate::convert;

pub(crate) const CODE_RANGE: &str = "textDocument/codeRange";
pub(crate) const LUA_DOCUMENT: &str = "nml/luaDocument";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaParams {
	pub text_document: TextDocumentIdentifier,
}

/// A range of code in the document, and its language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeRangeInfo {
	pub range: Range,
	pub language: String,
}

/// All the Lua of a document, in order, as one Lua document: its fragments share their
/// state, so LuaLS must see them together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaDocument {
	pub content: String,
	pub fragments: Vec<Fragment>,
}

/// Where a fragment's code is in the `.nml` document (`range`), and where it starts in the
/// Lua document (`line`, `character`). Lines map one to one; on the first line of a fragment
/// columns are shifted by `character - range.start.character`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fragment {
	pub range: Range,
	pub line: u32,
	pub character: u32,
}

/// The Lua code of a tree, in order: (code, whether it is a role's).
fn fragments<'t>(
	nodes: &'t [Node],
	language: &nml_lang::Language,
	file: &Url,
	out: &mut Vec<(&'t Token, bool)>,
) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => fragments(children, language, file, out),
			Node::Match(m) => {
				let name = language.rules().get(m.rule).map(|rule| rule.name.as_str());
				if let (Some("lua" | "lua-role"), Some(code)) = (name, m.capture("code"))
					&& code.token.root().source().location() == file
				{
					out.push((&code.token, name == Some("lua-role")));
				}
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						fragments(nodes, language, file, out);
					}
				}
			}
			_ => {}
		}
	}
}

/// Whether code is a Lua expression, as `:lua` roles decide it. The code is only compiled,
/// never run, so a plain Lua state is enough.
fn is_expression(lua: &mlua::Lua, code: &str) -> bool {
	lua.load(format!("return {code}")).into_function().is_ok()
}

impl Server {
	fn lua_fragments(&self, url: &Url) -> Vec<(Token, bool)> {
		let Some(analysis) = self.analyses.get(url) else {
			return Vec::new();
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut out = Vec::new();
		fragments(&analysis.tree, language, url, &mut out);
		out.into_iter()
			.map(|(token, role)| (token.clone(), role))
			.collect()
	}

	pub(crate) fn code_ranges(&self, params: LuaParams) -> Result<Vec<CodeRangeInfo>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		Ok(self
			.lua_fragments(&url)
			.into_iter()
			.map(|(token, _)| CodeRangeInfo {
				range: convert::range(&token),
				language: "lua".into(),
			})
			.collect())
	}

	pub(crate) fn lua_document(&self, params: LuaParams) -> Result<LuaDocument> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		Ok(self.lua_document_of(&url))
	}

	/// The virtual Lua document of an open document.
	pub(crate) fn lua_document_of(&self, url: &Url) -> LuaDocument {
		let lua = mlua::Lua::new();
		let mut document = LuaDocument {
			content: String::new(),
			fragments: Vec::new(),
		};
		for (token, role) in self.lua_fragments(url) {
			let code = token.content();
			let (prefix, suffix) = match (role, is_expression(&lua, code)) {
				(true, true) => ("local _ = (", ")"),
				(true, false) => ("do ", " end"),
				(false, _) => ("", ""),
			};
			let line = document.content.lines().count() as u32;
			document.fragments.push(Fragment {
				range: convert::range(&token),
				line,
				character: prefix.encode_utf16().count() as u32,
			});
			document.content.push_str(prefix);
			document.content.push_str(code);
			document.content.push_str(suffix);
			if !document.content.ends_with('\n') {
				document.content.push('\n');
			}
		}
		document
	}
}

impl Fragment {
	/// The last line of the fragment's code: a block's code ends with its line break, so its
	/// range ends at the start of the next line, which is not part of it.
	fn last_line(&self) -> u32 {
		let end = self.range.end;
		if end.character == 0 && end.line > self.range.start.line {
			end.line - 1
		} else {
			end.line
		}
	}

	fn contains(&self, position: lsp_types::Position) -> bool {
		self.range.start <= position
			&& position <= self.range.end
			&& position.line <= self.last_line()
	}
}

impl LuaDocument {
	/// The position in the Lua document of a position in the `.nml` document, if it is in a
	/// fragment.
	pub(crate) fn to_lua(&self, position: lsp_types::Position) -> Option<lsp_types::Position> {
		let fragment = self.fragments.iter().find(|f| f.contains(position))?;
		let line = fragment.line + (position.line - fragment.range.start.line);
		let character = if position.line == fragment.range.start.line {
			position.character - fragment.range.start.character + fragment.character
		} else {
			position.character
		};
		Some(lsp_types::Position::new(line, character))
	}

	/// The position in the `.nml` document of a position in the Lua document, if it is in a
	/// fragment (not in the code added around roles).
	pub(crate) fn to_nml(&self, position: lsp_types::Position) -> Option<lsp_types::Position> {
		let fragment = self.fragments.iter().find(|f| {
			let lines = f.last_line() - f.range.start.line;
			f.line <= position.line && position.line <= f.line + lines
		})?;
		let line = fragment.range.start.line + (position.line - fragment.line);
		let character = if position.line == fragment.line {
			fragment.range.start.character + position.character.checked_sub(fragment.character)?
		} else {
			position.character
		};
		let mapped = lsp_types::Position::new(line, character);
		fragment.contains(mapped).then_some(mapped)
	}

	pub(crate) fn range_to_nml(&self, range: Range) -> Option<Range> {
		Some(Range::new(
			self.to_nml(range.start)?,
			self.to_nml(range.end)?,
		))
	}
}

#[cfg(test)]
mod tests {
	use lsp_types::Position;

	use super::*;

	#[test]
	fn mapping() {
		let document = LuaDocument {
			content: "x = 1\ny = 2\nlocal _ = (f())\n".into(),
			fragments: vec![
				Fragment {
					range: Range::new(Position::new(2, 0), Position::new(4, 0)),
					line: 0,
					character: 0,
				},
				Fragment {
					range: Range::new(Position::new(6, 7), Position::new(6, 10)),
					line: 2,
					character: 11,
				},
			],
		};
		assert_eq!(
			document.to_lua(Position::new(3, 2)),
			Some(Position::new(1, 2))
		);
		assert_eq!(
			document.to_lua(Position::new(6, 8)),
			Some(Position::new(2, 12))
		);
		assert_eq!(document.to_lua(Position::new(5, 0)), None);
		// The end of a block is the start of the next line, not part of it.
		assert_eq!(document.to_lua(Position::new(4, 0)), None);
		assert_eq!(
			document.to_nml(Position::new(1, 2)),
			Some(Position::new(3, 2))
		);
		assert_eq!(
			document.to_nml(Position::new(2, 12)),
			Some(Position::new(6, 8))
		);
		// In the wrapper added around the role.
		assert_eq!(document.to_nml(Position::new(2, 3)), None);
		assert_eq!(document.to_nml(Position::new(2, 16)), None);
	}
}
