use crate::lua::wrappers::{LuaUdVecProxy, LuaUdVecProxyMut};

/// Now any `LuaVec<T>` where `T: UserData + Clone + 'static`
/// can be turned into a table of `T` userdata.
impl<T> mlua::IntoLua for LuaUdVecProxy<T>
where
	T: mlua::UserData + Clone + 'static,
{
	fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
		let tbl: mlua::Table = lua.create_table()?;
		let r = unsafe { &*self.0 as &Vec<T> };
		for (i, item) in r.iter().enumerate() {
			// clone out the T, wrap in userdata, stick at 1‑based index
			let ud = lua.create_userdata(item.clone())?;
			tbl.set(i + 1, ud)?;
		}
		Ok(mlua::Value::Table(tbl))
	}
}

impl<T> mlua::IntoLua for LuaUdVecProxyMut<T>
where
	T: mlua::UserData + Clone + 'static,
{
	fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
		let tbl: mlua::Table = lua.create_table()?;
		let r = unsafe { &*self.0 as &Vec<T> };
		for (i, item) in r.iter().enumerate() {
			// clone out the T, wrap in userdata, stick at 1‑based index
			let ud = lua.create_userdata(item.clone())?;
			tbl.set(i + 1, ud)?;
		}
		Ok(mlua::Value::Table(tbl))
	}
}
