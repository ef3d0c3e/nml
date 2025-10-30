use std::sync::Arc;
use std::sync::OnceLock;

use ariadne::Span;
use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use mlua::UserData;
use mlua::Value;
use parking_lot::RwLock;

use crate::add_documented_method;
use crate::add_documented_method_mut;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::lua::wrappers::*;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::ReferenceableElement;
use crate::unit::references::InternalReference;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug, Clone)]
pub struct FieldInternalReference(pub Option<Arc<InternalReference>>);

impl UserData for &mut FieldInternalReference {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Reference",
			"is_some",
			|_lua, this, ()| { Ok(this.0.is_some()) },
			"Return true if the reference is set",
			vec!["self",],
			Some("bool True if the reference is set")
		);
		add_documented_method!(
			methods,
			"Reference",
			"get",
			|lua, this, ()| {
				match &this.0 {
					Some(r) => {
						let r: &'static _ = unsafe { &*Arc::as_ptr(r) };
						Ok(Value::UserData(lua.create_userdata(r)?))
					}
					None => Ok(Value::Nil),
				}
			},
			"Get the reference value, or nil if unset",
			vec!["self",],
			Some("reference? The reference value")
		);
		add_documented_method_mut!(
			methods,
			"Reference",
			"set",
			|lua, this, (name,): (Option<crate::unit::references::Refname>,)| {
				let Some(name) = name else {
					this.0 = None;
					return Ok(());
				};
				let crate::unit::references::Refname::Internal(_) = &name else {
					return Err(mlua::Error::BadArgument {
						to: Some("reference:set()".into()),
						pos: 1,
						name: Some("name".into()),
						cause: Arc::new(mlua::Error::RuntimeError(
							"Expected an internal reference name".into(),
						)),
					});
				};
				crate::lua::kernel::Kernel::with_context(lua, |ctx| {
					println!("FI CALLED");
					this.0 = Some(Arc::new(InternalReference::new(ctx.location.clone(), name)));
				});
				Ok(())
			},
			"",
			vec!["self",],
			Some("bool true if the reference is set")
		);
	}
}

impl UserData for FieldInternalReference {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Reference",
			"is_some",
			|_lua, this, ()| { Ok(this.0.is_some()) },
			"Return true if the reference is set",
			vec!["self",],
			Some("bool True if the reference is set")
		);
		add_documented_method!(
			methods,
			"Reference",
			"get",
			|lua, this, ()| {
				match &this.0 {
					Some(r) => {
						let r: &'static _ = unsafe { &*Arc::as_ptr(r) };
						Ok(Value::UserData(lua.create_userdata(r)?))
					}
					None => Ok(Value::Nil),
				}
			},
			"Get the reference value, or nil if unset",
			vec!["self",],
			Some("reference? The reference value")
		);
		add_documented_method_mut!(
			methods,
			"Reference",
			"set",
			|lua, this, (name,): (Option<crate::unit::references::Refname>,)| {
				let Some(name) = name else {
					this.0 = None;
					return Ok(());
				};
				let crate::unit::references::Refname::Internal(_) = &name else {
					return Err(mlua::Error::BadArgument {
						to: Some("reference:set()".into()),
						pos: 1,
						name: Some("name".into()),
						cause: Arc::new(mlua::Error::RuntimeError(
							"Expected an internal reference name".into(),
						)),
					});
				};
				crate::lua::kernel::Kernel::with_context(lua, |ctx| {
					println!("Heading:set() called!");
					this.0 = Some(Arc::new(InternalReference::new(ctx.location.clone(), name)));
				});
				Ok(())
			},
			"",
			vec!["self",],
			Some("bool true if the reference is set")
		);
	}
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct Heading {
	pub(crate) location: Token,
	/// Heading display
	#[lua_map(VecScopeWrapper)]
	pub(crate) display: Vec<Arc<RwLock<Scope>>>,
	/// Nesting depth
	#[lua_value]
	pub(crate) depth: usize,
	#[lua_value]
	pub(crate) numbered: bool,
	#[lua_value]
	pub(crate) in_toc: bool,
	//#[lua_map(InternalReferenceWrapper)]
	//pub(crate) reference: Option<Arc<InternalReference>>,
	#[lua_ud]
	pub(crate) reference: FieldInternalReference,
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
				if self.reference.0.is_some() {
					output.add_content(format!(
						"<a id=\"{}\">",
						compiler.sanitize(self.link.get().unwrap())
					));
				}
				for (scope, elem) in (&self.display[0]).content_iter(false) {
					elem.compile(scope, compiler, output)?;
				}
				if self.reference.0.is_some() {
					output.add_content("</a>");
				}
				output.add_content(format!("</h{}>", self.depth));
			}
			_ => todo!(""),
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Heading

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
			self.reference
				.0
				.as_ref()
				.map_or("*None*".to_string(), |r| r.name().to_string())
		))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		if self.reference.0.is_some() {
			Some(self)
		} else {
			None
		}
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
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

impl ReferenceableElement for Heading {
	fn reference(&self) -> Arc<InternalReference> {
		self.reference.0.to_owned().unwrap()
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
