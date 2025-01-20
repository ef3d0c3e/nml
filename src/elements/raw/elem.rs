use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Raw {
	pub(crate) location: Token,
	pub(crate) kind: ElemKind,
	pub(crate) content: String,
}

impl Element for Raw {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { self.kind.clone() }

	fn element_name(&self) -> &'static str { "Raw" }

	fn compile<'e>(
		&self,
		_compiler: &Compiler,
		_document: &dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		output.add_content(self.content.as_str());
		Ok(output)
	}
}
