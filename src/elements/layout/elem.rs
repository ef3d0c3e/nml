use std::any::Any;
use std::sync::Arc;

use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;
use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;

use crate::elements::layout::state::Layout;
use crate::parser::source::Token;
use crate::unit::scope::Scope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LayoutToken
{
	Start,
	Next,
	End,
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct LayoutElem {
	pub(crate) location: Token,
	pub(crate) id: usize,
	#[lua_ignore]
	pub(crate) layout: Arc<dyn Layout + Send + Sync>,
	#[lua_ignore]
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
		self.layout.compile(scope, compiler, output, self.id, self.token, &self.params)
    }

    fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
    }
}
