//! Tests of the modules that come with NML.

use nml_diagnostics::assert_diagnostics;

use crate::tests::compile;
use crate::tests::html;

fn body(text: &str) -> String {
	let compiled = compile(text);
	assert_diagnostics!(compiled.diagnostics, []);
	let html = html(&compiled);
	let start =
		html.find("<main class=\"nml-main\">\n").unwrap() + "<main class=\"nml-main\">\n".len();
	let end = html.find("</main>").unwrap();
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
	// The headings keep the ids the table links to, and are numbered.
	assert!(body.contains("<h1 id=\"intro\"><a class=\"heading-link\" href=\"#intro\">🔗</a> <span class=\"heading-number\">1.</span> Intro</h1>"), "{body}");
	assert!(body.contains("<h2 id=\"setup\"><a class=\"heading-link\" href=\"#setup\">🔗</a> <span class=\"heading-number\">1.1.</span> Setup</h2>"), "{body}");
	assert!(
		body.contains("<h1 id=\"hidden\" class=\"unlisted\"><a class=\"heading-link\" href=\"#hidden\">🔗</a> <span class=\"heading-number\">2.</span> Hidden</h1>"),
		"{body}"
	);
	assert!(body.contains("<h1 id=\"intro-2\"><a class=\"heading-link\" href=\"#intro-2\">🔗</a> <span class=\"heading-number\">3.</span> Intro</h1>"), "{body}");
}

#[test]
fn empty_table_of_contents() {
	assert_eq!(
		body(":import std.toc\n\n:toc\n"),
		"<nav class=\"toc\">\n</nav>\n"
	);
}

#[test]
fn modules_are_documented() {
	for (name, source) in crate::STD {
		let doc = nml_lang::module_doc(source);
		assert!(
			doc.contains(&format!(":import std.{name}")),
			"std.{name}: {doc}"
		);
		// The syntax it adds is documented, and can be written from completion.
		let compiled = compile(&format!(":import std.{name}\n"));
		assert_diagnostics!(compiled.diagnostics, []);
		let language = compiled.language.expect("the module adds syntax");
		let rules: Vec<_> = (language.rules().iter())
			.map(|(_, rule)| rule)
			.filter(|rule| rule.origin.is_some())
			.collect();
		assert!(!rules.is_empty(), "std.{name}");
		for rule in rules {
			assert!(
				!rule.doc.is_empty(),
				"std.{name}: `{}` has no doc",
				rule.name
			);
			assert!(
				!rule.snippet.is_empty(),
				"std.{name}: `{}` has no snippet",
				rule.name
			);
		}
	}
}

#[test]
fn modules_are_written_for_editors() {
	let root = std::env::temp_dir().join(format!("nml-std-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&root);
	let dir = root.join(crate::STD_DIR);
	crate::write_std(&root).unwrap();
	for (name, content) in crate::STD {
		let path = dir.join(format!("{name}.nml"));
		assert_eq!(std::fs::read_to_string(&path).unwrap(), *content);
		assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
	}
	// Again: nothing to do.
	crate::write_std(&root).unwrap();

	// An edited copy is restored; other files go.
	let toc = dir.join("toc.nml");
	let mut permissions = std::fs::metadata(&toc).unwrap().permissions();
	#[allow(clippy::permissions_set_readonly_false)]
	permissions.set_readonly(false);
	std::fs::set_permissions(&toc, permissions).unwrap();
	std::fs::write(&toc, "edited").unwrap();
	std::fs::write(dir.join("old.nml"), "gone").unwrap();
	std::fs::create_dir_all(dir.join("sub")).unwrap();
	crate::write_std(&root).unwrap();
	assert_eq!(
		std::fs::read_to_string(&toc).unwrap(),
		crate::STD.iter().find(|(n, _)| *n == "toc").unwrap().1
	);
	assert!(std::fs::metadata(&toc).unwrap().permissions().readonly());
	let mut names: Vec<_> = std::fs::read_dir(&dir)
		.unwrap()
		.map(|e| e.unwrap().file_name().into_string().unwrap())
		.collect();
	names.sort();
	assert_eq!(names, ["nav.nml", "toc.nml"]);
	let _ = std::fs::remove_dir_all(&root);
}
