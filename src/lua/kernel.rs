use std::cell::RefCell;
use std::collections::HashMap;

use graphviz_rust::attributes::overlap_scaling;
use mlua::IntoLua;
use mlua::Lua;
use mlua::Table;

use crate::document::document::Document;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Token;

/// Redirected data from lua execution
pub struct KernelRedirect {
	/// Message source e.g print()
	pub source: String,
	/// Message content
	pub content: String,
}

pub struct KernelContext<'a, 'b, 'c> {
	pub location: Token,
	pub state: &'a ParserState<'a, 'b>,
	pub document: &'c dyn Document<'c>,
	pub redirects: Vec<KernelRedirect>,
	pub reports: Vec<Report>,
}

impl<'a, 'b, 'c> KernelContext<'a, 'b, 'c> {
	pub fn new(
		location: Token,
		state: &'a ParserState<'a, 'b>,
		document: &'c dyn Document<'c>,
	) -> Self {
		Self {
			location,
			state,
			document,
			redirects: vec![],
			reports: vec![],
		}
	}
}

thread_local! {
	pub static CTX: RefCell<Option<&'static mut KernelContext<'static, 'static, 'static>>> = const { RefCell::new(None) };
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
			nml_table
				.set("tables", lua.create_table().unwrap())
				.unwrap();

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

		lua.globals()
			.set(
				"print",
				lua.create_function(|_, msg: String| {
					CTX.with_borrow_mut(|ctx| {
						ctx.as_mut().map(|ctx| {
							ctx.redirects.push(KernelRedirect {
								source: "print".into(),
								content: msg,
							});
						});
					});
					Ok(())
				})
				.unwrap(),
			)
			.unwrap();

		Self { lua }
	}

	/// Runs a procedure with a context
	///
	/// This is the only way lua code shoule be ran, because exported
	/// functions may require the context in order to operate
	pub fn run_with_context<T, F>(&self, context: &mut KernelContext, f: F) -> T
	where
		F: FnOnce(&Lua) -> T,
	{
		// Redirects
		CTX.set(Some(unsafe { std::mem::transmute(context) }));
		let ret = f(&self.lua);
		CTX.set(None);

		ret
	}

	/// Exports a table to lua
	pub fn export_table<'lua, K: IntoLua<'lua>>(
		&'lua self,
		name: &str,
		table: Vec<K>,
	) -> Result<(), String> {
		let nml: Table<'_> = self.lua.globals().get("nml").unwrap();
		let tables: Table<'_> = nml.get("tables").unwrap();
		if let Err(err) = tables.raw_set(name, table) {
			return Err(err.to_string());
		}

		Ok(())
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
