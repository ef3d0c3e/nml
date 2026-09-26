//! The sandboxed Lua 5.4 runtime: bindings, script rules, events and slots.

// `#[api]` refers to this crate as `::nml_lua`, also from inside it.
extern crate self as nml_lua;

mod api;
mod deferred;
mod elements;
mod error;
mod events;
mod files;
mod module;
mod post_parse;
mod rules;
mod runtime;
mod script;
#[cfg(test)]
mod std_modules;
mod system;
mod tables;
mod unit;
mod variables;

pub use api::API;
pub use api::ApiFunction;
pub use api::CLASSES;
pub use api::DISABLED_LIBRARIES;
pub use api::LuaClass;
pub use api::LuaField;
pub use api::TYPES_DIR;
pub use api::luacats;
pub use api::write_types;
pub use deferred::DeferredRuntime;
pub use deferred::PageInfo;
pub use nml_lua_macros::api;
pub use nml_lua_macros::userdata;
pub use runtime::Limits;
pub use runtime::Runtime;
pub use script::add;

/// The modules that come with NML, as `std.<name>`, unless the workspace declares a module
/// named `std`: their name and source.
pub const STD: &[(&str, &str)] = &[
	("toc", include_str!("../std/toc.nml")),
	("nav", include_str!("../std/nav.nml")),
];

/// Where editors find the modules that come with NML, relative to the workspace root: a
/// copy of each, `std/<name>.nml`, written by [`write_std`].
pub const STD_DIR: &str = ".nml/std";

/// Writes the modules that come with NML to [`STD_DIR`], read-only, for editors to open:
/// those that changed, and removes the others.
pub fn write_std(root: &std::path::Path) -> std::io::Result<()> {
	let dir = root.join(STD_DIR);
	std::fs::create_dir_all(&dir)?;
	for (name, content) in STD {
		let path = dir.join(format!("{name}.nml"));
		if std::fs::read_to_string(&path).is_ok_and(|current| current == *content) {
			continue;
		}
		if let Ok(metadata) = std::fs::metadata(&path) {
			let mut permissions = metadata.permissions();
			#[allow(clippy::permissions_set_readonly_false)]
			permissions.set_readonly(false);
			std::fs::set_permissions(&path, permissions)?;
		}
		std::fs::write(&path, content)?;
		let mut permissions = std::fs::metadata(&path)?.permissions();
		permissions.set_readonly(true);
		std::fs::set_permissions(&path, permissions)?;
	}
	for entry in std::fs::read_dir(&dir)? {
		let path = entry?.path();
		let known = (path.file_stem())
			.and_then(|stem| stem.to_str())
			.is_some_and(|stem| STD.iter().any(|(name, _)| *name == stem));
		if !known || path.extension().is_none_or(|e| e != "nml") {
			// Read-only files can be removed: the directory is writable.
			if path.is_dir() {
				std::fs::remove_dir_all(&path)?;
			} else {
				std::fs::remove_file(&path)?;
			}
		}
	}
	Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
	use std::sync::Arc;

	use nml_lang::Compiled;
	use nml_lang::Language;
	use nml_lang::MemoryResolver;
	use nml_source::Source;
	use nml_source::Url;

	/// Compiles `page.nml` with the built-in rules and Lua.
	pub(crate) fn compile(text: &str) -> Compiled {
		compile_with(text, &[])
	}

	/// Compiles `page.nml` with other files in memory, and the module `std` at
	/// `modules/std`.
	pub(crate) fn compile_with(text: &str, files: &[(&str, &str)]) -> Compiled {
		let mut resolver = MemoryResolver::default();
		for (path, content) in files {
			resolver.files.insert(path.to_string(), content.to_string());
		}
		compile_with_resolver(text, resolver)
	}

	/// Compiles `page.nml` with a resolver, to which it and the module `std` are added.
	pub(crate) fn compile_with_resolver(text: &str, mut resolver: MemoryResolver) -> Compiled {
		let mut language = Language::builtin();
		crate::add(&mut language).unwrap();
		let source = Source::new(Url::parse("file:///page.nml").unwrap(), text.into(), 0);
		resolver.files.insert("page.nml".into(), text.into());
		resolver.modules.insert("std".into(), "modules/std".into());
		for (name, content) in crate::STD {
			let path = format!("modules/std/{name}.nml");
			resolver.files.entry(path).or_insert(content.to_string());
		}
		nml_lang::compile(&language, Arc::new(source), "page.nml", Arc::new(resolver))
	}

	/// The HTML of a compiled unit, without filling its slots.
	pub(crate) fn html(compiled: &Compiled) -> String {
		let options = nml_html::PageOptions::default();
		nml_html::compile(&compiled.unit.document, &options).content
	}
}
