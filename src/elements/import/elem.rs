use std::{cell::RefCell, rc::Rc};

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, parser::{reports::Report, source::Token}, unit::{element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement}, scope::Scope}};


#[derive(Debug)]
pub struct Import
{
	pub(crate) location: Token,
	pub(crate) content: Vec<Rc<RefCell<Scope>>>,
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
		    scope: Rc<RefCell<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
        todo!()
    }

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { None }
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { None }
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { Some(self) }
}

impl ContainerElement for Import {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
        self.content.as_slice()
    }
}
