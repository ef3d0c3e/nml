use std::sync::Arc;

use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;

#[derive(Debug)]
#[auto_userdata(proxy = "RawProxy", immutable, mutable)]
pub struct Raw {
	#[lua_ud]
	pub(crate) location: Token,
	#[lua_value]
	pub(crate) kind: ElemKind,
	pub(crate) content: String,
}

impl Element for Raw {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		self.kind.clone()
	}

	fn element_name(&self) -> &'static str {
		"Raw"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		_compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		output.add_content(&self.content);
		Ok(())
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(RawProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(RawProxyMut(self as *mut _)).unwrap()
	}
}
