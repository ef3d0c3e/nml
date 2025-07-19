use std::cell::LazyCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use graphviz_rust::print;
use mlua::IntoLua;
use mlua::LightUserData;
use mlua::Lua;
use mlua::Table;
use mlua::Value;
use parking_lot::Mutex;

use crate::parser::parser::ParserRuleAccessor;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::translation::TranslationUnit;

use super::unit::UnitWrapper;

/// Lua function
pub struct LuaFunc {
	/// Table path of the function
	pub name: &'static str,
	/// Function doc string
	pub doc: &'static str,
	/// Funcction arguments doc
	pub args: Vec<&'static str>,
	/// Function return value doc
	pub ret: &'static str,
	/// Function
	pub fun: std::sync::Arc<
		dyn for<'lua> Fn(&'lua Lua, mlua::MultiValue<'lua>) -> mlua::Result<mlua::MultiValue<'lua>>
			+ Send
			+ Sync,
	>,
}

pub static LUA_FUNC: LazyLock<Mutex<HashMap<&'static str, LuaFunc>>> =
	LazyLock::new(|| Mutex::new(HashMap::default()));

#[macro_export]
macro_rules! add_documented_function {
	($name:literal, $handler:expr, $documentation:expr, $args:expr, $ret:expr) => {{
		fn wrap_lua_fn<A, R, F>(
			f: F,
		) -> std::sync::Arc<
			dyn for<'lua> Fn(
					&'lua mlua::Lua,
					mlua::MultiValue<'lua>,
				) -> mlua::Result<mlua::MultiValue<'lua>>
				+ Send
				+ Sync,
		>
		where
			A: for<'lua> mlua::FromLuaMulti<'lua> + 'static,
			R: for<'lua> mlua::IntoLuaMulti<'lua> + 'static,
			F: for<'lua> Fn(&'lua mlua::Lua, A) -> mlua::Result<R> + Send + Sync + 'static,
		{
			std::sync::Arc::new(move |lua, args| {
				let a = A::from_lua_multi(args, lua)?;
				let r = f(lua, a)?;
				r.into_lua_multi(lua)
			})
		}
		let mut funs = crate::lua::kernel::LUA_FUNC.lock();
		let fun_name = $name.split_once(".").map_or($name, |(_, last)| last);
		funs.insert(
			$name,
			crate::lua::kernel::LuaFunc {
				name: fun_name,
				doc: $documentation,
				args: $args,
				ret: $ret,
				fun: wrap_lua_fn($handler),
			},
		);
	}};
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KernelName(pub String);

impl TryFrom<String> for KernelName {
	type Error = String;

	fn try_from(s: String) -> Result<Self, Self::Error> {
		for c in s.chars() {
			if c.is_whitespace() {
				return Err(format!(
					"Kernel names cannot contain whitespaces, found `{c}`"
				));
			}
			if c.is_ascii_control() {
				return Err(format!("Kernel names cannot contain control sequences"));
			}
			if c.is_ascii_punctuation() {
				return Err(format!(
					"Kernel names cannot contain punctuaction, found `{c}`"
				));
			}
		}
		Ok(KernelName(s))
	}
}

/// Redirected data from lua execution
pub struct KernelRedirect {
	/// Message source e.g print()
	pub source: String,
	/// Message content
	pub content: String,
}

/// Lua execution context
pub struct KernelContext<'ctx> {
	pub location: Token,
	pub unit: &'ctx mut TranslationUnit,
	pub redirects: Vec<KernelRedirect>,
	pub reports: Vec<Report>,
}

impl<'ctx> KernelContext<'ctx> {
	pub fn new(location: Token, unit: &'ctx mut TranslationUnit) -> Self {
		Self {
			location,
			unit,
			redirects: vec![],
			reports: vec![],
		}
	}
}

/// Stores lua related informations for a translation unit
pub struct Kernel {
	lua: Lua,
	//context: Rc<RefCell<Option<KernelContext<'static, 'static>>>>,
}

unsafe impl Send for Kernel {}
unsafe impl Sync for Kernel {}

