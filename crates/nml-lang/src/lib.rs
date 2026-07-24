//! The built-in NML language: Rust rules for every core element.

mod code;
mod heading;
mod import;
mod include;
mod inline;
mod language;
mod list;
mod media;
mod raw;
mod reference;
mod resolve;
mod style;
mod variable;

pub use language::AddRuleError;
pub use language::Compiled;
pub use language::Finisher;
pub use language::GroupHandler;
pub use language::GroupKey;
pub use language::Handler;
pub use language::Import;
pub use language::ImportHook;
pub use language::Language;
pub use language::Lower;
pub use language::compile;
pub use resolve::MemoryResolver;
pub use resolve::ResolveError;
pub use resolve::Resolver;
pub use resolve::output_path;
pub use resolve::relative_output_url;
pub use resolve::relative_url;
pub use resolve::resolve_file;
pub use resolve::resolve_path;

impl Language {
	/// The built-in language.
	pub fn builtin() -> Self {
		let mut language = Self::empty();
		style::add(&mut language).expect("built-in rules are valid");
		heading::add(&mut language).expect("built-in rules are valid");
		list::add(&mut language).expect("built-in rules are valid");
		code::add(&mut language).expect("built-in rules are valid");
		media::add(&mut language).expect("built-in rules are valid");
		raw::add(&mut language).expect("built-in rules are valid");
		reference::add(&mut language).expect("built-in rules are valid");
		inline::add(&mut language).expect("built-in rules are valid");
		variable::add(&mut language).expect("built-in rules are valid");
		import::add(&mut language).expect("built-in rules are valid");
		include::add(&mut language).expect("built-in rules are valid");
		language
	}
}

#[cfg(test)]
pub(crate) mod tests {
	use std::sync::Arc;

	use nml_document::Document;
	use nml_document::ElementId;
	use nml_document::ElementKind;
	use nml_source::Source;
	use nml_source::Url;

	use super::*;

	pub(crate) fn compile_text(text: &str) -> Compiled {
		compile_with(text, &[])
	}

	/// Compiles `page.nml`, with other files of the workspace in memory, and the module
	/// `std` at `modules/std`.
	pub(crate) fn compile_with(text: &str, files: &[(&str, &str)]) -> Compiled {
		let source = Source::new(Url::parse("file:///page.nml").unwrap(), text.into(), 0);
		let mut resolver = MemoryResolver::default();
		resolver.files.insert("page.nml".into(), text.into());
		for (path, content) in files {
			resolver.files.insert(path.to_string(), content.to_string());
		}
		resolver.modules.insert("std".into(), "modules/std".into());
		compile(
			&Language::builtin(),
			Arc::new(source),
			"page.nml",
			Arc::new(resolver),
		)
	}

	/// A compact rendering of a document: `kind[children]`, text quoted.
	pub(crate) fn show(document: &Document, id: ElementId) -> String {
		let element = document.get(id);
		let children: Vec<_> = element
			.children()
			.iter()
			.map(|child| show(document, *child))
			.collect();
		let name = match &element.kind {
			ElementKind::Text(text) => return format!("{text:?}"),
			ElementKind::Style(style) => format!("{style:?}"),
			ElementKind::Heading { level } => format!("h{level}"),
			kind => kind.name().to_string(),
		};
		let id = match &element.attributes.id {
			Some(id) => format!("#{id}"),
			None => String::new(),
		};
		format!("{name}{id}[{}]", children.join(" "))
	}

	pub(crate) fn check(text: &str, expected: &str) -> Compiled {
		let compiled = compile_text(text);
		let document = &compiled.unit.document;
		assert_eq!(show(document, document.root()), expected);
		compiled
	}

	#[test]
	fn paragraphs_and_emphasis() {
		let compiled = check(
			"Some **bold *and* italic** text,\nand \\*stars\\*.\n\nNext.",
			r#"document[paragraph["Some " Bold["bold " Italic["and"] " italic"] " text,\nand *stars*."] paragraph["Next."]]"#,
		);
		assert!(compiled.diagnostics.is_empty());
	}

	#[test]
	fn bold_italic() {
		let compiled = check("***x***", r#"document[paragraph[Bold[Italic["x"]]]]"#);
		assert!(compiled.diagnostics.is_empty());
	}

	#[test]
	fn styles() {
		let compiled = check(
			"__u__ ==h== ~~s~~ __a ==b ~~c~~==__",
			r#"document[paragraph[Underline["u"] " " Highlight["h"] " " Strikethrough["s"] " " Underline["a " Highlight["b " Strikethrough["c"]]]]]"#,
		);
		assert!(compiled.diagnostics.is_empty());
		// Single characters are text.
		check("a_b = c~d", r#"document[paragraph["a_b = c~d"]]"#);
	}

	#[test]
	fn comments() {
		let compiled = check(
			":: a comment\nSome text :: and a note\n:: more\non two lines, std::fs.\n\n:: alone",
			r#"document[paragraph["Some text " "\n" "on two lines, std::fs."]]"#,
		);
		assert!(compiled.diagnostics.is_empty());
	}

	#[test]
	fn unclosed_emphasis_keeps_its_text() {
		let compiled = check("a **b", r#"document[paragraph["a " "b"]]"#);
		nml_diagnostics::assert_diagnostics!(compiled.diagnostics, [1001 @ 2..4]);
	}
}
