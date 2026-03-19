use std::any::Any;
use std::sync::Arc;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::elements::layout::state::Layout;
use crate::parser::source::Token;
use crate::unit::scope::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum LayoutToken {
	Start,
	Next,
	End,
}

#[derive(Debug)]
#[auto_userdata(proxy = "LayoutElemProxy", immutable, mutable)]
pub struct LayoutElem {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) id: usize,
	#[lua_ignore]
	pub(crate) layout: Arc<dyn Layout + Send + Sync>,
	#[lua_value]
	pub(crate) token: LayoutToken,
	#[lua_ignore]
	pub(crate) params: Option<Box<dyn Any + Send + Sync>>,
}

impl Element for LayoutElem {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Invisible
	}

	fn element_name(&self) -> &'static str {
		"Layout"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		self.layout
			.compile(scope, compiler, output, self.id, self.token, &self.params)
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LayoutElemProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LayoutElemProxyMut(self as *mut _))
			.unwrap()
	}
}
