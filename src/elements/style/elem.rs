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

use super::state::Style;
use super::state::StyleProxy;

#[derive(Debug)]
#[auto_userdata(proxy = "StyleElemProxy", immutable, mutable)]
pub struct StyleElem {
	/// Elem location
	#[lua_ud]
	pub(crate) location: Token,
	/// Linked style
	#[lua_proxy(StyleProxy, Arc, immutable)]
	pub(crate) style: Arc<Style>,
	/// Whether to enable or disable
	pub(crate) enable: bool,
}

impl Element for StyleElem {
	fn location(&self) -> &crate::parser::source::Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		ElemKind::Inline
	}

	fn element_name(&self) -> &'static str {
		"Style"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		(self.style.compile)(self.enable, scope, compiler, output)
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Style Toggle

# Properties
 * **Name**: `{}`
 * **Status**: *{}*",
			self.style.name,
			["disable", "enable"][self.enable as usize]
		))
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(StyleElemProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(StyleElemProxyMut(self as *mut _))
			.unwrap()
	}
}
