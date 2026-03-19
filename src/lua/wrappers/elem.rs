use mlua::FromLua;
use mlua::LuaSerdeExt;
use mlua::UserData;
use mlua::Value;

use crate::lua::wrappers::ElemWrapper;
use crate::lua::wrappers::ElemWrapperMut;
use crate::unit::element::Element;

impl UserData for ElemWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("name", |lua, this, ()| lua.to_value(this.0.element_name()));
		methods.add_method("kind", |lua, this, ()| lua.to_value(&this.0.kind()));
		methods.add_method("downcast", |lua, this, ()| Ok(this.0.lua_ud(lua)));
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
		Ok(ElemWrapper(wrapper.0.clone()))
	}
}

impl UserData for ElemWrapperMut {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("name", |lua, this, ()| {
			let r: &dyn Element = unsafe { &*this.0 };
			lua.to_value(r.element_name())
		});
		methods.add_method("kind", |lua, this, ()| {
			let r: &dyn Element = unsafe { &*this.0 };
			lua.to_value(&r.kind())
		});
		methods.add_method_mut("downcast", |lua, this, ()| {
			Ok(unsafe { (*this.0).lua_ud_mut(lua) })
		});
	}
}
