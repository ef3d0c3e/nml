use std::sync::Arc;

use mlua::Lua;
use nml_document::Dependency;
use nml_lang::Lower;
use nml_lang::ResolveError;
use nml_lang::Resolver;

use crate::api;

/// What scripts read outside of their document (files, the environment, the clock), kept
/// in the unit's Lua state. Only while the unit is parsed: nothing records a dependency at
/// link time.
pub(crate) struct Files {
	pub resolver: Arc<dyn Resolver>,
	/// The unit's file: paths are relative to it.
	unit: String,
	/// Files read so far, with the hash of their content.
	read: Vec<(String, String)>,
	/// Environment variables read so far, with the hash of their value.
	pub env: Vec<(String, String)>,
	/// Whether the clock was read.
	pub clock: bool,
}

impl Files {
	pub fn new(resolver: Arc<dyn Resolver>, unit: String) -> Self {
		Self {
			resolver,
			unit,
			read: Vec::new(),
			env: Vec::new(),
			clock: false,
		}
	}

	/// The unit's tracker, or an error naming `what` outside of a parse.
	pub fn of<'l>(lua: &'l Lua, what: &str) -> mlua::Result<mlua::AppDataRefMut<'l, Files>> {
		lua.app_data_mut::<Files>().ok_or_else(|| {
			mlua::Error::runtime(format!(
				"{what} is only available while the document is parsed"
			))
		})
	}
}

/// Reads a text file of the workspace, such as a CSV.
///
/// `path` is relative to the document, or to the workspace root with a leading `/`. The
/// document is built again when the file changes.
#[api]
fn read(lua: &Lua, path: String) -> mlua::Result<String> {
	let mut files = Files::of(lua, "`nml.read`")?;
	let (resolved, content) =
		files
			.resolver
			.read(&files.unit, &path)
			.map_err(|error| match error {
				ResolveError::NotFound { path } | ResolveError::UnknownModule { name: path } => {
					mlua::Error::runtime(format!("`{path}` does not exist in the workspace"))
				}
			})?;
	let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
	if !files.read.iter().any(|(path, _)| *path == resolved) {
		files.read.push((resolved, hash));
	}
	Ok(content)
}

/// Records what was read as dependencies of the unit. Called when the lowering ends.
pub(crate) fn record(lower: &mut Lower<'_, '_>) {
	let lua = crate::unit::unit_lua(lower).runtime.lua().clone();
	let mut files = lua.app_data_mut::<Files>().unwrap();
	for (path, hash) in std::mem::take(&mut files.read) {
		lower.depend(Dependency::File { path, hash });
	}
	for (name, hash) in std::mem::take(&mut files.env) {
		lower.depend(Dependency::Env { name, hash });
	}
	if std::mem::take(&mut files.clock) {
		lower.depend(Dependency::Clock);
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;

	use crate::tests::compile_with;
	use crate::tests::html;

	#[test]
	fn reads_files() {
		let compiled = compile_with(
			":lua\n```\nlocal total = 0\nfor n in nml.read('data/n.csv'):gmatch('%d+') do total = total + n end\nsum = total\n```\n\nSum: :lua`sum` :lua`#nml.read('/data/n.csv')`\n",
			&[("data/n.csv", "1,2,3\n")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>Sum: 6 6</p>"));
		// Read twice, recorded once.
		assert_eq!(
			compiled.unit.dependencies,
			[Dependency::File {
				path: "data/n.csv".into(),
				hash: blake3::hash(b"1,2,3\n").to_hex().to_string()
			}]
		);
	}

	#[test]
	fn missing_files() {
		let compiled = compile_with(":lua`nml.read('nope.csv')`\n", &[]);
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let nml_diagnostics::DiagnosticKind::LuaError { message } = &compiled.diagnostics[0].kind
		else {
			panic!()
		};
		assert!(message.contains("`nope.csv` does not exist"), "{message}");
		let compiled = compile_with(":lua`nml.read('../outside')`\n", &[]);
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}
}
