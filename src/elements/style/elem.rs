use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Style {
	pub(crate) location: Token,
	pub(crate) kind: usize,
	pub(crate) close: bool,
}

impl Element for Style {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Inline
	}
	fn element_name(&self) -> &'static str {
		"Style"
	}
	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				Ok([
					// Bold
					"<b>", "</b>", // Italic
					"<i>", "</i>", // Underline
					"<u>", "</u>", // Code
					"<em>", "</em>",
				][self.kind * 2 + self.close as usize]
					.to_string())
			}
			_ => todo!(""),
		}
	}
}
