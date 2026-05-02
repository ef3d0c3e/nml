use std::sync::Arc;

use ariadne::Span;
use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::lua::wrappers::*;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug)]
#[auto_userdata(proxy = "LinkProxy", immutable, mutable)]
pub struct Link {
	#[lua_ud]
	pub(crate) location: Token,
	/// Link display content
	#[lua_proxy(VecScopeProxy)]
	pub(crate) display: Vec<Arc<RwLock<Scope>>>,
	/// Url of link
	#[lua_value]
	pub(crate) url: url::Url,
}

impl Element for Link {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Inline
	}
	fn element_name(&self) -> &'static str {
		"Link"
	}
	fn compile<'e>(
		&'e self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				output.add_content(format!(
					"<a href=\"{}\">",
					compiler.sanitize(self.url.as_str())
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

	fn provide_hover(&self) -> Option<String> {
	    Some(format!("Link

# Properties
 * **Location**: [{0}] ({1}..{2})
 * **Url**: [{3}]({3})",
				self.location.source().name().display(),
				self.location().range.start(),
				self.location().range.end(),
				self.url.to_string()))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LinkProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LinkProxyMut(self as *mut _)).unwrap()
	}
}

impl ContainerElement for Link {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		self.display.as_slice()
	}

	fn nested_kind(&self) -> ElemKind {
		if self.kind() != ElemKind::Compound {
			return self.kind();
		}

		for contained in self.contained() {
			for it in contained.content_iter(true) {
				match it.1.kind() {
					ElemKind::Block => return ElemKind::Block,
					ElemKind::Compound => {
						if let Some(container) = it.1.as_container() {
							if container.nested_kind() == ElemKind::Block {
								return ElemKind::Block;
							}
						}
					}
					_ => {}
				}
			}
		}
		ElemKind::Inline
	}
}
