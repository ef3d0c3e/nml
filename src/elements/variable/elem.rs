use std::sync::Arc;

use crate::lua::wrappers::*;
use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
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

#[derive(Debug)]
#[auto_userdata(proxy = "VariableDefinitionProxy", immutable, mutable)]
pub struct VariableDefinition {
	#[lua_ud]
	pub(crate) location: Token,
	#[lua_udc(VariableWrapper)]
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
		var.location().source().name().display(),
		var.visibility(),
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

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(VariableDefinitionProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(VariableDefinitionProxyMut(self as *mut _)).unwrap()
	}
}

/// Holds the generated ast from a variable invocation
#[derive(Debug)]
#[auto_userdata(proxy = "VariableSubstitutionProxy", immutable, mutable)]
pub struct VariableSubstitution {
	#[lua_ud]
	pub location: Token,
	#[lua_udc(VariableWrapper)]
	pub variable: Arc<dyn Variable>,
	#[lua_proxy(VecScopeProxy)]
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

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(VariableSubstitutionProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(VariableSubstitutionProxyMut(self as *mut _)).unwrap()
	}
}

impl ContainerElement for VariableSubstitution {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		&self.content
	}
}
