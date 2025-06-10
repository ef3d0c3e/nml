use std::sync::Arc;

use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::LinkableElement;
use crate::unit::element::ReferenceableElement;
use crate::unit::scope::Scope;

use super::state::Style;

#[derive(Debug)]
pub struct StyleElem {
	/// Elem location
	pub(crate) location: Token,
	/// Linked style
	pub(crate) style: Arc<Style>,
	/// Whether to enable or disable
	pub(crate) enable: bool,
}

impl Element for StyleElem {
	fn location(&self) -> &crate::parser::source::Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		ElemKind::Inline
	}

	fn element_name(&self) -> &'static str {
		"Style"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		(self.style.compile)(self.enable, scope, compiler, output)
	}

	fn provide_hover(&self) -> Option<String> {
	    Some(format!("Style Toggle

# Properties
 * **Name**: `{}`
 * **Status**: *{}*", self.style.name, ["disable", "enable"][self.enable as usize]))
	}
}
