use std::{cell::{RefCell, RefMut}, rc::Rc};

use mlua::{Error, FromLua, Lua, UserData, UserDataMethods};

use crate::{document::document::Document, parser::{parser::Parser, source::Token}};

pub struct KernelContext<'a>
{
	pub location: Token,
	pub parser: &'a dyn Parser,
	pub document: &'a Document<'a>,
	//pub parser: &'a dyn Parser,
}

thread_local! {
    pub static CTX: RefCell<Option<KernelContext<'static>>> = RefCell::new(None);
}

#[derive(Debug)]
pub struct Kernel
{
	lua: Lua,
}

impl Kernel {

	// TODO: Take parser as arg and 
	// iterate over the rules
	// to find export the bindings (if some)
    pub fn new(parser: &dyn Parser) -> Self {
		let lua = Lua::new();

		{
			let nml_table = lua.create_table().unwrap();

			for rule in parser.rules()
			{
				let table = lua.create_table().unwrap();
				let name = rule.name().to_lowercase();

				for (fun_name, fun) in rule.lua_bindings(&lua)
				{
					table.set(fun_name, fun).unwrap();
				}

				nml_table.set(name, table).unwrap();
			}
			lua.globals().set("nml", nml_table).unwrap();
		}

		Self { lua }
    }

	/// Runs a procedure with a context
	///
	/// This is the only way lua code shoule be ran, because exported
	/// functions may require the context in order to operate
	pub fn run_with_context<T, F>(&self, context: KernelContext, f: F)
		-> T
	where
		F: FnOnce(&Lua) -> T
	{
		CTX.set(Some(unsafe { std::mem::transmute(context) }));
		let ret = f(&self.lua);
		CTX.set(None);

		ret
	}
}

pub trait KernelHolder
{
	fn get_kernel(&self, name: &str) -> Option<RefMut<'_, Kernel>>;

	fn insert_kernel(&self, name: String, kernel: Kernel) -> RefMut<'_, Kernel>;
}
