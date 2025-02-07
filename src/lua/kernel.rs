use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::IntoLua;
use mlua::Lua;
use mlua::Table;
use mlua::UserData;

use crate::document::document::Document;
use crate::parser::parser::ParserRuleAccessor;
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

/// Lua execution context
pub struct KernelContext<'u> {
	pub location: Token,
	pub unit: &'u mut TranslationUnit<'u>,
	pub redirects: Vec<KernelRedirect>,
	pub reports: Vec<Report>,
}

impl<'u> KernelContext<'u> {
	pub fn new(location: Token, unit: &'u mut TranslationUnit<'u>) -> Self {
		Self {
			location,
			unit,
			redirects: vec![],
			reports: vec![],
		}
	}
}

pub trait ContextAccessor {
	fn with_context<F, R>(&self, f: F) -> R
	where
		F: FnOnce(Ref<'_, KernelContext<'static>>) -> R;

	fn with_context_mut<F, R>(&self, f: F) -> R
	where
		F: FnOnce(RefMut<'_, KernelContext<'static>>) -> R;
}

impl ContextAccessor for Rc<RefCell<Option<KernelContext<'static>>>>
{
    fn with_context<F, R>(&self, f: F) -> R
	    where
		    F: FnOnce(Ref<'_, KernelContext<'static>>) -> R {

		f(Ref::map((*self.clone()).borrow(), |context| {
			unsafe { std::mem::transmute( context.as_ref().unwrap() ) }
		}))
    }

    fn with_context_mut<F, R>(&self, f: F) -> R
	    where
		    F: FnOnce(RefMut<'_, KernelContext<'static>>) -> R {
		f(RefMut::map((*self.clone()).borrow_mut(), |context| {
			unsafe { std::mem::transmute( context.as_ref().unwrap() ) }
		}))
    }
}

/// Stores lua related informations for a translation unit
pub struct Kernel {
	lua: Lua,
	context: Rc<RefCell<Option<KernelContext<'static>>>>,
}

impl Kernel {
	pub fn new(unit: &TranslationUnit) -> Self {
		let kernel = Self { lua: Lua::new(), context: Rc::new(RefCell::default()) };

		// Export modified print function to redirect it's output
		
		kernel.lua.globals()
			.set(
				"print",
				kernel.lua.create_function({ let ctx = kernel.context.clone(); move |lua, msg: String| {
					//kernel.with_context_mut(|mut ctx| {
					//	ctx.redirects.push(KernelRedirect {
					//		source: "print".into(),
					//		content: msg,
					//	});
					//});
					Ok(())
				} })
				.unwrap(),
			)
			.unwrap();

		// Create `nml` table
		let nml_table = kernel.lua.create_table().unwrap();

		// Export tables
		nml_table
			.set("tables", kernel.lua.create_table().unwrap())
			.unwrap();

		// Register functions from parser rules
		unit.parser().rules_iter().for_each(|rule| {
			let table = kernel.lua.create_table().unwrap();

			rule.register_bindings(&kernel, table.clone());

			// TODO: Export this so we can check for duplicate rules based on this name
			let name = rule.name().to_lowercase().replace(' ', "_");
			nml_table.set(name, table).unwrap();
		});

		kernel.lua.globals().set("nml", nml_table).unwrap();

		kernel
	}

	/// Runs a procedure with a context
	///
	/// This is the only way lua code should be ran, because exported
	/// functions may require the context in order to operate
	pub fn run_with_context<'lua, 'ctx, F, R>(
		&'lua self,
		ctx: KernelContext<'ctx>,
		f: F,
	) -> Result<R, mlua::Error>
	where
		F: FnOnce(&'lua Lua) -> R,
	{
		self.context.replace(unsafe { std::mem::transmute(Some(ctx)) });

		let val = f(&self.lua);

		self.context.replace(None);
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

	/// Creates a function and inserts it into a table
	pub fn create_function<'lua, A, R, F>(&'lua self, table: mlua::Table, name: &str, f: F)
	where
        A: mlua::FromLuaMulti<'lua>,
        R: mlua::IntoLuaMulti<'lua>,
        F: Fn(Rc<RefCell<Option<KernelContext<'static>>>>, &'lua Lua, A) -> mlua::Result<R> + 'static,
	{
		let ctx = self.context.clone();
		let fun = self.lua.create_function(move |lua: &'lua Lua, a: A| f(ctx.clone(), lua, a)).unwrap();
		table.set(name, fun);
	}
}

/// Holds all the lua kernels
#[derive(Default)]
pub struct KernelHolder {
	kernels: HashMap<String, Kernel>,
}

impl KernelHolder {
	/// Call this function after [`KernelHolder`] creation to create the default `main` lua kernel
	pub fn initialize_main(&mut self, unit: &TranslationUnit) {
		self.kernels.insert("main".into(), Kernel::new(unit));
	}

	pub fn get<S: AsRef<str>>(&self, name: S) -> Option<&Kernel> {
		self.kernels.get(name.as_ref())
	}

	pub fn insert(&mut self, name: String, kernel: Kernel) {
		self.kernels.insert(name, kernel);
	}
}
