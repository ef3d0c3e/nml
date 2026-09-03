//! Semantic tokens, from the classes of rule captures, and from LuaLS for the Lua of a
//! document.

use lsp_types::Position;
use lsp_types::SemanticToken;
use lsp_types::SemanticTokenModifier;
use lsp_types::SemanticTokenType;
use lsp_types::SemanticTokensLegend;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Encoding;
use nml_source::Source;
use nml_source::Url;

use crate::lua::LuaDocument;

/// The token types the server reports: a capture's class is one of these names. The others
/// are LuaLS's.
const TYPES: &[SemanticTokenType] = &[
	SemanticTokenType::KEYWORD,
	SemanticTokenType::OPERATOR,
	SemanticTokenType::STRING,
	SemanticTokenType::VARIABLE,
	SemanticTokenType::NAMESPACE,
	SemanticTokenType::FUNCTION,
	SemanticTokenType::NUMBER,
	SemanticTokenType::COMMENT,
	SemanticTokenType::TYPE,
	SemanticTokenType::PROPERTY,
	SemanticTokenType::PARAMETER,
	SemanticTokenType::MACRO,
	SemanticTokenType::DECORATOR,
	SemanticTokenType::CLASS,
	SemanticTokenType::ENUM,
	SemanticTokenType::INTERFACE,
	SemanticTokenType::STRUCT,
	SemanticTokenType::TYPE_PARAMETER,
	SemanticTokenType::ENUM_MEMBER,
	SemanticTokenType::EVENT,
	SemanticTokenType::METHOD,
	SemanticTokenType::MODIFIER,
	SemanticTokenType::REGEXP,
];

/// The token modifiers, which only LuaLS's tokens have. `global` is LuaLS's own.
fn modifiers() -> Vec<SemanticTokenModifier> {
	vec![
		SemanticTokenModifier::DECLARATION,
		SemanticTokenModifier::DEFINITION,
		SemanticTokenModifier::READONLY,
		SemanticTokenModifier::STATIC,
		SemanticTokenModifier::DEPRECATED,
		SemanticTokenModifier::ABSTRACT,
		SemanticTokenModifier::ASYNC,
		SemanticTokenModifier::MODIFICATION,
		SemanticTokenModifier::DOCUMENTATION,
		SemanticTokenModifier::DEFAULT_LIBRARY,
		SemanticTokenModifier::new("global"),
	]
}

pub(crate) fn legend() -> SemanticTokensLegend {
	SemanticTokensLegend {
		token_types: TYPES.to_vec(),
		token_modifiers: modifiers(),
	}
}

/// A token at its position in the document, on one line, in UTF-16 units; `kind` and
/// `modifiers` are in the server's legend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Token {
	line: u32,
	column: u32,
	length: u32,
	kind: u32,
	modifiers: u32,
}

/// The semantic tokens of a parse tree, in the file at `file` (tokens in other files, such
/// as included ones, are left out).
pub(crate) fn tokens(tree: &[Node], file: &Url) -> Vec<Token> {
	let mut spans = Vec::new();
	collect(tree, file, &mut spans);
	spans.sort_by_key(|(start, end, _, _)| (*start, std::cmp::Reverse(*end)));

	let mut tokens = Vec::new();
	let mut covered = 0;
	for (start, end, kind, source) in spans {
		// Overlapping tokens are not allowed: the outer one wins.
		if start < covered {
			continue;
		}
		covered = end;
		for (line, column, length) in lines(&source, start, end) {
			tokens.push(Token {
				line,
				column,
				length,
				kind,
				modifiers: 0,
			});
		}
	}
	tokens
}

