use mlua::UserData;

use crate::lua::wrappers::OnceLockWrapper;

impl<T> UserData for OnceLockWrapper<T>
where
	for<'lua> T: mlua::FromLua<'lua> + mlua::IntoLua<'lua> + Clone,
	T: Send + Sync + 'static,
{
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("get", |_lua, this, ()| match this.0.get() {
			Some(v) => Ok(Some(v.clone())),
			None => Ok(None),
		});

		methods.add_method("set", |_lua, this, value: T| match this.0.set(value) {
			Ok(()) => Ok(true),
			Err(_already) => Ok(false),
		});
	}
}
