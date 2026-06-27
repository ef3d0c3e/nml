use std::sync::Arc;

use mlua::Lua;
use nml_document::Dependency;
use nml_lang::Lower;
use nml_lang::ResolveError;
use nml_lang::Resolver;

use crate::api;

/// What scripts can read, kept in the unit's Lua state.
pub(crate) struct Files {
	resolver: Arc<dyn Resolver>,
	/// The unit's file: paths are relative to it.
	unit: String,
	/// Files read so far, with the hash of their content.
	read: Vec<(String, String)>,
}

impl Files {
	pub fn new(resolver: Arc<dyn Resolver>, unit: String) -> Self {
		Self {
			resolver,
			unit,
			read: Vec::new(),
		}
	}
}

/// Reads a text file of the workspace, such as a CSV.
///
/// `path` is relative to the document, or to the workspace root with a leading `/`. The
/// document is built again when the file changes.
#[api]
fn read(lua: &Lua, path: String) -> mlua::Result<String> {
	let mut files = lua
		.app_data_mut::<Files>()
		.expect("the unit's Lua state has its files");
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

/// Records the files read as dependencies of the unit. Called when the lowering ends.
pub(crate) fn record(lower: &mut Lower<'_, '_>) {
	let lua = crate::unit::unit_lua(lower).runtime.lua().clone();
	let read = std::mem::take(&mut lua.app_data_mut::<Files>().unwrap().read);
	for (path, hash) in read {
		lower.depend(Dependency::File { path, hash });
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
