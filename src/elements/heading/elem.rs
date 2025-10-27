use std::sync::Arc;
use std::sync::OnceLock;

use ariadne::Span;
use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::lua::scope::VecScopeWrapper;
use crate::lua::wrappers::OnceLockWrapper;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::ReferenceableElement;
use crate::unit::references::InternalReference;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug, AutoUserData)]
pub struct Heading {
	pub(crate) location: Token,
	/// Heading display
	#[lua_map(VecScopeWrapper)]
	pub(crate) display: Vec<Arc<RwLock<Scope>>>,
	/// Nesting depth
	pub(crate) depth: usize,
	pub(crate) numbered: bool,
	pub(crate) in_toc: bool,
	pub(crate) reference: Option<Arc<InternalReference>>,
	#[lua_map(OnceLockWrapper)]
	pub(crate) link: OnceLock<String>,
}

impl Element for Heading {
	fn location(&self) -> &Token {
		&self.location
	}
	fn kind(&self) -> ElemKind {
		ElemKind::Block
	}
	fn element_name(&self) -> &'static str {
		"Heading"
	}
	fn compile<'e>(
		&'e self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				output.add_content(format!("<h{}>", self.depth));
				if self.reference.is_some()
				{
					output.add_content(format!("<a id=\"{}\">", compiler.sanitize(self.link.get().unwrap())));
				}
				for (scope, elem) in (&self.display[0]).content_iter(false) {
					elem.compile(scope, compiler, output)?;
				}
				if self.reference.is_some()
				{
					output.add_content("</a>");
				}
				output.add_content(format!("</h{}>", self.depth));
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
	    Some(format!("Heading

# Properties
 * **Location**: [{0}] ({1}..{2})
 * **Depth**: {3}
 * **Numbered**: {4}
 * **In TOC**: {5}
 * **Refname**: {6}",
				self.location.source().name().display(),
				self.location().range.start(),
				self.location().range.end(),
				self.depth,
				self.numbered,
				self.in_toc,
				self.reference.as_ref().map_or("*None*".to_string(), |r| r.name().to_string())))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		if self.reference.is_some()
		{
			Some(self)
		} else {
			None
		}
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		Some(lua.create_userdata(self.clone()).unwrap())
	}
}

impl ContainerElement for Heading {
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

impl ReferenceableElement for Heading
{
    fn reference(&self) -> Arc<InternalReference> {
		self.reference.to_owned().unwrap()
    }

    fn refcount_key(&self) -> &'static str {
        "heading"
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
