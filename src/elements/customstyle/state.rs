use ariadne::Fmt;
use parser::state::Scope;
use std::collections::HashMap;

use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::parser::state::RuleState;

pub static STATE_NAME: &str = "elements.custom_style";

pub struct CustomStyleState {
	pub toggled: HashMap<String, Token>,
}

impl RuleState for CustomStyleState {
	fn scope(&self) -> Scope {
		Scope::PARAGRAPH
	}

	fn on_remove(&self, state: &ParserState, document: &dyn Document) -> Vec<Report> {
		let mut reports = vec![];

		self.toggled.iter().for_each(|(style, token)| {
			let container = std::cell::Ref::filter_map(document.content().borrow(), |content| {
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
				"Unterminated Custom Style".into(),
				span(
					token.range.clone(),
					format!("Style {} starts here", style.fg(state.parser.colors().info))
				),
				span(paragraph_end.1, "Paragraph ends here".into()),
				note("Styles cannot span multiple documents (i.e @import)".into())
			);
		});

		reports
	}
}
