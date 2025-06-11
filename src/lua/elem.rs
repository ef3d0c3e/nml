use std::sync::Arc;

use mlua::LuaSerdeExt;
use mlua::UserData;

use crate::unit::element::Element;

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
				return Err(mlua::Error::RuntimeError(format!(
					"Element {} doesn't support downcasting!",
					this.inner.element_name()
				)));
			};

			Ok(down)
		});
	}
}
