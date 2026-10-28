//! Code actions: the fixes of diagnostics.

use std::collections::HashMap;
use std::str::FromStr;

use lsp_types::CodeAction;
use lsp_types::CodeActionKind;
use lsp_types::CodeActionOrCommand;
use lsp_types::CodeActionParams;
use lsp_types::Range;
use lsp_types::TextEdit;
use lsp_types::Uri;
use lsp_types::WorkspaceEdit;
use nml_source::Url;

use crate::Result;
use crate::Server;
use crate::convert;

/// Whether two ranges share a position: a cursor on a diagnostic's edge counts.
fn touches(a: &Range, b: &Range) -> bool {
	a.start <= b.end && b.start <= a.end
}

impl Server {
	/// The fixes of the document's diagnostics in the requested range.
	pub(crate) fn code_actions(
		&self,
		params: &CodeActionParams,
	) -> Result<Vec<CodeActionOrCommand>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(Vec::new());
		};
		let mut actions = Vec::new();
		for diagnostic in &analysis.diagnostics {
			if convert::file(diagnostic) != url
				|| !touches(
					&convert::range(&diagnostic.primary().location),
					&params.range,
				) {
				continue;
			}
			let localized = self.localizer.localize(diagnostic);
			for (fix, title) in diagnostic.fixes.iter().zip(localized.fixes) {
				// `Uri` caches parsed parts inside, which this lint flags; it is the key type
				// of the protocol's `WorkspaceEdit`, and is never changed once built.
				#[allow(clippy::mutable_key_type)]
				let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
				for edit in &fix.edits {
					let file = edit.location.root().source().location().clone();
					changes
						.entry(Uri::from_str(file.as_str())?)
						.or_default()
						.push(TextEdit::new(
							convert::range(&edit.location),
							edit.replacement.clone(),
						));
				}
				actions.push(CodeActionOrCommand::CodeAction(CodeAction {
					title,
					kind: Some(CodeActionKind::QUICKFIX),
					diagnostics: Some(vec![convert::diagnostic(diagnostic, &self.localizer)]),
					edit: Some(WorkspaceEdit {
						changes: Some(changes),
						..WorkspaceEdit::default()
					}),
					is_preferred: Some(true),
					..CodeAction::default()
				}));
			}
		}
		actions.extend(self.structure_actions(&url, params.range.start.line));
		actions.extend(self.table_actions(&url, params.range));
		actions.extend(self.checkbox_actions(&url, params.range.start.line));
		Ok(actions)
	}
}

/// The checkboxes of list entries in a tree, written in the file.
pub(crate) fn checkboxes(
	nodes: &[nml_grammar::Node],
	language: &nml_lang::Language,
	file: &Url,
	out: &mut Vec<nml_source::Token>,
) {
	for node in nodes {
		match node {
			nml_grammar::Node::Paragraph { children, .. } => {
				checkboxes(children, language, file, out)
			}
			nml_grammar::Node::Match(m) => {
				let name = language.rules().get(m.rule).map(|rule| rule.name.as_str());
				if matches!(name, Some("bullet_entry" | "numbered_entry"))
					&& let Some(checkbox) = m.capture("checkbox")
					&& checkbox.token.root().source().location() == file
				{
					out.push(checkbox.token.root());
				}
				for capture in &m.captures {
					if let nml_grammar::Value::Nodes(nodes) = &capture.value {
						checkboxes(nodes, language, file, out);
					}
				}
			}
			_ => {}
		}
	}
}

/// What a checkbox becomes when toggled: done if it was not, not done if it was.
pub(crate) fn toggled(checkbox: &str) -> &'static str {
	match checkbox.trim() {
		"[x]" | "[X]" => "[ ]",
		_ => "[x]",
	}
}

impl Server {
	/// "Toggle the checkbox" on a line with a list entry's checkbox.
	fn checkbox_actions(&self, url: &Url, line: u32) -> Vec<CodeActionOrCommand> {
		let Some(analysis) = self.analyses.get(url) else {
			return Vec::new();
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut found = Vec::new();
		checkboxes(&analysis.tree, language, url, &mut found);
		let Some(checkbox) = found
			.into_iter()
			.find(|token| convert::range(token).start.line == line)
		else {
			return Vec::new();
		};
		// Without the blank before it.
		let content = checkbox.content();
		let blank = content.len() - content.trim_start().len();
		let token = nml_source::Token::new(
			checkbox.source().clone(),
			checkbox.start() + blank..checkbox.end(),
		);
		let Ok(uri) = Uri::from_str(url.as_str()) else {
			return Vec::new();
		};
		let edit = TextEdit::new(convert::range(&token), toggled(content).into());
		#[allow(clippy::mutable_key_type)] // See above.
		let changes = HashMap::from([(uri, vec![edit])]);
		vec![CodeActionOrCommand::CodeAction(CodeAction {
			title: "Toggle the checkbox".into(),
			kind: Some(CodeActionKind::new("refactor.rewrite")),
			edit: Some(WorkspaceEdit {
				changes: Some(changes),
				..WorkspaceEdit::default()
			}),
			..CodeAction::default()
		})]
	}
}
