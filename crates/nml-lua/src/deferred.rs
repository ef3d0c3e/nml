use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use mlua::AnyUserData;
use mlua::Lua;
use mlua::Table;
use mlua::UserData;
use mlua::Value;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::DeferredSlot;
use nml_document::SerialValue;
use nml_grammar::Node;
use nml_lang::Language;
use nml_lang::Resolver;
use nml_lang::relative_output_url;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;
use nml_source::VirtualSourceBuilder;

use crate::Limits;
use crate::Runtime;
use crate::api;
use crate::error::Chunks;

/// The value of `nml.defer`: shown by a `:lua` role, it becomes a deferred slot.
pub(crate) struct Defer(pub DeferredSlot);

impl UserData for Defer {}

/// Defers content to link time: shown by a `:lua` role, the value becomes what
/// `module.function(args)` returns (text, elements, or a list of them) when the page is
/// linked, possibly in a later build. The
/// function runs in a fresh Lua state with the query API (`nml.pages`), so it can list the
/// workspace's pages; the page is relinked when they change. The module must be imported by
/// the document, and `args` must be data: nil, booleans, numbers, strings, and tables of
/// them.
#[api(returns = "userdata")]
fn defer(
	lua: &Lua,
	module: String,
	function: String,
	args: Option<Value>,
) -> mlua::Result<AnyUserData> {
	let args = match args {
		Some(args) => serial(args)?,
		None => SerialValue::Nil,
	};
	lua.create_userdata(Defer(DeferredSlot {
		module,
		function,
		args,
	}))
}

/// A Lua value as stored data.
fn serial(value: Value) -> mlua::Result<SerialValue> {
	Ok(match value {
		Value::Nil => SerialValue::Nil,
		Value::Boolean(b) => SerialValue::Bool(b),
		Value::Integer(n) => SerialValue::Integer(n),
		Value::Number(n) => SerialValue::Number(n),
		Value::String(s) => SerialValue::String(s.to_str()?.to_string()),
		Value::Table(table) => {
			let length = table.raw_len();
			let mut count = 0;
			let mut map = Vec::new();
			let mut list = Vec::new();
			for pair in table.pairs::<Value, Value>() {
				let (key, value) = pair?;
				count += 1;
				match key {
					Value::String(key) => map.push((key.to_str()?.to_string(), serial(value)?)),
					Value::Integer(_) => {}
					_ => return Err(mlua::Error::runtime("table keys must be strings or a list")),
				}
			}
			if map.is_empty() && count == length {
				for value in table.sequence_values::<Value>() {
					list.push(serial(value?)?);
				}
				SerialValue::List(list)
			} else if map.len() == count {
				map.sort_by(|(a, _), (b, _)| a.cmp(b));
				SerialValue::Map(map)
			} else {
				return Err(mlua::Error::runtime(
					"a table is either a list or has string keys",
				));
			}
		}
		other => {
			return Err(mlua::Error::runtime(format!(
				"deferred arguments are data, not a {}",
				other.type_name()
			)));
		}
	})
}

/// Stored data as a Lua value.
fn lua_value(lua: &Lua, value: &SerialValue) -> mlua::Result<Value> {
	Ok(match value {
		SerialValue::Nil => Value::Nil,
		SerialValue::Bool(b) => Value::Boolean(*b),
		SerialValue::Integer(n) => Value::Integer(*n),
		SerialValue::Number(n) => Value::Number(*n),
		SerialValue::String(s) => Value::String(lua.create_string(s)?),
		SerialValue::List(list) => {
			let table = lua.create_table()?;
			for value in list {
				table.raw_push(lua_value(lua, value)?)?;
			}
			Value::Table(table)
		}
		SerialValue::Map(map) => {
			let table = lua.create_table()?;
			for (key, value) in map {
				table.raw_set(key.as_str(), lua_value(lua, value)?)?;
			}
			Value::Table(table)
		}
	})
}

/// A page of the workspace, as the query API shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInfo {
	/// Relative to the workspace root.
	pub path: String,
	/// The reference key.
	pub name: String,
	pub title: String,
	/// Where the page is written, relative to the output directory.
	pub output: String,
	/// The variables the page exports, by name, with their values as written.
	pub variables: Vec<(String, String)>,
}

/// What a deferred function can query: the workspace's pages, seen from the page being
/// linked.
struct QueryContext {
	pages: Arc<Vec<PageInfo>>,
	current: String,
}

