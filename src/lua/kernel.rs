
use mlua::IntoLua;
use mlua::LightUserData;
use mlua::Lua;
use mlua::Table;

use crate::parser::parser::ParserRuleAccessor;
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
pub struct KernelContext<'ctx, 'u> {
	pub location: Token,
	pub unit: &'ctx mut TranslationUnit<'u>,
	pub redirects: Vec<KernelRedirect>,
	pub reports: Vec<Report>,
}

impl<'ctx, 'u> KernelContext<'ctx, 'u> {
	pub fn new(location: Token, unit: &'ctx mut TranslationUnit<'u>) -> Self {
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

		//kernel
		//	.lua
		//	.globals()
		//	.set(
		//		"print",
		//		kernel
		//			.lua
		//			.create_function({
		//				let ctx = kernel.context.clone();
		//				move |lua, msg: String| {
		//					// TODO
		//					//kernel.with_context_mut(|mut ctx| {
		//					//	ctx.redirects.push(KernelRedirect {
		//					//		source: "print".into(),
		//					//		content: msg,
		//					//	});
		//					//});
		//					Ok(())
		//				}
		//			})
		//			.unwrap(),
		//	)
		//	.unwrap();

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

	/// Evaluates callback with context
	pub fn with_context<'lua, F, R>(lua: &'lua Lua, f: F) -> R
	where
		F: FnOnce(&mut KernelContext<'lua, 'lua>) -> R,
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
	pub fn run_with_context<'lua, 'ctx, 'u, F, R>(
		&'lua self,
		mut ctx: KernelContext<'ctx, 'u>,
		f: F,
	) -> R
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

	/// Creates a function and inserts it into a table
	pub fn create_function<'lua, A, R, F>(&'lua self, table: mlua::Table, name: &str, f: F)
	where
		A: mlua::FromLuaMulti<'lua>,
		R: mlua::IntoLuaMulti<'lua>,
		F: Fn(&'lua Lua, A) -> mlua::Result<R> + 'static,
	{
		let fun = self.lua.create_function(f).unwrap();
		table.set(name, fun).unwrap();
	}
}
