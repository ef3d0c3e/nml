use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::{ContainerElement, ElemKind, Element};
use crate::unit::scope::{Scope, ScopeAccessor};

#[derive(Debug)]
pub struct Link {
	pub(crate) location: Token,
	/// Link display content
	pub(crate) display: Vec<Rc<RefCell<Scope>>>,
	/// Url of link
	pub(crate) url: url::Url,
}

impl Element for Link {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Link" }
	fn compile<'e>(
		&'e self,
		_scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(format!(
					"<a href=\"{}\">",
					compiler.sanitize(self.url.as_str())
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

	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> { Some(self) }
}

impl ContainerElement for Link {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
		self.display.as_slice()
    }

    fn nested_kind(&self) -> ElemKind {
		    if self.kind() != ElemKind::Compound
		    {
			    return self.kind();
		    }

		    for contained in self.contained()
		    {
			    for it in contained.content_iter(true)
			    {
				    match it.1.kind()
				    {
					    ElemKind::Block => return ElemKind::Block,
					    ElemKind::Compound => {
						    if let Some(container) = it.1.as_container()
						    {
							    if container.nested_kind() == ElemKind::Block 
							    {
								    return ElemKind::Block
							    }
						    }
					    },
					    _ => {},
				    }
			    }
		    }
		    ElemKind::Inline
	    }
}
