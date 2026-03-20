use core::panic;

use mlua::{FromLua, IntoLua, UserData};

use crate::lua::wrappers::{IntoLuaProxy, LuaProxyVecProxy, LuaProxyVecProxyMut, LuaProxyVecProxyOwned};

impl<T> IntoLua for LuaProxyVecProxy<T>
where
	T: IntoLuaProxy,
	T::Proxy: UserData + 'static,
{
	fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
		let tbl: mlua::Table = lua.create_table()?;
		let r = unsafe { &*self.0 as &Vec<T> };
		for (i, item) in r.iter().enumerate() {
			let proxy = T::as_proxy(item as _);
			let ud = lua.create_userdata(proxy)?;
			tbl.set(i + 1, ud)?;
		}
		Ok(mlua::Value::Table(tbl))
	}
}

impl<T> IntoLua for LuaProxyVecProxyMut<T>
where
	T: IntoLuaProxy,
	T::ProxyMut: UserData + 'static,
{
	fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
		let tbl: mlua::Table = lua.create_table()?;
		let r = unsafe { &mut *self.0 as &mut Vec<T> };
		for (i, item) in r.iter_mut().enumerate() {
			let proxy = T::as_proxy_mut(item as _);
			let ud = lua.create_userdata(proxy)?;
			tbl.set(i + 1, ud)?;
		}
		Ok(mlua::Value::Table(tbl))
	}
}

impl<T> FromLua for LuaProxyVecProxyOwned<T>
where
    T: IntoLuaProxy,
    T::Proxy: UserData + 'static,
    T::ProxyMut: UserData + 'static,
{
    fn from_lua(val: mlua::Value, lua: &mlua::Lua) -> mlua::Result<Self> {
        let table = mlua::Table::from_lua(val, lua)?;
        let mut vec = Vec::new();
        for i in 1..=table.len()? {
            let ud: mlua::AnyUserData = table.get(i)?;
            // Try immutable proxy first, then mutable
            let item = ud.borrow::<T::Proxy>()
                .map(|p| T::from_proxy(&p))
                .or_else(|_| ud.borrow::<T::ProxyMut>()
                    .map(|p| T::from_proxy_mut(&p)))?;
            vec.push(item);
        }
        Ok(LuaProxyVecProxyOwned(vec))
    }
}
