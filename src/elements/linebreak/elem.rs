use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::scope::Scope;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct LineBreak {
	pub(crate) location: Token,
	pub(crate) length: usize,
}

/*impl Paragraph {
	pub fn find_back<P: FnMut(&&Box<dyn Element + 'static>) -> bool>(
		&self,
		predicate: P,
	) -> Option<&Box<dyn Element>> {
		self.content.iter().rev().find(predicate)
	}
}*/

impl Element for LineBreak {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Special }

	fn element_name(&self) -> &'static str { "Break" }

	fn compile<'e>(
		&'e self,
		_scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => todo!(),
			_ => todo!("Unimplemented compiler"),
		}
		//Ok(())
	}
}
