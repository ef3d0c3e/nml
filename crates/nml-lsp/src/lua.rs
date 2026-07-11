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
		let lua = mlua::Lua::new();
		let mut document = LuaDocument {
			content: String::new(),
			fragments: Vec::new(),
		};
		for (token, role) in self.lua_fragments(&url) {
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
		Ok(document)
	}
}
