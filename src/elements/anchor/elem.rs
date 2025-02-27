use std::{cell::RefCell, rc::{Rc, Weak}};

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, document::{element::{ElemKind, Element, ReferenceableElement}, references::{InternalReference, Refname}}, parser::{reports::Report, scope::Scope, source::Token}};


#[derive(Debug)]
pub struct Anchor {
	pub(crate) location: Token,
	pub(crate) refname: Refname,
	pub(crate) reference: Rc<InternalReference>
}

impl Element for Anchor {
    fn location(&self) -> &crate::parser::source::Token {
		&self.location
    }

    fn kind(&self) -> ElemKind {
		ElemKind::Invisible
    }

    fn element_name(&self) -> &'static str {
		"Anchor"
    }

    fn compile(
		    &self,
		    _scope: Rc<RefCell<Scope>>,
		    _compiler: &Compiler,
		    _output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> { Ok(()) }

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { Some(self) }
}

impl ReferenceableElement for Anchor
{
    fn reference_name(&self) -> Option<&String> {
		match &self.refname {
			Refname::Internal(name) => Some(name),
			_ => { panic!() },
		}
    }

    fn reference(&self) -> Rc<InternalReference> {
        self.reference.clone()
    }

    fn refcount_key(&self) -> &'static str {
		"anchor"
    }

    fn refid(&self, _compiler: &Compiler, refid: usize) -> String {
		refid.to_string()
    }
}
