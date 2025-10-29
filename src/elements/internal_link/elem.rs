use std::sync::Arc;
use std::sync::OnceLock;

use crate::lua::wrappers::*;
use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::FromLua;
use mlua::IntoLua;
use mlua::Lua;
use mlua::LuaSerdeExt;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::LinkableElement;
use crate::unit::element::ReferenceableElement;
use crate::unit::references::Refname;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::unit::Reference;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceTarget {
	//// Link to the reference
	pub link: String,
	/// Reference
	pub reference: Reference,
}

impl IntoLua for ReferenceTarget {
    fn into_lua(self, lua: &Lua) -> mlua::Result<mlua::Value> {
        lua.to_value(&self)
    }
}

impl FromLua for ReferenceTarget {
    fn from_lua(value: mlua::Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct InternalLink {
	pub(crate) location: Token,
	#[lua_value]
	pub(crate) refname: Refname,
	#[lua_map(VecScopeWrapper)]
	pub(crate) display: Vec<Arc<RwLock<Scope>>>,
	#[lua_map(OnceLockWrapper)]
	pub(crate) reference: OnceLock<ReferenceTarget>,
}

impl Element for InternalLink {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Internal Link"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				output.add_content(format!(
					"<a href=\"{}\">",
					self.reference.get().unwrap().link
				));

				let display = &self.display[0];
				for (scope, elem) in display.content_iter(false) {
					elem.compile(scope, compiler, output)?;
				}

				output.add_content("</a>");
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		None
	}
	fn as_linkable(self: Arc<Self>) -> Option<Arc<dyn LinkableElement>> {
		Some(self)
	}
	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}

impl ContainerElement for InternalLink {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		&self.display
	}
}

impl LinkableElement for InternalLink {
	fn wants_refname(&self) -> &Refname {
		&self.refname
	}

	fn wants_link(&self) -> bool {
		self.reference.get().is_none()
	}

	fn set_link(&self, reference: Reference, link: String) {
		self.reference
			.set(ReferenceTarget { link, reference })
			.unwrap();
	}
}
