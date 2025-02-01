use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::IntoLua;
use mlua::Lua;
use mlua::Table;
use mlua::UserData;

use crate::document::document::Document;
use crate::parser::new::Parser;
use crate::parser::new::ParserRuleAccessor;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::parser::translation::TranslationUnit;

/// Redirected data from lua execution
pub struct KernelRedirect {
	/// Message source e.g print()
	pub source: String,
	/// Message content
	pub content: String,
}

pub struct KernelContext<'u> {
	pub location: Token,
	pub unit: &'u mut TranslationUnit<'u>,
	pub redirects: Vec<KernelRedirect>,
	pub reports: Vec<Report>,
}

impl<'u> KernelContext<'u> {
	pub fn new(
		location: Token,
		unit: &'u mut TranslationUnit<'u>,
	) -> Self {
		Self {
			location,
			unit,
			redirects: vec![],
			reports: vec![],
		}
	}
}

pub struct ContextWrapper<'u> {
	context: KernelContext<'u>,
}

impl<'u> UserData for ContextWrapper<'u> {}

thread_local! {
	pub static CTX: RefCell<Option<&'static mut KernelContext<'static>>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub struct Kernel {
	pub lua: Lua,
}

impl Kernel {
	pub fn new(parser: &Parser) -> Self {
		let lua = Lua::new();

		{
			let nml_table = lua.create_table().unwrap();
			nml_table
				.set("tables", lua.create_table().unwrap())
				.unwrap();

			parser.iter_rules().for_each(|rule| {
				let table = lua.create_table().unwrap();
				// TODO: Export this so we can check for duplicate rules based on this name
				let name = rule.name().to_lowercase().replace(' ', "_");
				for (fun_name, fun) in rule.register_bindings(&lua) {
					table.set(fun_name, fun).unwrap();
				}
				nml_table.set(name, table).unwrap();
			});
			lua.globals().set("nml", nml_table).unwrap();
		}

		lua.globals()
			.set(
				"print",
				lua.create_function(|_, msg: String| {
					CTX.with_borrow_mut(|ctx| {
						if let Some(ctx) = ctx.as_mut() { ctx.redirects.push(KernelRedirect {
								source: "print".into(),
								content: msg,
							}); }
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
	/// This is the only way lua code should be ran, because exported
	/// functions may require the context in order to operate
	pub fn run_with_context<'lua, F, R>(&'lua self, ctx: KernelContext<'lua>, f: F) -> Result<R, mlua::Error>
		where F: FnOnce(&'lua Lua) -> R
	{
		let ctx_wrapper = ContextWrapper { context: unsafe { std::mem::transmute(ctx) } };
		self.lua.globals().set("nml.ctx", ctx_wrapper)?;
		
		let val = f(&self.lua);

		return Ok(val);
	}

	/// Exports a table to lua
	///
	/// This function exports a table to lua. The exported table is available under `nml.tables.{name}`.
	/// This function will overwrite any previously defined table using the same name.
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

/// Holds all the lua kernels
pub struct KernelHolder {
	kernels: HashMap<String, Kernel>,
}

impl KernelHolder {
	pub fn new(parser: &Parser) -> Self {
		let mut kernels = HashMap::default();
		// Add default kernel
		kernels.insert("main".into(), Kernel::new(parser));
		Self {
			kernels
		}
	}
	pub fn get(&self, kernel_name: &str) -> Option<&Kernel> { self.kernels.get(kernel_name) }

	pub fn insert(&mut self, kernel_name: String, kernel: Kernel) {
		self.kernels.insert(kernel_name, kernel);
	}
}
