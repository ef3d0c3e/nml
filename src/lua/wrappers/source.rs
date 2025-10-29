use mlua::IntoLua;
use mlua::LuaSerdeExt;
use mlua::UserData;
use crate::add_documented_method;

use crate::lua::wrappers::SourceWrapper;

impl UserData for SourceWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Source",
			"location",
			|lua, this, ()| {
				match this.0.location().cloned()
				{
					Some(token) => {
						let ud = lua.create_userdata(token)?;
						Ok(ud.into_lua(lua)?)
					},
					None => Ok(mlua::Value::Nil),
				}
			},
			"Get the parent token of the source",
			vec![],
			Some("token? The source's parent token")
		);
		add_documented_method!(
			methods,
			"Source",
			"name",
			|lua, this, ()| {
				Ok(lua.to_value(this.0.name()))
			},
			"Get the name of the source. For SourceFile, this corresponde to the source's path",
			vec![],
			Some("string The source's name")
		);
		add_documented_method!(
			methods,
			"Source",
			"url",
			|lua, this, ()| {
				Ok(lua.to_value(this.0.url()))
			},
			"Get the url of the source",
			vec![],
			Some("string The source's url")
		);
		add_documented_method!(
			methods,
			"Source",
			"content",
			|lua, this, ()| {
				Ok(lua.to_value(this.0.content()))
			},
			"Get the source's content",
			vec![],
			Some("string The source's content")
		);
	}
}