/// LuaLS's tokens of a virtual Lua document (`data`, in its `legend`), in the `.nml`
/// document. Tokens outside the document's own code, or of types the server does not know,
/// are left out.
pub(crate) fn lua_tokens(
	data: &[u32],
	legend: &SemanticTokensLegend,
	document: &LuaDocument,
) -> Vec<Token> {
	let mut tokens = Vec::new();
	let (mut line, mut column) = (0u32, 0u32);
	let ours = modifiers();
	for token in data.chunks_exact(5) {
		let [delta_line, delta_start, length, kind, bits] = token else {
			unreachable!()
		};
		line += delta_line;
		column = if *delta_line == 0 {
			column + delta_start
		} else {
			*delta_start
		};
		let Some(kind) = (legend.token_types.get(*kind as usize))
			.and_then(|kind| TYPES.iter().position(|t| t == kind))
		else {
			continue;
		};
		let modifiers = (legend.token_modifiers.iter().enumerate())
			.filter(|(bit, _)| bits & (1 << bit) != 0)
			.filter_map(|(_, modifier)| ours.iter().position(|m| m == modifier))
			.fold(0, |bits, bit| bits | 1 << bit);
		let start = document.to_nml(Position::new(line, column));
		let end = document.to_nml(Position::new(line, column + length));
		if let (Some(start), Some(end)) = (start, end)
			&& start.line == end.line
		{
			tokens.push(Token {
				line: start.line,
				column: start.character,
				length: *length,
				kind: kind as u32,
				modifiers,
			});
		}
	}
	tokens
}

/// NML's tokens with LuaLS's: where they overlap, LuaLS's.
pub(crate) fn merge(nml: Vec<Token>, lua: Vec<Token>) -> Vec<Token> {
	let overlaps = |a: &Token, b: &Token| {
		a.line == b.line && a.column < b.column + b.length && b.column < a.column + a.length
	};
	let mut tokens: Vec<_> = (nml.into_iter())
		.filter(|token| !lua.iter().any(|lua| overlaps(token, lua)))
		.collect();
	tokens.extend(lua);
	tokens.sort();
	tokens.dedup_by(|b, a| overlaps(a, b));
	tokens
}

/// Tokens in the protocol's relative encoding; they must be sorted.
pub(crate) fn encode(tokens: &[Token]) -> Vec<SemanticToken> {
	let mut previous = (0u32, 0u32);
	(tokens.iter())
		.map(|token| {
			let delta_line = token.line - previous.0;
			let delta_start = if delta_line == 0 {
				token.column - previous.1
			} else {
				token.column
			};
			previous = (token.line, token.column);
			SemanticToken {
				delta_line,
				delta_start,
				length: token.length,
				token_type: token.kind,
				token_modifiers_bitset: token.modifiers,
			}
		})
		.collect()
}

/// The (start, end, type, source) of the captures with a known class, in the file.
fn collect(nodes: &[Node], file: &Url, out: &mut Vec<(usize, usize, u32, std::sync::Arc<Source>)>) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => collect(children, file, out),
			Node::Match(m) => {
				for capture in &m.captures {
					if let Some(class) = &capture.class
						&& let Some(kind) = TYPES.iter().position(|t| t.as_str() == &**class)
					{
						let root = capture.token.root();
						if root.source().location() == file && !root.range().is_empty() {
							out.push((
								root.start(),
								root.end(),
								kind as u32,
								root.source().clone(),
							));
						}
					}
					if let Value::Nodes(nodes) = &capture.value {
						collect(nodes, file, out);
					}
				}
			}
			Node::Comment(token) => {
				let root = token.root();
				if root.source().location() == file {
					let kind = TYPES.iter().position(|t| *t == SemanticTokenType::COMMENT);
					out.push((
						root.start(),
						root.end(),
						kind.unwrap() as u32,
						root.source().clone(),
					));
				}
			}
			Node::Text(_) | Node::Escape(_) => {}
		}
	}
}

/// A range split at line breaks: (line, column, length), in UTF-16 units.
fn lines(source: &Source, start: usize, end: usize) -> Vec<(u32, u32, u32)> {
	let mut parts = Vec::new();
	let first = source.position(start, Encoding::Utf16).unwrap();
	let last = source.position(end, Encoding::Utf16).unwrap();
	for line in first.line..=last.line {
		let range = source.line_range(line).unwrap();
		let from = if line == first.line {
			start
		} else {
			range.start
		};
		let to = if line == last.line { end } else { range.end };
		if to <= from {
			continue;
		}
		let column = source.position(from, Encoding::Utf16).unwrap().column;
		let length = source.content()[from..to].encode_utf16().count();
		parts.push((line as u32, column as u32, length as u32));
	}
	parts
}

