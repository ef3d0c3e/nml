use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::scope::Scope;
use crate::parser::scope::ScopeAccessor;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Link {
	pub(crate) location: Token,
	/// Link display content
	pub(crate) contained: Vec<Rc<RefCell<Scope>>>,
	/// Url of link
	pub(crate) url: String,
}

impl Element for Link {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Link" }
	fn compile<'e>(
		&'e self,
		scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(format!(
					"<a href=\"{}\">",
					compiler.sanitize(self.url.as_str())
				));

				let display = &self.contained[0];
				for (scope, elem) in display.content_iter() {
					elem.compile(scope, compiler, output)?;
				}

				output.add_content("</a>");
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Link {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
		self.contained.as_slice()
    }
}
