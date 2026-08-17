//! Tests of the modules that come with NML.

use nml_diagnostics::assert_diagnostics;

use crate::tests::compile;
use crate::tests::html;

fn body(text: &str) -> String {
	let compiled = compile(text);
	assert_diagnostics!(compiled.diagnostics, []);
	let html = html(&compiled);
	let start = html.find("<body>\n").unwrap() + 7;
	let end = html.find("</body>").unwrap();
	html[start..end].to_string()
}

#[test]
fn table_of_contents() {
	let body = body(
		":import std.toc\n\n:toc Contents\n\n# Intro\n## Setup {#setup}\n## Notes {.unnumbered}\n# Hidden {.unlisted}\n# Intro\n",
	);
	assert!(
		body.starts_with(
			"<nav class=\"toc\">\n<p class=\"toc-title\">Contents</p>\n<ol>\n\
			<li><a href=\"#intro\">Intro</a><ol>\n\
			<li><a href=\"#setup\">Setup</a></li>\n\
			<li class=\"unnumbered\"><a href=\"#notes\">Notes</a></li>\n</ol>\n</li>\n\
			<li><a href=\"#intro-2\">Intro</a></li>\n</ol>\n</nav>\n"
		),
		"{body}"
	);
	assert!(body.contains("<h1 id=\"intro\">Intro</h1>"), "{body}");
	assert!(body.contains("<h2 id=\"setup\">Setup</h2>"), "{body}");
	assert!(
		body.contains("<h1 class=\"unlisted\">Hidden</h1>"),
		"{body}"
	);
	assert!(body.contains("<h1 id=\"intro-2\">Intro</h1>"), "{body}");
}

#[test]
fn empty_table_of_contents() {
	assert_eq!(
		body(":import std.toc\n\n:toc\n"),
		"<nav class=\"toc\">\n</nav>\n"
	);
}
