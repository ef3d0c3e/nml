//! The custom `textDocument/conceal` and `textDocument/style` requests of the original
//! server, for editors that render a document in place (nml-nvim).

use lsp_types::Range;
use lsp_types::TextDocumentIdentifier;
use nml_document::CalloutKind;
use nml_document::Checkbox;
use nml_document::ElementKind;
use nml_document::Style;
use nml_grammar::Capture;
use nml_grammar::Node;
use nml_grammar::RuleMatch;
use nml_grammar::Value;
use nml_source::Encoding;
use nml_source::Token;
use nml_source::Url;
use serde::Deserialize;
use serde::Serialize;

use crate::Result;
use crate::Server;
use crate::convert;

pub(crate) const CONCEAL: &str = "textDocument/conceal";
pub(crate) const STYLE: &str = "textDocument/style";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisplayParams {
	pub text_document: TextDocumentIdentifier,
}

/// A range the editor can hide, or replace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConcealInfo {
	pub range: Range,
	pub conceal_text: ConcealTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConcealTarget {
	/// Shown instead of the range; empty to hide it.
	Text(String),
	/// Drawn by the editor as it chooses: `bullet` `{depth, numbered}`, `checkbox`
	/// `{state}`, `block` and `block_name` `{name}` (a quote's or callout's), and `code`
	/// `{language, name}` (a code block's opening line).
	Token {
		token: String,
		params: serde_json::Value,
	},
}

/// A range the editor can render styled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StyleInfo {
	pub range: Range,
	pub style: StyleTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StyleTarget {
	/// A highlight group of the editor, as tree-sitter names them (`@markup.strong`).
	Group(String),
}

impl Server {
	/// The markers that can be hidden: the captures named `open` and `close` (`**`, `[[`, …),
	/// and the tokens of list markers, checkboxes, quotes, callouts and code blocks.
	pub(crate) fn conceal(&self, params: DisplayParams) -> Result<Vec<ConcealInfo>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let mut conceals = Vec::new();
		if let Some(analysis) = self.analyses.get(&url) {
			let language = analysis.language.as_deref().unwrap_or(&self.language);
			let mut markers = Markers {
				language,
				file: &url,
				out: &mut conceals,
			};
			markers.nodes(&analysis.tree);
		}
		Ok(conceals)
	}

	/// The styled elements and headings of the document.
	pub(crate) fn style(&self, params: DisplayParams) -> Result<Vec<StyleInfo>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(Vec::new());
		};
		let document = &analysis.document;
		let mut styles = Vec::new();
		for (_, element) in document.descendants(document.root()) {
			let group = match &element.kind {
				ElementKind::Style(Style::Bold) => "@markup.strong",
				ElementKind::Style(Style::Italic) => "@markup.italic",
				ElementKind::Style(Style::Underline) => "@markup.underline",
				ElementKind::Style(Style::Strikethrough) => "@markup.strikethrough",
				ElementKind::Style(Style::Highlight) => "@markup.highlight",
				ElementKind::Heading { .. } => "@markup.heading",
				ElementKind::Code { .. } => "@markup.raw",
				ElementKind::Link { .. } | ElementKind::Reference { .. } => "@markup.link",
				_ => continue,
			};
			let root = element.token.root();
			if root.source().location() != &url {
				continue;
			}
			styles.push(StyleInfo {
				range: convert::range(&root),
				style: StyleTarget::Group(group.into()),
			});
		}
		Ok(styles)
	}
}

/// Collects the conceals of a tree.
struct Markers<'a> {
	language: &'a nml_lang::Language,
	file: &'a Url,
	out: &'a mut Vec<ConcealInfo>,
}

