use std::fs;
use std::path::PathBuf;

use nml_diagnostics::assert_diagnostics;

use super::*;

/// A temporary workspace, removed when dropped.
struct Workspace(PathBuf);

impl Drop for Workspace {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Workspace {
	fn new(name: &str, files: &[(&str, &str)]) -> Self {
		let dir = std::env::temp_dir().join(format!("nml-build-{name}-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let workspace = Self(dir);
		for (path, content) in files {
			workspace.write(path, content);
		}
		workspace
	}

	fn write(&self, path: &str, content: &str) {
		let path = self.0.join(path);
		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, content).unwrap();
	}

	fn remove(&self, path: &str) {
		fs::remove_file(self.0.join(path)).unwrap();
	}

	fn output(&self, path: &str) -> String {
		fs::read_to_string(self.0.join("public").join(path)).unwrap()
	}

	fn build(&self) -> BuildReport {
		build(&self.0).unwrap()
	}
}

/// The body of a page, without the head.
fn body(page: &str) -> &str {
	let start = page.find("<body>\n").unwrap() + 7;
	let end = page.find("</body>").unwrap();
	&page[start..end]
}

#[test]
fn builds_pages() {
	let workspace = Workspace::new(
		"pages",
		&[
			(
				"nml.toml",
				"exclude = [\"lib/**\"]\n[html]\nlanguage = \"en\"\n",
			),
			("index.nml", "# Welcome {#top}\n\nSome **bold** text.\n"),
			("docs/guide.nml", "A *guide*."),
			("lib/util.nml", "not a page"),
			("broken.nml", "**unclosed"),
			(".hidden/x.nml", "hidden"),
		],
	);
	let report = workspace.build();

	assert_eq!(report.written, ["docs/guide.nml", "index.nml"]);
	assert_eq!(report.failed, ["broken.nml"]);
	assert_diagnostics!(report.diagnostics, [1001 @ 0..2]);

	let index = workspace.output("index.html");
	assert!(index.contains("<html lang=\"en\">"));
	assert!(index.contains("<title>Welcome</title>"));
	assert_eq!(
		body(&index),
		"<h1 id=\"top\">Welcome</h1>\n<p>Some <strong>bold</strong> text.</p>\n"
	);
	let guide = workspace.output("docs/guide.html");
	assert!(guide.contains("<title>guide</title>"));
	assert!(!workspace.0.join("public/broken.html").exists());
	assert!(!workspace.0.join("public/lib/util.html").exists());
	assert!(workspace.0.join(".nml/index.db").exists());
}

#[test]
fn needs_a_config() {
	let workspace = Workspace::new("config", &[("a.nml", "x")]);
	assert!(matches!(build(&workspace.0), Err(BuildError::NoConfig(_))));
}

#[test]
fn resolves_references() {
	let workspace = Workspace::new(
		"references",
		&[
			("nml.toml", ""),
			(
				"index.nml",
				"# Home {#top}\nSee [[guide#install]], [[#faq|the FAQ]], [[guide#]] and [[top]].\n",
			),
			(
				"docs/guide.nml",
				"# The guide\n## Installing {#install}\n[[index#top|Back]]\n## FAQ {#faq}\n",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(
		body(&workspace.output("index.html")),
		"<h1 id=\"top\">Home</h1>\n<p>See <a href=\"docs/guide.html#install\">Installing</a>, \
		<a href=\"docs/guide.html#faq\">the FAQ</a>, <a href=\"docs/guide.html\">The guide</a> \
		and <a href=\"#top\">Home</a>.</p>\n"
	);
	assert!(
		body(&workspace.output("docs/guide.html"))
			.contains("<p><a href=\"../index.html#top\">Back</a></p>")
	);
}

#[test]
fn relinks_only_what_changed() {
	let workspace = Workspace::new(
		"relink",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#sec]]\n"),
			("b.nml", "# B\n## Section {#sec}\ntext\n"),
			("c.nml", "# C\nNo references.\n"),
		],
	);
	let first = workspace.build();
	assert_eq!(first.parsed, ["a.nml", "b.nml", "c.nml"]);
	assert_eq!(first.written, ["a.nml", "b.nml", "c.nml"]);

	// Nothing changed: nothing is parsed or written.
	let second = workspace.build();
	assert!(second.parsed.is_empty());
	assert!(second.written.is_empty());

	// A change that dependents cannot see: only the unit itself.
	workspace.write("b.nml", "# B\n## Section {#sec}\nother text\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	assert_eq!(report.written, ["b.nml"]);

	// A new title for the symbol: `a` is relinked, without being parsed.
	workspace.write("b.nml", "# B\n## Renamed {#sec}\nother text\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"b.html#sec\">Renamed</a>")
	);

	// The symbol disappears: `a` shows an unresolved reference.
	workspace.write("b.nml", "# B\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert_diagnostics!(report.diagnostics, [3001 @ 4..13]);
	assert!(workspace.output("a.html").contains("nml-unresolved"));

	// It comes back.
	workspace.write("b.nml", "# B\n## Back {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert_diagnostics!(report.diagnostics, []);
}

#[test]
fn unit_identity_changes() {
	let workspace = Workspace::new(
		"identity",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#]] [[setup#]]\n"),
			("b.nml", "# Bee\n"),
			("c.nml", "# C\n"),
		],
	);
	workspace.build();

	// A new page title: users of the page are relinked.
	workspace.write("b.nml", "# Bee two\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("a.html").contains(">Bee two</a>"));

	// `c` takes the key `setup`: `[[setup#]]` now resolves.
	workspace.write("c.nml", ":set nml.reference_key = setup\n# C\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "c.nml"]);
	assert_diagnostics!(report.diagnostics, []);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"c.html\">C</a>")
	);

	// `b` is removed: `a` is relinked.
	workspace.remove("b.nml");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [3001]);
}

#[test]
fn ambiguous_references() {
	let workspace = Workspace::new(
		"ambiguous",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[#x]] [[same#]]\n"),
			("b.nml", "# B {#x}\n"),
			("one/same.nml", "# One\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);

	// A second `x` and a second unit named `same`.
	workspace.write("c.nml", "# C {#x}\n");
	workspace.write("two/same.nml", "# Two\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "c.nml", "two/same.nml"]);
	assert_diagnostics!(report.diagnostics, [3006 @ 4..10, 3006 @ 11..20]);
	assert_eq!(
		report.diagnostics[0].kind,
		DiagnosticKind::AmbiguousReference {
			target: "#x".into(),
			count: 2
		}
	);
}

#[test]
fn failed_units_keep_their_last_state() {
	let workspace = Workspace::new(
		"failed",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#sec]]\n"),
			("b.nml", "# B\n## Section {#sec}\n"),
		],
	);
	workspace.build();

	// `b` breaks: its page and symbols stay as they were.
	workspace.write("b.nml", "# B\n## Section {#sec}\n**unclosed\n");
	let report = workspace.build();
	assert_eq!(report.failed, ["b.nml"]);
	assert!(report.written.is_empty());
	assert!(workspace.output("b.html").contains("Section"));

	// It is parsed again on every build until it is fixed.
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	workspace.write("b.nml", "# B\n## Fixed {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("a.html").contains(">Fixed</a>"));
}

#[test]
fn links_to_libraries_are_unresolved() {
	let workspace = Workspace::new(
		"library",
		&[
			("nml.toml", "exclude = [\"lib/**\"]\n"),
			("a.nml", "[[util#x]] [[util#x|caption]]\n"),
			("lib/util.nml", "# Utilities {#x}\n"),
		],
	);
	let report = workspace.build();
	// The title resolves, but a library has no page to link to.
	assert_diagnostics!(report.diagnostics, [3001 @ 0..10, 3001 @ 11..29]);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"\">Utilities</a>")
	);
}

#[test]
fn link_errors_are_reported_until_fixed() {
	let workspace = Workspace::new(
		"link-errors",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				"[[#missing]]
",
			),
			(
				"b.nml", "# B
",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3001]);

	// Nothing changed, but `a` is linked again to report its error.
	let report = workspace.build();
	assert!(report.parsed.is_empty());
	assert_eq!(report.written, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [3001]);

	// Another unit defines the symbol: fixed, and then left alone.
	workspace.write(
		"b.nml",
		"# B {#missing}
",
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let report = workspace.build();
	assert!(report.written.is_empty());
}
