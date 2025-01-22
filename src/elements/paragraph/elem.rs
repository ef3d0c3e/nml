use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
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
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Special }

	fn element_name(&self) -> &'static str { "Paragraph" }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		if self.content.is_empty() {
			return Ok(());
		}

		match compiler.target() {
			HTML => {
				output.add_content("<p>");
				for elem in &self.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</p>");
			}
			_ => todo!("Unimplemented compiler"),
		}
		Ok(())
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Paragraph {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

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
