use std::sync::Arc;

use mlua::UserData;
use mlua::Value;
use parking_lot::RwLock;

use crate::add_documented_method;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

use super::elem::ElemWrapper;

/// Wrapper for Scopes
#[auto_registry::auto_registry(registry = "lua")]
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

/// Wrapper for `Vec<Arc<RwLock<Scope>>>`
#[auto_registry::auto_registry(registry = "lua")]
pub struct VecScopeWrapper {
	pub inner: Vec<Arc<RwLock<Scope>>>,
}

impl UserData for VecScopeWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope",
			"scope",
			|_lua, this, (id,): (usize,)| { 
				if let Some(scope) = this.inner.get(id).cloned() {
					Ok(ScopeWrapper { inner: scope })
				} else {
					Err(mlua::Error::BadArgument {
						to: Some("scope".into()),
						pos: 1,
						name: Some("id".into()),
						cause: Arc::new(mlua::Error::RuntimeError("Index out of bounds".into())),
					})
				}
			},
			"Gets a scope by id",
			vec!["self", "id:number Id of the scope to get"],
			Some("Scope")
		);
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
