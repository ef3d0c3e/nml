//! Footnotes in the editor: hover and go to definition on `[^name]`, and the names of the
//! document's notes after `[^`.

use lsp_types::CompletionItem;
use lsp_types::CompletionItemKind;
use lsp_types::Location;
use lsp_types::TextDocumentPositionParams;
use nml_grammar::Node;
use nml_grammar::RuleMatch;
use nml_grammar::Value;
use nml_source::Url;

use crate::Analysis;
use crate::Result;
use crate::Server;
use crate::convert;
use crate::find;

/// The definitions of notes in a tree, `[^name]: …`, in order.
fn definitions<'t>(nodes: &'t [Node], language: &nml_lang::Language, out: &mut Vec<&'t RuleMatch>) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => definitions(children, language, out),
			Node::Match(m) => {
				if language
					.rules()
					.get(m.rule)
					.is_some_and(|rule| rule.name == "footnote")
				{
					out.push(m);
				}
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						definitions(nodes, language, out);
					}
				}
			}
			_ => {}
		}
	}
}

impl Server {
	/// The definition of the note referred to at a position, and the reference's token.
	fn footnote_at<'a>(
		&self,
		analysis: &'a Analysis,
		url: &Url,
		offset: usize,
	) -> Option<(&'a RuleMatch, nml_source::Token)> {
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let hit = find::at(&analysis.tree, url, offset)
			.into_iter()
			.rev()
			.find(|hit| {
				language
					.rules()
					.get(hit.matched.rule)
					.is_some_and(|rule| rule.name == "footnote_ref")
			})?;
		let name = hit.matched.capture("name")?.token.content();
		let mut found = Vec::new();
		definitions(&analysis.tree, language, &mut found);
		let definition = found
			.into_iter()
			.find(|m| m.capture("name").is_some_and(|c| c.token.content() == name))?;
		Some((definition, hit.matched.token.clone()))
	}

	/// The note referred to under the cursor, in Markdown.
	pub(crate) fn footnote_hover(
		&self,
		position: &TextDocumentPositionParams,
	) -> Result<Option<(String, nml_source::Token)>> {
		let Some((analysis, offset, url)) = self.analysis(position)? else {
			return Ok(None);
		};
		let Some((definition, token)) = self.footnote_at(analysis, &url, offset) else {
			return Ok(None);
		};
		let name = definition.capture("name").unwrap().token.content();
		let body = definition
			.capture("body")
			.map_or("", |body| body.token.content());
		Ok(Some((
			format!("**Note** `[^{name}]`\n\n{}", body.trim()),
			token,
		)))
	}

	/// Where the note referred to under the cursor is defined.
	pub(crate) fn footnote_definition(
		&self,
		position: &TextDocumentPositionParams,
	) -> Result<Option<Location>> {
		let Some((analysis, offset, url)) = self.analysis(position)? else {
			return Ok(None);
		};
		let Some((definition, _)) = self.footnote_at(analysis, &url, offset) else {
			return Ok(None);
		};
		let name = &definition.capture("name").unwrap().token;
		let uri = std::str::FromStr::from_str(name.root().source().location().as_str())?;
		Ok(Some(Location::new(uri, convert::range(name))))
	}

	/// The names of the document's notes, after `[^` and part of a name.
	pub(crate) fn footnote_completion(
		&self,
		analysis: &Analysis,
		prefix: &str,
	) -> Option<Vec<CompletionItem>> {
		let start = prefix.rfind("[^")?;
		let typed = &prefix[start + 2..];
		if !typed
			.chars()
			.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
		{
			return None;
		}
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut found = Vec::new();
		definitions(&analysis.tree, language, &mut found);
		let mut names: Vec<_> = (found.iter())
			.filter_map(|m| {
				let name = m.capture("name")?.token.content();
				let body = m.capture("body").map_or("", |body| body.token.content());
				Some((name.to_string(), body.trim().to_string()))
			})
			.collect();
		names.sort();
		names.dedup_by(|a, b| a.0 == b.0);
		Some(
			(names.into_iter())
				.map(|(name, body)| CompletionItem {
					label: name,
					kind: Some(CompletionItemKind::REFERENCE),
					detail: Some(body.lines().next().unwrap_or_default().to_string()),
					..CompletionItem::default()
				})
				.collect(),
		)
	}
}
