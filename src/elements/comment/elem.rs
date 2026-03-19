use std::sync::Arc;

use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind::Invisible;
use crate::unit::element::ElemKind::{self};
use crate::unit::element::Element;
use crate::unit::scope::Scope;

#[derive(Debug)]
#[auto_userdata(proxy = "CommentProxy", immutable, mutable)]
pub struct Comment {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) content: String,
}

impl Element for Comment {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		Invisible
	}

	fn element_name(&self) -> &'static str {
		"Comment"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		_compiler: &Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(CommentProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(CommentProxyMut(self as *mut _))
			.unwrap()
	}
}
