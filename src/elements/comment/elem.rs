use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
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
	fn compile<'e>(
		&self,
		_compiler: &Compiler,
		_document: &dyn Document,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}
}
