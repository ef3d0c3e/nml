//! The Lua of a document, for LuaLS: the original's `textDocument/codeRange`, and the
//! virtual Lua document, `nml/luaDocument`.

use std::collections::BTreeMap;

use lsp_types::Range;
use lsp_types::TextDocumentIdentifier;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_lang::Resolver;
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

/// A part of the Lua of a tree.
enum Piece<'t> {
	/// Code, and whether it is a role's.
	Code(&'t Token, bool),
	/// An imported module (as written) and its namespace: its Lua runs there.
	Import(&'t Token, String),
}

/// The Lua of a tree, in order.
fn pieces<'t>(
	nodes: &'t [Node],
	language: &nml_lang::Language,
	file: &Url,
	out: &mut Vec<Piece<'t>>,
) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => pieces(children, language, file, out),
			Node::Match(m) => {
				let name = language.rules().get(m.rule).map(|rule| rule.name.as_str());
				let in_file = m.token.root().source().location() == file;
				match (name, m.capture("code"), m.capture("module")) {
					(Some("lua" | "lua-role"), Some(code), _)
						if code.token.root().source().location() == file =>
					{
						out.push(Piece::Code(&code.token, name == Some("lua-role")));
					}
					(Some("import"), _, Some(module)) if in_file => {
						let namespace = match m.capture("alias") {
							Some(alias) => alias.token.content(),
							None => module.token.content().rsplit('.').next().unwrap(),
						};
						out.push(Piece::Import(&module.token, namespace.to_string()));
					}
					_ => {}
				}
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						pieces(nodes, language, file, out);
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

/// Whether a name can be written as a Lua field name.
fn is_name(name: &str) -> bool {
	const KEYWORDS: &[&str] = &[
		"and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if",
		"in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
	];
	name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
		&& name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
		&& !KEYWORDS.contains(&name)
}

/// What the Lua of imported modules is made into, for LuaLS.
struct Modules<'a> {
	language: &'a nml_lang::Language,
	resolver: &'a dyn Resolver,
	/// The names each module exports, by module file.
	exports: &'a BTreeMap<String, Vec<String>>,
	/// The module files being written, against import cycles.
	writing: Vec<String>,
}

impl Modules<'_> {
	/// The Lua that stands for a module imported under `namespace`: its code, in a
	/// function that returns its exports, which LuaLS can follow to their definitions.
	/// `None` if the module was not loaded.
	fn lua(&mut self, module: &str, namespace: &str) -> Option<String> {
		let (path, source) = self.resolver.module(module).ok()?;
		if self.writing.contains(&path) {
			return None;
		}
		let names = self.exports.get(&path)?;
		let tree = nml_grammar::parse(self.language.rules(), source.clone()).nodes;
		let mut parts = Vec::new();
		pieces(&tree, self.language, source.location(), &mut parts);
		self.writing.push(path);
		let mut lua = format!("{namespace} = (function()\n");
		for part in parts {
			let code = match part {
				// Modules run their blocks only.
				Piece::Code(code, false) => Some(code.content().to_string()),
				Piece::Code(_, true) => None,
				Piece::Import(module, namespace) => self.lua(module.content(), &namespace),
			};
			if let Some(code) = code {
				lua.push_str(&code);
				if !lua.ends_with('\n') {
					lua.push('\n');
				}
			}
		}
		self.writing.pop();
		lua.push_str("return {");
		for name in names.iter().filter(|name| is_name(name)) {
			lua.push_str(&format!(" {name} = {name},"));
		}
		lua.push_str(" }\nend)()\n");
		Some(lua)
	}
}

impl Server {
	/// The Lua of an open document, written in it.
	fn lua_fragments(&self, url: &Url) -> Vec<(Token, bool)> {
		let Some(analysis) = self.analyses.get(url) else {
			return Vec::new();
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut out = Vec::new();
		pieces(&analysis.tree, language, url, &mut out);
		out.into_iter()
			.filter_map(|piece| match piece {
				Piece::Code(token, role) => Some((token.clone(), role)),
				Piece::Import(..) => None,
			})
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

	/// The virtual Lua document of an open document: its Lua, and that of the modules it
	/// imports where it imports them. Only the document's own code maps back to it.
	pub(crate) fn lua_document_of(&self, url: &Url) -> LuaDocument {
		let mut document = LuaDocument {
			content: String::new(),
			fragments: Vec::new(),
		};
		let Some(analysis) = self.analyses.get(url) else {
			return document;
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut parts = Vec::new();
		pieces(&analysis.tree, language, url, &mut parts);
		let resolver = self.resolver();
		let mut modules = Modules {
			language: &self.language,
			resolver: &resolver,
			exports: &analysis.module_exports,
			writing: Vec::new(),
		};
		let lua = mlua::Lua::new();
		for part in parts {
			let token = match part {
				Piece::Code(token, role) => (token, role),
				Piece::Import(module, namespace) => {
					if let Some(code) = modules.lua(module.content(), &namespace) {
						document.content.push_str(&code);
					}
					continue;
				}
			};
			let (token, role) = token;
			let code = token.content();
			let (prefix, suffix) = match (role, is_expression(&lua, code)) {
				(true, true) => ("local _ = (", ")"),
				(true, false) => ("do ", " end"),
				(false, _) => ("", ""),
			};
			let line = document.content.lines().count() as u32;
			document.fragments.push(Fragment {
				range: convert::range(token),
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
