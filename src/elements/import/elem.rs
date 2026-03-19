use std::sync::Arc;

use crate::lua::wrappers::*;
use ariadne::Span;
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

#[derive(Debug)]
#[auto_userdata(proxy = "InternalLinkProxy", immutable, mutable)]
pub struct Import {
	#[lua_ud]
	pub(crate) location: Token,
	#[lua_proxy(VecScopeProxy)]
	pub(crate) content: Vec<Arc<RwLock<Scope>>>,
}

impl Element for Import {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Import"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		for scope in self.content.iter().cloned() {
			for (scope, elem) in scope.content_iter(false) {
				elem.compile(scope, compiler, output)?
			}
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Import

# Properties
 * **Location**: [{}] ({}..{})",
			self.location.source().name().display(),
			self.location().range.start(),
			self.location().range.end(),
		))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(InternalLinkProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(InternalLinkProxyMut(self as *mut _))
			.unwrap()
	}
}

impl ContainerElement for Import {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		self.content.as_slice()
	}
}
