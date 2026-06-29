//! Semantic tokens, from the classes of rule captures.

use lsp_types::SemanticToken;
use lsp_types::SemanticTokenType;
use lsp_types::SemanticTokensLegend;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Encoding;
use nml_source::Source;
use nml_source::Url;

/// The token types the server reports: a capture's class is one of these names.
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
];

pub(crate) fn legend() -> SemanticTokensLegend {
	SemanticTokensLegend {
		token_types: TYPES.to_vec(),
		token_modifiers: Vec::new(),
	}
}

/// The semantic tokens of a parse tree, in the file at `file` (tokens in other files, such
/// as included ones, are left out).
pub(crate) fn tokens(tree: &[Node], file: &Url) -> Vec<SemanticToken> {
	let mut spans = Vec::new();
	collect(tree, file, &mut spans);
	spans.sort_by_key(|(start, end, _, _)| (*start, std::cmp::Reverse(*end)));

	let mut tokens = Vec::new();
	let mut previous = (0u32, 0u32);
	let mut covered = 0;
	for (start, end, kind, source) in spans {
		// Overlapping tokens are not allowed: the outer one wins.
		if start < covered {
			continue;
		}
		covered = end;
		for (line, column, length) in lines(&source, start, end) {
			let delta_line = line - previous.0;
			let delta_start = if delta_line == 0 {
				column - previous.1
			} else {
				column
			};
			tokens.push(SemanticToken {
				delta_line,
				delta_start,
				length,
				token_type: kind,
				token_modifiers_bitset: 0,
			});
			previous = (line, column);
		}
	}
	tokens
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
