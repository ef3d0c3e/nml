use std::{cell::RefCell, rc::{Rc, Weak}};

use crate::{compiler::{compiler::{Compiler, Target}, output::{self, CompilerOutput}}, parser::{reports::Report, source::Token}, unit::{element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement}, references::{InternalReference, Refname}, scope::Scope}};


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
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
		// Get link
		let link = output.get_internal_link(&self.refname).unwrap();

		match compiler.target() {
			Target::HTML => {
				output.add_content(format!(
						"<a id=\"{link}\"></a>",
				));
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { Some(self) }
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { None }
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { None }
}

impl ReferenceableElement for Anchor
{
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
