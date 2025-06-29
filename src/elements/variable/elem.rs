use std::sync::Arc;

use auto_userdata::AutoUserData;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::variable::Variable;
use crate::lua::variable::VariableWrapper;

#[derive(Debug, AutoUserData)]
pub struct VariableDefinition {
	pub(crate) location: Token,
	#[lua_map(VariableWrapper)]
	pub(crate) variable: Arc<dyn Variable>,
}

fn get_documentation(title: &str, var: &Arc<dyn Variable>) -> String {
	let range = if var.location().end() != 0 {
		format!(" ({}..{})", var.location().start(), var.location().end())
	} else {
		"".into()
	};
	format!(
		"{title}

# Variable `{}`

```{}```

# Properties
 * **Type**: *{}*
 * **Definition**: [{}](){range}
 * **Visibility**: *{}*
 * **Mutability**: *{}*",
		var.name(),
		var.to_string(),
		var.variable_typename(),
		var.location().source().name(),
		var.visility(),
		var.mutability()
	)
}

impl Element for VariableDefinition {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Variable Definition"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		_compiler: &Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(get_documentation(self.element_name(), &self.variable))
	}
}

/// Holds the generated ast from a variable invocation
#[derive(Debug)]
pub struct VariableSubstitution {
	pub location: Token,
	pub variable: Arc<dyn Variable>,
	pub content: Vec<Arc<RwLock<Scope>>>,
}

impl Element for VariableSubstitution {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Invisible
	}

	fn element_name(&self) -> &'static str {
		"Variable Substitution"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<crate::parser::reports::Report>> {
		for (scope, elem) in self.content[0].content_iter(false) {
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(get_documentation(self.element_name(), &self.variable))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}
}

impl ContainerElement for VariableSubstitution
{
    fn contained(&self) -> &[Arc<RwLock<Scope>>] {
        &self.content
    }
}
