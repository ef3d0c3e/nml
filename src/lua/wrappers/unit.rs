use crate::add_documented_method;
use crate::lua::kernel::Kernel;
use crate::lua::wrappers::ElemWrapper;
use crate::lua::wrappers::IteratorWrapper;
use crate::lua::wrappers::ScopeWrapper;
use crate::lua::wrappers::UnitWrapper;
use crate::lua::wrappers::VariableWrapper;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::VariableName;
use mlua::UserData;

impl UserData for UnitWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("entry_scope", |_lua, this, ()| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			Ok(ScopeWrapper(r.get_entry_scope()))
		});
		methods.add_method("content", |_lua, this, (recurse,): (bool,)| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			let it = r.get_entry_scope().content_iter(recurse);
			Ok(IteratorWrapper(Box::new(it)))
		});
		methods.add_method("get_variable", |_lua, this, (name,): (String,)| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			let Some((var, _)) = r.get_entry_scope().get_variable(&VariableName(name)) else {
				return Ok(None);
			};
			Ok(Some(VariableWrapper(&var)))
		});
		add_documented_method!(
			methods,
			"Unit",
			"add_content",
			|lua, _this, (elem,): (ElemWrapper,)| {
				Kernel::with_context(lua, |ctx| {
					ctx.unit.add_content_raw(elem.0.clone());
					if let Some(reference) = elem.0.as_referenceable()
					{
						ctx.unit.add_reference(reference);
					}
				});
				Ok(())
			},
			"Insert content in the unit at the current position",
			vec!["self", "elem:Element Element to insert"],
			None
		);
	}
}
