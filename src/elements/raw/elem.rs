use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Raw {
	pub(crate) location: Token,
	pub(crate) kind: ElemKind,
	pub(crate) content: String,
}

impl Element for Raw {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		self.kind.clone()
	}

	fn element_name(&self) -> &'static str {
		"Raw"
	}

	fn compile(
		&self,
		_compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		Ok(self.content.clone())
	}
}
