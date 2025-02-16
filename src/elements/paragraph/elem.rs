use std::cell::RefCell;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::scope::Scope;
use crate::parser::scope::ScopeAccessor;
use crate::parser::source::Token;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphToken {
	Start,
	End,
}

#[derive(Debug)]
pub struct Paragraph {
	pub(crate) location: Token,
	pub(crate) token: ParagraphToken,
}

/*impl Paragraph {
	pub fn find_back<P: FnMut(&&Box<dyn Element + 'static>) -> bool>(
		&self,
		predicate: P,
	) -> Option<&Box<dyn Element>> {
		self.content.iter().rev().find(predicate)
	}
}*/

impl Element for Paragraph {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Special }

	fn element_name(&self) -> &'static str { "Paragraph" }

	fn compile<'e>(
		&'e self,
		_scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => match self.token {
				ParagraphToken::Start => output.add_content("<p>"),
				ParagraphToken::End => output.add_content("</p>"),
			},
			_ => todo!("Unimplemented compiler"),
		}
		Ok(())
	}
}
