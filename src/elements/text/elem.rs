use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Text {
	pub(crate) location: Token,
	pub(crate) content: String,
}

impl Text {
	pub fn new(location: Token, content: String) -> Text {
		Text { location, content }
	}
}

impl Element for Text {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Inline
	}
	fn element_name(&self) -> &'static str {
		"Text"
	}

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		Ok(Compiler::sanitize(compiler.target(), self.content.as_str()))
	}
}