/// The pages of the workspace, for deferred functions: a list of `{ path, name, title,
/// url, current, variables }`. `url` is relative to the page being linked, `current` tells
/// whether it is that page, and `variables` holds the variables the page exports
/// (`:export`), as written. Only available at link time.
#[api(
	returns = "{ path: string, name: string, title: string, url: string, current: boolean, variables: table<string, string> }[]"
)]
fn pages(lua: &Lua) -> mlua::Result<Table> {
	let context = lua.app_data_ref::<QueryContext>().ok_or_else(|| {
		mlua::Error::runtime("`nml.pages` is only available in deferred functions, at link time")
	})?;
	let list = lua.create_table()?;
	let current = context
		.pages
		.iter()
		.find(|page| page.path == context.current)
		.map(|page| page.output.clone())
		.unwrap_or_else(|| nml_lang::output_path(&context.current));
	for page in context.pages.iter() {
		let entry = lua.create_table()?;
		entry.raw_set("path", page.path.as_str())?;
		entry.raw_set("name", page.name.as_str())?;
		entry.raw_set("title", page.title.as_str())?;
		entry.raw_set("url", relative_output_url(&current, &page.output))?;
		entry.raw_set("current", page.path == context.current)?;
		let variables = lua.create_table()?;
		for (name, value) in &page.variables {
			variables.raw_set(name.as_str(), value.as_str())?;
		}
		entry.raw_set("variables", variables)?;
		list.raw_push(entry)?;
	}
	Ok(list)
}

/// Fills the deferred slots of one page: a fresh Lua state, with the query API, where the
/// modules are loaded as they are needed.
pub struct DeferredRuntime {
	runtime: Runtime,
	language: Language,
	resolver: Arc<dyn Resolver>,
	chunks: Rc<RefCell<Chunks>>,
	/// Exports by module path.
	modules: HashMap<String, Table>,
}

impl std::fmt::Debug for DeferredRuntime {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("DeferredRuntime").finish_non_exhaustive()
	}
}

impl DeferredRuntime {
	/// A runtime for the page at `current`.
	pub fn new(resolver: Arc<dyn Resolver>, pages: Arc<Vec<PageInfo>>, current: String) -> Self {
		let runtime = Runtime::new(Limits::default()).expect("a Lua state can be created");
		runtime.lua().set_app_data(QueryContext { pages, current });
		let mut language = Language::builtin();
		crate::add(&mut language).expect("the Lua rules are valid");
		Self {
			runtime,
			language,
			resolver,
			chunks: Rc::default(),
			modules: HashMap::new(),
		}
	}

	/// The HTML of a deferred slot written at `at`.
	pub fn fill(&mut self, slot: &DeferredSlot, at: &Token) -> Result<String, Diagnostic> {
		let lua_error = |chunks: &RefCell<Chunks>, error: mlua::Error| {
			chunks.borrow().diagnostic(&error, at.clone())
		};
		let exports = self.module(&slot.module, at, &mut Vec::new())?;
		let lua = self.runtime.lua();
		let function: mlua::Function = exports.get(slot.function.as_str()).map_err(|_| {
			lua_error(
				&self.chunks,
				mlua::Error::runtime(format!(
					"`{}` has no function `{}`",
					slot.module, slot.function
				)),
			)
		})?;
		let args = lua_value(lua, &slot.args).map_err(|e| lua_error(&self.chunks, e))?;
		function
			.call::<Value>(args)
			.and_then(crate::elements::nodes)
			.and_then(|elements| crate::elements::html(&elements, None))
			.map_err(|e| lua_error(&self.chunks, e))
	}

	/// Loads a module and its imports, once.
	fn module(
		&mut self,
		module: &str,
		at: &Token,
		loading: &mut Vec<String>,
	) -> Result<Table, Diagnostic> {
		if let Some(exports) = self.modules.get(module) {
			return Ok(exports.clone());
		}
		let report = |kind| Diagnostic::new(kind, DiagnosticMessage::new(at.clone()));
		if loading.iter().any(|m| m == module) {
			return Err(report(DiagnosticKind::ImportCycle {
				path: module.into(),
			}));
		}
		let (path, source) = self.resolver.module(module).map_err(|_| {
			report(DiagnosticKind::FileNotFound {
				path: module.into(),
			})
		})?;
		loading.push(module.to_string());
		let exports = self.run(&path, source, at, loading);
		loading.pop();
		let exports = exports?;
		self.modules.insert(module.to_string(), exports.clone());
		Ok(exports)
	}

