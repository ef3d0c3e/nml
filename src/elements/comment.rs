use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use regex::Captures;
use regex::Regex;

#[derive(Debug)]
pub struct Comment {
	pub location: Token,
	#[allow(unused)]
	pub content: String,
}

impl Element for Comment {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Invisible }
	fn element_name(&self) -> &'static str { "Comment" }
	fn compile(
		&self,
		_compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		Ok("".to_string())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::comment")]
pub struct CommentRule {
	re: [Regex; 1],
}

impl CommentRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:(?:^|\n)|[^\S\n]+)::(.*)").unwrap()],
		}
	}
}

impl RegexRule for CommentRule {
	fn name(&self) -> &'static str { "Comment" }

	fn previous(&self) -> Option<&'static str> { None }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

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

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::style::Style;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;
	use std::rc::Rc;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
NOT COMMENT: `std::cmp`
:: Commented line
COMMENT ::Test
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Text; Style; Text; Style;
				Comment { content == "Commented line" };
				Text; Comment { content == "Test" };
			};
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
::Test
 ::Another
	:: Another
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);

		validate_semantics!(state, source.clone(), 0,
		comment { delta_line == 1, delta_start == 0, length == 6 };
		comment { delta_line == 1, delta_start == 1, length == 9 };
		comment { delta_line == 1, delta_start == 1, length == 10 };
		);
	}
}