#[cfg(test)]
mod tests {
	use lsp_types::Range;

	use super::*;
	use crate::lua::Fragment;

	fn token(line: u32, column: u32, length: u32, kind: &str, modifiers: u32) -> Token {
		let kind = TYPES.iter().position(|t| t.as_str() == kind).unwrap() as u32;
		Token {
			line,
			column,
			length,
			kind,
			modifiers,
		}
	}

	#[test]
	fn lua_tokens_are_mapped() {
		// `:lua` block on lines 2–3, then the role `:lua\`f(x)\`` at 6:7.
		let document = LuaDocument {
			content: "x = 1\nlocal y\nlocal _ = (f(x))\n".into(),
			fragments: vec![
				Fragment {
					range: Range::new(Position::new(2, 0), Position::new(4, 0)),
					line: 0,
					character: 0,
				},
				Fragment {
					range: Range::new(Position::new(6, 7), Position::new(6, 11)),
					line: 2,
					character: 11,
				},
			],
		};
		// LuaLS's own legend, in its order.
		let legend = SemanticTokensLegend {
			token_types: ["keyword", "variable", "function", "label"]
				.map(SemanticTokenType::new)
				.to_vec(),
			token_modifiers: ["global", "declaration", "unknown", "readonly"]
				.map(SemanticTokenModifier::new)
				.to_vec(),
		};
		#[rustfmt::skip]
		let data = [
			// `x`, global and declared.
			0, 0, 1, 1, 0b0011,
			// `local`, `y`: readonly, with an unknown modifier.
			1, 0, 5, 0, 0,
			0, 6, 1, 1, 0b1100,
			// The wrapper of the role: `local`, `_`.
			1, 0, 5, 0, 0,
			0, 6, 1, 1, 0,
			// `f` and `x`, in the role.
			0, 5, 1, 2, 0b0001,
			0, 2, 1, 1, 0,
			// A type the server does not know.
			0, 1, 1, 3, 0,
			// Past the role's end.
			0, 1, 2, 1, 0,
		];
		let global = 1 << 10;
		let declaration = 1;
		let readonly = 1 << 2;
		assert_eq!(
			lua_tokens(&data, &legend, &document),
			[
				token(2, 0, 1, "variable", global | declaration),
				token(3, 0, 5, "keyword", 0),
				token(3, 6, 1, "variable", readonly),
				token(6, 7, 1, "function", global),
				token(6, 9, 1, "variable", 0),
			]
		);
		// A truncated token is ignored.
		assert_eq!(lua_tokens(&data[..3], &legend, &document), []);
	}

	#[test]
	fn lua_tokens_win() {
		let nml = vec![
			token(0, 0, 4, "keyword", 0),
			token(0, 4, 1, "operator", 0),
			token(0, 5, 3, "string", 0),
			token(1, 0, 2, "operator", 0),
		];
		let lua = vec![token(0, 6, 1, "variable", 0), token(0, 5, 1, "function", 0)];
		let merged = merge(nml, lua);
		assert_eq!(
			merged,
			[
				token(0, 0, 4, "keyword", 0),
				token(0, 4, 1, "operator", 0),
				token(0, 5, 1, "function", 0),
				token(0, 6, 1, "variable", 0),
				token(1, 0, 2, "operator", 0),
			]
		);
		let encoded: Vec<_> = (encode(&merged).iter())
			.map(|t| (t.delta_line, t.delta_start, t.length))
			.collect();
		assert_eq!(
			encoded,
			[(0, 0, 4), (0, 4, 1), (0, 1, 1), (0, 1, 1), (1, 0, 2)]
		);
	}
}
