use std::collections::HashMap;
use std::sync::Arc;

use mlua::Table;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::DiagnosticSeverity;
use nml_document::Dependency;
use nml_grammar::Node;
use nml_lang::Language;
use nml_lang::Lower;
use nml_source::Source;
use nml_source::Token;

use crate::script::chunk;
use crate::unit::unit_lua;

/// The modules loaded in a unit's Lua state, and those being loaded.
#[derive(Default)]
pub(crate) struct Modules {
	/// Exports by module file.
	loaded: HashMap<String, Table>,
	/// The module files being loaded, outermost first.
	loading: Vec<String>,
}

/// Loads imported modules' Lua into the unit's state.
pub(crate) fn add(language: &mut Language) {
	language.on_import(|lower, import| {
		if let Some(exports) = load(lower, &import.path, import.source.clone(), &import.token) {
			let globals = unit_lua(lower).runtime.lua().globals();
			if let Err(error) = globals.raw_set(import.namespace.as_str(), exports) {
				let diagnostic = unit_lua(lower)
					.chunks
					.borrow()
					.diagnostic(&error, import.token.clone());
				lower.diagnostics.push(diagnostic);
			}
		}
	});
}

/// The exports of a module: what its Lua defines, run in an environment of its own so that
/// nothing leaks into the importer's globals. A module is loaded once per unit. `token` is
/// where it is imported, for errors. Returns `None` after reporting an error.
fn load(
	lower: &mut Lower<'_, '_>,
	path: &str,
	source: Arc<Source>,
	token: &Token,
) -> Option<Table> {
	let modules = &mut unit_lua(lower).modules;
	if let Some(exports) = modules.loaded.get(path) {
		return Some(exports.clone());
	}
	if modules.loading.iter().any(|loading| loading == path) {
		let kind = DiagnosticKind::ImportCycle { path: path.into() };
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(token.clone())));
		return None;
	}
	modules.loading.push(path.to_string());
	let exports = run(lower, path, source);
	let modules = &mut unit_lua(lower).modules;
	modules.loading.pop();
	let exports = exports?;
	modules.loaded.insert(path.to_string(), exports.clone());
	Some(exports)
}

/// Runs a module's `:lua` blocks and imports, and returns its exports.
fn run(lower: &mut Lower<'_, '_>, path: &str, source: Arc<Source>) -> Option<Table> {
	let rules = lower.language().rules();
	let lua_rule = rules.id("lua");
	let import_rule = rules.id("import");
	let parsed = nml_grammar::parse(rules, source);
	let failed = parsed
		.diagnostics
		.iter()
		.any(|d| d.severity == DiagnosticSeverity::Error);
	lower.diagnostics.extend(parsed.diagnostics);
	if failed {
		return None;
	}

	let lua = unit_lua(lower).runtime.lua().clone();
	let report = |lower: &mut Lower<'_, '_>, error: mlua::Error, at: &Token| {
		let diagnostic = unit_lua(lower)
			.chunks
			.borrow()
			.diagnostic(&error, at.clone());
		lower.diagnostics.push(diagnostic);
	};
	let env = lua.create_table().ok()?;
	let meta = lua.create_table().ok()?;
	meta.raw_set("__index", lua.globals()).ok()?;
	env.set_metatable(Some(meta)).ok()?;

	for node in &parsed.nodes {
		let Node::Match(m) = node else { continue };
		if Some(m.rule) == lua_rule {
			let code = m.capture("code").unwrap().token.clone();
			let (name, chunk_source) = chunk(lower, path, &code, "");
			let result = unit_lua(lower)
				.runtime
				.load_in(chunk_source.content(), &name, env.clone())
				.and_then(|function| function.call::<()>(()));
			if let Err(error) = result {
				report(lower, error, &m.token);
				return None;
			}
		} else if Some(m.rule) == import_rule {
			let module = &m.capture("module").unwrap().token;
			let namespace = match m.capture("alias") {
				Some(alias) => alias.token.content().to_string(),
				None => module.content().rsplit('.').next().unwrap().to_string(),
			};
			let Ok((file, source)) = lower.resolver.module(module.content()) else {
				let kind = DiagnosticKind::FileNotFound {
					path: module.content().into(),
				};
				let message = DiagnosticMessage::new(module.clone());
				lower.diagnostics.push(Diagnostic::new(kind, message));
				return None;
			};
			lower.depend(Dependency::Import(module.content().into()));
			let exports = load(lower, &file, source, module)?;
			if let Err(error) = env.raw_set(namespace, exports) {
				report(lower, error, module);
				return None;
			}
		}
	}

	// The exports are what the module defined, without the fallback to the globals.
	let exports = lua.create_table().ok()?;
	for pair in env.pairs::<mlua::Value, mlua::Value>() {
		let (key, value) = pair.ok()?;
		exports.raw_set(key, value).ok()?;
	}
	Some(exports)
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile_with;
	use crate::tests::html;

	fn util() -> (&'static str, &'static str) {
		(
			"modules/std/util.nml",
			":lua\n```\nlocal prefix = 'Hello, '\nfunction greet(name) return prefix .. name end\nversion = 2\n```\n\nContent is not imported.\n",
		)
	}

	#[test]
	fn modules_export_their_globals() {
		let compiled = compile_with(
			":import std.util\n:import std.util as u\n\n:lua`util.greet('NML')` :lua`u.version` :lua`tostring(greet == nil and version == nil and prefix == nil)`\n",
			&[util()],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let body = html(&compiled);
		assert!(body.contains("<p>Hello, NML 2 true</p>"), "{body}");
		assert!(!body.contains("Content is not imported"), "{body}");
	}

	#[test]
	fn modules_import_modules() {
		let compiled = compile_with(
			":import std.page\n\n:lua`page.title('x')`\n",
			&[
				util(),
				(
					"modules/std/page.nml",
					":import std.util as base\n:lua\n```\nfunction title(t) return base.greet(t) .. '!' end\n```\n",
				),
			],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>Hello, x!</p>"));
		assert_eq!(
			compiled.unit.dependencies,
			[
				nml_document::Dependency::Import("std.page".into()),
				nml_document::Dependency::Import("std.util".into())
			]
		);
	}

	#[test]
	fn import_cycles() {
		let compiled = compile_with(
			":import std.a\n",
			&[
				("modules/std/a.nml", ":import std.b\n"),
				("modules/std/b.nml", ":import std.a\n"),
			],
		);
		assert_diagnostics!(compiled.diagnostics, [3010 @ 8..13]);
		assert_eq!(
			compiled.diagnostics[0]
				.primary()
				.location
				.source()
				.location()
				.as_str(),
			"file:///modules/std/b.nml"
		);
	}

	#[test]
	fn module_errors_point_into_the_module() {
		let compiled = compile_with(
			":import std.bad\n",
			&[("modules/std/bad.nml", ":lua\n```\nx = nil + 1\n```\n")],
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(
			location.source().location().as_str(),
			"file:///modules/std/bad.nml"
		);
		assert_eq!(location.content(), "x = nil + 1");

		let compiled = compile_with(
			":import std.bad\n",
			&[("modules/std/bad.nml", "**unclosed\n")],
		);
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}
}
