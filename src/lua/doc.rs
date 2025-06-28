use std::{collections::HashMap, sync::{LazyLock, Mutex}};
use mlua::Lua;

/// Documented userdata method
#[derive(Debug)]
pub struct LuaMethodDoc {
	pub name: &'static str,
	pub args: Vec<&'static str>,
	pub returns: Option<&'static str>,
	pub doc: &'static str,
}

/// Documented userdata
#[derive(Debug)]
pub struct UserDataDoc {
	pub type_name: &'static str,
	pub methods: Vec<LuaMethodDoc>,
}

pub static LUA_DOCS: LazyLock<Mutex<HashMap<&'static str, UserDataDoc>>> = LazyLock::new(|| Mutex::new(HashMap::default()));

#[macro_export]
macro_rules! add_documented_method {
    ($methods:ident, $type_name:literal, $name:literal, $handler:expr, $documentation:expr, $args:expr, $ret:expr) => {{
			$methods.add_method($name, $handler);
			let mut docs = crate::lua::doc::LUA_DOCS.lock().unwrap();
			if let Some(doc) = docs.get_mut($type_name) {
				doc.methods.push(crate::lua::doc::LuaMethodDoc {
					name: $name,
					args: $args,
					returns: $ret,
					doc: $documentation,
				});
			}
			else
			{
				docs
					.insert($type_name, crate::lua::doc::UserDataDoc {
						type_name: $type_name,
						methods:
							vec![
							crate::lua::doc::LuaMethodDoc {
								name: $name,
								args: $args,
								returns: $ret,
								doc: $documentation,
							}
							]
					});
			}
    }};
}

macro_rules! add_lua_docs {
	($t:ty) => {{
		let lua = Lua::new();

		lua.register_userdata_type::<$t>(|reg| <$t as mlua::UserData>::add_methods(reg)).unwrap();
		0
	}};
}

#[auto_registry::generate_registry(registry = "lua", mapper = add_lua_docs, output = make_lua_docs)]
pub fn get_lua_docs()
{
	make_lua_docs!();
	fn document_class(doc: &UserDataDoc) -> String
	{
		let mut buf = String::new();
		// Class header
		buf += &format!("--- @class {}\n", doc.type_name);
		buf += &format!("local {} = {{}}\n\n", doc.type_name);

		// Methods
		for m in &doc.methods {
			// doc comment
			buf += &format!("--- {}\n", m.doc);
			// @return if any
			if let Some(ret) = &m.returns {
				buf += &format!("--- @return {}\n", ret);
			}
			// function signature
			// we ignore args other than self, since Lua methods always get :self
			buf += &format!(
				"function {}:{}() end\n\n",
				doc.type_name, m.name
			);
		}
		buf
	}

	let docs = LUA_DOCS.lock().unwrap();
	for (_, doc) in docs.iter()
	{
		println!("{}", document_class(doc));
	}
}
