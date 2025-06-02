use std::cell::RefCell;
use std::rc::Rc;

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
	pub scope: [Rc<RefCell<Scope>>; 1],
}

impl Element for ScopeElement
{
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
		    _scope: Rc<RefCell<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
		for (scope, elem) in self.scope[0].content_iter(false)
		{
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
    }

    fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> {
        None
    }

    fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> {
        None
    }

    fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> {
        Some(self)
    }
}

impl ContainerElement for ScopeElement {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
        &self.scope
    }
}
