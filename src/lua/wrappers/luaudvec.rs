use crate::lua::wrappers::LuaUDVec;

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
