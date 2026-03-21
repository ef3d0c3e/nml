use std::sync::Arc;

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
use crate::unit::element::LinkableElement;
use crate::unit::element::ReferenceableElement;
use crate::unit::scope::Scope;

#[derive(Debug)]
#[auto_userdata(proxy = "EofProxy", immutable, mutable)]
pub struct Eof {
	#[lua_ud]
	pub(crate) location: Token,
}

impl Element for Eof {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Invisible
	}

	fn element_name(&self) -> &'static str {
		"EOF"
	}

	fn compile<'e>(
		&'e self,
		_scope: Arc<RwLock<Scope>>,
		_compiler: &'e Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(EofProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(EofProxyMut(self as *mut _)).unwrap()
	}
}
