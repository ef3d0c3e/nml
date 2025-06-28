use std::{collections::HashMap, sync::{LazyLock, Mutex}};

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
