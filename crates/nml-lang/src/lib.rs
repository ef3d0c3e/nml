//! The built-in NML language: Rust rules for every core element.

mod heading;
mod language;
mod style;

pub use language::Compiled;
pub use language::Handler;
pub use language::Language;
pub use language::Lower;
pub use language::compile;

impl Language {
	/// The built-in language.
	pub fn builtin() -> Self {
		let mut language = Self::empty();
		style::add(&mut language).expect("built-in rules are valid");
		heading::add(&mut language).expect("built-in rules are valid");
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
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), text.into(), 0);
		compile(&Language::builtin(), Arc::new(source))
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
	fn unclosed_emphasis_keeps_its_text() {
		let compiled = check("a **b", r#"document[paragraph["a " "b"]]"#);
		nml_diagnostics::assert_diagnostics!(compiled.diagnostics, [1001 @ 2..4]);
	}
}
