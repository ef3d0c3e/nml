use std::{cell::RefCell, rc::Rc, sync::Arc};

use parking_lot::RwLock;

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, parser::{reports::Report, source::Token}, unit::{element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement}, scope::{self, Scope}}};

use super::state::Style;


#[derive(Debug)]
pub struct StyleElem {
	/// Elem location
	pub(crate) location: Token,
	/// Linked style
	pub(crate) style: Arc<Style>,
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
		    scope: Arc<RwLock<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
        (self.style.compile)(self.enable, scope, compiler, output)
    }

    fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
        None
    }

    fn as_linkable(self: Arc<Self>) -> Option<Arc<dyn LinkableElement>> {
        None
    }

    fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
        None
    }
}
