use std::cell::RefCell;
use std::collections::HashMap;

use mlua::Lua;

use crate::document::document::Document;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::Token;

pub struct KernelContext<'a, 'b, 'c> {
	pub location: Token,
	pub state: &'a ParserState<'a, 'b>,
	pub document: &'c dyn Document<'c>,
}

thread_local! {
	pub static CTX: RefCell<Option<KernelContext<'static, 'static, 'static>>> = RefCell::new(None);
}

#[derive(Debug)]
pub struct Kernel {
	lua: Lua,
}

impl Kernel {
	pub fn new(parser: &dyn Parser) -> Self {
		let lua = Lua::new();

		{
			let nml_table = lua.create_table().unwrap();

			for rule in parser.rules() {
				let table = lua.create_table().unwrap();
				// TODO: Export this so we can check for duplicate rules based on this name
				let name = rule.name().to_lowercase().replace(' ', "_");
				for (fun_name, fun) in rule.register_bindings(&lua) {
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
	pub fn run_with_context<T, F>(&self, context: KernelContext, f: F) -> T
	where
		F: FnOnce(&Lua) -> T,
	{
		CTX.set(Some(unsafe { std::mem::transmute(context) }));
		let ret = f(&self.lua);
		CTX.set(None);

		ret
	}
}

#[derive(Default)]
pub struct KernelHolder {
	kernels: HashMap<String, Kernel>,
}

impl KernelHolder {
	pub fn get(&self, kernel_name: &str) -> Option<&Kernel> { self.kernels.get(kernel_name) }

	pub fn insert(&mut self, kernel_name: String, kernel: Kernel) {
		self.kernels.insert(kernel_name, kernel);
	}
}
