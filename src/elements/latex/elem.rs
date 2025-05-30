use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement};
use crate::unit::scope::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TexKind {
	Block, Inline
}

impl From<TexKind> for ElemKind {
    fn from(value: TexKind) -> Self {
        match value {
            TexKind::Block => ElemKind::Block,
            TexKind::Inline => ElemKind::Inline,
        }
    }
}

#[derive(Debug)]
pub struct Latex {
	pub(crate) location: Token,
	pub(crate) mathmode: bool,
	pub(crate) kind: TexKind,
	pub(crate) env: String,
	pub(crate) tex: String,
}

impl Element for Latex {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { self.kind.into() }

	fn element_name(&self) -> &'static str { "Latex" }

	fn compile<'e>(
		&'e self,
		scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		let fut = async move {
			std::thread::sleep(Duration::from_millis(2000));
			Ok("ok".into())
		};
		output.add_task(self.location.clone(), "Latex".into(), Box::pin(fut) );
		Ok(())
	}

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { None }
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { None }
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { None }
}
