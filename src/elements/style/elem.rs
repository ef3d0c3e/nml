use std::{cell::RefCell, rc::Rc};

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, parser::{reports::Report, source::Token}, unit::{element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement}, scope}};

use super::state::Style;


#[derive(Debug)]
pub struct StyleElem {
	/// Elem location
	pub(crate) location: Token,
	/// Linked style
	pub(crate) style: Rc<Style>,
	/// Whether to enable or disable
	pub(crate) enable: bool,
}

impl Element for StyleElem {
    fn location(&self) -> &crate::parser::source::Token {
        &self.location
    }

    fn kind(&self) -> crate::unit::element::ElemKind {
        ElemKind::Inline
    }

    fn element_name(&self) -> &'static str {
        "Style"
    }

    fn compile(
		    &self,
		    scope: Rc<RefCell<scope::Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
        todo!()
    }

    fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> {
        None
    }

    fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> {
        None
    }

    fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> {
        None
    }
}
