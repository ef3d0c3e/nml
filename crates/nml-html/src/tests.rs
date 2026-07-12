//! The HTML of NML elements, compiled from source text.

use std::sync::Arc;

use nml_lang::Language;
use nml_lang::MemoryResolver;
use nml_source::Source;
use nml_source::Url;

use crate::PageOptions;
use crate::compile;

/// The body of the page compiled from `text`, which must have no errors.
fn body(text: &str) -> String {
	let source = Source::new(Url::parse("file:///page.nml").unwrap(), text.into(), 0);
	let compiled = nml_lang::compile(
		&Language::builtin(),
		Arc::new(source),
		"page.nml",
		Arc::new(MemoryResolver::default()),
	);
	assert!(
		compiled.diagnostics.is_empty(),
		"{:?}",
		compiled.diagnostics
	);
	let template = compile(&compiled.unit.document, &PageOptions::default());
	let start = template.content.find("<body>\n").unwrap() + "<body>\n".len();
	let end = template.content.find("</body>").unwrap();
	template.content[start..end].to_string()
}

#[test]
fn lists() {
	assert_eq!(
		body("- a\n- *b*\n\ntext"),
		"<ul>\n<li>a</li>\n<li><em>b</em></li>\n</ul>\n<p>text</p>\n"
	);
	assert_eq!(body("1. a\n2. b"), "<ol>\n<li>a</li>\n<li>b</li>\n</ol>\n");
	assert_eq!(body("3. a"), "<ol start=\"3\">\n<li>a</li>\n</ol>\n");
	// An item with several paragraphs keeps them; a nested list follows the text.
	assert_eq!(
		body("- a\n\n  b\n- c\n  - d"),
		"<ul>\n<li><p>a</p>\n<p>b</p>\n</li>\n<li>c<ul>\n<li>d</li>\n</ul>\n</li>\n</ul>\n"
	);
}

#[test]
fn checkboxes() {
	assert_eq!(
		body("- [ ] a\n- [-] b\n- [x] c"),
		"<ul>\n\
		<li><input type=\"checkbox\" disabled> a</li>\n\
		<li><input type=\"checkbox\" disabled aria-checked=\"mixed\"> b</li>\n\
		<li><input type=\"checkbox\" disabled checked> c</li>\n\
		</ul>\n"
	);
}
