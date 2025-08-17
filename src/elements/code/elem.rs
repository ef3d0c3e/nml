use std::sync::Arc;

use parking_lot::RwLock;
use syntect::parsing::SyntaxSet;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct CodeDisplay {
	pub title: Option<String>,
	pub line_gutter: bool,
	pub line_offset: usize,
	pub inline: bool,
}

#[derive(Debug, Clone)]
pub struct Code {
	pub(crate) location: Token,
	pub(crate) language: String,
	pub(crate) display: CodeDisplay,
	pub(crate) content: String,
}

impl Code {
	pub fn syntaxes() -> &'static SyntaxSet {
		lazy_static! {
			static ref set: SyntaxSet = SyntaxSet::load_defaults_newlines();
		}
		&set
	}
}

impl Element for Code {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		if self.display.inline {
			ElemKind::Inline
		} else {
			ElemKind::Block
		}
	}

	fn element_name(&self) -> &'static str {
		"Code"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		todo!()
	}
}
