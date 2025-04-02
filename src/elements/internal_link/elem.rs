use std::{cell::{OnceCell, RefCell}, rc::Rc};

use crate::{compiler::{compiler::{Compiler, Target}, output::CompilerOutput}, parser::{reports::Report, source::Token}, unit::{element::{ContainerElement, ElemKind, Element, LinkableElement, ReferenceableElement}, references::Refname, scope::{Scope, ScopeAccessor}, unit::Reference}};

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

    fn kind(&self) -> ElemKind {
        ElemKind::Compound
    }

    fn element_name(&self) -> &'static str {
		"Internal Link"
    }

    fn compile(
		    &self,
		    _scope: Rc<RefCell<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
		// Get link
		let link = output.get_link(&self.refname);

		match compiler.target() {
			Target::HTML => {
				output.add_content(format!(
						"<a href=\"#{link}\">",
				));

				let display = &self.display[0];
				for (scope, elem) in display.content_iter(false) {
					elem.compile(scope, compiler, output)?;
				}

				output.add_content("</a>");
			}
			_ => todo!(""),
		}
		Ok(())
    }

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> { None }
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> { Some(self) }
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { Some(self) }
}

impl ContainerElement for InternalLink {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
		&self.display
    }
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
