use std::sync::Arc;

use auto_userdata::AutoUserData;
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

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "*"]
pub struct Comment {
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

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}
