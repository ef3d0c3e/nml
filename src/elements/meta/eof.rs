use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement};
use crate::unit::scope::Scope;

#[derive(Debug)]
pub struct Eof {
	pub(crate) location: Token,
}

impl Element for Eof {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Invisible }

	fn element_name(&self) -> &'static str { "Enf of File" }

	fn compile<'e>(
		&'e self,
		_scope: Arc<RwLock<Scope>>,
		_compiler: &'e Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> { None }
	fn as_linkable(self: Arc<Self>) -> Option<Arc<dyn LinkableElement>> { None }
	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> { None }
}
