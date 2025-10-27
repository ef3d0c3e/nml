use std::sync::Arc;

use mlua::UserData;

use crate::add_documented_method;
use crate::lua::wrappers::{IteratorWrapper, ScopeWrapper, VecScopeWrapper};
use crate::unit::scope::ScopeAccessor;

impl UserData for ScopeWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope",
			"content",
			|_lua, this, (recurse,): (bool,)| {
				let it = this.0.content_iter(recurse);
				Ok(IteratorWrapper(Box::new(it)))
			},
			"Gets an iterator to the scope's content",
			vec![
				"self",
				"recurse:bool Recursively iterate over nested scopes"
			],
			None
		);
	}
}

impl UserData for VecScopeWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Scope[]",
			"scope",
			|_lua, this, (id,): (usize,)| {
				if let Some(scope) = this.0.get(id).cloned() {
					Ok(ScopeWrapper(scope))
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
