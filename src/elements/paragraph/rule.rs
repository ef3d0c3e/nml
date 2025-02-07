use std::any::Any;
use std::sync::Arc;

use regex::Regex;

use crate::parser::rule::Rule;
use crate::parser::scope::ScopeAccessor;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::state::ParseMode;
use crate::parser::translation::TranslationAccessors;
use crate::parser::translation::TranslationUnit;

use super::elem::Paragraph;
use super::elem::ParagraphToken;

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
		mode: &ParseMode,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)>
	{
		if mode.paragraph_only
		{
			return None;
		}

		self.re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| (m.start(), Box::new(()) as Box<dyn Any>))
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit<'u>,
		cursor: &Cursor,
		_match_data: Box<dyn Any>,
	) -> Cursor {
		let end_cursor = match self.re.captures_at(cursor.source().content(), cursor.pos()) {
			None => panic!("Unknown error"),
			Some(capture) => cursor.at(capture.get(0).unwrap().end() - 1),
		};

		// Terminate paragraph
		if let Some(paragraph) = unit.get_scope().current_paragraph() {
			unit.add_content(Arc::new(Paragraph {
				location: Token::new(cursor.pos()..end_cursor.pos(), cursor.source().clone()),
				token: ParagraphToken::End
			}));
		}
		
		end_cursor
	}
}
