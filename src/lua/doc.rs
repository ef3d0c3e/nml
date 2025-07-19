use std::{collections::HashMap, sync::{LazyLock}};
use mlua::Lua;
use parking_lot::Mutex;

use crate::{lua::kernel::{LuaFunc, LUA_FUNC}, parser::parser::Parser};

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
			let mut docs = crate::lua::doc::LUA_DOCS.lock();
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
	fn document_args(params: &Vec<&'static str>) -> (String, String)
	{
		let mut docs = String::new();
		let mut args = String::new();
		for param in params
		{
			let (arg, doc) = param.split_once(" ").map_or_else(
				|| (param.trim_start().trim_end(), ""),
				|(arg, doc)| (arg.trim_start().trim_end(), doc.trim_start().trim_end())
			);
			if let Some((name, ty)) = arg.split_once(":") {
				if !doc.is_empty()
				{
					docs += &format!("--- @param {name} {ty} {doc}\n");
				}
				else
				{
					docs += &format!("--- @param {name} {ty}\n");
				}
				if !args.is_empty() { args += ", " }
				args += &format!("{name}");
			}
			else {
				if !doc.is_empty()
				{
					docs += &format!("--- @param {arg} {doc}\n");
				}
				else
				{
					docs += &format!("--- @param {arg} {doc}\n");
				}
				if !args.is_empty() { args += ", " }
				args += format!("{arg}").as_str();
			}
		}
		(docs, args)
	}

	fn document_class(doc: &UserDataDoc) -> String
	{
		let mut buf = String::new();
		// Class header
		buf += &format!("--- @type {}\n", doc.type_name);
		buf += &format!("local {} = {{}}\n\n", doc.type_name);

		// Methods
		for m in &doc.methods {
			// doc comment
			buf += &format!("--- {}\n", m.doc);
			let mut args = String::default();
			let mut args_doc = String::default();
			let (args_doc, args) = document_args(&m.args);
			buf += args_doc.as_str();
			// @return if any
			if let Some(ret) = &m.returns {
				buf += &format!("--- @return {}\n", ret);
			}
			// function signature
			// we ignore args other than self, since Lua methods always get :self
			buf += &format!(
				"function {}:{}({args}) end\n\n",
				doc.type_name, m.name
			);
		}
		buf
	}

	// Create documentation for classes
	make_lua_docs!();
	let docs = LUA_DOCS.lock();
	for (_, doc) in docs.iter()
	{
		println!("{}", document_class(doc));
	}

	fn document_function(name: &'static str, fun: &LuaFunc) -> String
	{
		let mut buf = String::new();
		if !fun.doc.is_empty()
		{
			buf += &format!("--- {}\n", fun.doc);
		}
		let (args_doc, args) = document_args(&fun.args);
		buf += args_doc.as_str();
		if !fun.ret.is_empty() {
			buf += &format!("--- @return {}\n", fun.ret);
		}
		buf += &format!(
			"function nml.{}({args}) end\n\n",
			name
		);
		buf
	}


	println!("local nml = {{}}\n");
	// Create parser to populate bindings
	let _parser = Parser::new();
	let docs = LUA_FUNC.lock();
	for (name, fun) in docs.iter()
	{
		println!("{}", document_function(name, fun));
	}
}
