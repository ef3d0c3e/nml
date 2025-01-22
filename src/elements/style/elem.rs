use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Style {
	pub(crate) location: Token,
	pub(crate) kind: usize,
	pub(crate) close: bool,
}

impl Element for Style {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Style" }
	fn compile<'e>(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(
					[
						// Bold
						"<b>", "</b>", // Italic
						"<i>", "</i>", // Underline
						"<u>", "</u>", // Code
						"<em>", "</em>",
					][self.kind * 2 + self.close as usize],
				);
			}
			_ => todo!(""),
		}
		Ok(())
	}
}
