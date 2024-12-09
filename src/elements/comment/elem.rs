use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

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
