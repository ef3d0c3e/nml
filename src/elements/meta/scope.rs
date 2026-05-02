use std::sync::Arc;

use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::lua::wrappers::*;
use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug)]
#[auto_userdata(proxy = "ScopeElementProxy", immutable, mutable)]
pub struct ScopeElement {
	#[lua_ud]
	pub token: Token,
	#[lua_ud(ScopeWrapper)]
	pub scope: Arc<RwLock<Scope>>,
}

impl Element for ScopeElement {
	fn location(&self) -> &Token {
		&self.token
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Scope"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		for (scope, elem) in self.scope.content_iter(false) {
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(ScopeElementProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(ScopeElementProxyMut(self as *mut _))
			.unwrap()
	}
}

impl ContainerElement for ScopeElement {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		std::slice::from_ref(&self.scope)
	}
}
