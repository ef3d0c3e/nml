use std::collections::HashMap;

use mlua::Lua;
use nml_lang::Lower;

use crate::api;

/// The unit's variables, as they are when a script runs.
#[derive(Default)]
pub(crate) struct Variables(HashMap<String, String>);

/// The value of a variable set with `:set` or `:export` before the script runs, or exported
/// by an imported module (`namespace.name`), as written: its markup is not parsed. `nil` if
/// it is not set. Only known while the document is parsed.
#[api]
fn variable(lua: &Lua, name: String) -> mlua::Result<Option<String>> {
	let variables = lua.app_data_ref::<Variables>().ok_or_else(|| {
		mlua::Error::runtime("variables are only known while the document is parsed")
	})?;
	Ok(variables.0.get(&name).cloned())
}

/// Copies the unit's variables into its Lua state.
pub(crate) fn sync(lower: &Lower<'_, '_>, lua: &Lua) {
	let variables = lower
		.variables
		.iter()
		.map(|(name, variable)| (name.clone(), variable.value.content().to_string()))
		.collect();
	lua.set_app_data(Variables(variables));
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile_with;
	use crate::tests::html;

	#[test]
	fn variables() {
		let compiled = compile_with(
			":import std.meta\n:set name = *NML*\n\n:lua`nml.variable('name')` :lua`nml.variable('meta.version')` :lua`tostring(nml.variable('later'))`\n:set later = x\n:lua`nml.variable('later')`\n",
			&[("modules/std/meta.nml", ":export version = 2\n")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		assert!(html.contains("<p>*NML* 2 nil</p>\n<p>x</p>"), "{html}");
	}
}
