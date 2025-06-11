use std::sync::Arc;

use mlua::LuaSerdeExt;
use mlua::UserData;
use mlua::Value;
use parking_lot::RwLock;

use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationUnit;

pub struct UnitWrapper<'a> {
	pub inner: &'a mut TranslationUnit,
}

impl<'a> UserData for UnitWrapper<'a> {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("entry_scope", |_lua, this, ()| {
			Ok(ScopeWrapper { inner: this.inner.get_entry_scope().clone() })
		});
		methods.add_method("content", |_lua, this, (recurse,): (bool,)| {
			let it = this.inner.get_entry_scope().content_iter(recurse);
			Ok(IteratorWrapper { iter: Box::new(it) })
		});
	}
}

pub struct ElemWrapper {
	pub inner: Arc<dyn Element>,
}

impl UserData for ElemWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("name", |lua, this, ()| {
			lua.to_value(this.inner.element_name())
		});
		methods.add_method("kind", |lua, this, ()| lua.to_value(&this.inner.kind()));
		methods.add_method("downcast", |lua, this, ()| {
			let Some(down) = this.inner.clone().lua_wrap(lua) else {
				return Err(mlua::Error::RuntimeError(format!("Element {} doesn't support downcasting!", this.inner.element_name())))
			};

			Ok(down)
		});
	}
}

pub struct ScopeWrapper {
	pub inner: Arc<RwLock<Scope>>,
}

impl UserData for ScopeWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("content", |_lua, this, (recurse,): (bool,)| {
			let it = this.inner.content_iter(recurse);
			Ok(IteratorWrapper { iter: Box::new(it) })
		});
	}
}

pub struct IteratorWrapper {
	iter: Box<dyn Iterator<Item = (Arc<RwLock<Scope>>, Arc<dyn Element>)>>,
}

impl UserData for IteratorWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_meta_function(
			mlua::MetaMethod::Call,
			|lua, (this,): (mlua::AnyUserData,)| {
				let mut iter = this.borrow_mut::<IteratorWrapper>()?;
				if let Some((scope, elem)) = iter.iter.next() {
					Ok(mlua::MultiValue::from_vec(vec![
						Value::UserData(lua.create_userdata(ScopeWrapper { inner: scope }).unwrap()),
						Value::UserData(lua.create_userdata(ElemWrapper { inner: elem }).unwrap()),
					]))
				} else {
					Ok(mlua::MultiValue::new())
				}
			},
		);
	}
}
