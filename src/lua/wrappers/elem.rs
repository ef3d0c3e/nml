use mlua::FromLua;
use mlua::LuaSerdeExt;
use mlua::UserData;
use mlua::Value;

use crate::lua::wrappers::ElemWrapper;

impl UserData for ElemWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("name", |lua, this, ()| lua.to_value(this.0.element_name()));
		methods.add_method("kind", |lua, this, ()| lua.to_value(&this.0.kind()));
		methods.add_method("downcast", |lua, this, ()| {
			let Some(down) = this.0.clone().lua_wrap(lua) else {
				return Err(mlua::Error::RuntimeError(format!(
					"Element {} doesn't support downcasting!",
					this.0.element_name()
				)));
			};

			Ok(down)
		});
	}
}

impl FromLua for ElemWrapper {
	fn from_lua(value: Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
		let ud = match value {
			Value::UserData(ud) => ud,
			other => {
				return Err(mlua::Error::FromLuaConversionError {
					from: other.type_name(),
					to: "ElemWrapper".into(),
					message: Some("expected ElemWrapper userdata".into()),
				})
			}
		};
		let wrapper = ud.borrow::<ElemWrapper>()?;
		Ok(wrapper.clone())
	}
}
