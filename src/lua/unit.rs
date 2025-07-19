use crate::add_documented_method;
use crate::lua::elem::ElemWrapper;
use crate::lua::kernel::Kernel;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::VariableName;
use mlua::UserData;

use super::scope::IteratorWrapper;
use super::scope::ScopeWrapper;
use super::variable::VariableWrapper;

#[auto_registry::auto_registry(registry = "lua")]
pub struct UnitWrapper<'a> {
	pub inner: &'a mut TranslationUnit,
}

impl<'a> UserData for UnitWrapper<'a> {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("entry_scope", |_lua, this, ()| {
			Ok(ScopeWrapper {
				inner: this.inner.get_entry_scope().clone(),
			})
		});
		methods.add_method("content", |_lua, this, (recurse,): (bool,)| {
			let it = this.inner.get_entry_scope().content_iter(recurse);
			Ok(IteratorWrapper { iter: Box::new(it) })
		});
		methods.add_method("get_variable", |_lua, this, (name,): (String,)| {
			let Some((var, _)) = this
				.inner
				.get_entry_scope()
				.get_variable(&VariableName(name))
			else {
				return Ok(None);
			};
			Ok(Some(VariableWrapper { inner: var }))
		});
		add_documented_method!(
			methods,
			"Unit",
			"add_content",
			|lua, _this, (elem,): (ElemWrapper,)| {
				Kernel::with_context(lua, |ctx| {
					ctx.unit.add_content(elem.inner.clone());
				});
				Ok(())
			},
			"Inserts content in the unit at the current position",
			vec!["self", "elem:Element Element to insert"],
			None
		);
	}
}
