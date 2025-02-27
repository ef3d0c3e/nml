use std::{cell::RefCell, rc::Rc};

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, document::{element::{ElemKind, Element}, references::{InternalReference, Refname}}, parser::{reports::Report, scope::Scope, source::Token}};

#[derive(Debug)]
pub struct InternalLink {
	pub(crate) location: Token,
	pub(crate) refname: Refname,
	pub(crate) display: Vec<Rc<RefCell<Scope>>>,
	/// Data resolved at parse time for internal reference
	pub(crate) resolved: Option<Rc<InternalReference>>,
}

impl Element for InternalLink {
    fn location(&self) -> &Token {
        &self.location
    }

    fn kind(&self) -> crate::document::element::ElemKind {
        ElemKind::Inline
    }

    fn element_name(&self) -> &'static str {
		"Internal Link"
    }

    fn compile(
		    &self,
		    scope: Rc<RefCell<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
        todo!()
    }
}
