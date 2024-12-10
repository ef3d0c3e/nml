use ariadne::Fmt;

use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
static STATE_NAME: &str = "elements.style";

pub struct StyleState {
	pub(crate) toggled: [Option<Token>; 4],
}

impl StyleState {
	const NAMES: [&'static str; 4] = ["Bold", "Italic", "Underline", "Code"];

	pub fn new() -> Self {
		Self {
			toggled: [None, None, None, None],
		}
	}
}

impl RuleState for StyleState {
	fn scope(&self) -> Scope { Scope::PARAGRAPH }

	fn on_remove(&self, state: &ParserState, document: &dyn Document) -> Vec<Report> {
		let mut reports = vec![];

		self.toggled
			.iter()
			.zip(StyleState::NAMES)
			.for_each(|(token, name)| {
				if token.is_none() {
					return;
				} // Style not enabled
				let token = token.as_ref().unwrap();

				let container =
					std::cell::Ref::filter_map(document.content().borrow(), |content| {
						content.last().and_then(|last| last.as_container())
					})
					.ok();
				if container.is_none() {
					return;
				}
				let paragraph_end = container
					.unwrap()
					.contained()
					.last()
					.map(|last| {
						(
							last.location().source(),
							last.location().end_offset(1)..last.location().end(),
						)
					})
					.unwrap();
				report_err!(
					&mut reports,
					token.source(),
					"Unterminated Style".into(),
					span(
						token.range.clone(),
						format!("Style {} starts here", name.fg(state.parser.colors().info))
					),
					span(paragraph_end.1, "Paragraph ends here".into()),
					note("Styles cannot span multiple documents (i.e @import)".into())
				);
			});

		reports
	}
}
