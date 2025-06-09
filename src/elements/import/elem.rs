use std::sync::Arc;

use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::LinkableElement;
use crate::unit::element::ReferenceableElement;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug)]
pub struct Import {
	pub(crate) location: Token,
	pub(crate) content: Vec<Arc<RwLock<Scope>>>,
}

impl Element for Import {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Import"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		for scope in self.content.iter().cloned() {
			for (scope, elem) in scope.content_iter(false) {
				elem.compile(scope, compiler, output)?
			}
		}
		Ok(())
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		None
	}
	fn as_linkable(self: Arc<Self>) -> Option<Arc<dyn LinkableElement>> {
		None
	}
	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}
}

impl ContainerElement for Import {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		self.content.as_slice()
	}
}
