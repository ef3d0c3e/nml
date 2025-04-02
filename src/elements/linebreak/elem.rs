use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::{ElemKind, Element};
use crate::unit::scope::Scope;

#[derive(Debug)]
pub struct LineBreak {
	pub(crate) location: Token,
	pub(crate) length: usize,
}

impl Element for LineBreak {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Invisible }

	fn element_name(&self) -> &'static str { "Break" }

	fn compile<'e>(
		&'e self,
		scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				if output.in_paragraph(&scope)
				{
					output.add_content("</p>");
					output.set_paragraph(&scope, false);
				}
			},
			_ => todo!("Unimplemented compiler"),
		}
		Ok(())
	}
}
