use std::cell::Cell;
use std::rc::Rc;

use mlua::HookTriggers;
use mlua::Lua;
use mlua::LuaOptions;
use mlua::MultiValue;
use mlua::StdLib;
use mlua::VmState;

/// Base functions that are not exposed: they reach files or load code from strings.
const REMOVED: &[&str] = &[
	"load",
	"loadfile",
	"dofile",
	"require",
	"collectgarbage",
	"print",
];

/// How much a unit's scripts may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
	/// Instructions, for all of a unit's scripts together.
	pub instructions: u64,
	/// Bytes of memory.
	pub memory: usize,
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			instructions: 100_000_000,
			memory: 64 << 20,
		}
	}
}

/// How often the instruction count is checked.
const HOOK_INTERVAL: u32 = 1000;

/// The Lua state of one unit: standard Lua 5.4, sandboxed.
///
/// Scripts get the base functions (without `load`, `loadfile`, `dofile`, `require`,
/// `collectgarbage` and `print`), and `string`, `table`, `math`, `utf8` and `coroutine`.
/// There is no `io`, `os`, `debug` or `package`: everything outside the script goes through
/// the `nml` table, which records it as a dependency.
#[derive(Debug)]
pub struct Runtime {
	lua: Lua,
}

impl Runtime {
	pub fn new(limits: Limits) -> mlua::Result<Self> {
		let libs = StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE;
		let lua = Lua::new_with(libs, LuaOptions::default())?;
		let globals = lua.globals();
		for name in REMOVED {
			globals.raw_set(*name, mlua::Nil)?;
		}
		// Bytecode is never loaded, so it does not need to be produced either.
		let string: mlua::Table = globals.get("string")?;
		string.raw_set("dump", mlua::Nil)?;
		let api = lua.create_table()?;
		crate::api::register(&lua, &api, crate::API)?;
		crate::rules::register_values(&api)?;
		crate::tables::register(&lua, &api)?;
		globals.raw_set("nml", api)?;

		lua.set_memory_limit(limits.memory)?;
		let executed = Rc::new(Cell::new(0u64));
		lua.set_global_hook(
			HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
			move |_, _| {
				executed.set(executed.get() + u64::from(HOOK_INTERVAL));
				if executed.get() > limits.instructions {
					return Err(mlua::Error::runtime(format!(
						"the scripts of this document ran more than {} instructions",
						limits.instructions
					)));
				}
				Ok(VmState::Continue)
			},
		)?;
		Ok(Self { lua })
	}

	pub fn lua(&self) -> &Lua {
		&self.lua
	}

	/// The `nml` table: the API scripts see.
	pub fn api(&self) -> mlua::Table {
		self.lua
			.globals()
			.get("nml")
			.expect("the runtime defines `nml`")
	}

	/// Compiles a chunk without running it.
	pub fn load(&self, code: &str, name: &str) -> mlua::Result<mlua::Function> {
		self.lua
			.load(code)
			.set_name(format!("={name}"))
			.into_function()
	}

	/// Compiles a chunk whose globals are `env`.
	pub fn load_in(
		&self,
		code: &str,
		name: &str,
		env: mlua::Table,
	) -> mlua::Result<mlua::Function> {
		self.lua
			.load(code)
			.set_name(format!("={name}"))
			.set_environment(env)
			.into_function()
	}

	/// Runs a chunk. `name` is what Lua's errors show for it (`name:3: …`).
	pub fn run(&self, code: &str, name: &str) -> mlua::Result<MultiValue> {
		self.lua.load(code).set_name(format!("={name}")).eval()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn runtime() -> Runtime {
		Runtime::new(Limits::default()).unwrap()
	}

	fn eval(runtime: &Runtime, code: &str) -> mlua::Result<String> {
		let values = runtime.run(code, "test")?;
		Ok(values
			.iter()
			.map(|value| value.to_string().unwrap_or_default())
			.collect::<Vec<_>>()
			.join(","))
	}

	#[test]
	fn standard_libraries() {
		let runtime = runtime();
		let code = r#"
			local t = {}
			for word in string.gmatch("a b c", "%a") do table.insert(t, word:upper()) end
			local co = coroutine.wrap(function() coroutine.yield(1) end)
			return table.concat(t, "-"), math.max(1, 2), utf8.len("é"), co(), type(nml)
		"#;
		assert_eq!(eval(&runtime, code).unwrap(), "A-B-C,2,1,1,table");
	}

	#[test]
	fn sandbox() {
		let runtime = runtime();
		for name in [
			"io",
			"os",
			"debug",
			"package",
			"load",
			"loadfile",
			"dofile",
			"require",
			"collectgarbage",
			"print",
			"string.dump",
		] {
			let value = eval(&runtime, &format!("return type({name})")).unwrap();
			assert_eq!(value, "nil", "{name} is exposed");
		}
		// The state persists between chunks of the unit.
		runtime.run("x = 41", "a").unwrap();
		assert_eq!(eval(&runtime, "return x + 1").unwrap(), "42");
	}

	#[test]
	fn errors_name_their_chunk() {
		let error = runtime()
			.run("\n\nerror('boom')", "page.nml:lua:0")
			.unwrap_err();
		assert!(
			error.to_string().contains("page.nml:lua:0:3: boom"),
			"{error}"
		);
	}

	#[test]
	fn instruction_limit() {
		let runtime = Runtime::new(Limits {
			instructions: 100_000,
			..Limits::default()
		})
		.unwrap();
		let error = runtime.run("while true do end", "loop").unwrap_err();
		assert!(
			error.to_string().contains("more than 100000 instructions"),
			"{error}"
		);
		// Also inside coroutines.
		let runtime = Runtime::new(Limits {
			instructions: 100_000,
			..Limits::default()
		})
		.unwrap();
		let error = runtime
			.run("coroutine.wrap(function() while true do end end)()", "co")
			.unwrap_err();
		assert!(error.to_string().contains("instructions"), "{error}");
	}

	#[test]
	fn memory_limit() {
		let runtime = Runtime::new(Limits {
			memory: 1 << 20,
			..Limits::default()
		})
		.unwrap();
		let error = runtime
			.run("local t = {} for i = 1, 1e7 do t[i] = i end", "memory")
			.unwrap_err();
		assert!(matches!(error, mlua::Error::MemoryError(_)), "{error}");
	}
}
