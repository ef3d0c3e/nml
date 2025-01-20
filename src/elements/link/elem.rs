use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Link {
	pub(crate) location: Token,
	/// Display content of link
	pub(crate) display: Vec<Box<dyn Element>>,
	/// Url of link
	pub(crate) url: String,
}

impl Element for Link {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Link" }
	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(format!(
					"<a href=\"{}\">",
					Compiler::sanitize(compiler.target(), self.url.as_str())
				));

				for elem in &self.display {
					output = elem.compile(compiler, document, output)?;
				}

				output.add_content("</a>");
			}
			_ => todo!(""),
		}
		Ok(output)
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Link {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.display }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.downcast_ref::<Link>().is_some() {
			return Err("Tried to push a link inside of a link".to_string());
		}
		self.display.push(elem);
		Ok(())
	}
}
