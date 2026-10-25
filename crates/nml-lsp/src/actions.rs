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
		Ok(actions)
	}
}
