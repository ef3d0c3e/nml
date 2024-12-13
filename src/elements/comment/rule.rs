use document::document::Document;
use lsp::semantic::Semantics;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::rule::RegexRule;
use parser::source::Token;
use regex::Captures;
use regex::Regex;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use super::elem::Comment;

#[auto_registry::auto_registry(registry = "rules")]
pub struct CommentRule {
	re: [Regex; 1],
}

impl Default for CommentRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(?:(?:^|\n)|[^\S\n]+)::(.*)").unwrap()],
		}
	}
}

impl RegexRule for CommentRule {
	fn name(&self) -> &'static str {
		"Comment"
	}

	fn previous(&self) -> Option<&'static str> {
		None
	}

	fn regexes(&self) -> &[Regex] {
		&self.re
	}

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool {
		true
	}

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let content = match matches.get(1) {
			None => panic!("Unknown error"),
			Some(comment) => {
				let trimmed = comment.as_str().trim_start().trim_end().to_string();
				if trimmed.is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Comment".into(),
						span(comment.range(), "Comment is empty".into())
					);
				}

				trimmed
			}
		};

		state.push(
			document,
			Box::new(Comment {
				location: token.clone(),
				content,
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let comment = matches.get(1).unwrap().range();
			sems.add(comment.start - 2..comment.end, tokens.comment);
		}

		reports
	}
}
