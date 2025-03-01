use std::{cell::{OnceCell, RefCell}, rc::Rc};

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, document::{element::{ElemKind, Element, LinkableElement}, references::{InternalReference, Refname}}, parser::{reports::Report, resolver::Reference, scope::Scope, source::Token}};

#[derive(Debug)]
pub struct InternalLink {
	pub(crate) location: Token,
	pub(crate) refname: Refname,
	pub(crate) display: Vec<Rc<RefCell<Scope>>>,
	pub(crate) reference: OnceCell<Reference>,
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

	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { Some(self) }
}

impl LinkableElement for InternalLink {
    fn wants_refname(&self) -> &Refname {
        &self.refname
    }

	fn wants_link(&self) -> bool { self.reference.get().is_none() }

    fn link(&self, reference: Reference) {
		self.reference.set(reference).unwrap();
    }
	
}