	fn run(
		&mut self,
		path: &str,
		source: Arc<Source>,
		at: &Token,
		loading: &mut Vec<String>,
	) -> Result<Table, Diagnostic> {
		let lua = self.runtime.lua().clone();
		let lua_error = |chunks: &RefCell<Chunks>, error: mlua::Error, at: &Token| {
			chunks.borrow().diagnostic(&error, at.clone())
		};
		let rules = self.language.rules();
		let (lua_rule, import_rule) = (rules.id("lua"), rules.id("import"));
		let parsed = nml_grammar::parse(rules, source);
		if let Some(error) = parsed.diagnostics.into_iter().next() {
			return Err(error);
		}
		let setup = || -> mlua::Result<Table> {
			let env = lua.create_table()?;
			let meta = lua.create_table()?;
			meta.raw_set("__index", lua.globals())?;
			env.set_metatable(Some(meta))?;
			Ok(env)
		};
		let env = setup().map_err(|e| lua_error(&self.chunks, e, at))?;
		for node in &parsed.nodes {
			let Node::Match(m) = node else { continue };
			if Some(m.rule) == lua_rule {
				let code = &m.capture("code").unwrap().token;
				let name = format!("{path}@{}", code.start());
				let url = Url::parse(&format!("virtual:///{path}/lua/{}", code.start())).unwrap();
				let mut builder =
					VirtualSourceBuilder::new(code.source().clone(), url, code.start());
				builder.copy(code.range());
				let chunk = Arc::new(builder.build());
				self.chunks.borrow_mut().add(name.clone(), chunk.clone());
				self.runtime
					.load_in(chunk.content(), &name, env.clone())
					.and_then(|function| function.call::<()>(()))
					.map_err(|e| lua_error(&self.chunks, e, &m.token))?;
			} else if Some(m.rule) == import_rule {
				let module = m.capture("module").unwrap().token.content();
				let namespace = match m.capture("alias") {
					Some(alias) => alias.token.content().to_string(),
					None => module.rsplit('.').next().unwrap().to_string(),
				};
				let exports = self.module(module, &m.token, loading)?;
				env.raw_set(namespace, exports)
					.map_err(|e| lua_error(&self.chunks, e, &m.token))?;
			}
		}
		let exports = lua
			.create_table()
			.map_err(|e| lua_error(&self.chunks, e, at))?;
		for pair in env.pairs::<Value, Value>() {
			let (key, value) = pair.map_err(|e| lua_error(&self.chunks, e, at))?;
			exports
				.raw_set(key, value)
				.map_err(|e| lua_error(&self.chunks, e, at))?;
		}
		Ok(exports)
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Slot;
	use nml_lang::MemoryResolver;

	use super::*;
	use crate::tests::compile_with;

	const NAV: &str = r#":lua
```
function menu(args)
	local links = {}
	for _, page in ipairs(nml.pages()) do
		if #links < args.limit then
			table.insert(links, nml.element('link', { url = page.url, page.title }))
		end
	end
	local items = {}
	for i, link in ipairs(links) do
		if i > 1 then table.insert(items, args.separator) end
		table.insert(items, link)
	end
	return items
end
```
"#;

	#[test]
	fn deferred_slots() {
		let compiled = compile_with(
			":import std.nav\n\n:lua`nml.defer('std.nav', 'menu', { limit = 2, separator = ' <|> ' })`\n",
			&[("modules/std/nav.nml", NAV)],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let (_, Slot::Deferred(slot)) = document.slots().next().unwrap() else {
			panic!()
		};
		assert_eq!(slot.module, "std.nav");
		assert_eq!(
			slot.args,
			SerialValue::Map(vec![
				("limit".into(), SerialValue::Integer(2)),
				("separator".into(), SerialValue::String(" <|> ".into())),
			])
		);

		let mut resolver = MemoryResolver::default();
		resolver
			.files
			.insert("modules/std/nav.nml".into(), NAV.into());
		resolver.modules.insert("std".into(), "modules/std".into());
		let page = |path: &str, title: &str, output: &str| PageInfo {
			path: path.into(),
			name: path.into(),
			title: title.into(),
			output: output.into(),
			variables: Vec::new(),
		};
		// URLs go from the page's output to the others', wherever their sources are.
		let pages = Arc::new(vec![
			page("index.nml", "Home", "index.html"),
			page("docs/guide.nml", "Guide", "guide/index.html"),
			page("docs/faq.nml", "FAQ", "docs/faq.html"),
		]);
		let mut runtime = DeferredRuntime::new(Arc::new(resolver), pages, "docs/faq.nml".into());
		let at = document.get(document.root()).token.clone();
		assert_eq!(
			runtime.fill(slot, &at).unwrap(),
			r#"<a href="../index.html">Home</a> &lt;|&gt; <a href="../guide/index.html">Guide</a>"#
		);
	}

	#[test]
	fn defer_errors() {
		// The module must be imported.
		let compiled = compile_with(
			":lua`nml.defer('std.nav', 'menu')`\n",
			&[("modules/std/nav.nml", NAV)],
		);
		assert_diagnostics!(compiled.diagnostics, [4005]);

		// Arguments are data.
		let compiled = compile_with(
			":import std.nav\n\n:lua`nml.defer('std.nav', 'menu', { f = function() end })`\n",
			&[("modules/std/nav.nml", NAV)],
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);

		// The query API only exists at link time.
		let compiled = compile_with(":lua`#nml.pages()`\n", &[]);
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn link_time_errors() {
		let mut resolver = MemoryResolver::default();
		resolver.files.insert(
			"modules/std/bad.nml".into(),
			":lua\n```\nfunction f() error('late') end\nfunction env() return nml.env('HOME') end\nfunction read() return nml.read('a.nml') end\nfunction time() return nml.date().year end\nfunction local_media() return nml.element('media', {url = 'a.png'}) end\nfunction external_media() return nml.element('media', {url = 'https://x.y/a.png'}) end\nfunction figure() return nml.element('figure', {'x'}) end\n```\n".into(),
		);
		resolver.modules.insert("std".into(), "modules/std".into());
		let mut runtime = DeferredRuntime::new(Arc::new(resolver), Arc::default(), "a.nml".into());
		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"x".into(),
			0,
		));
		let at = Token::new(source, 0..1);
		let slot = |module: &str, function: &str| DeferredSlot {
			module: module.into(),
			function: function.into(),
			args: SerialValue::Nil,
		};

		let error = runtime.fill(&slot("std.bad", "f"), &at).unwrap_err();
		assert_eq!(
			error.primary().location.root().source().location().as_str(),
			"file:///modules/std/bad.nml"
		);
		let error = runtime.fill(&slot("std.bad", "g"), &at).unwrap_err();
		assert!(matches!(error.kind, DiagnosticKind::LuaError { .. }));
		let error = runtime.fill(&slot("std.gone", "f"), &at).unwrap_err();
		assert_eq!(error.kind.id(), 3008);

		// Media in the workspace and figures need the parse.
		for (function, message) in [
			(
				"local_media",
				"only external media can be shown at link time",
			),
			("figure", "a figure is numbered during the parse"),
		] {
			let error = runtime.fill(&slot("std.bad", function), &at).unwrap_err();
			let DiagnosticKind::LuaError { message: error } = &error.kind else {
				panic!("{error:?}");
			};
			assert!(error.contains(message), "{error}");
		}
		let html = runtime
			.fill(&slot("std.bad", "external_media"), &at)
			.unwrap();
		assert!(html.contains("<img src=\"https://x.y/a.png\""), "{html}");

		// Nothing records dependencies at link time: what reads outside the document fails.
		for (function, name) in [
			("env", "`nml.env`"),
			("read", "`nml.read`"),
			("time", "`nml.time`"),
		] {
			let error = runtime.fill(&slot("std.bad", function), &at).unwrap_err();
			let DiagnosticKind::LuaError { message } = &error.kind else {
				panic!("{error:?}");
			};
			assert!(
				message.contains(&format!(
					"{name} is only available while the document is parsed"
				)),
				"{message}"
			);
		}
	}

	#[test]
	fn arguments_are_data() {
		let runtime = Runtime::new(Limits::default()).unwrap();
		let convert = |code: &str| serial(runtime.lua().load(code).eval::<Value>().unwrap());
		assert_eq!(
			convert("return { 1, 'two', { x = true } }").unwrap(),
			SerialValue::List(vec![
				SerialValue::Integer(1),
				SerialValue::String("two".into()),
				SerialValue::Map(vec![("x".into(), SerialValue::Bool(true))]),
			])
		);
		assert_eq!(
			convert("return { b = 1.5, a = {} }").unwrap(),
			SerialValue::Map(vec![
				("a".into(), SerialValue::List(Vec::new())),
				("b".into(), SerialValue::Number(1.5)),
			])
		);
		assert!(convert("return { 1, a = 2 }").is_err());
		assert!(convert("return { [true] = 1 }").is_err());

		// And back.
		let value = SerialValue::Map(vec![(
			"list".into(),
			SerialValue::List(vec![SerialValue::Integer(1), SerialValue::Nil]),
		)]);
		let lua = runtime.lua();
		lua.globals()
			.set("v", lua_value(lua, &value).unwrap())
			.unwrap();
		assert_eq!(
			lua.load("return #v.list .. type(v.list[1])")
				.eval::<String>()
				.unwrap(),
			"1number"
		);
	}
}
