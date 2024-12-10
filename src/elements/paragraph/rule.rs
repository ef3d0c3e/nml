use std::any::Any;

use regex::Regex;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;

use super::elem::Paragraph;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ParagraphRule {
	re: Regex,
}

impl Default for ParagraphRule {
	fn default() -> Self {
		Self {
			re: Regex::new(r"\n{2,}").unwrap(),
		}
	}
}

impl Rule for ParagraphRule {
	fn name(&self) -> &'static str { "Paragraph" }

	fn previous(&self) -> Option<&'static str> { Some("Comment") }

	fn next_match(
		&self,
		_mode: &ParseMode,
		_state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		self.re
			.find_at(cursor.source.content(), cursor.pos)
			.map(|m| (m.start(), Box::new(()) as Box<dyn Any>))
	}

	fn on_match(
		&self,
		state: &ParserState,
		document: &dyn Document,
		cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let end_cursor = match self.re.captures_at(cursor.source.content(), cursor.pos) {
			None => panic!("Unknown error"),
			Some(capture) => cursor.at(capture.get(0).unwrap().end() - 1),
		};

		state.push(
			document,
			Box::new(Paragraph {
				location: Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()),
				content: Vec::new(),
			}),
		);

		(end_cursor, Vec::new())
	}
}
