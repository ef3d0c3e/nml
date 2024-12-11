use std::rc::Rc;

use ariadne::Fmt;

use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;

use super::data::LayoutType;

pub static STATE_NAME: &str = "elements.layout";

pub struct LayoutState {
	/// The layout stack
	pub(crate) stack: Vec<(Vec<Token>, Rc<dyn LayoutType>)>,
}

impl RuleState for LayoutState {
	fn scope(&self) -> Scope { Scope::DOCUMENT }

	fn on_remove(&self, state: &ParserState, document: &dyn Document) -> Vec<Report> {
		let mut reports = vec![];

		let doc_borrow = document.content().borrow();
		let at = doc_borrow.last().map_or(
			Token::new(
				document.source().content().len()..document.source().content().len(),
				document.source(),
			),
			|last| last.location().to_owned(),
		);

		for (tokens, layout_type) in &self.stack {
			let start = tokens.first().unwrap();
			report_err!(
				&mut reports,
				start.source(),
				"Unterminated Layout".into(),
				span(
					start.source(),
					start.range.start + 1..start.range.end,
					format!(
						"Layout {} stars here",
						layout_type.name().fg(state.parser.colors().info)
					)
				),
				span(at.source(), at.range.clone(), "Document ends here".into())
			);
		}

		reports
	}
}