impl Markers<'_> {
	fn nodes(&mut self, nodes: &[Node]) {
		for node in nodes {
			match node {
				Node::Paragraph { children, .. } => self.nodes(children),
				Node::Match(m) => {
					let rule = self.language.rules().get(m.rule);
					// Whether the rule's markers are handled: else, its `open` and `close`
					// are hidden.
					let handled = match rule.map(|rule| rule.name.as_str()) {
						Some("bullet_entry" | "numbered_entry") => self.list_entry(m),
						Some("quote") => self.quote(m, m.capture("marker"), "Quote"),
						Some("callout") => self.callout(m),
						Some("code_block") => self.code_block(m),
						// The command stays, and its block shows as code in its language.
						Some(name @ ("lua" | "graph" | "tex" | "typst")) => {
							self.fenced(m, crate::lua::block_language(name).unwrap())
						}
						// A role keeps its fences: `:typst$a$` would read as Typst.
						Some("lua-role" | "typst_role") => true,
						_ => false,
					};
					for capture in &m.captures {
						if !handled && matches!(&*capture.name, "open" | "close") {
							self.text(&capture.token);
						}
						if let Value::Nodes(nodes) = &capture.value {
							self.nodes(nodes);
						}
					}
				}
				_ => {}
			}
		}
	}

	/// Adds a conceal over `token`, if it is in the file.
	fn add(&mut self, token: &Token, target: ConcealTarget) {
		let root = token.root();
		if root.source().location() == self.file {
			self.out.push(ConcealInfo {
				range: convert::range(&root),
				conceal_text: target,
			});
		}
	}

	fn text(&mut self, token: &Token) {
		self.add(token, ConcealTarget::Text(String::new()));
	}

	fn token(&mut self, range: &Token, token: &str, params: serde_json::Value) {
		let token = token.into();
		self.add(range, ConcealTarget::Token { token, params });
	}

	/// Each marker of a list entry, and its checkbox.
	fn list_entry(&mut self, m: &RuleMatch) -> bool {
		let markers = m.captures("marker").chain(m.captures("markers"));
		let mut depth = 0;
		for capture in markers {
			let token = &capture.token;
			for (offset, marker) in token.content().char_indices() {
				let start = token.start() + offset;
				let marker_token = Token::new(token.source().clone(), start..start + 1);
				let params = serde_json::json!({ "depth": depth, "numbered": marker == '-' });
				self.token(&marker_token, "bullet", params);
				depth += 1;
			}
		}
		if let Some(checkbox) = m.capture("checkbox") {
			// ` [x]`: the box, without the blank before it.
			let token = &checkbox.token;
			let state = match token.content().trim() {
				"[-]" => Checkbox::Partial,
				"[x]" | "[X]" => Checkbox::Checked,
				_ => Checkbox::Unchecked,
			};
			let range = Token::new(token.source().clone(), token.start() + 1..token.end());
			let params = serde_json::json!({ "state": format!("{state:?}") });
			self.token(&range, "checkbox", params);
		}
		true
	}

	/// The `>` of each line of a quote or callout, the first one at `marker`.
	fn quote(&mut self, m: &RuleMatch, marker: Option<&Capture>, name: &str) -> bool {
		let Some(marker) = marker else {
			return false;
		};
		let params = serde_json::json!({ "name": name });
		let first = marker.token.root();
		let source = first.source().clone();
		let gt = |start: usize| Token::new(source.clone(), start..start + 1);
		self.token(&gt(first.start()), "block", params.clone());

		// The lines after the first start with `>` at the same column.
		let end = m.token.root().end();
		let Some(position) = source.position(first.start(), Encoding::Utf8) else {
			return true;
		};
		let column = position.column;
		for line in position.line + 1..source.line_count() {
			let Some(range) = source.line_range(line) else {
				break;
			};
			let start = range.start + column;
			if range.start >= end || start >= range.end {
				break;
			}
			if source.content()[start..].starts_with('>') {
				self.token(&gt(start), "block", params.clone());
			}
		}
		true
	}

	/// A callout: its `>` like a quote's, and `[!kind]` as its name.
	fn callout(&mut self, m: &RuleMatch) -> bool {
		let (Some(open), Some(kind), Some(close)) =
			(m.capture("open"), m.capture("kind"), m.capture("close"))
		else {
			return false;
		};
		let written = kind.token.content();
		let name = if written.eq_ignore_ascii_case("quote") {
			"Quote".to_string()
		} else if let Some(kind) = CalloutKind::from_name(written) {
			let name = kind.name();
			name[..1].to_uppercase() + &name[1..]
		} else {
			return false;
		};
		let open_token = &open.token;
		if !self.quote(m, Some(open), &name) {
			return false;
		}
		// `[!kind]`, after `> `.
		let range = Token::new(
			open_token.source().clone(),
			open_token.start() + 2..close.token.end(),
		);
		self.token(&range, "block_name", serde_json::json!({ "name": name }));
		true
	}

	/// A code block's opening line, with its language and title. Its closing fence is hidden.
	fn code_block(&mut self, m: &RuleMatch) -> bool {
		let Some(fence) = m.capture("fence") else {
			return false;
		};
		let end = (m
			.capture("title")
			.or(m.capture("ref"))
			.or(m.capture("options"))
			.or(m.capture("language")))
		.map_or(fence.token.end(), |capture| capture.token.end());
		let line = Token::new(fence.token.source().clone(), fence.token.start()..end);
		let params = serde_json::json!({
			"language": m.capture("language").map_or("", |c| c.token.content()),
			"name": m.capture("title").map_or("", |c| c.token.content()),
		});
		self.token(&line, "code", params);
		if let Some(close) = m.capture("close") {
			self.text(&close.token);
		}
		true
	}

	/// The fences of a block of `:lua`, `:graph`, … : shown as a code block's, in `language`.
	fn fenced(&mut self, m: &RuleMatch, language: &str) -> bool {
		let (Some(fence), Some(info)) = (m.capture("fence"), m.capture("info")) else {
			return true;
		};
		let line = Token::new(
			fence.token.source().clone(),
			fence.token.start()..info.token.end(),
		);
		let params = serde_json::json!({ "language": language, "name": "" });
		self.token(&line, "code", params);
		if let Some(close) = m.capture("close") {
			self.text(&close.token);
		}
		true
	}
}
