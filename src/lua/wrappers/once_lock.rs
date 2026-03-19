use std::sync::OnceLock;

use mlua::UserData;

use crate::lua::wrappers::OnceLockWrapper;

impl<T> UserData for OnceLockWrapper<T>
where
	T: mlua::FromLua + mlua::IntoLua + Clone + Send + Sync + 'static,
{
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("get", |_lua, this, ()| match unsafe { &*this.0 as &OnceLock<T> }.get() {
			Some(v) => Ok(Some(v.clone())),
			None => Ok(None),
		});

		methods.add_method("set", |_lua, this, value: T| match unsafe { &*this.0 as &OnceLock<T> }.set(value) {
			Ok(()) => Ok(true),
			Err(_already) => Ok(false),
		});
	}
}
