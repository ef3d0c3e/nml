use std::sync::Arc;

use crate::add_documented_method;
use mlua::IntoLua;
use mlua::LuaSerdeExt;
use mlua::UserData;

use crate::lua::wrappers::SourceWrapper;
use crate::parser::source::Source;

impl UserData for SourceWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Source",
			"location",
			|lua, this, ()| {
				let r = unsafe { &*this.0 as &Arc<dyn Source> };
				match r.location().cloned() {
					Some(token) => {
						let ud = lua.create_userdata(token)?;
						Ok(ud.into_lua(lua)?)
					}
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
				let r = unsafe { &*this.0 as &Arc<dyn Source> };
				Ok(lua.to_value(r.name()))
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
				let r = unsafe { &*this.0 as &Arc<dyn Source> };
				Ok(lua.to_value(r.url()))
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
				let r = unsafe { &*this.0 as &Arc<dyn Source> };
				Ok(lua.to_value(r.content()))
			},
			"Get the source's content",
			vec![],
			Some("string The source's content")
		);
	}
}
