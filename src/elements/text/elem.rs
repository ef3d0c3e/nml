use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement};
use crate::unit::scope::Scope;

#[derive(Debug)]
pub struct Text {
	pub(crate) location: Token,
	pub(crate) content: String,
}

impl Text {
	pub fn new(location: Token, content: String) -> Text { Text { location, content } }
}

impl Element for Text {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Text" }

	fn compile<'e>(
		&self,
		_scope: Rc<RefCell<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		output.add_content(compiler.sanitize(self.content.as_str()));
		Ok(())
	}

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { None }
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { None }
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { None }
}
