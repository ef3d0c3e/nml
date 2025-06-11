use std::sync::Arc;

use mlua::{UserData, Value};
use parking_lot::RwLock;

use crate::unit::{element::Element, scope::{Scope, ScopeAccessor}};

use super::elem::ElemWrapper;

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
	pub iter: Box<dyn Iterator<Item = (Arc<RwLock<Scope>>, Arc<dyn Element>)>>,
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
						Value::UserData(
							lua.create_userdata(ScopeWrapper { inner: scope }).unwrap(),
						),
						Value::UserData(lua.create_userdata(ElemWrapper { inner: elem }).unwrap()),
					]))
				} else {
					Ok(mlua::MultiValue::new())
				}
			},
		);
	}
}
