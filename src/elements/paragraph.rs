use std::any::Any;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;
use mlua::Function;
use mlua::Lua;
use regex::Regex;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
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

	fn to_string(&self) -> String { format!("{:#?}", self) }

	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		if self.content.is_empty() {
			return Ok(String::new());
		}

		match compiler.target() {
			Target::HTML => {
				let mut result = String::new();
				//if prev.is_none() || prev.unwrap().downcast_ref::<Paragraph>().is_none()
				{
					result.push_str("<p>");
				}
				//else
				//{ result.push_str(" "); }

				let err = self.content.iter().try_for_each(|elem| {
					match elem.compile(compiler, document) {
						Err(e) => return Err(e),
						Ok(content) => {
							result.push_str(content.as_str());
							Ok(())
						}
					}
				});
				//if next.is_none() || next.unwrap().downcast_ref::<Paragraph>().is_none()
				{
					result.push_str("</p>");
				}

				match err {
					Err(e) => Err(e),
					Ok(()) => Ok(result),
				}
			}
			Target::LATEX => todo!("Unimplemented compiler"),
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
		self.content.push(elem);
		Ok(())
	}
}

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
	fn name(&self) -> &'static str { "Paragraphing" }

	fn next_match(&self, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.re
			.find_at(cursor.source.content(), cursor.pos)
			.and_then(|m| Some((m.start(), Box::new([false; 0]) as Box<dyn Any>)))
	}

	fn on_match(
		&self,
		parser: &dyn Parser,
		document: &dyn Document,
		cursor: Cursor,
		_match_data: Option<Box<dyn Any>>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let end_cursor = match self.re.captures_at(cursor.source.content(), cursor.pos) {
			None => panic!("Unknown error"),
			Some(capture) => cursor.at(capture.get(0).unwrap().end() - 1),
		};

		parser.push(
			document,
			Box::new(Paragraph {
				location: Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()),
				content: Vec::new(),
			}),
		);

		(end_cursor, Vec::new())
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
