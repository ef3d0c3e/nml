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
#[auto_userdata(proxy = "TextProxy", immutable, mutable)]
pub struct Text {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) content: String,
}

impl Text {
	pub fn new(location: Token, content: String) -> Text {
		Text { location, content }
	}
}

impl Element for Text {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Inline
	}
	fn element_name(&self) -> &'static str {
		"Text"
	}

	fn compile<'e>(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		output.add_content(compiler.sanitize(self.content.as_str()));
		Ok(())
	}

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TextProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TextProxyMut(self as *mut _)).unwrap()
	}
}
