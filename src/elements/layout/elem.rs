use std::any::Any;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

use super::custom::LayoutToken;
use super::data::LayoutType;

#[derive(Debug)]
pub struct Layout {
	pub(crate) location: Token,
	pub(crate) layout: Rc<dyn LayoutType>,
	pub(crate) id: usize,
	pub(crate) token: LayoutToken,
	pub(crate) properties: Box<dyn Any>,
}

impl Element for Layout {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Layout" }
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		self.layout
			.compile(self.token, self.id, &self.properties, compiler, document)
	}
}
