use std::sync::Arc;

use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
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
#[auto_userdata(proxy = "LineBreakProxy", immutable, mutable)]
pub struct LineBreak {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) length: usize,
}

impl Element for LineBreak {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Invisible
	}

	fn element_name(&self) -> &'static str {
		"Break"
	}

	fn compile<'e>(
		&'e self,
		scope: Arc<RwLock<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		if !output.in_paragraph(&scope) {
			return Ok(());
		}
		match compiler.target() {
			HTML => {
				if output.in_paragraph(&scope) {
					output.add_content("</p>");
					output.set_paragraph(&scope, false);
				} else
				// FIXME: temporary fix
				{
					output.add_content("<br>");
					output.add_content("<br>");
				}
			}
			_ => todo!("Unimplemented compiler"),
		}
		Ok(())
	}

	fn lua_ud(self: &Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LineBreakProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(self: &mut Self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LineBreakProxyMut(self as *mut _))
			.unwrap()
	}
}
