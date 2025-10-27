use std::collections::HashMap;
use std::sync::LazyLock;

use mlua::IntoLua;
use mlua::LightUserData;
use mlua::Lua;
use mlua::Table;
use mlua::Value;
use parking_lot::Mutex;

use crate::lua::wrappers::UnitWrapper;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::translation::TranslationUnit;

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
						let wrapper = lua.create_userdata(UnitWrapper (
							&mut ctx_ref.unit,
						))?;

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
					// Create table if not found
					Ok(Value::Nil) => {
						let table = kernel.lua.create_table().unwrap();
						top.set(name, table.clone()).unwrap();
						stack.push(table);
					}
					// Add table to stack if present
					Ok(Value::Table(tbl)) => {
						stack.push(tbl);
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

/// Lua function
pub struct LuaFunc {
	/// Table path of the function
	pub name: &'static str,
	/// Function doc string
	#[allow(unused)]
	pub doc: &'static str,
	/// Funcction arguments doc
	#[allow(unused)]
	pub args: Vec<&'static str>,
	/// Function return value doc
	#[allow(unused)]
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

pub fn wrap_lua_fn<A, R, F>(
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

#[macro_export]
macro_rules! add_documented_function {
	($name:literal, $handler:expr, $documentation:expr, $args:expr, $ret:expr) => {{
		let mut funs = crate::lua::kernel::LUA_FUNC.lock();
		let fun_name = $name.rsplit_once(".").map_or($name, |(_, last)| last);
		funs.insert(
			$name,
			crate::lua::kernel::LuaFunc {
				name: fun_name,
				doc: $documentation,
				args: $args,
				ret: $ret,
				fun: crate::lua::kernel::wrap_lua_fn($handler),
			},
		);
	}};
}

// For functions that need mlua::Value or other lifetime-bound types
pub fn wrap_lua_fn_with_values<R, F>(
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
	R: for<'lua> mlua::IntoLuaMulti<'lua>,
	F: for<'lua> Fn(&'lua mlua::Lua, mlua::MultiValue<'lua>) -> mlua::Result<R>
		+ Send
		+ Sync
		+ 'static,
{
	std::sync::Arc::new(move |lua, args| {
		let r = f(lua, args)?;
		r.into_lua_multi(lua)
	})
}

#[macro_export]
macro_rules! add_documented_function_values {
	($name:literal, $handler:expr, $documentation:expr, $args:expr, $ret:expr) => {{
		let mut funs = crate::lua::kernel::LUA_FUNC.lock();
		let fun_name = $name.rsplit_once(".").map_or($name, |(_, last)| last);
		funs.insert(
			$name,
			crate::lua::kernel::LuaFunc {
				name: fun_name,
				doc: $documentation,
				args: $args,
				ret: $ret,
				fun: crate::lua::kernel::wrap_lua_fn_with_values($handler),
			},
		);
	}};
}

#[macro_export]
macro_rules! convert_lua_args {
	($lua:expr, $args:expr, $($arg_spec:tt),+ $(,)?) => {{
		let lua = $lua;
		let args: &mlua::MultiValue = &$args;
		let arg_count = args.len();
		let (required_count, total_count) = convert_lua_args!(@count_args $($arg_spec),+);

		if arg_count < required_count {
			return Err(mlua::Error::RuntimeError(format!(
						"Not enough arguments: expected at least {}, got {}",
						required_count, arg_count
			)));
		}
		if arg_count > total_count {
			return Err(mlua::Error::RuntimeError(format!(
						"Too many arguments: expected at most {}, got {}",
						total_count, arg_count
			)));
		}

		let mut arg_iter = args.iter().enumerate();
		($(convert_lua_args!(@convert_arg lua, arg_iter, $arg_spec)?),+)
	}};
	// Count required and total arguments
	(@count_args $($arg_spec:tt),+) => {{
		let mut required = 0usize;
		let mut total = 0usize;
		$(
			let (req, tot) = convert_lua_args!(@count_single $arg_spec);
			required += req;
			total += tot;
		)+
			(required, total)
	}};
	// Count a single argument spec
	(@count_single ($type:ty, $name:literal)) => { (1, 1) };
	(@count_single ($type:ty, $name:literal, userdata)) => { (1, 1) };
	(@count_single ($type:ty, $name:literal, vuserdata)) => { (1, 1) };
	// TODO: Optional arguments
	// Converts a single argument by deserialization
	(@convert_arg $lua:expr, $iter:expr, ($type:ty, $name:literal)) => {{
		// Required argument
		match $iter.next() {
			Some((idx, value)) => {
				$lua.from_value::<$type>(value.clone())
					.map_err(|e| mlua::Error::RuntimeError(format!(
								"Invalid type for argument '{}' at position {}: {}",
								$name, idx + 1, e
					)))
			},
			None => {
				Err(mlua::Error::RuntimeError(format!(
							"Missing required argument '{}'",
							$name
				)))
			}
		}
	}};
	// Converts an array of userdata
	(@convert_arg $lua:expr, $iter:expr, ($inner:ty, $name:literal, vuserdata)) => {{
		// Required Vec<UserData> argument
		match $iter.next() {
			Some((idx, value)) => {
				match value {
					mlua::Value::Table(table) => {
						let mut vec = Vec::new();
						// Iterate through table as array
						(|| {
							for i in 1..=table.len().map_err(|e| mlua::Error::RuntimeError(format!(
								"Failed to get length of table for argument '{}' at position {}: {}",
								$name, idx + 1, e
							)))?
							{
								let item_value = table.get::<i32, mlua::Value>(i as i32)
									.map_err(|e| mlua::Error::RuntimeError(format!(
										"Failed to get item {} from table for argument '{}' at position {}: {}",
										i, $name, idx + 1, e
									)))?;

								match item_value {
									mlua::Value::UserData(ud) => {
										let item = ud.borrow::<$inner>()
											.map_err(|e| mlua::Error::RuntimeError(format!(
												"Invalid UserData type for item {} in argument '{}' at position {}: {}",
												i, $name, idx + 1, e
											)))?
											.clone();
										vec.push(item);
									},
									_ => {
										return Err(mlua::Error::RuntimeError(format!(
													"Expected UserData for item {} in argument '{}' at position {}, got {}",
													i, $name, idx + 1, item_value.type_name()
										)))
									}
								}
							}
							Ok(vec)
						})()
					},
					_ => {
						Err(mlua::Error::RuntimeError(format!(
									"Expected table for Vec<UserData> argument '{}' at position {}, got {}",
									$name, idx + 1, value.type_name()
						)))
					}
				}
			},
			None => {
				Err(mlua::Error::RuntimeError(format!(
							"Missing required Vec<UserData> argument '{}'",
							$name
				)))
			}
		}
	}};
	// Converts a single userdata argument
	(@convert_arg $lua:expr, $iter:expr, ($type:ty, $name:literal, userdata)) => {{
		match $iter.next() {
			Some((idx, value)) => {
				match value {
					mlua::Value::UserData(ud) => {
						ud.borrow::<$type>()
							.map_err(|e| mlua::Error::RuntimeError(format!(
								"Invalid UserData type for argument '{}' at position {}: {}",
								$name, idx + 1, e
							)))
					},
					_ => {
						Err(mlua::Error::RuntimeError(format!(
							"Expected UserData for argument '{}' at position {}, got {}",
							$name, idx + 1, value.type_name()
						)))
					}
				}
			},
			None => {
				Err(mlua::Error::RuntimeError(format!(
							"Missing required UserData argument '{}'",
							$name
				)))
			}
		}
	}};
}
