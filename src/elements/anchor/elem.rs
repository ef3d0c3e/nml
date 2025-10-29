use std::sync::Arc;
use std::sync::OnceLock;

use ariadne::Span;
use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use mlua::LuaSerdeExt;
use parking_lot::RwLock;

use crate::lua::wrappers::*;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::ReferenceableElement;
use crate::unit::references::InternalReference;
use crate::unit::references::Refname;
use crate::unit::scope::Scope;

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct Anchor {
	pub(crate) location: Token,
	#[lua_value]
	pub(crate) refname: Refname,
	#[lua_arc_deref]
	pub(crate) reference: Arc<InternalReference>,
	#[lua_map(OnceLockWrapper)]
	pub(crate) link: OnceLock<String>,
}

impl Element for Anchor {
	fn location(&self) -> &crate::parser::source::Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Invisible
	}

	fn element_name(&self) -> &'static str {
		"Anchor"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		// Get link
		let link = self.get_link().unwrap();

		match compiler.target() {
			Target::HTML => {
				output.add_content(format!("<a id=\"{link}\"></a>",));
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
	    Some(format!(
"Anchor

# Properties
 * **Location**: {} ({}..{})
 * **Name**: {}",
			self.location.source().name().display(),
			self.location().range.start(),
			self.location().range.end(),
			self.refname.to_string()))
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		Some(self)
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}

impl ReferenceableElement for Anchor {
	fn reference(&self) -> Arc<InternalReference> {
		self.reference.clone()
	}

	fn refcount_key(&self) -> &'static str {
		"anchor"
	}

	fn refid(&self, _compiler: &Compiler, refid: usize) -> String {
		refid.to_string()
	}

	fn get_link(&self) -> Option<&String> {
		self.link.get()
	}

	fn set_link(&self, url: String) {
		self.link.set(url).expect("set_url can only be called once");
	}
}
