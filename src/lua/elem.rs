use std::sync::Arc;

use mlua::FromLua;
use mlua::LuaSerdeExt;
use mlua::UserData;
use mlua::Value;

use crate::unit::element::Element;

#[derive(Clone)]
pub struct ElemWrapper(pub Arc<dyn Element>);

impl UserData for ElemWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("name", |lua, this, ()| {
			lua.to_value(this.0.element_name())
		});
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

impl<'lua> FromLua<'lua> for ElemWrapper {
	fn from_lua(value: Value<'lua>, _lua: &'lua mlua::Lua) -> mlua::Result<Self> {
		let ud = match value {
			Value::UserData(ud) => ud,
			other => {
				return Err(mlua::Error::FromLuaConversionError {
					from: other.type_name(),
					to: "ElemWrapper",
					message: Some("expected ElemWrapper userdata".into()),
				})
			}
		};
		let wrapper = ud.borrow::<ElemWrapper>()?;
		Ok(wrapper.clone())
	}
}

/// Wrapper for a Vector of UserData objects
pub struct LuaUDVec<T>(pub Vec<T>);

/// Now any `LuaVec<T>` where `T: UserData + Clone + 'static`
/// can be turned into a table of `T` userdata.
impl<'lua, T> mlua::IntoLua<'lua> for LuaUDVec<T>
where
	T: mlua::UserData + Clone + 'static,
{
	fn into_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
		let tbl: mlua::Table = lua.create_table()?;
		for (i, item) in self.0.iter().enumerate() {
			// clone out the T, wrap in userdata, stick at 1‑based index
			let ud = lua.create_userdata(item.clone())?;
			tbl.set(i + 1, ud)?;
		}
		Ok(mlua::Value::Table(tbl))
	}
}
