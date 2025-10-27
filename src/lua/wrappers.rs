use std::sync::OnceLock;

use mlua::{IntoLua, UserData};

#[auto_registry::auto_registry(registry = "lua")]
pub struct OnceLockWrapper<T>(pub OnceLock<T>);

impl<T> UserData for OnceLockWrapper<T>
where
    for<'lua> T: mlua::FromLua<'lua> + mlua::IntoLua<'lua> + Clone + core::fmt::Debug,
    T: Send + Sync + 'static,
{
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("get", |_lua, this, ()| {
			eprintln!("IN GET : {:#?}", this.0);
			match this.0.get() {
				Some(v) => Ok(Some(v.clone())),
				None => Ok(None),
			}
		});

		methods.add_method("set", |_lua, this, value: T| {
			match this.0.set(value) {
				Ok(()) => Ok(true),
				Err(_already) => Ok(false),
			}
		});
	}
}
