//! The sandboxed Lua 5.4 runtime: bindings, script rules, events and slots.

// `#[api]` refers to this crate as `::nml_lua`, also from inside it.
extern crate self as nml_lua;

mod api;
mod error;
mod events;
mod runtime;
mod script;
mod unit;

pub use api::API;
pub use api::ApiFunction;
pub use api::TYPES_DIR;
pub use api::luacats;
pub use api::write_types;
pub use nml_lua_macros::api;
pub use runtime::Limits;
pub use runtime::Runtime;
pub use script::add;

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
		let mut language = Language::builtin();
		crate::add(&mut language).unwrap();
		let source = Source::new(Url::parse("file:///page.nml").unwrap(), text.into(), 0);
		let mut resolver = MemoryResolver::default();
		resolver.files.insert("page.nml".into(), text.into());
		nml_lang::compile(&language, Arc::new(source), "page.nml", &resolver)
	}

	/// The HTML of a compiled unit, without filling its slots.
	pub(crate) fn html(compiled: &Compiled) -> String {
		let options = nml_html::PageOptions::default();
		nml_html::compile(&compiled.unit.document, &options).content
	}
}
