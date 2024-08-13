use std::any::Any;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;
use regex::Regex;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::ParserState;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;

// TODO: Full refactor
// Problem is that document parsed from other sources i.e by variables
// are not merged correctly into existing paragraph
// A solution would be to use the "(\n){2,}" regex to split paragraph, which would reduce the work needed for process_text
// Another fix would be to keep parsing (recursively) into the same document (like previous version)
// The issue is that this would break the current `Token` implementation
// Which would need to be reworked
#[derive(Debug)]
pub struct Paragraph {
	pub location: Token,
	pub content: Vec<Box<dyn Element>>,
}

impl Paragraph {
	pub fn is_empty(&self) -> bool { self.content.is_empty() }

	pub fn find_back<P: FnMut(&&Box<dyn Element + 'static>) -> bool>(
		&self,
		predicate: P,
	) -> Option<&Box<dyn Element>> {
		self.content.iter().rev().find(predicate)
	}
}

impl Element for Paragraph {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Special }

	fn element_name(&self) -> &'static str { "Paragraph" }

	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String> {
		if self.content.is_empty() {
			return Ok(String::new());
		}

		match compiler.target() {
			Target::HTML => {
				if self.content.is_empty() {
					return Ok(String::new());
				}

				let mut result = String::new();
				result.push_str("<p>");

				for elems in &self.content {
					result += elems.compile(compiler, document, cursor+result.len())?.as_str();
				}

				result.push_str("</p>");
				Ok(result)
			}
			_ => todo!("Unimplemented compiler"),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Paragraph {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.location().source() == self.location().source() {
			self.location.range = self.location.start()..elem.location().end();
		}
		if elem.kind() == ElemKind::Block {
			return Err("Attempted to push block element inside a paragraph".to_string());
		}
		self.content.push(elem);
		Ok(())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::paragraph")]
pub struct ParagraphRule {
	re: Regex,
}

impl ParagraphRule {
	pub fn new() -> Self {
		Self {
			re: Regex::new(r"\n{2,}").unwrap(),
		}
	}
}

impl Rule for ParagraphRule {
	fn name(&self) -> &'static str { "Paragraph" }
	fn previous(&self) -> Option<&'static str> { Some("Comment") }

	fn next_match(&self, _state: &ParserState, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.re
			.find_at(cursor.source.content(), cursor.pos)
			.and_then(|m| Some((m.start(), Box::new([false; 0]) as Box<dyn Any>)))
	}

	fn on_match(
		&self,
		state: &ParserState,
		document: &dyn Document,
		cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
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

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn parse() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
First paragraph
Second line

Second paragraph\
<- literal \\n


Last paragraph
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Text { content == "First paragraph Second line" };
			};
			Paragraph {
				Text { content == "Second paragraph\n<- literal \\n" };
			};
			Paragraph {
				Text { content == "Last paragraph " };
			};
		);
	}
}
