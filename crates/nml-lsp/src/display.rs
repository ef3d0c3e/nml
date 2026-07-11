//! The custom `textDocument/conceal` and `textDocument/style` requests of the original
//! server, for editors that render a document in place (nml-nvim).

use lsp_types::Range;
use lsp_types::TextDocumentIdentifier;
use nml_document::ElementKind;
use nml_document::Style;
use nml_grammar::Node;
use nml_grammar::Value;
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
	/// The markers that can be hidden: the captures named `open` and `close` (`**`, `[[`, …).
	pub(crate) fn conceal(&self, params: DisplayParams) -> Result<Vec<ConcealInfo>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let mut conceals = Vec::new();
		if let Some(analysis) = self.analyses.get(&url) {
			markers(&analysis.tree, &url, &mut conceals);
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

fn markers(nodes: &[Node], file: &Url, out: &mut Vec<ConcealInfo>) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => markers(children, file, out),
			Node::Match(m) => {
				for capture in &m.captures {
					let root = capture.token.root();
					if matches!(&*capture.name, "open" | "close")
						&& root.source().location() == file
					{
						out.push(ConcealInfo {
							range: convert::range(&root),
							conceal_text: ConcealTarget::Text(String::new()),
						});
					}
					if let Value::Nodes(nodes) = &capture.value {
						markers(nodes, file, out);
					}
				}
			}
			_ => {}
		}
	}
}
