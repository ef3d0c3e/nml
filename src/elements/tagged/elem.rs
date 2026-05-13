use std::sync::Arc;
use auto_userdata::auto_userdata;
use crate::lua::wrappers::*;
use crate::parser::reports::Report;
use mlua::AnyUserData;
use mlua::Lua;

use parking_lot::RwLock;

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, parser::source::Token, unit::{element::{ContainerElement, ElemKind, Element}, scope::Scope}};

#[derive(Debug, Clone)]
#[auto_userdata(proxy = "TaggedProxy", immutable, mutable)]
pub struct Tagged {
	#[lua_ud]
	pub location: Token,
	#[lua_proxy(VecScopeProxy)]
	pub content: Vec<Arc<RwLock<Scope>>>,
}

impl Element for Tagged {
    fn location(&self) -> &Token {
        &self.location
    }

    fn kind(&self) -> ElemKind {
        ElemKind::Compound
    }

    fn element_name(&self) -> &'static str {
		"Tagged"
    }

    fn compile(
		    &self,
		    _scope: Arc<RwLock<Scope>>,
		    _compiler: &Compiler,
		    _output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
        Ok(())
    }

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TaggedProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TaggedProxyMut(self as *mut _)).unwrap()
	}
}

impl ContainerElement for Tagged {
    fn contained(&self) -> &[Arc<RwLock<Scope>>] {
        &self.content
    }
}
