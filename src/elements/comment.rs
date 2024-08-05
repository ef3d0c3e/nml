use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug)]
pub struct Comment {
	location: Token,
	content: String,
}

impl Comment {
	pub fn new(location: Token, content: String) -> Self {
		Self {
			location: location,
			content,
		}
	}
}

impl Element for Comment {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Invisible }
	fn element_name(&self) -> &'static str { "Comment" }
	fn compile(&self, _compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		Ok("".to_string())
	}
}

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

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		parser: &dyn Parser,
		document: &'a dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let content = match matches.get(1) {
			None => panic!("Unknown error"),
			Some(comment) => {
				let trimmed = comment.as_str().trim_start().trim_end().to_string();
				if trimmed.is_empty() {
					reports.push(
						Report::build(ReportKind::Warning, token.source(), comment.start())
							.with_message("Empty comment")
							.with_label(
								Label::new((token.source(), comment.range()))
									.with_message("Comment is empty")
									.with_color(parser.colors().warning),
							)
							.finish(),
					);
				}

				trimmed
			}
		};

		parser.push(document, Box::new(Comment::new(token.clone(), content)));

		return reports;
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
use crate::elements::style::Style;
use crate::elements::text::Text;
use crate::parser::langparser::LangParser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

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
		let doc = parser.parse(source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Text; Style; Text; Style;
				Comment { content == "Commented line" };
				Text; Comment { content == "Test" };
			};
		);
	}
}
