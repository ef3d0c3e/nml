use std::{cell::RefCell, rc::Rc};

use crate::{document::element::{ContainerElement, ElemKind, Element}, elements::internal_link::elem, parser::{scope::Scope, source::Token}};

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

    fn kind(&self) -> crate::document::element::ElemKind {
        ElemKind::Special
    }

    fn element_name(&self) -> &'static str {
		"Import"
    }

    fn compile(
		    &self,
		    scope: std::rc::Rc<std::cell::RefCell<crate::parser::scope::Scope>>,
		    compiler: &crate::compiler::compiler::Compiler,
		    output: &mut crate::compiler::output::CompilerOutput,
	    ) -> Result<(), Vec<crate::parser::reports::Report>> {
        todo!()
    }

	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> {
	    Some(self)
	}
}

impl ContainerElement for Import {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
        self.content.as_slice()
    }
}
