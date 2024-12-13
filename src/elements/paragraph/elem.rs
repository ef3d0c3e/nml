use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Paragraph {
	pub(crate) location: Token,
	pub(crate) content: Vec<Box<dyn Element>>,
}

impl Paragraph {
	pub fn find_back<P: FnMut(&&Box<dyn Element + 'static>) -> bool>(
		&self,
		predicate: P,
	) -> Option<&Box<dyn Element>> {
		self.content.iter().rev().find(predicate)
	}
}

impl Element for Paragraph {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Special
	}

	fn element_name(&self) -> &'static str {
		"Paragraph"
	}

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		if self.content.is_empty() {
			return Ok(String::new());
		}

		match compiler.target() {
			HTML => {
				if self.content.is_empty() {
					return Ok(String::new());
				}

				let mut result = String::new();
				result.push_str("<p>");

				for elems in &self.content {
					result += elems
						.compile(compiler, document, cursor + result.len())?
						.as_str();
				}

				result.push_str("</p>");
				Ok(result)
			}
			_ => todo!("Unimplemented compiler"),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> {
		Some(self)
	}
}

impl ContainerElement for Paragraph {
	fn contained(&self) -> &Vec<Box<dyn Element>> {
		&self.content
	}

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
