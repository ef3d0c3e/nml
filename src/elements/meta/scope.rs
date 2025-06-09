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
pub struct ScopeElement {
	pub token: Token,
	pub scope: [Arc<RwLock<Scope>>; 1],
}

impl Element for ScopeElement {
	fn location(&self) -> &Token {
		&self.token
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Scope"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		for (scope, elem) in self.scope[0].content_iter(false) {
			elem.compile(scope, compiler, output)?;
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

impl ContainerElement for ScopeElement {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		&self.scope
	}
}
