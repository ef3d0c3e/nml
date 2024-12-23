use std::any::Any;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

use super::data::BlockType;

#[derive(Debug)]
pub struct Block {
	pub(crate) location: Token,
	pub(crate) content: Vec<Box<dyn Element>>,
	pub(crate) block_type: Rc<dyn BlockType>,
	pub(crate) block_properties: Box<dyn Any>,
}

impl Element for Block {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Block
	}
	fn element_name(&self) -> &'static str {
		"Block"
	}
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		self.block_type
			.compile(self, &self.block_properties, compiler, document, cursor)
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> {
		Some(self)
	}
}

impl ContainerElement for Block {
	fn contained(&self) -> &Vec<Box<dyn Element>> {
		&self.content
	}

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		self.content.push(elem);
		Ok(())
	}
}
