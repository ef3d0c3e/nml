use std::sync::Arc;

use graphviz_rust::print;
use mlua::UserData;
use parking_lot::RwLock;

use crate::add_documented_method;
use crate::lua::wrappers::{
	ElemWrapper, IteratorWrapper, ScopeWrapper, VecScopeProxy, VecScopeProxyMut
};
use crate::unit::scope::{Scope, ScopeAccessor};

impl UserData for ScopeWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope",
			"content",
			|_lua, this, (recurse,): (bool,)| {
				let r = this.0.clone();
				let it = r.content_iter(recurse);
				Ok(IteratorWrapper(Box::new(it)))
			},
			"Gets an iterator to the scope's content",
			vec![
				"self",
				"recurse:bool Recursively iterate over nested scopes"
			],
			None
		);
		add_documented_method!(
			methods,
			"Scope",
			"insert",
			|_lua, this, (index, elem): (usize, ElemWrapper)| {
				let r = this.0.clone();
				let Some(mut scope) = r.try_write() else {
					return Err(mlua::Error::RuntimeError(format!("Attempted to modify immutable Scope")))
				};
				if index > scope.content.len() {
					scope.content.push(elem.0);
				} else {
					scope.content.insert(index, elem.0);
				}
				Ok(())
			},
			"Insert an element in the scope",
			vec![
				"self",
				"index:integer Index to insert atm 0 to insert at start",
				"elem:ElemWrapper Element to insert"
			],
			None
		);
		add_documented_method!(
			methods,
			"Scope",
			"push",
			|_lua, this, elem : ElemWrapper | {
				let r = this.0.clone();
				let Some(mut scope) = r.try_write() else {
					return Err(mlua::Error::RuntimeError(format!("Attempted to modify immutable Scope")))
				};
				scope.content.push(elem.0);
				Ok(())
			},
			"Add an element at the scope's end",
			vec![
				"self",
				"elem:ElemWrapper Element to insert"
			],
			None
		);
	}
}

impl UserData for VecScopeProxy {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope[]",
			"scope",
			|_lua, this, (id,): (usize,)| {
				let r = unsafe { &*this.0 as &Vec<Arc<RwLock<Scope>>> };
				if let Some(scope) = r.get(id) {
					Ok(ScopeWrapper(scope.clone()))
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

impl UserData for VecScopeProxyMut {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope[]",
			"scope",
			|_lua, this, (id,): (usize,)| {
				let r = unsafe { &mut *this.0 as &mut Vec<Arc<RwLock<Scope>>> };
				if let Some(scope) = r.get(id) {
					Ok(ScopeWrapper(scope.clone()))
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