impl Kernel {
	pub fn new(unit: &TranslationUnit) -> Self {
		let kernel = Self {
			lua: Lua::new(),
			//context: Rc::new(RefCell::default()),
		};

		// Export modified print function to redirect it's output

		kernel
			.lua
			.globals()
			.set(
				"print",
				kernel
					.lua
					.create_function({
						move |lua, msg: String| {
							Kernel::with_context(lua, |ctx| {
								ctx.redirects.push(KernelRedirect {
									source: "print".into(),
									content: msg,
								});
							});
							Ok(())
						}
					})
					.unwrap(),
			)
			.unwrap();

		// Create `nml` table
		let nml_table = kernel.lua.create_table().unwrap();

		// Export tables
		nml_table
			.set("tables", kernel.lua.create_table().unwrap())
			.unwrap();

		// Register accessors
		nml_table
			.set(
				"unit",
				kernel
					.lua
					.create_function(|lua, ()| {
						// Get KernelContext from registry
						let ctx: LightUserData = lua.named_registry_value("__REGISTRY_NML_CTX")?;
						let ctx = ctx.0 as *mut KernelContext;
						let ctx_ref = unsafe { &mut *ctx };

						// Wrap the unit (not as userdata! use a light proxy)
						// Instead of passing a reference directly, create a proxy object with dynamic dispatch
						let wrapper = lua.create_userdata(UnitWrapper {
							inner: &mut ctx_ref.unit,
						})?;

						Ok(wrapper)
					})
					.unwrap(),
			)
			.unwrap();

		// Register functions from parser rules
		let lock = LUA_FUNC.lock();
		for (name, fun) in lock.iter() {
			let mut stack = vec![nml_table.clone()];
			let mut path = *name;
			loop {
				let (name, rest) = path.split_once(".").unwrap_or((path, ""));
				path = rest;
				let top = stack.last().unwrap();

				if rest.is_empty() {
					// Check for duplicate
					match top.get::<_, Value>(name) {
						Ok(Value::Nil) => {}
						_ => panic!("Duplicate binding: {}", fun.name),
					}

					// Insert handler
					let handler = fun.fun.clone();
					top.set(
						name,
						kernel
							.lua
							.create_function(move |lua, args: mlua::MultiValue| {
								(handler)(lua, args)
							})
							.unwrap(),
					)
					.unwrap();
					break;
				}

				match top.get::<_, Value>(name) {
					Ok(Value::Nil) => {
						let table = kernel.lua.create_table().unwrap();
						top.set(name, table.clone());
						stack.push(table);
					}
					_ => {}
				}
			}
		}
		kernel.lua.globals().set("nml", nml_table).unwrap();

		kernel
	}

	/// Evaluates callback with context
	pub fn with_context<'lua, F, R>(lua: &'lua Lua, f: F) -> R
	where
		F: FnOnce(&mut KernelContext<'lua>) -> R,
	{
		let data: LightUserData = lua.named_registry_value("__REGISTRY_NML_CTX").unwrap();
		let ctx = data.0 as *mut KernelContext;
		let ctx_ref = unsafe { &mut *ctx };
		f(ctx_ref)
	}

	/// Runs a procedure with a context
	///
	/// This is the only way lua code should be ran, because exported
	/// functions may require the context in order to operate
	pub fn run_with_context<'lua, 'ctx, F, R>(&'lua self, mut ctx: KernelContext<'ctx>, f: F) -> R
	where
		F: FnOnce(&'lua Lua) -> R,
	{
		let ctx_ptr: *mut KernelContext = &mut ctx as *mut _;
		let data = LightUserData(ctx_ptr as _);
		self.lua
			.set_named_registry_value("__REGISTRY_NML_CTX", data)
			.unwrap();
		let val = f(&self.lua);
		self.lua
			.unset_named_registry_value("__REGISTRY_NML_CTX")
			.unwrap();
		//self.context
		//	.replace(unsafe { std::mem::transmute(Some(ctx)) });
		//self.context.replace(None);
		val
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
